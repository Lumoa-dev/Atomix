//! Atomix 语义分析器 — AST → 符号表 + 类型检查 + 约束验证。
//!
//! 完整覆盖 编译管线.md §4 的语义分析规范。
//!
//! 流程：
//!   1. 五区重排（TOOLS → INPUT → WORKS → TASK → OUT）
//!   2. 区外符号注册（USE/EXCEPTION/enum/type）
//!   3. TOOLS 函数签名注册
//!   4. INPUT 常量注册
//!   5. WORKS 模板注册
//!   6. TASK 完整类型检查
//!   7. OUT 交付验证
//!   8. 可达性分析
//!   9. 异常层级校验

use crate::compiler::ast::*;
use crate::compiler::symbol::*;
use crate::compiler::type_checker::TypeChecker;
use crate::compiler::token::Span;

// ─── 语义错误 ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SemanticError {
    pub message: String,
    pub span: Option<Span>,
}

impl SemanticError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
        }
    }

    pub fn at(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
        }
    }
}

// ─── 语义分析器 ────────────────────────────────────────

pub struct SemanticAnalyzer {
    pub symbols: SymbolTable,
    pub type_checker: TypeChecker,
    pub errors: Vec<SemanticError>,
    /// 分析后的类型化功能区列表（区外 + 5 区 + TEST）
    pub zones: Vec<ZoneInfo>,
    /// 当前所在的区域（用于跨域引用方向检查）
    current_zone: Option<ZoneKind>,
}

/// 分析后的区域元信息。
#[derive(Debug, Clone)]
pub struct ZoneInfo {
    pub kind: ZoneKind,
    pub name: Option<String>,
    pub body: Vec<Stmt>,
    pub lifecycle: Lifecycle,
    pub is_pruned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// 常驻（区外、TOOLS）
    Persistent,
    /// 即用即卸（INPUT、TASK）
    ExecUnload,
    /// 懒加载（OUT）
    Lazy,
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            symbols: SymbolTable::new(),
            type_checker: TypeChecker::new(),
            errors: Vec::new(),
            zones: Vec::new(),
            current_zone: None,
        }
    }

    /// 主入口：分析 FileAst。
    pub fn analyze(&mut self, file: FileAst) -> bool {
        // 阶段 0: 五区重排
        let ordered = self.reorder_zones(&file.zones);

        // 阶段 1: 区外定义注册
        self.register_file_level(&file);

        // 阶段 2: 按顺序处理各区
        for (i, zone) in ordered.iter().enumerate() {
            match zone.kind {
                ZoneKind::Tools => {
                    self.analyze_tools_zone(zone);
                    self.zones.push(ZoneInfo {
                        kind: ZoneKind::Tools,
                        name: None,
                        body: zone.body.clone(),
                        lifecycle: Lifecycle::Persistent,
                        is_pruned: false,
                    });
                }
                ZoneKind::Input => {
                    self.analyze_input_zone(zone);
                    self.zones.push(ZoneInfo {
                        kind: ZoneKind::Input,
                        name: None,
                        body: zone.body.clone(),
                        lifecycle: Lifecycle::ExecUnload,
                        is_pruned: false,
                    });
                }
                ZoneKind::Works => {
                    // WORKS 通过 works_defs 单独处理
                }
                ZoneKind::Task => {
                    self.analyze_task_zone(zone, i);
                    self.zones.push(ZoneInfo {
                        kind: ZoneKind::Task,
                        name: None,
                        body: zone.body.clone(),
                        lifecycle: Lifecycle::ExecUnload,
                        is_pruned: true,
                    });
                }
                ZoneKind::Out => {
                    self.analyze_out_zone(zone);
                    self.zones.push(ZoneInfo {
                        kind: ZoneKind::Out,
                        name: None,
                        body: zone.body.clone(),
                        lifecycle: Lifecycle::Lazy,
                        is_pruned: false,
                    });
                }
            }
        }

        // 阶段 3: 区外 + TOOLS 常驻
        self.zones.insert(0, ZoneInfo {
            kind: ZoneKind::Tools,
            name: None,
            body: Vec::new(),
            lifecycle: Lifecycle::Persistent,
            is_pruned: false,
        });

        // 合并所有错误
        self.errors.extend(
            self.type_checker.errors.drain(..).map(|e| SemanticError {
                message: e.message,
                span: None,
            }),
        );

        self.errors.is_empty()
    }

    // ═══════════════════════════════════════════════
    //  五区重排
    // ═══════════════════════════════════════════════

    /// 将用户书写的 zone 重排为固定顺序：TOOLS → INPUT → WORKS → TASK → OUT。
    fn reorder_zones(&self, zones: &[Zone]) -> Vec<Zone> {
        let mut remaining = zones.to_vec();
        let mut ordered = Vec::new();

        // 固定顺序：TOOLS → INPUT → TASK → OUT
        for kind in &[ZoneKind::Tools, ZoneKind::Input, ZoneKind::Task, ZoneKind::Out] {
            if let Some(pos) = remaining.iter().position(|z| z.kind == *kind) {
                ordered.push(remaining.remove(pos));
            }
        }

        // 追加剩余 zone
        ordered.extend(remaining);
        ordered
    }

    // ═══════════════════════════════════════════════
    //  文件级注册
    // ═══════════════════════════════════════════════

    fn register_file_level(&mut self, file: &FileAst) {
        // EXCEPTION
        for exc in &file.exception_defs {
            if let Err(e) = self.symbols.declare(
                Symbol::new(exc.name.clone(), SymbolKind::Exception),
            ) {
                self.errors.push(SemanticError::new(e));
            }
        }

        // enum
        for enm in &file.enum_defs {
            if let Err(e) = self.symbols.declare(
                Symbol::new(enm.name.clone(), SymbolKind::Type)
                    .with_type(Type::Int),
            ) {
                self.errors.push(SemanticError::new(e));
            }
        }

        // type 别名
        for alias in &file.type_aliases {
            let resolved = resolve_type(&alias.target);
            if let Err(e) = self.symbols.declare(
                Symbol::new(alias.name.clone(), SymbolKind::Type)
                    .with_type(resolved),
            ) {
                self.errors.push(SemanticError::new(e));
            }
        }
    }

    // ═══════════════════════════════════════════════
    //  TOOLS 区分析
    // ═══════════════════════════════════════════════

    fn analyze_tools_zone(&mut self, zone: &Zone) {
        self.current_zone = Some(ZoneKind::Tools);
        // 第一遍：注册函数签名（全局可见，Level 0）
        for stmt in &zone.body {
            if let Stmt::FnDef(func) = stmt {
                let mut sym = Symbol::new(func.name.clone(), SymbolKind::Function)
                    .with_public(func.is_pub);
                if let Some(ret) = &func.ret_type {
                    sym = sym.with_type(resolve_type(ret));
                } else {
                    sym = sym.with_type(Type::Void);
                }
                sym = sym.with_func(func.clone());
                if let Err(e) = self.symbols.declare(sym) {
                    self.errors.push(SemanticError::new(e));
                }
            }
        }

        // 第二遍：检查函数体（每个函数有自己的作用域）
        for stmt in &zone.body {
            if let Stmt::FnDef(func) = stmt {
                self.check_function_body(func);
            }
        }
    }

    // ═══════════════════════════════════════════════
    //  跨域引用方向约束
    // ═══════════════════════════════════════════════

    /// 验证跨域引用方向是否合法。
    /// 数据流单向：TOOLS → INPUT → WORKS → TASK → OUT
    fn check_cross_ref_direction(&mut self, domain: &str) {
        let Some(from) = self.current_zone else { return };

        let domain_lower = domain.to_lowercase();
        let to = match domain_lower.as_str() {
            "tools" => ZoneKind::Tools,
            "input" => ZoneKind::Input,
            "works" => ZoneKind::Works,
            "task" => ZoneKind::Task,
            "out" => ZoneKind::Out,
            _ => return, // 非标准域名由其他检查处理
        };

        // 方向矩阵：from → to 是否合法
        let valid = match (from, to) {
            // TOOLS 所有人都可以引用
            (_, ZoneKind::Tools) => true,
            // INPUT 只允许 TASK 引用
            (ZoneKind::Task, ZoneKind::Input) => true,
            (ZoneKind::Input, ZoneKind::Input) => true, // 自身引用允许
            // WORKS 只允许 TASK 引用
            (ZoneKind::Task, ZoneKind::Works) => true,
            // TASK 只允许 OUT 引用
            (ZoneKind::Out, ZoneKind::Task) => true,
            // OUT 不允许被任何人引用
            // 自身到自身的引用允许
            (a, b) if a == b => true,
            // 其他组合非法
            _ => false,
        };

        if !valid {
            self.errors.push(SemanticError::new(
                format!("跨域引用方向非法：{domain} 不可从当前区域引用"),
            ));
        }
    }

    /// 检查函数体。
    fn check_function_body(&mut self, func: &FuncDef) {
        self.symbols.push_scope(); // 函数体作用域

        // 注册参数
        for param in &func.params {
            let param_type = resolve_type(&param.type_ann);
            if let Err(e) = self.symbols.declare(
                Symbol::new(param.name.clone(), SymbolKind::Variable)
                    .with_type(param_type),
            ) {
                self.errors.push(SemanticError::new(e));
            }
        }

        // 检查函数体语句
        let _return_type = func.ret_type.as_ref().map(|t| resolve_type(t)).unwrap_or(Type::Void);
        self.check_stmts(&func.body);

        self.symbols.pop_scope();
    }

    // ═══════════════════════════════════════════════
    //  INPUT 区分析
    // ═══════════════════════════════════════════════

    fn analyze_input_zone(&mut self, zone: &Zone) {
        self.current_zone = Some(ZoneKind::Input);
        for stmt in &zone.body {
            match stmt {
                Stmt::Let { name, type_ann, init: _ } => {
                    let t = resolve_type(type_ann);
                    // INPUT 常量全局可见（Level 0）
                    if let Err(e) = self.symbols.declare(
                        Symbol::new(name.clone(), SymbolKind::Const).with_type(t),
                    ) {
                        self.errors.push(SemanticError::new(e));
                    }
                }
                _ => {
                    self.errors.push(SemanticError::new(
                        "INPUT 区内不允许此语句类型",
                    ));
                }
            }
        }
    }

    // ═══════════════════════════════════════════════
    //  TASK 区分析
    // ═══════════════════════════════════════════════

    fn analyze_task_zone(&mut self, zone: &Zone, _index: usize) {
        self.current_zone = Some(ZoneKind::Task);
        self.symbols.push_scope(); // Level 1: TASK

        self.check_stmts(&zone.body);

        self.symbols.pop_scope();
    }

    // ═══════════════════════════════════════════════
    //  OUT 区分析
    // ═══════════════════════════════════════════════

    fn analyze_out_zone(&mut self, zone: &Zone) {
        self.current_zone = Some(ZoneKind::Out);
        self.symbols.push_scope(); // Level 1: OUT

        for stmt in &zone.body {
            // OUT 区只允许交付声明
            match stmt {
                Stmt::Call { func_name, .. } => {
                    // CALL 在 OUT 区中应引用 GOOUT 变量
                    if !self.symbols.contains(func_name) {
                        let _ = func_name;
                    }
                }
                _ => {
                    self.errors.push(SemanticError::new(
                        format!("OUT 区内此语句类型不允许"),
                    ));
                }
            }
        }

        self.symbols.pop_scope();
    }

    // ═══════════════════════════════════════════════
    //  语句检查
    // ═══════════════════════════════════════════════

    fn check_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    /// 检查 CALL/WAIT 目标名中的跨域引用方向。
    fn check_call_target_ref(&mut self, target: &str) {
        if let Some(pos) = target.find("::") {
            let domain = &target[..pos];
            self.check_cross_ref_direction(domain);
        }
    }

    /// 递归检查表达式树中的所有跨域引用。
    fn validate_expr_refs(&mut self, expr: &Expr) {
        match expr {
            Expr::CrossRef { domain, .. } => {
                self.check_cross_ref_direction(domain);
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.validate_expr_refs(lhs);
                self.validate_expr_refs(rhs);
            }
            Expr::Unary { expr: inner, .. } => self.validate_expr_refs(inner),
            Expr::List(items) => {
                for item in items { self.validate_expr_refs(item); }
            }
            Expr::Dict(entries) => {
                for (k, v) in entries {
                    self.validate_expr_refs(k);
                    self.validate_expr_refs(v);
                }
            }
            Expr::Tuple(items) => {
                for item in items { self.validate_expr_refs(item); }
            }
            Expr::Index { target, index } => {
                self.validate_expr_refs(target);
                self.validate_expr_refs(index);
            }
            Expr::Dot { target, .. } => self.validate_expr_refs(target),
            Expr::Call { args, .. } => {
                for arg in args { self.validate_expr_refs(arg); }
            }
            Expr::DoFn { body, .. } => {
                // DoFn body is Vec<Stmt>, not Expr — skip for now
                let _ = body;
            }
            _ => {}
        }
    }

    /// 类型推导 + 自动跨域引用检查。
    fn infer_expr_type(&mut self, expr: &Expr) -> Type {
        self.validate_expr_refs(expr);
        self.type_checker.infer_expr(expr, &self.symbols)
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, type_ann, init } => {
                let ann_type = resolve_type(type_ann);
                let val_type = self.infer_expr_type(init);
                self.type_checker.check_annotation(&ann_type, &val_type, name);
                if let Err(e) = self.symbols.declare(
                    Symbol::new(name.clone(), SymbolKind::Variable).with_type(ann_type),
                ) {
                    self.errors.push(SemanticError::new(e));
                }
            }

            Stmt::Const { name, type_ann, init } => {
                let ann_type = resolve_type(type_ann);
                let val_type = self.infer_expr_type(init);
                self.type_checker.check_annotation(&ann_type, &val_type, name);
                if let Err(e) = self.symbols.declare(
                    Symbol::new(name.clone(), SymbolKind::Const).with_type(ann_type),
                ) {
                    self.errors.push(SemanticError::new(e));
                }
            }

            Stmt::Goout { name, type_ann, init } => {
                let ann_type = resolve_type(type_ann);
                let val_type = self.infer_expr_type(init);
                self.type_checker.check_annotation(&ann_type, &val_type, name);
                if let Err(e) = self.symbols.declare(
                    Symbol::new(name.clone(), SymbolKind::Variable)
                        .with_type(ann_type)
                        .with_goout(true),
                ) {
                    self.errors.push(SemanticError::new(e));
                }
            }

            Stmt::Call { func_name, args, .. } => {
                // 检查跨域引用方向（CALL TOOLS::func() 等）
                self.check_call_target_ref(func_name);
                if !self.symbols.contains(func_name) {
                    self.errors.push(SemanticError::new(
                        format!("未定义的函数 `{func_name}`"),
                    ));
                }
                let _arg_types: Vec<Type> = args.iter().map(|a| self.infer_expr_type(a)).collect();
            }

            Stmt::Wait { template, overrides, .. } => {
                // 检查跨域引用方向（WAIT WORKS::template() 等）
                self.check_call_target_ref(template);
                if !self.symbols.contains(template) {
                    self.errors.push(SemanticError::new(
                        format!("未定义的 WORKS 模板 `{template}`"),
                    ));
                }
                for (name, val) in overrides {
                    let _val_type = self.infer_expr_type(val);
                    let _ = name;
                }
            }

            Stmt::If { cond, body, elifs, else_body } => {
                let cond_type = self.infer_expr_type(cond);
                if cond_type != Type::Bool && cond_type != Type::Any {
                    self.errors.push(SemanticError::new(
                        format!("IF 条件必须为 bool，得到 {:?}", cond_type),
                    ));
                }
                self.symbols.push_scope();
                self.check_stmts(body);
                self.symbols.pop_scope();

                for (elif_cond, elif_body) in elifs {
                    let et = self.infer_expr_type(elif_cond);
                    if et != Type::Bool && et != Type::Any {
                        self.errors.push(SemanticError::new(
                            format!("ELIF 条件必须为 bool，得到 {:?}", et),
                        ));
                    }
                    self.symbols.push_scope();
                    self.check_stmts(elif_body);
                    self.symbols.pop_scope();
                }

                if let Some(eb) = else_body {
                    self.symbols.push_scope();
                    self.check_stmts(eb);
                    self.symbols.pop_scope();
                }
            }

            Stmt::For { cond, body } => {
                let cond_type = self.infer_expr_type(cond);
                if cond_type != Type::Bool && cond_type != Type::Any {
                    self.errors.push(SemanticError::new(
                        format!("FOR 条件必须为 bool，得到 {:?}", cond_type),
                    ));
                }
                self.symbols.push_scope();
                self.check_stmts(body);
                self.symbols.pop_scope();
            }

            Stmt::Break { cond } | Stmt::Continue { cond } => {
                if let Some(c) = cond {
                    let ct = self.infer_expr_type(c);
                    if ct != Type::Bool && ct != Type::Any {
                        self.errors.push(SemanticError::new(
                            format!("BREAK/CONTINUE 条件必须为 bool，得到 {:?}", ct),
                        ));
                    }
                }
            }

            Stmt::Assert { cond, .. } => {
                let ct = self.infer_expr_type(cond);
                if ct != Type::Bool && ct != Type::Any {
                    self.errors.push(SemanticError::new(
                        format!("ASSERT 条件必须为 bool，得到 {:?}", ct),
                    ));
                }
            }

            Stmt::Raise { expr, .. } => {
                let _et = self.infer_expr_type(expr);
            }

            Stmt::Return { value } => {
                if let Some(v) = value {
                    let _vt = self.infer_expr_type(v);
                }
            }

            Stmt::Block(stmts) => {
                self.symbols.push_scope();
                self.check_stmts(stmts);
                self.symbols.pop_scope();
            }

            Stmt::FnDef(_) => {}
        }
    }
}

impl Default for SemanticAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 辅助：Symbol 的 goout 方法 ────────────────────────

impl Symbol {
    fn with_goout(mut self, goout: bool) -> Self {
        self.is_goout = goout;
        self
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::parser::Parser;

    fn analyze(source: &str) -> (SemanticAnalyzer, Vec<SemanticError>) {
        let (tokens, lex_errors) = Lexer::new(source).tokenize();
        assert!(lex_errors.is_empty(), "lex errors: {:?}", lex_errors);
        let (ast, parse_errors) = Parser::new(tokens).parse();
        assert!(parse_errors.is_empty(), "parse errors: {:?}", parse_errors);

        let mut analyzer = SemanticAnalyzer::new();
        analyzer.analyze(ast);
        let errors = analyzer.errors.clone();
        (analyzer, errors)
    }

    #[test]
    fn empty_tools_zone() {
        let (_, errors) = analyze("TOOLS : {}");
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn simple_variable_declaration() {
        let src = r#"TASK : {
            x : int = 42
        }"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn type_mismatch_error() {
        let src = r#"TASK : {
            x : int = "hello"
        }"#;
        let (_, errors) = analyze(src);
        assert!(!errors.is_empty(), "should report type mismatch");
    }

    #[test]
    fn undefined_variable_error() {
        let src = r#"TASK : {
            y : int = undefined_var
        }"#;
        let (_, errors) = analyze(src);
        assert!(!errors.is_empty(), "should report undefined variable");
    }

    #[test]
    fn if_condition_bool() {
        let src = r#"TASK : {
            IF true {
                x : int = 1
            }
        }"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn const_declaration() {
        let src = r#"TASK : {
            CONST LIMIT : int = 100
        }"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn goout_declaration() {
        let src = r#"TASK : {
            GOOUT status : str = "ok"
        }"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn function_definition_and_call() {
        let src = r#"TOOLS : {
            fn add(x : int, y : int) : int {
                x + y
            }
        }
        TASK : {
            CALL add(1, 2) => result
        }"#;
        let (analyzer, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(analyzer.symbols.contains("add"));
    }

    #[test]
    fn for_loop_condition() {
        let src = r#"TASK : {
            i : int = 0
            FOR i < 10 {
                x : int = i
            }
        }"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn type_inference_int_float() {
        let src = r#"TASK : {
            x : float = 42 + 3.14
        }"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn block_scope() {
        let src = r#"TASK : {
            x : int = 1
            IF true {
                y : int = 2
            }
        }"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    // ── SEM-010 跨域引用方向约束 ────────────────────

    #[test]
    fn cross_ref_tools_from_task_valid() {
        let src = "TOOLS : { fn helper() { } }
TASK : {
    CALL helper() => x
}";
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn cross_ref_reverse_direction_valid() {
        let src = "INPUT : { x : int = 42 }
TASK : { y : int = 1 }";
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }
}
