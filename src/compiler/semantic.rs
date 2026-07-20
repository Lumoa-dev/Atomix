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
use crate::compiler::token::Span;
use crate::compiler::type_checker::TypeChecker;

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

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.span {
            Some(span) => write!(
                f,
                "语义错误: {} (位置: {}:{})",
                self.message, span.start.line, span.start.col
            ),
            None => write!(f, "语义错误: {}", self.message),
        }
    }
}

// ─── 语义分析器 ────────────────────────────────────────

pub struct SemanticAnalyzer {
    pub symbols: SymbolTable,
    pub type_checker: TypeChecker,
    pub errors: Vec<SemanticError>,
    /// 分析警告（非阻断性）
    pub warnings: Vec<String>,
    /// 分析后的类型化功能区列表（区外 + 5 区 + TEST）
    pub zones: Vec<ZoneInfo>,
    /// 当前所在的区域（用于跨域引用方向检查）
    current_zone: Option<ZoneKind>,
    /// 泛型单态化映射：(func_name, type_params_str) → monomorphized_name
    pub monomorphizations: std::collections::HashMap<(String, String), String>,
}

/// 分析后的区域元信息。
#[derive(Debug, Clone)]
pub struct ZoneInfo {
    pub kind: ZoneKind,
    pub name: Option<String>,
    pub body: Vec<Stmt>,
    pub lifecycle: Lifecycle,
    pub is_pruned: bool,
    /// INPUT 区的数据源声明（供 codegen 使用）
    pub source_decls: Vec<SourceDecl>,
    /// OUT 区的数据交付声明（供 codegen 使用）
    pub target_decls: Vec<TargetDecl>,
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
            warnings: Vec::new(),
            zones: Vec::new(),
            current_zone: None,
            monomorphizations: std::collections::HashMap::new(),
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
                        source_decls: Vec::new(),
                        target_decls: Vec::new(),
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
                        source_decls: zone.source_decls.clone(),
                        target_decls: Vec::new(),
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
                        source_decls: Vec::new(),
                        target_decls: Vec::new(),
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
                        source_decls: Vec::new(),
                        target_decls: zone.target_decls.clone(),
                    });
                }
            }
        }

        // 阶段 3: 单态化后处理 — 将所有泛型调用点替换为单态化函数名
        self.resolve_generic_calls();

        // 阶段 4: 可达性分析
        self.analyze_reachability(&ordered);

        // 阶段 5: 区外 + TOOLS 常驻
        self.zones.insert(
            0,
            ZoneInfo {
                kind: ZoneKind::Tools,
                name: None,
                body: Vec::new(),
                lifecycle: Lifecycle::Persistent,
                is_pruned: false,
                source_decls: Vec::new(),
                target_decls: Vec::new(),
            },
        );

        // 合并所有错误
        self.errors
            .extend(self.type_checker.errors.drain(..).map(|e| SemanticError {
                message: e.message,
                span: None,
            }));

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
        for kind in &[
            ZoneKind::Tools,
            ZoneKind::Input,
            ZoneKind::Task,
            ZoneKind::Out,
        ] {
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
        // 第一遍：注册 EXCEPTION
        for exc in &file.exception_defs {
            if let Err(e) = self
                .symbols
                .declare(Symbol::new(exc.name.clone(), SymbolKind::Exception))
            {
                self.errors.push(SemanticError::new(e));
            }
        }

        // 第二遍：验证 EXCEPTION 层级合法性
        self.validate_exception_hierarchy(&file.exception_defs);

        // enum
        for enm in &file.enum_defs {
            if let Err(e) = self
                .symbols
                .declare(Symbol::new(enm.name.clone(), SymbolKind::Type).with_type(Type::Int))
            {
                self.errors.push(SemanticError::new(e));
            }
        }

        // type 别名
        for alias in &file.type_aliases {
            let resolved = resolve_type(&alias.target);
            if let Err(e) = self
                .symbols
                .declare(Symbol::new(alias.name.clone(), SymbolKind::Type).with_type(resolved))
            {
                self.errors.push(SemanticError::new(e));
            }
        }

        // 内置函数注册（全局可用，编译器内联展开为 IR）
        for entry in crate::compiler::builtins::ALL_BUILTINS {
            let sym = Symbol::new(entry.name.to_string(), SymbolKind::Builtin);
            if let Err(e) = self.symbols.declare(sym) {
                self.errors.push(SemanticError::new(e));
            }
        }

        // WORKS 模板注册
        for works in &file.works_defs {
            // 注册模板名
            if let Err(e) = self.symbols.declare(
                Symbol::new(works.name.clone(), SymbolKind::Works)
            ) {
                self.errors.push(SemanticError::new(e));
            }
            // 注册模板方法
            for method in &works.methods {
                let mut sym = Symbol::new(
                    format!("{}::{}", works.name, method.name),
                    SymbolKind::Function,
                ).with_public(method.is_pub);
                if let Some(ret) = &method.ret_type {
                    sym = sym.with_type(resolve_type(ret));
                } else {
                    sym = sym.with_type(Type::Void);
                }
                sym = sym.with_func(method.clone());
                if let Err(e) = self.symbols.declare(sym) {
                    self.errors.push(SemanticError::new(e));
                }
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
                let mut sym =
                    Symbol::new(func.name.clone(), SymbolKind::Function).with_public(func.is_pub);
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

    /// 验证 EXCEPTION 层级合法性。
    /// - 父异常必须已定义
    /// - 无循环继承（A::B 且 B::A）
    fn validate_exception_hierarchy(&mut self, defs: &[ExceptionDef]) {
        // 建父子映射
        let mut parents: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        for exc in defs {
            if let Some(parent) = &exc.parent {
                parents.insert(&exc.name, parent);
            }
        }

        for exc in defs {
            // 父异常存在性检查
            if let Some(parent) = &exc.parent
                && !self.symbols.contains(parent)
            {
                self.errors.push(SemanticError::new(format!(
                    "EXCEPTION `{}` 的父异常 `{parent}` 未定义",
                    exc.name
                )));
            }

            // 循环继承检测（沿父链向上走，看是否能回到自己）
            let mut visited = std::collections::HashSet::new();
            let mut current = exc.name.as_str();
            visited.insert(current);
            while let Some(parent) = parents.get(current) {
                if visited.contains(parent) {
                    self.errors.push(SemanticError::new(format!(
                        "EXCEPTION 循环继承: `{}` 和 `{parent}`",
                        exc.name
                    )));
                    break;
                }
                visited.insert(parent);
                current = parent;
            }
        }
    }

    // ═══════════════════════════════════════════════
    //  跨域引用方向约束
    // ═══════════════════════════════════════════════

    /// 验证跨域引用方向是否合法。
    /// 数据流单向：TOOLS → INPUT → WORKS → TASK → OUT
    fn check_cross_ref_direction(&mut self, domain: &str) {
        let Some(from) = self.current_zone else {
            return;
        };

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
            self.errors.push(SemanticError::new(format!(
                "跨域引用方向非法：{domain} 不可从当前区域引用"
            )));
        }
    }

    /// 检查函数体。
    fn check_function_body(&mut self, func: &FuncDef) {
        self.symbols.push_scope(); // 函数体作用域

        // 注册参数
        for param in &func.params {
            let param_type = resolve_type(&param.type_ann);
            if let Err(e) = self.symbols.declare(
                Symbol::new(param.name.clone(), SymbolKind::Variable).with_type(param_type),
            ) {
                self.errors.push(SemanticError::new(e));
            }
        }

        // 检查函数体语句
        let _return_type = func
            .ret_type
            .as_ref()
            .map(resolve_type)
            .unwrap_or(Type::Void);
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
                Stmt::Let {
                    name,
                    type_ann,
                    init,
                } => {
                    let t = resolve_type(type_ann);
                    // 验证初始值类型匹配标注
                    let val_type = self.infer_expr_type(init);
                    self.type_checker.check_annotation(&t, &val_type, name);
                    // INPUT 常量全局可见（Level 0）
                    if let Err(e) = self
                        .symbols
                        .declare(Symbol::new(name.clone(), SymbolKind::Const).with_type(t))
                    {
                        self.errors.push(SemanticError::new(e));
                    }
                }
                Stmt::Call { .. } | Stmt::Wait { .. } | Stmt::If { .. } | Stmt::For { .. } => {
                    self.errors.push(SemanticError::new(
                        "INPUT 区不允许控制流或调用语句".to_string(),
                    ));
                }
                _ => {
                    // 其他语句类型不报错（允许基础声明）
                }
            }
        }

        // 处理数据源声明
        for decl in &zone.source_decls {
            // 验证装饰器标识符引用已注册的 TOOLS 函数
            for deco in &decl.decorators {
                if !self.symbols.contains(deco) {
                    self.errors.push(SemanticError::new(format!(
                        "装饰器 `{deco}` 未定义（需在 TOOLS 区声明）"
                    )));
                }
            }
            // 如果有 target 变量，注册为常量
            if let Some(target) = &decl.target {
                let t = target
                    .type_ann
                    .as_ref()
                    .map(resolve_type)
                    .unwrap_or(Type::Any);
                if let Err(e) = self
                    .symbols
                    .declare(Symbol::new(target.var_name.clone(), SymbolKind::Const).with_type(t))
                {
                    self.errors.push(SemanticError::new(e));
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

        // 收集当前作用域中的 GOOUT 变量
        let goouts: Vec<Symbol> = self
            .symbols
            .current_scope()
            .filter(|(_, s)| s.is_goout)
            .map(|(_, s)| s.clone())
            .collect();

        self.symbols.pop_scope(); // 先弹出 TASK 作用域

        // 再将 GOOUT 变量注册到全局作用域（Level 0）
        for sym in goouts {
            let _ = self.symbols.declare(sym);
        }
    }

    // ═══════════════════════════════════════════════
    //  OUT 区分析
    // ═══════════════════════════════════════════════

    fn analyze_out_zone(&mut self, zone: &Zone) {
        self.current_zone = Some(ZoneKind::Out);
        self.symbols.push_scope(); // Level 1: OUT

        for stmt in &zone.body {
            match stmt {
                Stmt::Call { func_name, .. } => {
                    // 验证引用的变量是 GOOUT 声明的
                    match self.symbols.lookup(func_name) {
                        Some(sym) if !sym.is_goout => {
                            self.errors.push(SemanticError::new(format!(
                                "OUT 区引用的 `{func_name}` 不是 GOOUT 变量"
                            )));
                        }
                        None => {
                            self.errors.push(SemanticError::new(format!(
                                "OUT 区引用的 `{func_name}` 未定义"
                            )));
                        }
                        _ => {} // GOOUT 变量，通过
                    }
                }
                Stmt::Let { name: _, .. } => {
                    // OUT 区允许 Let 声明（临时变量）
                }
                _ => {
                    self.errors
                        .push(SemanticError::new("OUT 区内此语句类型不允许"));
                }
            }
        }

        // 处理数据交付声明
        for decl in &zone.target_decls {
            // 验证源变量是 GOOUT 声明的
            match self.symbols.lookup(&decl.source_var) {
                Some(sym) if !sym.is_goout => {
                    self.errors.push(SemanticError::new(format!(
                        "OUT 区引用的 `{}` 不是 GOOUT 变量",
                        decl.source_var
                    )));
                }
                None => {
                    self.errors.push(SemanticError::new(format!(
                        "OUT 区引用的 `{}` 未定义",
                        decl.source_var
                    )));
                }
                _ => {}
            }
            // 验证装饰器标识符引用已注册的 TOOLS 函数
            for deco in &decl.decorators {
                if !self.symbols.contains(deco) {
                    self.errors.push(SemanticError::new(format!(
                        "装饰器 `{deco}` 未定义（需在 TOOLS 区声明）"
                    )));
                }
            }
        }

        self.symbols.pop_scope();
    }

    // ═══════════════════════════════════════════════
    //  可达性分析
    // ═══════════════════════════════════════════════

    /// 从 TASK 入口出发，标记所有可达的 TOOLS 函数和 WORKS 模板。
    fn analyze_reachability(&mut self, zones: &[Zone]) {
        let mut reachable: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 从 TASK zone body 中收集所有被调用的函数/WORKS 名
        for zone in zones {
            if zone.kind == ZoneKind::Task {
                self.collect_call_targets(&zone.body, &mut reachable);
            }
        }

        // 为每个不可达的函数生成警告
        for sym in self.symbols.functions() {
            if !reachable.contains(&sym.name)
                && !sym.name.is_empty()
                && !sym.name.contains("::") // 单态化函数由编译器自动生成，不报可达性警告
            {
                // 不可达函数产生警告（非错误）
                self.warnings.push(format!(
                    "警告：函数 `{}` 不可达（未被任何 CALL/WAIT 引用）",
                    sym.name
                ));
            }
        }
    }

    /// 递归收集语句中的所有 CALL/WAIT 目标名。
    fn collect_call_targets(
        &self,
        stmts: &[Stmt],
        targets: &mut std::collections::HashSet<String>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Call { func_name, .. } => {
                    if let Some(pos) = func_name.find("::") {
                        targets.insert(func_name[(pos + 2)..].to_string());
                    } else {
                        targets.insert(func_name.clone());
                    }
                }
                Stmt::Wait { template, .. } => {
                    targets.insert(template.clone());
                }
                Stmt::If {
                    body,
                    elifs,
                    else_body,
                    ..
                } => {
                    self.collect_call_targets(body, targets);
                    for (_, eb) in elifs {
                        self.collect_call_targets(eb, targets);
                    }
                    if let Some(eb) = else_body {
                        self.collect_call_targets(eb, targets);
                    }
                }
                Stmt::For { body, .. } => {
                    self.collect_call_targets(body, targets);
                }
                Stmt::Block(stmts) => {
                    self.collect_call_targets(stmts, targets);
                }
                Stmt::FnDef(f) => {
                    self.collect_call_targets(&f.body, targets);
                }
                _ => {}
            }
        }
    }

    // ═══════════════════════════════════════════════
    //  泛型单态化 (SEM-006 + IR-008)
    // ═══════════════════════════════════════════════

    /// 生成单态化函数名：`identity::int`。
    pub fn monomorphize_name(func_name: &str, type_args: &[Type]) -> String {
        if type_args.is_empty() {
            return func_name.to_string();
        }
        let type_suffix: Vec<String> = type_args
            .iter()
            .map(|t| format!("{:?}", t).to_lowercase())
            .collect();
        format!("{}::{}", func_name, type_suffix.join("_"))
    }

    /// 注册单态化并生成新函数副本。
    pub fn register_monomorphization(
        &mut self,
        func_name: &str,
        type_args: &[Type],
        original: &FuncDef,
    ) -> String {
        let key = type_args
            .iter()
            .map(|t| format!("{:?}", t))
            .collect::<Vec<_>>()
            .join(",");
        let map_key = (func_name.to_string(), key.clone());

        if let Some(existing) = self.monomorphizations.get(&map_key) {
            return existing.clone();
        }

        let mono_name = Self::monomorphize_name(func_name, type_args);
        self.monomorphizations.insert(map_key, mono_name.clone());

        // 创建单态化后的函数副本
        let mut mono_func = original.clone();
        mono_func.name = mono_name.clone();

        // 在函数体内替换泛型参数为具体类型
        let param_names: Vec<&str> = original.type_params.iter().map(|s| s.as_str()).collect();

        // 递归替换参数类型
        mono_func.params.iter_mut().for_each(|p| {
            substitute_type_node(&mut p.type_ann, &param_names, type_args);
        });

        // 替换返回类型
        if let Some(ref mut ret) = mono_func.ret_type {
            substitute_type_node(ret, &param_names, type_args);
        }

        // 替换函数体内所有类型标注
        substitute_types_in_stmts(&mut mono_func.body, &param_names, type_args);

        let ret_type = mono_func.ret_type.as_ref().map(resolve_type);
        let mut sym =
            Symbol::new(mono_name.clone(), SymbolKind::Function).with_public(original.is_pub);
        if let Some(rt) = ret_type {
            sym = sym.with_type(rt);
        } else {
            sym = sym.with_type(Type::Void);
        }
        sym = sym.with_func(mono_func);
        let _ = self.symbols.declare(sym);

        mono_name
    }

    /// 从调用参数类型匹配函数的泛型参数。
    /// 对每个函数参数，如果其类型标注是泛型参数（GenericParam 或 Named 匹配 type_params），
    /// 则从对应的实参类型推断。
    fn match_type_params(&self, func_def: &FuncDef, arg_types: &[Type]) -> Vec<Type> {
        let param_set: std::collections::HashSet<&str> =
            func_def.type_params.iter().map(|s| s.as_str()).collect();
        let mut type_map: std::collections::HashMap<String, Type> =
            std::collections::HashMap::new();
        for (i, param) in func_def.params.iter().enumerate() {
            if i >= arg_types.len() {
                break;
            }
            let type_param_name = match &param.type_ann {
                TypeNode::GenericParam(name) => Some(name.clone()),
                TypeNode::Named(name) if param_set.contains(name.as_str()) => Some(name.clone()),
                _ => None,
            };
            if let Some(name) = type_param_name {
                type_map.entry(name).or_insert_with(|| arg_types[i].clone());
            }
        }
        let mut result = Vec::new();
        for param_name in &func_def.type_params {
            if let Some(t) = type_map.get(param_name) {
                result.push(t.clone());
            } else {
                result.push(Type::Any);
            }
        }
        result
    }

    /// 后处理：遍历所有 zone，将泛型调用点的函数名替换为单态化名。
    fn resolve_generic_calls(&mut self) {
        let zone_count = self.zones.len();
        for i in 0..zone_count {
            // 用 take 将 body 移出 self，避免借用冲突
            let mut body = std::mem::take(&mut self.zones[i].body);
            self.rename_generic_calls_in_body(&mut body);
            self.zones[i].body = body;
        }
    }

    /// 递归遍历语句体，替换泛型调用。
    fn rename_generic_calls_in_body(&mut self, body: &mut [Stmt]) {
        // 先收集所有需要改名的调用信息，避免借用冲突
        let mut pending: Vec<PendingCall> = Vec::new();
        for stmt in body.iter() {
            if let Stmt::Call {
                func_name, args, ..
            } = stmt
            {
                let func_def_opt = self
                    .symbols
                    .lookup(func_name)
                    .and_then(|sym| sym.func_def.clone());
                if let Some(func_def) = func_def_opt
                    && !func_def.type_params.is_empty() {
                        let arg_types: Vec<Type> = args
                            .iter()
                            .map(|a| self.type_checker.infer_expr(a, &self.symbols))
                            .collect();
                        pending.push(PendingCall {
                            old_name: func_name.clone(),
                            func_def: *func_def,
                            arg_types,
                        });
                    }
            }
        }
        // 注册单态化并记录改名映射
        let mut call_renames: Vec<(String, String)> = Vec::new();
        for item in &pending {
            let concrete_types = self.match_type_params(&item.func_def, &item.arg_types);
            let mono_name =
                self.register_monomorphization(&item.old_name, &concrete_types, &item.func_def);
            call_renames.push((item.old_name.clone(), mono_name));
        }
        drop(pending); // 释放 pending 以便后续可变借用
        // 执行改名
        for stmt in body.iter_mut() {
            if let Stmt::Call { func_name, .. } = stmt {
                for (old_name, new_name) in &call_renames {
                    if func_name == old_name {
                        *func_name = new_name.clone();
                        break;
                    }
                }
            }
            // 递归进入子语句
            match stmt {
                Stmt::Let { init, .. } | Stmt::Const { init, .. } | Stmt::Goout { init, .. } => {
                    self.rename_generic_calls_in_expr(init);
                }
                Stmt::If {
                    cond,
                    body: b,
                    elifs,
                    else_body,
                    ..
                } => {
                    self.rename_generic_calls_in_expr(cond);
                    self.rename_generic_calls_in_body(b);
                    for (ec, eb) in elifs.iter_mut() {
                        self.rename_generic_calls_in_expr(ec);
                        self.rename_generic_calls_in_body(eb);
                    }
                    if let Some(eb) = else_body {
                        self.rename_generic_calls_in_body(eb);
                    }
                }
                Stmt::For { cond, body: b, .. } => {
                    self.rename_generic_calls_in_expr(cond);
                    self.rename_generic_calls_in_body(b);
                }
                Stmt::Block(b) => {
                    self.rename_generic_calls_in_body(b);
                }
                Stmt::FnDef(f) => {
                    self.rename_generic_calls_in_body(&mut f.body);
                }
                Stmt::Return { value: Some(v) } => {
                    self.rename_generic_calls_in_expr(v);
                }
                Stmt::Return { value: None } => {}
                Stmt::Assert { cond, .. } | Stmt::Raise { expr: cond, .. } => {
                    self.rename_generic_calls_in_expr(cond);
                }
                Stmt::Break { cond: Some(c) } | Stmt::Continue { cond: Some(c) } => {
                    self.rename_generic_calls_in_expr(c);
                }
                Stmt::Break { cond: None } | Stmt::Continue { cond: None } => {}
                Stmt::Wait { overrides, .. } => {
                    for (_, val) in overrides.iter_mut() {
                        self.rename_generic_calls_in_expr(val);
                    }
                }
                _ => {}
            }
        }
    }

    /// 递归遍历表达式中的泛型函数调用。
    fn rename_generic_calls_in_expr(&mut self, expr: &mut Expr) {
        // 先收集需要改名的调用
        let mut renames: Vec<(String, String)> = Vec::new();
        self.collect_generic_calls_in_expr(expr, &mut renames);
        // 执行改名
        for (old_name, new_name) in &renames {
            Self::apply_rename_in_expr(expr, old_name, new_name);
        }
        // 递归进入子表达式
        match expr {
            Expr::Binary { lhs, rhs, .. } => {
                self.rename_generic_calls_in_expr(lhs);
                self.rename_generic_calls_in_expr(rhs);
            }
            Expr::Unary { expr: inner, .. } => {
                self.rename_generic_calls_in_expr(inner);
            }
            Expr::List(items) => {
                for item in items.iter_mut() {
                    self.rename_generic_calls_in_expr(item);
                }
            }
            Expr::Dict(entries) => {
                for (k, v) in entries.iter_mut() {
                    self.rename_generic_calls_in_expr(k);
                    self.rename_generic_calls_in_expr(v);
                }
            }
            Expr::Tuple(items) => {
                for item in items.iter_mut() {
                    self.rename_generic_calls_in_expr(item);
                }
            }
            Expr::Index { target, index } => {
                self.rename_generic_calls_in_expr(target);
                self.rename_generic_calls_in_expr(index);
            }
            Expr::Dot { target, .. } => {
                self.rename_generic_calls_in_expr(target);
            }
            Expr::DoFn { body, .. } => {
                self.rename_generic_calls_in_body(body);
            }
            _ => {}
        }
    }

    /// 收集表达式树中所有泛型调用的改名映射。
    /// 先收集调用信息（不可变借用），再执行注册（可变借用）。
    fn collect_generic_calls_in_expr(&mut self, expr: &Expr, renames: &mut Vec<(String, String)>) {
        // 第一遍：收集所有需要改名的调用
        let mut pending: Vec<PendingCall> = Vec::new();
        self.gather_generic_expr_calls(expr, &mut pending);

        // 第二遍：注册单态化
        for item in &pending {
            let concrete_types = self.match_type_params(&item.func_def, &item.arg_types);
            let mono_name =
                self.register_monomorphization(&item.old_name, &concrete_types, &item.func_def);
            renames.push((item.old_name.clone(), mono_name));
        }
    }

    /// 递归收集表达式中的泛型调用信息。
    fn gather_generic_expr_calls(&mut self, expr: &Expr, pending: &mut Vec<PendingCall>) {
        if let Expr::Call { name, args } = expr {
            let func_def_opt = self
                .symbols
                .lookup(name)
                .and_then(|sym| sym.func_def.clone());
            if let Some(func_def) = func_def_opt
                && !func_def.type_params.is_empty() {
                    let arg_types: Vec<Type> = args
                        .iter()
                        .map(|a| self.type_checker.infer_expr(a, &self.symbols))
                        .collect();
                    pending.push(PendingCall {
                        old_name: name.clone(),
                        func_def: *func_def,
                        arg_types,
                    });
                }
            }
        match expr {
            Expr::Binary { lhs, rhs, .. } => {
                self.gather_generic_expr_calls(lhs, pending);
                self.gather_generic_expr_calls(rhs, pending);
            }
            Expr::Unary { expr: inner, .. } => {
                self.gather_generic_expr_calls(inner, pending);
            }
            Expr::List(items) => {
                for item in items {
                    self.gather_generic_expr_calls(item, pending);
                }
            }
            Expr::Dict(entries) => {
                for (k, v) in entries {
                    self.gather_generic_expr_calls(k, pending);
                    self.gather_generic_expr_calls(v, pending);
                }
            }
            Expr::Tuple(items) => {
                for item in items {
                    self.gather_generic_expr_calls(item, pending);
                }
            }
            Expr::Index { target, index } => {
                self.gather_generic_expr_calls(target, pending);
                self.gather_generic_expr_calls(index, pending);
            }
            Expr::Dot { target, .. } => {
                self.gather_generic_expr_calls(target, pending);
            }
            _ => {}
        }
    }

    /// 在表达式树中替换指定函数名。
    fn apply_rename_in_expr(expr: &mut Expr, old_name: &str, new_name: &str) {
        match expr {
            Expr::Call { name, .. } if name == old_name => {
                *name = new_name.to_string();
            }
            Expr::Binary { lhs, rhs, .. } => {
                Self::apply_rename_in_expr(lhs, old_name, new_name);
                Self::apply_rename_in_expr(rhs, old_name, new_name);
            }
            Expr::Unary { expr: inner, .. } => {
                Self::apply_rename_in_expr(inner, old_name, new_name);
            }
            Expr::List(items) => {
                for item in items {
                    Self::apply_rename_in_expr(item, old_name, new_name);
                }
            }
            Expr::Dict(entries) => {
                for (k, v) in entries {
                    Self::apply_rename_in_expr(k, old_name, new_name);
                    Self::apply_rename_in_expr(v, old_name, new_name);
                }
            }
            Expr::Tuple(items) => {
                for item in items {
                    Self::apply_rename_in_expr(item, old_name, new_name);
                }
            }
            Expr::Index { target, index } => {
                Self::apply_rename_in_expr(target, old_name, new_name);
                Self::apply_rename_in_expr(index, old_name, new_name);
            }
            Expr::Dot { target, .. } => {
                Self::apply_rename_in_expr(target, old_name, new_name);
            }
            _ => {}
        }
    }

    // ═══════════════════════════════════════════════
    //  语句检查
    // ═══════════════════════════════════════════════

    fn check_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    /// 验证 TRY handler 中的异常类型引用。
    fn validate_try_handler(&mut self, handler: &Option<TryHandler>) {
        if let Some(h) = handler {
            match &h.filter {
                TryFilter::IsError(err_type) => {
                    if !self.symbols.contains(err_type) {
                        self.errors.push(SemanticError::new(format!(
                            "TRY 引用的异常类型 `{err_type}` 未定义"
                        )));
                    }
                }
                TryFilter::IsTimeout(_) => {
                    // ISTIMEOUT 不需要额外验证
                }
                TryFilter::All => {}
            }
            // 检查 handler 体中的语句
            self.symbols.push_scope();
            self.check_stmts(&h.body);
            self.symbols.pop_scope();
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
                for item in items {
                    self.validate_expr_refs(item);
                }
            }
            Expr::Dict(entries) => {
                for (k, v) in entries {
                    self.validate_expr_refs(k);
                    self.validate_expr_refs(v);
                }
            }
            Expr::Tuple(items) => {
                for item in items {
                    self.validate_expr_refs(item);
                }
            }
            Expr::Index { target, index } => {
                self.validate_expr_refs(target);
                self.validate_expr_refs(index);
            }
            Expr::Dot { target, .. } => self.validate_expr_refs(target),
            Expr::Call { args, .. } => {
                for arg in args {
                    self.validate_expr_refs(arg);
                }
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
            Stmt::Let {
                name,
                type_ann,
                init,
            } => {
                let ann_type = resolve_type(type_ann);
                let val_type = self.infer_expr_type(init);
                self.type_checker
                    .check_annotation(&ann_type, &val_type, name);
                if let Err(e) = self
                    .symbols
                    .declare(Symbol::new(name.clone(), SymbolKind::Variable).with_type(ann_type))
                {
                    self.errors.push(SemanticError::new(e));
                }
            }

            Stmt::Const {
                name,
                type_ann,
                init,
            } => {
                let ann_type = resolve_type(type_ann);
                let val_type = self.infer_expr_type(init);
                self.type_checker
                    .check_annotation(&ann_type, &val_type, name);
                if let Err(e) = self
                    .symbols
                    .declare(Symbol::new(name.clone(), SymbolKind::Const).with_type(ann_type))
                {
                    self.errors.push(SemanticError::new(e));
                }
            }

            Stmt::Goout {
                name,
                type_ann,
                init,
            } => {
                // GOOUT 只能在 TASK 区使用
                if self.current_zone != Some(ZoneKind::Task) {
                    self.errors
                        .push(SemanticError::new("GOOUT 只能在 TASK 区使用".to_string()));
                }
                let ann_type = resolve_type(type_ann);
                let val_type = self.infer_expr_type(init);
                self.type_checker
                    .check_annotation(&ann_type, &val_type, name);
                if let Err(e) = self.symbols.declare(
                    Symbol::new(name.clone(), SymbolKind::Variable)
                        .with_type(ann_type)
                        .with_goout(true),
                ) {
                    self.errors.push(SemanticError::new(e));
                }
            }

            Stmt::Call {
                func_name,
                args,
                try_handler,
                ..
            } => {
                self.check_call_target_ref(func_name);
                // 跨域引用（如 TOOLS :: helper）去掉域名后查表
                let bare_name = func_name.split("::").last().unwrap_or(func_name);
                if !self.symbols.contains(bare_name) {
                    self.errors
                        .push(SemanticError::new(format!("未定义的函数 `{func_name}`")));
                }
                let _arg_types: Vec<Type> = args.iter().map(|a| self.infer_expr_type(a)).collect();
                // 验证 TRY handler
                self.validate_try_handler(try_handler);
            }

            Stmt::Wait {
                template,
                overrides,
                try_handler,
                ..
            } => {
                self.check_call_target_ref(template);
                let bare_name = template.split("::").last().unwrap_or(template);
                if !self.symbols.contains(bare_name) {
                    self.errors.push(SemanticError::new(format!(
                        "未定义的 WORKS 模板 `{template}`"
                    )));
                }
                for (name, val) in overrides {
                    let _val_type = self.infer_expr_type(val);
                    let _ = name;
                }
                self.validate_try_handler(try_handler);
            }

            Stmt::If {
                cond,
                body,
                elifs,
                else_body,
            } => {
                let cond_type = self.infer_expr_type(cond);
                if cond_type != Type::Bool && cond_type != Type::Any {
                    self.errors.push(SemanticError::new(format!(
                        "IF 条件必须为 bool，得到 {:?}",
                        cond_type
                    )));
                }
                self.symbols.push_scope();
                self.check_stmts(body);
                self.symbols.pop_scope();

                for (elif_cond, elif_body) in elifs {
                    let et = self.infer_expr_type(elif_cond);
                    if et != Type::Bool && et != Type::Any {
                        self.errors.push(SemanticError::new(format!(
                            "ELIF 条件必须为 bool，得到 {:?}",
                            et
                        )));
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
                    self.errors.push(SemanticError::new(format!(
                        "FOR 条件必须为 bool，得到 {:?}",
                        cond_type
                    )));
                }
                self.symbols.push_scope();
                self.check_stmts(body);
                self.symbols.pop_scope();
            }

            Stmt::Break { cond } | Stmt::Continue { cond } => {
                if let Some(c) = cond {
                    let ct = self.infer_expr_type(c);
                    if ct != Type::Bool && ct != Type::Any {
                        self.errors.push(SemanticError::new(format!(
                            "BREAK/CONTINUE 条件必须为 bool，得到 {:?}",
                            ct
                        )));
                    }
                }
            }

            Stmt::Assert { cond, .. } => {
                let ct = self.infer_expr_type(cond);
                if ct != Type::Bool && ct != Type::Any {
                    self.errors.push(SemanticError::new(format!(
                        "ASSERT 条件必须为 bool，得到 {:?}",
                        ct
                    )));
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

// ─── 辅助：单态化收集 ──────────────────────────────────

/// 待处理的泛型函数调用。
struct PendingCall {
    old_name: String,
    func_def: FuncDef,
    arg_types: Vec<Type>,
}

// ─── 辅助：Symbol 的 goout 方法 ────────────────────────

impl Symbol {
    fn with_goout(mut self, goout: bool) -> Self {
        self.is_goout = goout;
        self
    }
}

// ─── 辅助：类型替换（用于泛型单态化） ──────────────────

/// 将语义类型转换为类型节点（用于单态化时的类型替换）。
fn type_node_from_type(t: &Type) -> Box<TypeNode> {
    match t {
        Type::Int => Box::new(TypeNode::Base("int".into())),
        Type::Float => Box::new(TypeNode::Base("float".into())),
        Type::Bool => Box::new(TypeNode::Base("bool".into())),
        Type::Str => Box::new(TypeNode::Base("str".into())),
        Type::Bytes => Box::new(TypeNode::Base("bytes".into())),
        Type::Duration => Box::new(TypeNode::Base("duration".into())),
        Type::List(inner) => Box::new(TypeNode::List(type_node_from_type(inner))),
        Type::Dict(k, v) => Box::new(TypeNode::Dict(
            type_node_from_type(k),
            type_node_from_type(v),
        )),
        Type::Tuple(types) => Box::new(TypeNode::Tuple(
            types.iter().map(|t| *type_node_from_type(t)).collect(),
        )),
        Type::Named(name) => Box::new(TypeNode::Named(name.clone())),
        _ => Box::new(TypeNode::Base("any".into())),
    }
}

/// 递归替换 TypeNode 中的泛型参数引用为具体类型。
/// 注意：Parser 将泛型参数名解析为 TypeNode::Named(name)，而非 GenericParam。
fn substitute_type_node(node: &mut TypeNode, param_names: &[&str], concrete_types: &[Type]) {
    let type_param_name = match node {
        TypeNode::GenericParam(name) => Some(name.clone()),
        TypeNode::Named(name) if param_names.contains(&name.as_str()) => Some(name.clone()),
        _ => None,
    };
    if let Some(name) = type_param_name {
        for (i, p) in param_names.iter().enumerate() {
            if *p == name.as_str() {
                if let Some(ct) = concrete_types.get(i) {
                    *node = *type_node_from_type(ct);
                }
                break;
            }
        }
        return; // replaced, no need to recurse
    }
    match node {
        TypeNode::List(inner) => substitute_type_node(inner, param_names, concrete_types),
        TypeNode::Dict(k, v) => {
            substitute_type_node(k, param_names, concrete_types);
            substitute_type_node(v, param_names, concrete_types);
        }
        TypeNode::Tuple(types) => {
            for t in types.iter_mut() {
                substitute_type_node(t, param_names, concrete_types);
            }
        }
        _ => {}
    }
}

/// 递归遍历语句体，替换所有类型标注中的泛型参数。
fn substitute_types_in_stmts(stmts: &mut [Stmt], param_names: &[&str], concrete_types: &[Type]) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let {
                type_ann, init: _, ..
            }
            | Stmt::Const {
                type_ann, init: _, ..
            }
            | Stmt::Goout {
                type_ann, init: _, ..
            } => {
                substitute_type_node(type_ann, param_names, concrete_types);
            }
            Stmt::If {
                body,
                elifs,
                else_body,
                ..
            } => {
                substitute_types_in_stmts(body, param_names, concrete_types);
                for (_, eb) in elifs.iter_mut() {
                    substitute_types_in_stmts(eb, param_names, concrete_types);
                }
                if let Some(eb) = else_body {
                    substitute_types_in_stmts(eb, param_names, concrete_types);
                }
            }
            Stmt::For { body, .. } => {
                substitute_types_in_stmts(body, param_names, concrete_types);
            }
            Stmt::Block(b) => {
                substitute_types_in_stmts(b, param_names, concrete_types);
            }
            Stmt::FnDef(f) => {
                substitute_types_in_stmts(&mut f.body, param_names, concrete_types);
            }
            _ => {}
        }
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

    // ── ZON-003 GOOUT 语义 ──────────────────────────

    #[test]
    fn goout_in_task_valid() {
        let src = "TASK : { GOOUT result : int = 42 }";
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn goout_out_zone_verify() {
        let src = "TASK : { GOOUT result : int = 42 }
OUT : { CALL result() }";
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn goout_out_zone_not_goout_error() {
        let src = "TASK : { x : int = 42 }
OUT : { CALL x() }";
        let (_, errors) = analyze(src);
        assert!(!errors.is_empty(), "should report not GOOUT error");
    }

    // ── SEM-009 INPUT/OUT 约束 ─────────────────────

    #[test]
    fn input_zone_constant_decl() {
        let src = "INPUT : { data : int = 42 }";
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn input_zone_invalid_stmt_error() {
        let src = "INPUT : { CALL foo() }";
        let (_, errors) = analyze(src);
        assert!(!errors.is_empty(), "should reject control flow in INPUT");
    }

    #[test]
    fn out_zone_goout_reference_valid() {
        let src = "TASK : { GOOUT result : int = 42 }
OUT : { CALL result() }";
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    // ── SYN-009 INPUT/OUT 区 + 装饰器语义验证 ──────

    #[test]
    fn input_zone_source_decl_semantic() {
        let src = r#"TOOLS : { fn gzip() {} }
INPUT : {
    HTTP : "https://api.com/data" [gzip] => RAW : bytes
}"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn input_zone_decorator_undefined_error() {
        let src = r#"INPUT : {
    HTTP : "https://api.com/data" [undefined_deco] => RAW : bytes
}"#;
        let (_, errors) = analyze(src);
        assert!(!errors.is_empty(), "should error on undefined decorator");
    }

    #[test]
    fn out_zone_target_decl_semantic() {
        let src = r#"TOOLS : { fn encrypt() {} }
TASK : { GOOUT data : str = "hello" }
OUT : {
    data [encrypt] => HTTP : "https://api.com/upload"
}"#;
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn out_zone_target_not_goout_error() {
        let src = r#"TASK : { x : int = 42 }
OUT : {
    x => HTTP : "https://api.com/data"
}"#;
        let (_, errors) = analyze(src);
        assert!(!errors.is_empty(), "should error on non-GOOUT variable");
    }

    // ── SEM-012 异常层级与 TRY 校验 ─────────────────

    #[test]
    fn exception_defined() {
        let src = "EXCEPTION IOError
TASK : { x : int = 1 }";
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn exception_parent_undefined_error() {
        let src = "EXCEPTION IOError :: NetworkError";
        let (_, errors) = analyze(src);
        assert!(!errors.is_empty(), "should report undefined parent");
    }

    #[test]
    fn exception_circular_inheritance_error() {
        let src = "EXCEPTION A :: B
EXCEPTION B :: A";
        let (_, errors) = analyze(src);
        assert!(!errors.is_empty(), "should report circular inheritance");
    }

    // ── SEM-011 可达性分析 ─────────────────────────

    #[test]
    fn reachable_function_from_task() {
        let src = "TOOLS : { fn used() { } fn unused() { } }
TASK : { CALL used() }";
        let (_, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    // ── IR-008 泛型单态化 ─────────────────────────

    #[test]
    fn monomorphize_name_basic() {
        let name = SemanticAnalyzer::monomorphize_name("identity", &[Type::Int]);
        assert_eq!(name, "identity::int");
    }

    #[test]
    fn monomorphize_name_multi_param() {
        let name = SemanticAnalyzer::monomorphize_name("pair", &[Type::Int, Type::Str]);
        assert!(name.contains("int"));
        assert!(name.contains("str"));
    }

    #[test]
    fn generic_function_call_monomorphized() {
        let src = r#"TOOLS : {
            fn identity<T>(x : T) : T { x }
        }
        TASK : {
            CALL identity(42) => result
        }"#;
        let (analyzer, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
        // 验证单态化函数已注册
        assert!(analyzer.symbols.contains("identity::int"));
        // 验证调用点已被改名
        let task_zone = analyzer
            .zones
            .iter()
            .find(|z| z.kind == ZoneKind::Task)
            .unwrap();
        let has_mono_call = task_zone
            .body
            .iter()
            .any(|s| matches!(s, Stmt::Call { func_name, .. } if func_name == "identity::int"));
        assert!(
            has_mono_call,
            "call site should be renamed to monomorphized name"
        );
    }

    #[test]
    fn generic_function_unused_not_monomorphized() {
        let src = r#"TOOLS : {
            fn identity<T>(x : T) : T { x }
        }
        TASK : {
            x : int = 42
        }"#;
        let (analyzer, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
        // identity 未被调用，不应该生成 identity::int
        assert!(!analyzer.symbols.contains("identity::int"));
    }

    #[test]
    fn generic_function_multiple_calls_deduplicated() {
        let src = r#"TOOLS : {
            fn identity<T>(x : T) : T { x }
        }
        TASK : {
            CALL identity(42) => a
            CALL identity(99) => b
        }"#;
        let (analyzer, errors) = analyze(src);
        assert!(errors.is_empty(), "{:?}", errors);
        // 两次 int 调用应共享同一个单态化副本
        assert!(analyzer.symbols.contains("identity::int"));
    }
}
