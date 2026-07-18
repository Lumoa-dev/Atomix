//! Atomix Token 类型定义。
//!
//! 完整覆盖编译管线.md §2.1 的全部 Token 类型。
//! 标识符原始大小写保留在 Span 中，关键词匹配时大小写折叠。

use std::fmt;

// ─── 源码位置 ──────────────────────────────────────────

/// 源码位置：行号 + 列号（均从 1 开始）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourcePos {
    pub line: usize,
    pub col: usize,
}

impl SourcePos {
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

impl fmt::Display for SourcePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Token 在源码中的跨度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: SourcePos,
    pub end: SourcePos,
}

impl Span {
    pub fn new(start: SourcePos, end: SourcePos) -> Self {
        Self { start, end }
    }
}

// ─── F-字符串片段 ──────────────────────────────────────

/// F-字符串的内部片段：纯文本或插值表达式。
#[derive(Debug, Clone, PartialEq)]
pub enum FStringPart {
    /// 普通文本片段。
    Text(String),
    /// 插值表达式（源码文本，由 Parser 进一步解析）。
    Interp(String),
}

// ─── Token 种类 ────────────────────────────────────────

/// 所有 Token 类型的枚举，严格对应编译管线.md §2.1。
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── 特殊 ──────────────────────────────────────────
    /// 文件结束。
    Eof,
    /// 元信息块（区外 `#! ... !#` 的内容）。
    MetaBlock(String),

    // ── 标识符与字面量 ────────────────────────────────
    /// 标识符（原始大小写保留在 Span 中）。
    Ident(String),
    /// 整数字面量（十进制/十六进制/二进制/八进制）。
    Int(i64),
    /// 浮点字面量（f64）。
    Float(f64),
    /// 字符串字面量（转义已解析）。
    Str(String),
    /// F-字符串字面量（含插值片段）。
    FStr(Vec<FStringPart>),

    // ── 关键字（大小写折叠后匹配） ────────────────────

    // 通用
    Use,
    True,
    False,
    And,
    Or,
    Not,
    Fn,
    Return,
    Do,
    If,
    Elif,
    Else,
    For,
    Break,
    Continue,
    Call,
    Wait,
    Join,
    Assert,
    Raise,
    Try,
    Const,
    Goout,
    Pub,
    /// 区域关键字
    Tools,
    Input,
    Works,
    Task,
    Out,
    Test,
    /// 区外定义
    Exception,
    Enum,
    Type,
    From,
    As,

    // 数据源/目标关键字
    Webs,
    Files,
    Mems,
    Http,
    Tcp,
    Db,
    Oss,
    Txt,
    Csv,
    Json,
    Jsons,
    Yaml,
    Toml,
    Xml,

    // 类型关键字
    IntTy,
    FloatTy,
    BoolTy,
    StrTy,
    BytesTy,
    ListTy,
    DictTy,
    TupleTy,
    Self_,

    // ── 符号 ──────────────────────────────────────────
    /// `:`
    Colon,
    /// `::`
    DColon,
    /// `=>`（前向移动 / 箭头右）
    ArrowR,
    /// `<=`（反向移动 / 小于等于，Parser 按上下文区分含义）
    ArrowL,
    /// `=`
    Eq,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `,`
    Comma,
    /// `.`
    Dot,
    /// `$`
    Dollar,

    // ── 运算符 ────────────────────────────────────────
    Plus,   // +
    Minus,  // -
    Star,   // *
    Slash,  // /
    Percent, // %
    Amp,    // &
    Pipe,   // |
    Caret,  // ^
    Tilde,  // ~
    Shl,    // <<
    Shr,    // >>
    EqEq,   // ==
    Neq,    // !=
    Lt,     // <
    Gt,     // >
    Ge,     // >=
}

/// 携带位置信息的 Token。
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

// ─── 关键字表（大小写折叠匹配） ────────────────────────

/// 从大小写不敏感的标识符匹配关键字。返回 Some(TokenKind) 表示是关键字。
pub fn match_keyword(s: &str) -> Option<TokenKind> {
    let folded = s.to_lowercase();
    match folded.as_str() {
        "use" => Some(TokenKind::Use),
        "true" => Some(TokenKind::True),
        "false" => Some(TokenKind::False),
        "and" => Some(TokenKind::And),
        "or" => Some(TokenKind::Or),
        "not" => Some(TokenKind::Not),
        "fn" => Some(TokenKind::Fn),
        "return" => Some(TokenKind::Return),
        "do" => Some(TokenKind::Do),
        "if" => Some(TokenKind::If),
        "elif" => Some(TokenKind::Elif),
        "else" => Some(TokenKind::Else),
        "for" => Some(TokenKind::For),
        "break" => Some(TokenKind::Break),
        "continue" => Some(TokenKind::Continue),
        "call" => Some(TokenKind::Call),
        "wait" => Some(TokenKind::Wait),
        "join" => Some(TokenKind::Join),
        "assert" => Some(TokenKind::Assert),
        "raise" => Some(TokenKind::Raise),
        "try" => Some(TokenKind::Try),
        "const" => Some(TokenKind::Const),
        "goout" => Some(TokenKind::Goout),
        "pub" => Some(TokenKind::Pub),
        "tools" => Some(TokenKind::Tools),
        "input" => Some(TokenKind::Input),
        "works" => Some(TokenKind::Works),
        "task" => Some(TokenKind::Task),
        "out" => Some(TokenKind::Out),
        "test" => Some(TokenKind::Test),
        "exception" => Some(TokenKind::Exception),
        "enum" => Some(TokenKind::Enum),
        "type" => Some(TokenKind::Type),
        "from" => Some(TokenKind::From),
        "as" => Some(TokenKind::As),
        "webs" => Some(TokenKind::Webs),
        "files" => Some(TokenKind::Files),
        "mems" => Some(TokenKind::Mems),
        "http" => Some(TokenKind::Http),
        "tcp" => Some(TokenKind::Tcp),
        "db" => Some(TokenKind::Db),
        "oss" => Some(TokenKind::Oss),
        "txt" => Some(TokenKind::Txt),
        "csv" => Some(TokenKind::Csv),
        "json" => Some(TokenKind::Json),
        "jsons" => Some(TokenKind::Jsons),
        "yaml" => Some(TokenKind::Yaml),
        "toml" => Some(TokenKind::Toml),
        "xml" => Some(TokenKind::Xml),
        "int" => Some(TokenKind::IntTy),
        "float" => Some(TokenKind::FloatTy),
        "bool" => Some(TokenKind::BoolTy),
        "str" => Some(TokenKind::StrTy),
        "bytes" => Some(TokenKind::BytesTy),
        "list" => Some(TokenKind::ListTy),
        "dict" => Some(TokenKind::DictTy),
        "tuple" => Some(TokenKind::TupleTy),
        "self" => Some(TokenKind::Self_),
        _ => None,
    }
}

/// 判断 Token 是否为区域关键字（用于区域级别语法检查）。
impl TokenKind {
    /// 是否为文件级定义关键字（出现在所有区域之前）。
    pub fn is_file_level_keyword(&self) -> bool {
        matches!(
            self,
            TokenKind::Use
                | TokenKind::Exception
                | TokenKind::Enum
                | TokenKind::Type
                | TokenKind::From
                | TokenKind::Test
        )
    }

    /// 是否为区域声明关键字。
    pub fn is_zone_keyword(&self) -> bool {
        matches!(
            self,
            TokenKind::Tools
                | TokenKind::Input
                | TokenKind::Works
                | TokenKind::Task
                | TokenKind::Out
        )
    }

    /// 是否为数据源/目标关键字（INPUT/OUT 区专用）。
    pub fn is_source_target_keyword(&self) -> bool {
        matches!(
            self,
            TokenKind::Webs
                | TokenKind::Files
                | TokenKind::Mems
                | TokenKind::Http
                | TokenKind::Tcp
                | TokenKind::Db
                | TokenKind::Oss
                | TokenKind::Txt
                | TokenKind::Csv
                | TokenKind::Json
                | TokenKind::Jsons
                | TokenKind::Yaml
                | TokenKind::Toml
                | TokenKind::Xml
        )
    }

    /// 是否为类型关键字。
    pub fn is_type_keyword(&self) -> bool {
        matches!(
            self,
            TokenKind::IntTy
                | TokenKind::FloatTy
                | TokenKind::BoolTy
                | TokenKind::StrTy
                | TokenKind::BytesTy
                | TokenKind::ListTy
                | TokenKind::DictTy
                | TokenKind::TupleTy
        )
    }

    /// 是否为运算符。
    pub fn is_operator(&self) -> bool {
        matches!(
            self,
            TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::Amp
                | TokenKind::Pipe
                | TokenKind::Caret
                | TokenKind::Tilde
                | TokenKind::Shl
                | TokenKind::Shr
                | TokenKind::EqEq
                | TokenKind::Neq
                | TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::Ge
                | TokenKind::And
                | TokenKind::Or
                | TokenKind::Not
        )
    }
}

// ─── Display ───────────────────────────────────────────

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eof => write!(f, "EOF"),
            Self::MetaBlock(_) => write!(f, "元信息块"),
            Self::Ident(s) => write!(f, "标识符 `{s}`"),
            Self::Int(n) => write!(f, "整数 `{n}`"),
            Self::Float(n) => write!(f, "浮点 `{n}`"),
            Self::Str(s) => write!(f, "字符串 `{s}`"),
            Self::FStr(_) => write!(f, "F-字符串"),
            // 关键字
            Self::Use => write!(f, "`USE`"),
            Self::True => write!(f, "`true`"),
            Self::False => write!(f, "`false`"),
            Self::And => write!(f, "`and`"),
            Self::Or => write!(f, "`or`"),
            Self::Not => write!(f, "`not`"),
            Self::Fn => write!(f, "`fn`"),
            Self::Return => write!(f, "`return`"),
            Self::Do => write!(f, "`do`"),
            Self::If => write!(f, "`IF`"),
            Self::Elif => write!(f, "`ELIF`"),
            Self::Else => write!(f, "`ELSE`"),
            Self::For => write!(f, "`FOR`"),
            Self::Break => write!(f, "`BREAK`"),
            Self::Continue => write!(f, "`CONTINUE`"),
            Self::Call => write!(f, "`CALL`"),
            Self::Wait => write!(f, "`WAIT`"),
            Self::Join => write!(f, "`JOIN`"),
            Self::Assert => write!(f, "`ASSERT`"),
            Self::Raise => write!(f, "`RAISE`"),
            Self::Try => write!(f, "`TRY`"),
            Self::Const => write!(f, "`CONST`"),
            Self::Goout => write!(f, "`GOOUT`"),
            Self::Pub => write!(f, "`PUB`"),
            Self::Tools => write!(f, "`TOOLS`"),
            Self::Input => write!(f, "`INPUT`"),
            Self::Works => write!(f, "`WORKS`"),
            Self::Task => write!(f, "`TASK`"),
            Self::Out => write!(f, "`OUT`"),
            Self::Test => write!(f, "`TEST`"),
            Self::Exception => write!(f, "`EXCEPTION`"),
            Self::Enum => write!(f, "`enum`"),
            Self::Type => write!(f, "`type`"),
            Self::From => write!(f, "`FROM`"),
            Self::As => write!(f, "`AS`"),
            Self::Webs => write!(f, "`WEBS`"),
            Self::Files => write!(f, "`FILES`"),
            Self::Mems => write!(f, "`MEMS`"),
            Self::Http => write!(f, "`HTTP`"),
            Self::Tcp => write!(f, "`TCP`"),
            Self::Db => write!(f, "`DB`"),
            Self::Oss => write!(f, "`OSS`"),
            Self::Txt => write!(f, "`TXT`"),
            Self::Csv => write!(f, "`CSV`"),
            Self::Json => write!(f, "`JSON`"),
            Self::Jsons => write!(f, "`JSONS`"),
            Self::Yaml => write!(f, "`YAML`"),
            Self::Toml => write!(f, "`TOML`"),
            Self::Xml => write!(f, "`XML`"),
            Self::IntTy => write!(f, "`int`"),
            Self::FloatTy => write!(f, "`float`"),
            Self::BoolTy => write!(f, "`bool`"),
            Self::StrTy => write!(f, "`str`"),
            Self::BytesTy => write!(f, "`bytes`"),
            Self::ListTy => write!(f, "`list`"),
            Self::DictTy => write!(f, "`dict`"),
            Self::TupleTy => write!(f, "`tuple`"),
            Self::Self_ => write!(f, "`self`"),
            // 符号
            Self::Colon => write!(f, "`:`"),
            Self::DColon => write!(f, "`::`"),
            Self::ArrowR => write!(f, "`=>`"),
            Self::ArrowL => write!(f, "`<=`"),
            Self::Eq => write!(f, "`=`"),
            Self::LBrace => write!(f, "`{{`"),
            Self::RBrace => write!(f, "`}}`"),
            Self::LParen => write!(f, "`(`"),
            Self::RParen => write!(f, "`)`"),
            Self::LBracket => write!(f, "`[`"),
            Self::RBracket => write!(f, "`]`"),
            Self::Comma => write!(f, "`,`"),
            Self::Dot => write!(f, "`.`"),
            Self::Dollar => write!(f, "`$`"),
            // 运算符
            Self::Plus => write!(f, "`+`"),
            Self::Minus => write!(f, "`-`"),
            Self::Star => write!(f, "`*`"),
            Self::Slash => write!(f, "`/`"),
            Self::Percent => write!(f, "`%`"),
            Self::Amp => write!(f, "`&`"),
            Self::Pipe => write!(f, "`|`"),
            Self::Caret => write!(f, "`^`"),
            Self::Tilde => write!(f, "`~`"),
            Self::Shl => write!(f, "`<<`"),
            Self::Shr => write!(f, "`>>`"),
            Self::EqEq => write!(f, "`==`"),
            Self::Neq => write!(f, "`!=`"),
            Self::Lt => write!(f, "`<`"),
            Self::Gt => write!(f, "`>`"),
            Self::Ge => write!(f, "`>=`"),
        }
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_match_folded() {
        assert_eq!(match_keyword("USE"), Some(TokenKind::Use));
        assert_eq!(match_keyword("use"), Some(TokenKind::Use));
        assert_eq!(match_keyword("Use"), Some(TokenKind::Use));
        assert_eq!(match_keyword("IF"), Some(TokenKind::If));
        assert_eq!(match_keyword("if"), Some(TokenKind::If));
        assert_eq!(match_keyword("Int"), Some(TokenKind::IntTy));
    }

    #[test]
    fn keyword_non_keyword() {
        assert_eq!(match_keyword("foo"), None);
        assert_eq!(match_keyword("class"), None);
        assert_eq!(match_keyword(""), None);
    }

    #[test]
    fn all_keywords_covered() {
        // 验证全部 50+ 关键字都被 match_keyword 覆盖
        let keywords = [
            "use", "true", "false", "and", "or", "not", "fn", "return", "do",
            "if", "elif", "else", "for", "break", "continue",
            "call", "wait", "join", "assert", "raise", "try",
            "const", "goout", "pub",
            "tools", "input", "works", "task", "out", "test",
            "exception", "enum", "type", "from", "as",
            "webs", "files", "mems", "http", "tcp", "db", "oss",
            "txt", "csv", "json", "jsons", "yaml", "toml", "xml",
            "int", "float", "bool", "str", "bytes", "list", "dict", "tuple", "self",
        ];
        for kw in &keywords {
            assert!(match_keyword(kw).is_some(), "keyword `{kw}` should match");
        }
    }

    #[test]
    fn token_kind_classification() {
        assert!(TokenKind::Use.is_file_level_keyword());
        assert!(TokenKind::Exception.is_file_level_keyword());
        assert!(TokenKind::Tools.is_zone_keyword());
        assert!(TokenKind::Task.is_zone_keyword());
        assert!(TokenKind::Http.is_source_target_keyword());
        assert!(TokenKind::Json.is_source_target_keyword());
        assert!(TokenKind::IntTy.is_type_keyword());
        assert!(TokenKind::Plus.is_operator());
    }

    #[test]
    fn source_pos_display() {
        let pos = SourcePos::new(1, 15);
        assert_eq!(pos.to_string(), "1:15");
    }
}
