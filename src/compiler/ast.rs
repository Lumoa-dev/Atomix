//! Atomix 抽象语法树 (AST) 节点定义。
//!
//! 完整覆盖编译管线.md §3.2 的全部节点类型。
//! **AST 节点不存储类型信息**——类型标注和推导结果在语义分析阶段
//! 填充到符号表和单独的类型映射结构中。

// ─── 类型标注 ──────────────────────────────────────────

/// 类型标注（语法层面）。
#[derive(Debug, Clone, PartialEq)]
pub enum TypeNode {
    /// int / float / bool / str / bytes
    Base(String),
    /// list[T]
    List(Box<TypeNode>),
    /// dict[K, V]
    Dict(Box<TypeNode>, Box<TypeNode>),
    /// tuple(T1, T2, ...)
    Tuple(Vec<TypeNode>),
    /// 枚举名 / 类型别名引用
    Named(String),
    /// 泛型参数名（如 `T`）
    GenericParam(String),
}

// ─── 表达式 ────────────────────────────────────────────

/// 表达式节点。
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// 二元运算：lhs op rhs
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// 一元运算：op expr
    Unary { op: UnOp, expr: Box<Expr> },
    /// 标识符引用
    Ident(String),
    /// 整数字面量
    Int(i64),
    /// 浮点字面量
    Float(f64),
    /// 字符串字面量
    Str(String),
    /// F-字符串字面量
    FStr(Vec<FStringFragment>),
    /// 布尔字面量
    Bool(bool),
    /// 列表字面量 [expr, ...]
    List(Vec<Expr>),
    /// 字典字面量 {key: val, ...}
    Dict(Vec<(Expr, Expr)>),
    /// 元组字面量 (expr, ...)
    Tuple(Vec<Expr>),
    /// 索引/下标访问 expr[index]
    Index { target: Box<Expr>, index: Box<Expr> },
    /// 字段访问 expr.field
    Dot { target: Box<Expr>, field: String },
    /// `$` 管道变量
    Dollar,
    /// `$[key]` 管道变量属性
    DollarKey(String),
    /// 跨域引用 `DOMAIN :: name`
    CrossRef { domain: String, name: String },
    /// 匿名函数 `do (params) [: ret] { body }`
    DoFn {
        params: Vec<FuncParam>,
        ret_type: Option<TypeNode>,
        body: Vec<Stmt>,
    },
    /// 函数调用（表达式上下文）
    Call { name: String, args: Vec<Expr> },
}

/// F-字符串片段。
#[derive(Debug, Clone, PartialEq)]
pub enum FStringFragment {
    Text(String),
    Interp(Expr),
}

/// 二元运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,    // +
    Sub,    // -
    Mul,    // *
    Div,    // /
    Mod,    // %
    And,    // and
    Or,     // or
    Eq,     // ==
    Ne,     // !=
    Lt,     // <
    Gt,     // >
    Le,     // <= (比较上下文)
    Ge,     // >=
    BitAnd, // &
    BitOr,  // |
    BitXor, // ^
    Shl,    // <<
    Shr,    // >>
}

/// 一元运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,    // -
    Not,    // not
    BitNot, // ~
}

// ─── 函数参数 ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct FuncParam {
    pub name: String,
    pub type_ann: TypeNode,
    pub default: Option<Expr>,
}

// ─── 语句 ──────────────────────────────────────────────

/// 语句节点。
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// 变量声明：x : Type = expr
    Let {
        name: String,
        type_ann: TypeNode,
        init: Expr,
    },
    /// 常量声明：CONST x : Type = expr
    Const {
        name: String,
        type_ann: TypeNode,
        init: Expr,
    },
    /// GOOUT 产出声明：GOOUT x : Type = expr
    Goout {
        name: String,
        type_ann: TypeNode,
        init: Expr,
    },
    /// CALL 语句（含 TRY 处理）
    Call {
        input: Option<Box<Expr>>,
        func_name: String,
        args: Vec<Expr>,
        output: Option<String>,
        /// 是否触发 `$` 管道模式
        pipe: bool,
        try_handler: Option<TryHandler>,
    },
    /// WAIT 语句
    Wait {
        input: Option<Box<Expr>>,
        template: String,
        overrides: Vec<(String, Expr)>,
        output: Option<String>,
        pipe: bool,
        try_handler: Option<TryHandler>,
    },
    /// IF 条件分支
    If {
        cond: Expr,
        body: Vec<Stmt>,
        elifs: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    /// FOR 循环
    For { cond: Expr, body: Vec<Stmt> },
    /// BREAK [cond]
    Break { cond: Option<Expr> },
    /// CONTINUE [cond]
    Continue { cond: Option<Expr> },
    /// ASSERT expr [, msg]
    Assert { cond: Expr, msg: Option<String> },
    /// RAISE expr [, msg]
    Raise { expr: Expr, msg: Option<String> },
    /// return [expr]
    Return { value: Option<Expr> },
    /// 语句块 { stmt* }
    Block(Vec<Stmt>),
    /// 函数定义（TOOLS/WORKS 中出现在语句位置）
    FnDef(FuncDef),
}

/// TRY 异常处理器。
#[derive(Debug, Clone, PartialEq)]
pub struct TryHandler {
    /// 过滤条件类型
    pub filter: TryFilter,
    /// 处理器体
    pub body: Vec<Stmt>,
}

/// TRY 过滤条件。
#[derive(Debug, Clone, PartialEq)]
pub enum TryFilter {
    /// 捕获全部（无 ISERROR/ISTIMEOUT）
    All,
    /// 按异常类型匹配：ISERROR is SomeError
    IsError(String),
    /// 按超时匹配：ISTIMEOUT == duration
    IsTimeout(Expr),
}

// ─── 函数定义 ──────────────────────────────────────────

/// 函数定义（TOOLS/WORKS 中）。
#[derive(Debug, Clone, PartialEq)]
pub struct FuncDef {
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<String>,
    pub params: Vec<FuncParam>,
    pub ret_type: Option<TypeNode>,
    pub body: Vec<Stmt>,
}

// ─── 顶层节点 ──────────────────────────────────────────

/// 元信息块。
#[derive(Debug, Clone, PartialEq)]
pub struct MetaBlock {
    pub content: String,
}

/// USE 声明。
#[derive(Debug, Clone, PartialEq)]
pub struct UseDecl {
    pub path: String,
}

/// FROM 声明。
#[derive(Debug, Clone, PartialEq)]
pub struct FromDecl {
    pub path: String,
    pub target: String,
    pub alias: Option<String>,
}

/// EXCEPTION 定义。
#[derive(Debug, Clone, PartialEq)]
pub struct ExceptionDef {
    pub name: String,
    pub parent: Option<String>,
}

/// Enum 变体。
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub value: Option<i64>,
}

/// Enum 定义。
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

/// 类型别名定义。
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAlias {
    pub name: String,
    pub type_params: Vec<String>,
    pub target: TypeNode,
}

// ─── 区域节点 ──────────────────────────────────────────

/// 区域类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZoneKind {
    Tools,
    Input,
    Works,
    Task,
    Out,
}

/// 区域节点。
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub kind: ZoneKind,
    /// WORKS 名称（仅 ZoneKind::Works 时有）
    pub name: Option<String>,
    /// 区域体语句（TOOLS/WORKS/TASK/OUT 区的主要语句）
    pub body: Vec<Stmt>,
    /// INPUT 区的数据源声明
    pub source_decls: Vec<SourceDecl>,
    /// OUT 区的数据交付声明
    pub target_decls: Vec<TargetDecl>,
}

// ─── WORKS 模板 ────────────────────────────────────────

/// WORKS 属性声明。
#[derive(Debug, Clone, PartialEq)]
pub struct WorksAttr {
    pub name: String,
    pub type_ann: TypeNode,
    pub default: Option<Expr>,
}

/// WORKS 钩子链。
#[derive(Debug, Clone, PartialEq)]
pub struct HookChain {
    pub trigger: String,
    pub chain: Vec<HookStep>,
}

/// 钩子步骤：条件或动作。
#[derive(Debug, Clone, PartialEq)]
pub enum HookStep {
    Condition(Expr),
    Action(String),
}

/// WORKS 模板定义。
#[derive(Debug, Clone, PartialEq)]
pub struct WorksDef {
    pub name: String,
    pub parents: Vec<String>,
    pub attrs: Vec<WorksAttr>,
    pub hooks: Vec<HookChain>,
    pub methods: Vec<FuncDef>,
}

// ─── 数据源声明（INPUT 区） ─────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct SourceDecl {
    pub source_kind: String, // HTTP, FILES, JSON, ...
    pub address: String,
    pub params: Vec<(String, String)>,
    pub decorators: Vec<String>,
    pub target: Option<SourceTarget>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceTarget {
    pub arrow: ArrowKind,
    pub var_name: String,
    pub type_ann: Option<TypeNode>,
}

/// 数据交付声明（OUT 区）。
#[derive(Debug, Clone, PartialEq)]
pub struct TargetDecl {
    pub source_var: String,
    pub decorators: Vec<String>,
    pub target_kind: String,
    pub address: String,
    pub params: Vec<(String, String)>,
}

// ─── 箭头类型 ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowKind {
    /// = (COPY)
    Copy,
    /// => (forward MOVE)
    Forward,
    /// <= (reverse MOVE)
    Reverse,
}

// ─── 文件根节点 ────────────────────────────────────────

/// 完整 AST 文件根节点。
#[derive(Debug, Clone, PartialEq)]
pub struct FileAst {
    pub meta: Option<MetaBlock>,
    pub use_decls: Vec<UseDecl>,
    pub from_decls: Vec<FromDecl>,
    pub exception_defs: Vec<ExceptionDef>,
    pub enum_defs: Vec<EnumDef>,
    pub type_aliases: Vec<TypeAlias>,
    pub zones: Vec<Zone>,
    pub works_defs: Vec<WorksDef>,
    pub test_blocks: Vec<TestBlock>,
}

/// 测试块。
#[derive(Debug, Clone, PartialEq)]
pub struct TestBlock {
    pub name: String,
    pub body: Vec<Stmt>,
}
