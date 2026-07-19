//! Atomix 递归下降语法分析器 (Parser)。
//!
//! 完整覆盖编译管线.md §3 的所有语法规则。
//! - 递归下降 + Pratt 表达式解析
//! - 恐慌模式错误恢复
//! - 完整的五区结构解析

use crate::compiler::ast::*;
use crate::compiler::token::*;

// ─── 解析错误 ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

// ─── Parser ─────────────────────────────────────────────

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<ParseError>,
    /// 当前所在的区域（用于关键字验证）
    current_zone: Option<ZoneKind>,
    /// 是否在循环体内（用于 break/continue 验证）
    in_loop: bool,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
            current_zone: None,
            in_loop: false,
        }
    }

    /// 消耗所有 Token 并返回 (FileAst, errors)。
    pub fn parse(mut self) -> (FileAst, Vec<ParseError>) {
        let file = self.parse_file();
        (file, self.errors)
    }

    // ── 核心辅助 ──────────────────────────────────

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.peek().map(|t| &t.kind)
    }

    fn advance(&mut self) -> &Token {
        let token = &self.tokens[self.pos];
        self.pos += 1;
        token
    }

    fn expect(&mut self, kind: &TokenKind) -> Option<Span> {
        if self.peek_kind() == Some(kind) {
            let span = self.peek().unwrap().span;
            self.advance();
            Some(span)
        } else {
            let got = self.peek().map(|t| &t.kind);
            let span = self.peek().map(|t| t.span).unwrap_or(Span::new(
                SourcePos::new(0, 0),
                SourcePos::new(0, 0),
            ));
            self.errors.push(ParseError::new(
                format!("期望 {}, 得到 {:?}", kind, got.unwrap_or(&TokenKind::Eof)),
                span,
            ));
            None
        }
    }

    /// 跳过到同步点（遇到 `}` 或特定关键字）。
    fn sync(&mut self) {
        while let Some(t) = self.peek() {
            match &t.kind {
                TokenKind::RBrace
                | TokenKind::Eof
                | TokenKind::Fn
                | TokenKind::If
                | TokenKind::For
                | TokenKind::Call
                | TokenKind::Wait
                | TokenKind::Return
                | TokenKind::Tools
                | TokenKind::Input
                | TokenKind::Works
                | TokenKind::Task
                | TokenKind::Out
                | TokenKind::Test => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    /// 检查当前 Token 是否为语句起始关键字（用于 stop 条件）。
    #[allow(dead_code)]
    fn is_stmt_start(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Fn
                | TokenKind::If
                | TokenKind::For
                | TokenKind::Call
                | TokenKind::Wait
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Assert
                | TokenKind::Raise
                | TokenKind::Goout
                | TokenKind::Const
                | TokenKind::LBrace
                | TokenKind::Ident(_)
                | TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::Str(_)
                | TokenKind::FStr(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Dollar
                | TokenKind::Minus
                | TokenKind::Not
                | TokenKind::Tilde
                | TokenKind::LParen
                | TokenKind::Do
                | TokenKind::Pub
        )
    }

    // ═══════════════════════════════════════════════
    //  顶层文件解析
    // ═══════════════════════════════════════════════

    fn parse_file(&mut self) -> FileAst {
        let mut meta = None;
        let mut use_decls = Vec::new();
        let mut from_decls = Vec::new();
        let mut exception_defs = Vec::new();
        let mut enum_defs = Vec::new();
        let mut type_aliases = Vec::new();
        let mut zones = Vec::new();
        let mut works_defs = Vec::new();
        let mut test_blocks = Vec::new();

        loop {
            match self.peek_kind().cloned() {
                None | Some(TokenKind::Eof) => break,
                Some(TokenKind::MetaBlock(_)) => {
                    if let Some(TokenKind::MetaBlock(content)) =
                        self.advance().kind.clone().into()
                    {
                        meta = Some(MetaBlock { content });
                    }
                }
                Some(TokenKind::Use) => {
                    self.advance();
                    self.expect(&TokenKind::Colon);
                    if let Some(TokenKind::Str(path)) = self.advance().kind.clone().into() {
                        use_decls.push(UseDecl { path });
                    }
                }
                Some(TokenKind::From) => {
                    if let Some(decl) = self.parse_from_decl() {
                        from_decls.push(decl);
                    }
                }
                Some(TokenKind::Exception) => {
                    if let Some(def) = self.parse_exception_def() {
                        exception_defs.push(def);
                    }
                }
                // CONST 允许出现在文件级
                Some(TokenKind::Const) => {
                    // 在文件级，CONST 声明解析为语句并存入 zones（预留给 TASK zone）
                    // 当前直接跳过，后续阶段处理
                    let _const_stmt = self.parse_const();
                }
                Some(TokenKind::Enum) => {
                    if let Some(def) = self.parse_enum_def() {
                        enum_defs.push(def);
                    }
                }
                Some(TokenKind::Type) => {
                    if let Some(alias) = self.parse_type_alias() {
                        type_aliases.push(alias);
                    }
                }
                Some(TokenKind::Test) => {
                    if let Some(block) = self.parse_test_block() {
                        test_blocks.push(block);
                    }
                }
                Some(TokenKind::Tools)
                | Some(TokenKind::Input)
                | Some(TokenKind::Task)
                | Some(TokenKind::Out) => {
                    if let Some(zone) = self.parse_named_zone() {
                        zones.push(zone);
                    }
                }
                Some(TokenKind::Works) => {
                    if let Some(works) = self.parse_works_def() {
                        works_defs.push(works);
                    }
                }
                Some(other) => {
                    let span = self.peek().unwrap().span;
                    self.errors.push(ParseError::new(
                        format!("文件级别意外的 Token: {other}"),
                        span,
                    ));
                    self.sync();
                }
            }
        }

        // 自动收集 WORKS 区域
        for zone in &zones {
            if zone.kind == ZoneKind::Works {
                // 已通过 parse_works_def 处理
            }
        }

        FileAst {
            meta,
            use_decls,
            from_decls,
            exception_defs,
            enum_defs,
            type_aliases,
            zones,
            works_defs,
            test_blocks,
        }
    }

    // ── 文件级定义 ────────────────────────────────

    fn parse_from_decl(&mut self) -> Option<FromDecl> {
        self.advance(); // FROM
        let path = self.parse_str_literal()?;
        self.expect(&TokenKind::Use);
        let target = self.parse_ident()?;
        let alias = if self.peek_kind() == Some(&TokenKind::As) {
            self.advance();
            Some(self.parse_ident()?)
        } else {
            None
        };
        Some(FromDecl { path, target, alias })
    }

    fn parse_exception_def(&mut self) -> Option<ExceptionDef> {
        self.advance(); // EXCEPTION
        let name = self.parse_ident()?;
        let parent = if self.peek_kind() == Some(&TokenKind::DColon) {
            self.advance();
            Some(self.parse_ident()?)
        } else {
            None
        };
        Some(ExceptionDef { name, parent })
    }

    fn parse_enum_def(&mut self) -> Option<EnumDef> {
        self.advance(); // enum
        let name = self.parse_ident()?;
        self.expect(&TokenKind::LBrace);
        let mut variants = Vec::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    self.advance();
                    break;
                }
                Some(TokenKind::Eof) => break,
                _ => {
                    let v_name = self.parse_ident()?;
                    let value = if self.peek_kind() == Some(&TokenKind::Eq) {
                        self.advance();
                        match self.peek_kind() {
                            Some(TokenKind::Int(n)) => {
                                let val = *n;
                                self.advance();
                                Some(val)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    variants.push(EnumVariant {
                        name: v_name,
                        value,
                    });
                    if self.peek_kind() == Some(&TokenKind::Comma) {
                        self.advance();
                    }
                }
            }
        }
        Some(EnumDef { name, variants })
    }

    fn parse_type_alias(&mut self) -> Option<TypeAlias> {
        self.advance(); // type
        let name = self.parse_ident()?;
        let type_params = self.parse_type_params();
        self.expect(&TokenKind::Eq);
        let target = self.parse_type_node()?;
        Some(TypeAlias {
            name,
            type_params,
            target,
        })
    }

    fn parse_test_block(&mut self) -> Option<TestBlock> {
        self.advance(); // TEST
        let name = self.parse_str_literal().unwrap_or_default();
        let body = self.parse_delimited_block();
        Some(TestBlock { name, body })
    }

    // ── 区域解析 ──────────────────────────────────

    fn parse_named_zone(&mut self) -> Option<Zone> {
        let kind = match self.peek_kind()? {
            TokenKind::Tools => ZoneKind::Tools,
            TokenKind::Input => ZoneKind::Input,
            TokenKind::Task => ZoneKind::Task,
            TokenKind::Out => ZoneKind::Out,
            _ => return None,
        };
        self.advance();
        self.current_zone = Some(kind);
        self.expect(&TokenKind::Colon);
        let body = self.parse_delimited_block();
        Some(Zone {
            kind,
            name: None,
            body,
        })
    }

    fn parse_works_def(&mut self) -> Option<WorksDef> {
        self.advance(); // WORKS
        self.current_zone = Some(ZoneKind::Works);
        let name = self.parse_ident()?;

        // 可选父模板
        let parents = if self.peek_kind() == Some(&TokenKind::LParen) {
            self.advance();
            let mut p = Vec::new();
            loop {
                match self.peek_kind() {
                    Some(TokenKind::RParen) => {
                        self.advance();
                        break;
                    }
                    Some(TokenKind::Eof) => break,
                    _ => {
                        p.push(self.parse_ident()?);
                        if self.peek_kind() == Some(&TokenKind::Comma) {
                            self.advance();
                        }
                    }
                }
            }
            p
        } else {
            Vec::new()
        };

        self.expect(&TokenKind::LBrace);
        let mut attrs = Vec::new();
        let mut hooks = Vec::new();
        let mut methods = Vec::new();

        loop {
            match self.peek_kind() {
                Some(TokenKind::RBrace) => {
                    self.advance();
                    break;
                }
                Some(TokenKind::Eof) => break,
                Some(TokenKind::Fn) | Some(TokenKind::Pub) => {
                    if let Some(f) = self.parse_func_def() {
                        methods.push(f);
                    }
                }
                // 属性声明: NAME : TYPE [= default]
                Some(TokenKind::Ident(_)) => {
                    let attr_name = self.parse_ident()?;
                    if self.peek_kind() == Some(&TokenKind::Colon) {
                        self.advance();
                        let type_ann = self.parse_type_node()?;
                        let default = if self.peek_kind() == Some(&TokenKind::Eq) {
                            self.advance();
                            Some(self.parse_expr())
                        } else {
                            None
                        };
                        attrs.push(WorksAttr {
                            name: attr_name,
                            type_ann,
                            default,
                        });
                    }
                    // 钩子链: TRIGGER :: ...
                    // 简化为跳过到下一个 `::` 或行尾
                    else if self.peek_kind() == Some(&TokenKind::DColon) {
                        let trigger = attr_name;
                        let mut chain = Vec::new();
                        loop {
                            match self.peek_kind() {
                                Some(TokenKind::DColon) => {
                                    self.advance();
                                }
                                Some(TokenKind::Ident(_)) => {
                                    let n = self.parse_ident()?;
                                    chain.push(HookStep::Action(n));
                                }
                                _ => break,
                            }
                        }
                        hooks.push(HookChain { trigger, chain });
                    }
                }
                _ => {
                    self.errors.push(ParseError::new(
                        format!("WORKS 体内意外的 Token: {:?}", self.peek_kind()),
                        self.peek().unwrap().span,
                    ));
                    self.sync();
                }
            }
        }

        Some(WorksDef {
            name,
            parents,
            attrs,
            hooks,
            methods,
        })
    }

    // ── 语句解析 ──────────────────────────────────

    fn parse_block_body(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::RBrace) | Some(TokenKind::Eof) => break,
                _ => {
                    let start_pos = self.pos;
                    if let Some(stmt) = self.parse_stmt() {
                        stmts.push(stmt);
                    } else {
                        self.sync();
                        // 防止无限循环：如果 position 没变，强制前进
                        if self.pos == start_pos {
                            self.advance();
                        }
                        if self.peek_kind() == Some(&TokenKind::RBrace) {
                            break;
                        }
                    }
                }
            }
        }
        stmts
    }

    /// 解析 `{ stmts }` 定界块，消费两端的括号。
    fn parse_delimited_block(&mut self) -> Vec<Stmt> {
        self.expect(&TokenKind::LBrace);
        let body = self.parse_block_body();
        self.expect(&TokenKind::RBrace);
        body
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        let kind = self.peek_kind()?.clone();
        match kind {
            // 以关键词开始的语句
            TokenKind::Const => Some(self.parse_const()),
            TokenKind::Goout => Some(self.parse_goout()),
            TokenKind::Call => Some(self.parse_call_wait_stmt(true)),
            TokenKind::Wait => Some(self.parse_call_wait_stmt(false)),
            TokenKind::If => Some(self.parse_if_stmt()),
            TokenKind::For => Some(self.parse_for_stmt()),
            TokenKind::Break => Some(self.parse_break_continue(true)),
            TokenKind::Continue => Some(self.parse_break_continue(false)),
            TokenKind::Assert => Some(self.parse_assert()),
            TokenKind::Raise => Some(self.parse_raise()),
            TokenKind::Return => Some(self.parse_return()),
            TokenKind::LBrace => Some(Stmt::Block(self.parse_delimited_block())),
            // 函数定义（TOOLS/WORKS 内）
            TokenKind::Fn | TokenKind::Pub => {
                self.parse_func_def().map(Stmt::FnDef)
            }
            // 以标识符开始的语句——可能是变量声明或表达式
            TokenKind::Ident(_) => {
                if let Some(stmt) = self.try_parse_ident_stmt() {
                    Some(stmt)
                } else {
                    None
                }
            }
            // 以表达式开始的语句
            _ => {
                if Self::is_expr_start(&kind) {
                    let expr = self.parse_expr();
                    // 检测赋值语句: ident = expr（无类型标注）
                    if let Expr::Ident(name) = &expr {
                        if self.peek_kind() == Some(&TokenKind::Eq) {
                            self.advance();
                            let init = self.parse_expr();
                            return Some(Stmt::Let {
                                name: name.clone(),
                                type_ann: TypeNode::Base("any".into()),
                                init,
                            });
                        }
                    }
                    self.errors.push(ParseError::new(
                        "表达式语句在此上下文中无效——变量声明必须带类型标注 `: type`",
                        self.peek().map(|t| t.span).unwrap(),
                    ));
                    None
                } else {
                    None
                }
            }
        }
    }

    /// 尝试解析以标识符开始的语句（变量声明或表达式）。
    /// 使用窥探法在两个 token 之前判断是声明还是表达式。
    fn try_parse_ident_stmt(&mut self) -> Option<Stmt> {
        // 先窥探第二个 token 来区分声明与表达式
        // 仅当 Ident 后紧跟 `:` 或 `=` 时才作为变量声明
        let is_decl = match self.pos + 1 < self.tokens.len() {
            true => {
                let next = &self.tokens[self.pos + 1].kind;
                matches!(next, TokenKind::Colon | TokenKind::Eq)
            }
            false => false,
        };

        if !is_decl {
            // 不是声明，由表达式解析器处理
            return None;
        }

        // 是变量声明，消费标识符
        let ident = self.parse_ident()?;

        match self.peek_kind() {
            // ident : Type = expr → 变量声明
            Some(TokenKind::Colon) => {
                self.advance();
                let type_ann = self.parse_type_node().unwrap_or(TypeNode::Base("any".into()));
                let init = if self.peek_kind() == Some(&TokenKind::Eq) {
                    self.advance();
                    Some(self.parse_expr())
                } else {
                    None
                };
                Some(Stmt::Let {
                    name: ident,
                    type_ann,
                    init: init.unwrap_or(Expr::Int(0)),
                })
            }
            // ident = expr → 赋值（类型推断为 any）
            Some(TokenKind::Eq) => {
                self.advance();
                let init = self.parse_expr();
                Some(Stmt::Let {
                    name: ident,
                    type_ann: TypeNode::Base("any".into()),
                    init,
                })
            }
            _ => None,
        }
    }

    // ── 具体语句解析 ──────────────────────────────

    #[allow(dead_code)]
    fn parse_let(&mut self, type_required: bool) -> Stmt {
        if type_required {
            self.advance(); // Let
        }
        let name = self.parse_ident().unwrap_or_default();
        let type_ann = if self.peek_kind() == Some(&TokenKind::Colon) {
            self.advance();
            self.parse_type_node().unwrap_or(TypeNode::Base("any".into()))
        } else {
            TypeNode::Base("any".into())
        };
        let init = if self.peek_kind() == Some(&TokenKind::Eq) {
            self.advance();
            self.parse_expr()
        } else {
            Expr::Int(0) // placeholder
        };
        Stmt::Let {
            name,
            type_ann,
            init,
        }
    }

    fn parse_const(&mut self) -> Stmt {
        self.advance(); // CONST
        let name = self.parse_ident().unwrap_or_default();
        self.expect(&TokenKind::Colon);
        let type_ann = self.parse_type_node().unwrap_or(TypeNode::Base("any".into()));
        self.expect(&TokenKind::Eq);
        let init = self.parse_expr();
        Stmt::Const {
            name,
            type_ann,
            init,
        }
    }

    fn parse_goout(&mut self) -> Stmt {
        self.advance(); // GOOUT
        let name = self.parse_ident().unwrap_or_default();
        self.expect(&TokenKind::Colon);
        let type_ann = self.parse_type_node().unwrap_or(TypeNode::Base("any".into()));
        self.expect(&TokenKind::Eq);
        let init = self.parse_expr();
        Stmt::Goout {
            name,
            type_ann,
            init,
        }
    }

    fn parse_call_wait_stmt(&mut self, is_call: bool) -> Stmt {
        self.advance(); // CALL 或 WAIT

        // 检查 CALL 的输入语法: CALL raw = func()
        let input = if is_call && self.peek_kind().map_or(false, |k| matches!(k, TokenKind::Ident(_))) {
            // 可能是 ident = func() 模式
            // 先检查后面是不是 =
            let saved = self.pos;
            let name = self.parse_ident().unwrap_or_default();
            if self.peek_kind() == Some(&TokenKind::Eq) {
                self.advance();
                Some(Box::new(Expr::Ident(name)))
            } else {
                // 恢复 — 是函数名
                self.pos = saved;
                None
            }
        } else {
            None
        };

        // 函数/模板名（可以是标识符或关键字，用于跨域引用）
        let name = self.parse_call_target();

        // CALL/WAIT 共有的参数部分
        let args = if is_call && self.peek_kind() == Some(&TokenKind::LParen) {
            self.parse_call_args()
        } else {
            Vec::new()
        };

        // WAIT 的覆盖参数
        let overrides = if !is_call && self.peek_kind() == Some(&TokenKind::LParen) {
            self.parse_wait_overrides()
        } else {
            Vec::new()
        };

        // 输出 + 管道
        let mut output = None;
        let mut pipe = false;

        // 检查 `=> ident` 或 `=> $`
        if self.peek_kind() == Some(&TokenKind::ArrowR) {
            self.advance();
            match self.peek_kind() {
                Some(TokenKind::Dollar) => {
                    pipe = true;
                    self.advance();
                }
                Some(TokenKind::Ident(_)) => {
                    output = Some(self.parse_ident().unwrap_or_default());
                }
                _ => {
                    self.errors.push(ParseError::new(
                        "`=>` 后需要标识符或 `$`",
                        self.peek().map(|t| t.span).unwrap(),
                    ));
                }
            }
        }
        // 直接 `$` 管道标记（无 `=>`）
        else if self.peek_kind() == Some(&TokenKind::Dollar) {
            pipe = true;
            self.advance();
        }

        // TRY 处理
        let try_handler = if self.peek_kind() == Some(&TokenKind::Try) {
            self.parse_try_handler()
        } else {
            None
        };

        if is_call {
            Stmt::Call {
                input,
                func_name: name,
                args,
                output,
                pipe,
                try_handler,
            }
        } else {
            Stmt::Wait {
                input,
                template: name,
                overrides,
                output,
                pipe,
                try_handler,
            }
        }
    }

    fn parse_call_args(&mut self) -> Vec<Expr> {
        self.advance(); // (
        let mut args = Vec::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::RParen) => {
                    self.advance();
                    break;
                }
                Some(TokenKind::Eof) => break,
                _ => {
                    args.push(self.parse_expr());
                    if self.peek_kind() == Some(&TokenKind::Comma) {
                        self.advance();
                    }
                }
            }
        }
        args
    }

    fn parse_wait_overrides(&mut self) -> Vec<(String, Expr)> {
        self.advance(); // (
        let mut overrides = Vec::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::RParen) => {
                    self.advance();
                    break;
                }
                Some(TokenKind::Eof) => break,
                _ => {
                    let name = self.parse_ident().unwrap_or_default();
                    self.expect(&TokenKind::Eq);
                    let val = self.parse_expr();
                    overrides.push((name, val));
                    if self.peek_kind() == Some(&TokenKind::Comma) {
                        self.advance();
                    }
                }
            }
        }
        overrides
    }

    fn parse_try_handler(&mut self) -> Option<TryHandler> {
        self.advance(); // TRY

        // 判断过滤条件
        let filter = match self.peek_kind() {
            Some(TokenKind::Ident(_)) => {
                let cond = self.parse_ident().unwrap_or_default();
                match cond.to_lowercase().as_str() {
                    "iserror" => {
                        // ISERROR is SomeType
                        self.expect(&TokenKind::Ident("is".into())).unwrap(); // 吃下 is
                        let err_type = self.parse_ident().unwrap_or_default();
                        TryFilter::IsError(err_type)
                    }
                    "istimeout" => {
                        // ISTIMEOUT == value
                        self.expect(&TokenKind::EqEq);
                        let val = self.parse_expr();
                        TryFilter::IsTimeout(val)
                    }
                    _ => TryFilter::All,
                }
            }
            _ => TryFilter::All,
        };

        // TRY 体
        let body = if self.peek_kind() == Some(&TokenKind::LBrace) {
            Some(self.parse_delimited_block())
        } else {
            None
        };

        Some(TryHandler {
            filter,
            body: body.unwrap_or_default(),
        })
    }

    fn parse_if_stmt(&mut self) -> Stmt {
        self.advance(); // IF
        let cond = self.parse_expr();
        let body = self.parse_delimited_block();

        let mut elifs = Vec::new();
        let mut else_body = None;

        loop {
            match self.peek_kind() {
                Some(TokenKind::Elif) => {
                    self.advance();
                    let elif_cond = self.parse_expr();
                    let elif_body = self.parse_delimited_block();
                    elifs.push((elif_cond, elif_body));
                }
                Some(TokenKind::Else) => {
                    self.advance();
                    else_body = Some(self.parse_delimited_block());
                    break;
                }
                _ => break,
            }
        }

        Stmt::If {
            cond,
            body,
            elifs,
            else_body,
        }
    }

    fn parse_for_stmt(&mut self) -> Stmt {
        self.advance(); // FOR
        let cond = self.parse_expr();
        let was_in_loop = self.in_loop;
        self.in_loop = true;
        let body = self.parse_delimited_block();
        self.in_loop = was_in_loop;
        Stmt::For { cond, body }
    }

    fn parse_break_continue(&mut self, is_break: bool) -> Stmt {
        self.advance(); // BREAK / CONTINUE
        if !self.in_loop {
            self.errors.push(ParseError::new(
                if is_break {
                    "`BREAK` 只能在循环体内使用"
                } else {
                    "`CONTINUE` 只能在循环体内使用"
                },
                self.peek().map(|t| t.span).unwrap(),
            ));
        }
        let cond = if self.peek_kind().map_or(false, |k| {
            Self::is_expr_start(k)
        }) {
            Some(self.parse_expr())
        } else {
            None
        };
        if is_break {
            Stmt::Break { cond }
        } else {
            Stmt::Continue { cond }
        }
    }

    fn parse_assert(&mut self) -> Stmt {
        self.advance(); // ASSERT
        let cond = self.parse_expr();
        let msg = if self.peek_kind() == Some(&TokenKind::Comma) {
            self.advance();
            match self.peek_kind() {
                Some(TokenKind::Str(s)) => {
                    let s = s.clone();
                    self.advance();
                    Some(s)
                }
                _ => None,
            }
        } else {
            None
        };
        Stmt::Assert { cond, msg }
    }

    fn parse_raise(&mut self) -> Stmt {
        self.advance(); // RAISE
        let expr = self.parse_expr();
        let msg = if self.peek_kind() == Some(&TokenKind::Comma) {
            self.advance();
            match self.peek_kind() {
                Some(TokenKind::Str(s)) => {
                    let s = s.clone();
                    self.advance();
                    Some(s)
                }
                _ => None,
            }
        } else {
            None
        };
        Stmt::Raise { expr, msg }
    }

    fn parse_return(&mut self) -> Stmt {
        self.advance(); // return
        let value = if self.peek_kind().map_or(false, |k| {
            Self::is_expr_start(k)
        }) {
            Some(self.parse_expr())
        } else {
            None
        };
        Stmt::Return { value }
    }

    // ── 函数定义 ──────────────────────────────────

    fn parse_func_def(&mut self) -> Option<FuncDef> {
        let is_pub = if self.peek_kind() == Some(&TokenKind::Pub) {
            self.advance();
            true
        } else {
            false
        };

        self.expect(&TokenKind::Fn)?;
        let name = self.parse_ident()?;
        let type_params = self.parse_type_params();
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_func_params();
        self.expect(&TokenKind::RParen)?;
        let ret_type = if self.peek_kind() == Some(&TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_node().unwrap_or(TypeNode::Base("any".into())))
        } else {
            None
        };
        let body = self.parse_delimited_block();
        Some(FuncDef {
            name,
            is_pub,
            type_params,
            params,
            ret_type,
            body,
        })
    }

    fn parse_func_params(&mut self) -> Vec<FuncParam> {
        let mut params = Vec::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::RParen) | Some(TokenKind::Eof) => break,
                _ => {
                    let name = self.parse_ident().unwrap_or_default();
                    let type_ann = if self.peek_kind() == Some(&TokenKind::Colon) {
                        self.advance();
                        self.parse_type_node().unwrap_or(TypeNode::Base("any".into()))
                    } else {
                        TypeNode::Base("any".into())
                    };
                    let default = if self.peek_kind() == Some(&TokenKind::Eq) {
                        self.advance();
                        Some(self.parse_expr())
                    } else {
                        None
                    };
                    params.push(FuncParam {
                        name,
                        type_ann,
                        default,
                    });
                    if self.peek_kind() == Some(&TokenKind::Comma) {
                        self.advance();
                    }
                }
            }
        }
        params
    }

    fn parse_type_params(&mut self) -> Vec<String> {
        if self.peek_kind() == Some(&TokenKind::Lt) {
            self.advance();
            let mut params = Vec::new();
            loop {
                match self.peek_kind() {
                    Some(TokenKind::Gt) | Some(TokenKind::Shr) => {
                        self.advance();
                        break;
                    }
                    Some(TokenKind::Eof) => break,
                    _ => {
                        params.push(self.parse_ident().unwrap_or_default());
                        if self.peek_kind() == Some(&TokenKind::Comma) {
                            self.advance();
                        }
                    }
                }
            }
            params
        } else {
            Vec::new()
        }
    }

    // ═══════════════════════════════════════════════
    //  表达式解析 (Pratt/Precedence Climbing)
    // ═══════════════════════════════════════════════

    fn is_expr_start(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Ident(_)
                | TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::Str(_)
                | TokenKind::FStr(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Dollar
                | TokenKind::Minus
                | TokenKind::Not
                | TokenKind::Tilde
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::Do
                | TokenKind::Fn
        ) || kind.is_zone_keyword()
            || kind.is_source_target_keyword()
    }

    fn parse_expr(&mut self) -> Expr {
        self.parse_expr_bp(0)
    }

    /// Pratt 解析的核心——绑定能力 (Binding Power) 方法。
    fn parse_expr_bp(&mut self, min_bp: u8) -> Expr {
        let mut lhs = self.parse_prefix();

        loop {
            // 检查是否是语句/表达式结束符
            match self.peek_kind() {
                None | Some(TokenKind::Eof)
                | Some(TokenKind::RBrace)
                | Some(TokenKind::RParen)
                | Some(TokenKind::RBracket)
                | Some(TokenKind::Comma)
                | Some(TokenKind::Colon)
                | Some(TokenKind::ArrowR)
                | Some(TokenKind::ArrowL) => break,
                _ => {}
            }

            if let Some((l_bp, r_bp)) = self.infix_binding_power() {
                if l_bp < min_bp {
                    break;
                }
                // 获取运算符种类（consume 之前记录）
                let op_kind = self.peek_kind().cloned();
                self.advance(); // consume operator token
                let op = self.token_to_bin_op(op_kind);
                let rhs = self.parse_expr_bp(r_bp);
                lhs = Expr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };
            } else {
                break;
            }
        }

        lhs
    }

    /// 前缀表达式（原子表达式 + 一元运算符）。
    fn parse_prefix(&mut self) -> Expr {
        match self.peek_kind().cloned() {
            Some(TokenKind::Int(n)) => {
                self.advance();
                Expr::Int(n)
            }
            Some(TokenKind::Float(n)) => {
                self.advance();
                Expr::Float(n)
            }
            Some(TokenKind::Str(s)) => {
                self.advance();
                Expr::Str(s)
            }
            Some(TokenKind::FStr(parts)) => {
                self.advance();
                // F-字符串中的插值表达式暂存为 Ident，后续由语义分析处理
                let fragments: Vec<FStringFragment> = parts
                    .into_iter()
                    .map(|part| match part {
                        FStringPart::Text(t) => FStringFragment::Text(t),
                        FStringPart::Interp(_) => {
                            FStringFragment::Text("{expr}".into())
                        }
                    })
                    .collect();
                Expr::FStr(fragments)
            }
            Some(TokenKind::True) => {
                self.advance();
                Expr::Bool(true)
            }
            Some(TokenKind::False) => {
                self.advance();
                Expr::Bool(false)
            }
            Some(TokenKind::Dollar) => {
                self.advance();
                if self.peek_kind() == Some(&TokenKind::LBracket) {
                    self.advance();
                    let key = match self.peek_kind() {
                        Some(TokenKind::Str(s)) => s.clone(),
                        Some(TokenKind::Ident(s)) => s.clone(),
                        _ => String::new(),
                    };
                    self.advance(); // key
                    self.expect(&TokenKind::RBracket);
                    Expr::DollarKey(key)
                } else {
                    Expr::Dollar
                }
            }
            Some(TokenKind::Minus) => {
                self.advance();
                let expr = self.parse_expr_bp(9); // prefix binding power
                Expr::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(expr),
                }
            }
            Some(TokenKind::Not) => {
                self.advance();
                let expr = self.parse_expr_bp(9);
                Expr::Unary {
                    op: UnOp::Not,
                    expr: Box::new(expr),
                }
            }
            Some(TokenKind::Tilde) => {
                self.advance();
                let expr = self.parse_expr_bp(9);
                Expr::Unary {
                    op: UnOp::BitNot,
                    expr: Box::new(expr),
                }
            }
            Some(TokenKind::LParen) => {
                self.advance();
                let mut exprs = Vec::new();
                loop {
                    match self.peek_kind() {
                        Some(TokenKind::RParen) => {
                            self.advance();
                            break;
                        }
                        Some(TokenKind::Eof) => break,
                        _ => {
                            exprs.push(self.parse_expr());
                            if self.peek_kind() == Some(&TokenKind::Comma) {
                                self.advance();
                            }
                        }
                    }
                }
                if exprs.len() == 1 {
                    exprs.into_iter().next().unwrap()
                } else {
                    Expr::Tuple(exprs)
                }
            }
            Some(TokenKind::LBracket) => {
                self.advance();
                let mut exprs = Vec::new();
                loop {
                    match self.peek_kind() {
                        Some(TokenKind::RBracket) => {
                            self.advance();
                            break;
                        }
                        Some(TokenKind::Eof) => break,
                        _ => {
                            exprs.push(self.parse_expr());
                            if self.peek_kind() == Some(&TokenKind::Comma) {
                                self.advance();
                            }
                        }
                    }
                }
                Expr::List(exprs)
            }
            // 字典字面量 {key: val, ...}
            Some(TokenKind::LBrace) => {
                self.advance();
                let mut entries = Vec::new();
                loop {
                    match self.peek_kind() {
                        Some(TokenKind::RBrace) => {
                            self.advance();
                            break;
                        }
                        Some(TokenKind::Eof) => break,
                        _ => {
                            let key = self.parse_expr();
                            self.expect(&TokenKind::Colon);
                            let val = self.parse_expr();
                            entries.push((key, val));
                            if self.peek_kind() == Some(&TokenKind::Comma) {
                                self.advance();
                            }
                        }
                    }
                }
                Expr::Dict(entries)
            }
            Some(TokenKind::Do) => {
                self.advance();
                self.expect(&TokenKind::LParen);
                let params = self.parse_func_params();
                self.expect(&TokenKind::RParen);
                let ret_type = if self.peek_kind() == Some(&TokenKind::Colon) {
                    self.advance();
                    Some(self.parse_type_node().unwrap_or(TypeNode::Base("any".into())))
                } else {
                    None
                };
                let body = self.parse_delimited_block();
                Expr::DoFn {
                    params,
                    ret_type,
                    body,
                }
            }
            // 标识符或关键字（用于跨域引用）
            Some(TokenKind::Ident(name)) => {
                self.advance(); // 必须消费 token，否则表达式会重复读取
                self.parse_ident_or_cross_ref(name)
            }
            // 关键字也可作为跨域引用的域名
            Some(kind) if kind.is_zone_keyword()
                || kind.is_source_target_keyword()
                || matches!(kind, TokenKind::Exception | TokenKind::Enum | TokenKind::Type) =>
            {
                let name = format!("{}", kind);
                self.advance();
                self.parse_ident_or_cross_ref(name)
            }
            // 无法识别的起始
            Some(other) => {
                self.errors.push(ParseError::new(
                    format!("意外的表达式起始: {other}"),
                    self.peek().map(|t| t.span).unwrap(),
                ));
                self.advance();
                Expr::Int(0) // placeholder
            }
            None => Expr::Int(0),
        }
    }

    /// 中缀运算符的绑定能力 (left_bp, right_bp)。
    fn infix_binding_power(&self) -> Option<(u8, u8)> {
        let kind = self.peek_kind()?;
        let (l_bp, r_bp) = match kind {
            // 优先级从低到高
            TokenKind::And => (1, 2),
            TokenKind::Or => (1, 2),
            // 比较
            TokenKind::EqEq => (2, 3),
            TokenKind::Neq => (2, 3),
            TokenKind::Lt => (2, 3),
            TokenKind::Gt => (2, 3),
            TokenKind::ArrowL => (2, 3), // <= 在表达式上下文中是比较符
            TokenKind::Ge => (2, 3),
            // 位或
            TokenKind::Pipe => (3, 4),
            // 位异或
            TokenKind::Caret => (4, 5),
            // 位与
            TokenKind::Amp => (5, 6),
            // 移位
            TokenKind::Shl => (6, 7),
            TokenKind::Shr => (6, 7),
            // 加减
            TokenKind::Plus => (7, 8),
            TokenKind::Minus => (7, 8),
            // 乘除模
            TokenKind::Star => (8, 9),
            TokenKind::Slash => (8, 9),
            TokenKind::Percent => (8, 9),
            // 以下不是中缀运算符
            _ => return None,
        };
        Some((l_bp, r_bp))
    }

    fn token_to_bin_op(&self, kind: Option<TokenKind>) -> BinOp {
        match kind {
            Some(TokenKind::Plus) => BinOp::Add,
            Some(TokenKind::Minus) => BinOp::Sub,
            Some(TokenKind::Star) => BinOp::Mul,
            Some(TokenKind::Slash) => BinOp::Div,
            Some(TokenKind::Percent) => BinOp::Mod,
            Some(TokenKind::EqEq) => BinOp::Eq,
            Some(TokenKind::Neq) => BinOp::Ne,
            Some(TokenKind::Lt) => BinOp::Lt,
            Some(TokenKind::Gt) => BinOp::Gt,
            Some(TokenKind::ArrowL) => BinOp::Le,
            Some(TokenKind::Ge) => BinOp::Ge,
            Some(TokenKind::And) => BinOp::And,
            Some(TokenKind::Or) => BinOp::Or,
            Some(TokenKind::Amp) => BinOp::BitAnd,
            Some(TokenKind::Pipe) => BinOp::BitOr,
            Some(TokenKind::Caret) => BinOp::BitXor,
            Some(TokenKind::Shl) => BinOp::Shl,
            Some(TokenKind::Shr) => BinOp::Shr,
            _ => BinOp::Add, // fallback
        }
    }

    // ═══════════════════════════════════════════════
    //  类型节点解析
    // ═══════════════════════════════════════════════

    fn parse_type_node(&mut self) -> Option<TypeNode> {
        match self.peek_kind()? {
            TokenKind::IntTy => {
                self.advance();
                Some(TypeNode::Base("int".into()))
            }
            TokenKind::FloatTy => {
                self.advance();
                Some(TypeNode::Base("float".into()))
            }
            TokenKind::BoolTy => {
                self.advance();
                Some(TypeNode::Base("bool".into()))
            }
            TokenKind::StrTy => {
                self.advance();
                Some(TypeNode::Base("str".into()))
            }
            TokenKind::BytesTy => {
                self.advance();
                Some(TypeNode::Base("bytes".into()))
            }
            TokenKind::ListTy => {
                self.advance();
                self.expect(&TokenKind::LBracket)?;
                let inner = self.parse_type_node()?;
                self.expect(&TokenKind::RBracket)?;
                Some(TypeNode::List(Box::new(inner)))
            }
            TokenKind::DictTy => {
                self.advance();
                self.expect(&TokenKind::LBracket)?;
                let k = self.parse_type_node()?;
                self.expect(&TokenKind::Comma)?;
                let v = self.parse_type_node()?;
                self.expect(&TokenKind::RBracket)?;
                Some(TypeNode::Dict(Box::new(k), Box::new(v)))
            }
            TokenKind::TupleTy => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let mut types = Vec::new();
                loop {
                    match self.peek_kind() {
                        Some(TokenKind::RParen) => {
                            self.advance();
                            break;
                        }
                        Some(TokenKind::Eof) => break,
                        _ => {
                            types.push(self.parse_type_node()?);
                            if self.peek_kind() == Some(&TokenKind::Comma) {
                                self.advance();
                            }
                        }
                    }
                }
                Some(TypeNode::Tuple(types))
            }
            TokenKind::Ident(_) => {
                let name = self.parse_ident()?;
                Some(TypeNode::Named(name))
            }
            _ => {
                self.errors.push(ParseError::new(
                    format!("期望类型标注, 得到 {:?}", self.peek_kind()),
                    self.peek().map(|t| t.span).unwrap(),
                ));
                None
            }
        }
    }

    // ── 辅助解析 ──────────────────────────────────

    fn parse_ident(&mut self) -> Option<String> {
        match self.peek_kind()? {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Some(s)
            }
            _ => {
                let span = self.peek().map(|t| t.span).unwrap();
                self.errors.push(ParseError::new(
                    format!("期望标识符, 得到 {:?}", self.peek_kind()),
                    span,
                ));
                None
            }
        }
    }

    /// 解析 CALL/WAIT 目标名称（标识符或关键字，用于 `TOOLS::func` 跨域引用）。
    fn parse_call_target(&mut self) -> String {
        // 关键字也可作为跨域引用中的域名
        let is_keyword_domain = |kind: &TokenKind| -> bool {
            kind.is_zone_keyword()
                || kind.is_source_target_keyword()
                || matches!(kind, TokenKind::Exception | TokenKind::Enum | TokenKind::Type)
        };

        let name = match self.peek_kind() {
            Some(TokenKind::Ident(_)) => self.parse_ident().unwrap_or_default(),
            Some(kind) if is_keyword_domain(kind) => {
                let s = kind.to_string().to_lowercase();
                self.advance();
                s
            }
            _ => {
                self.errors.push(ParseError::new(
                    format!("期望标识符或跨域引用域名, 得到 {:?}", self.peek_kind()),
                    self.peek().map(|t| t.span).unwrap(),
                ));
                String::new()
            }
        };
        // 检查是否为跨域引用 `DOMAIN :: name`
        if self.peek_kind() == Some(&TokenKind::DColon) {
            self.advance();
            match self.peek_kind() {
                Some(TokenKind::Ident(_)) => {
                    let target = self.parse_ident().unwrap_or_default();
                    format!("{}::{}", name, target)
                }
                _ => name,
            }
        } else {
            name
        }
    }

    /// 解析标识符或跨域引用 `DOMAIN :: name`。
    fn parse_ident_or_cross_ref(&mut self, name: String) -> Expr {
        if self.peek_kind() == Some(&TokenKind::DColon) {
            self.advance();
            let target = self.parse_ident().unwrap_or_default();
            Expr::CrossRef {
                domain: name,
                name: target,
            }
        } else {
            Expr::Ident(name)
        }
    }

    fn parse_str_literal(&mut self) -> Option<String> {
        match self.peek_kind()? {
            TokenKind::Str(s) => {
                let s = s.clone();
                self.advance();
                Some(s)
            }
            _ => {
                let span = self.peek().map(|t| t.span).unwrap();
                self.errors.push(ParseError::new(
                    format!("期望字符串字面量, 得到 {:?}", self.peek_kind()),
                    span,
                ));
                None
            }
        }
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;

    fn parse(source: &str) -> (FileAst, Vec<ParseError>) {
        let (tokens, lex_errors) = Lexer::new(source).tokenize();
        assert!(lex_errors.is_empty(), "lex errors: {:?}", lex_errors);
        Parser::new(tokens).parse()
    }

    #[test]
    fn empty_tools_zone() {
        let (ast, errors) = parse("TOOLS : {}");
        assert!(errors.is_empty(), "{:?}", errors);
        assert_eq!(ast.zones.len(), 1);
        assert_eq!(ast.zones[0].kind, ZoneKind::Tools);
    }

    #[test]
    fn all_zones_empty() {
        let src = "TOOLS : {} INPUT : {} TASK : {} OUT : {}";
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
        assert_eq!(ast.zones.len(), 4);
    }

    #[test]
    fn variable_declaration() {
        let src = "TOOLS : { fn foo() { x : int = 42 } }";
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
        assert_eq!(ast.zones.len(), 1);
    }

    #[test]
    fn if_elif_else() {
        let src = r#"TASK : {
            IF a > 10 {
                CALL foo()
            } ELIF a > 5 {
                CALL bar()
            } ELSE {
                CALL baz()
            }
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn for_loop() {
        let src = r#"TASK : {
            FOR i < 10 {
                CALL process(i)
                BREAK i > 8
                CONTINUE i == 5
            }
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn call_with_input_output() {
        let src = r#"TASK : {
            CALL process(a, b) => result
            CALL raw = fetch() => output
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn wait_statement() {
        let src = r#"TASK : {
            WAIT DataProcessor (RAW = input_data) => result
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn cross_ref() {
        let src = r#"TASK : {
            CALL TOOLS :: compress(data)
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn expression_precedence() {
        let src = r#"TASK : {
            x : int = 2 + 3 * 4
            y : bool = 1 < 2 and true
            z : int = not true and false
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn dollar_pipe() {
        let src = r#"TASK : {
            CALL classify(orders[0]) $
            x : int = $["ok"]
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn use_and_const() {
        let src = r#"USE : "std/csv"
        USE : "std/validate"

        CONST LIMIT : int = 5000
        CONST TIMEOUT : int = 3000

        TASK : {
            CALL process()
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
        assert_eq!(ast.use_decls.len(), 2);
    }

    #[test]
    fn function_definition() {
        let src = r#"TOOLS : {
            fn add(x : int, y : int) : int {
                x + y
            }
            pub fn greet(name : str) {
                CALL log(name)
            }
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn assert_and_raise() {
        let src = r#"TASK : {
            ASSERT x > 0, "x must be positive"
            RAISE SomeError, "something went wrong"
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn goout_declaration() {
        let src = r#"TASK : {
            GOOUT status : str = "ok"
            GOOUT total : int = 0
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn type_annotations() {
        let src = r#"TOOLS : {
            fn foo() {
                a : int = 1
                b : float = 3.14
                c : str = "hello"
                d : bool = true
                e : list[int] = [1, 2, 3]
                f : dict[str, int] = {"a": 1}
                g : tuple(int, bool) = (1, true)
            }
        }"#;
        let (ast, errors) = parse(src);
        assert!(errors.is_empty(), "{:?}", errors);
    }
}
