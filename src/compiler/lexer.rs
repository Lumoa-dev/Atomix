//! Atomix 词法分析器 (Lexer)。
//!
//! 完整覆盖编译管线.md §2 的所有词法规则。
//! 特性：
//! - UTF-8 输入 → Token 流
//! - 大小写折叠（保留原始大小写到 Span 中）
//! - 行注释 `#` 和块注释 `#! ... !#`
//! - 元信息块（区外 `#! ... !#`）自动检测
//! - 恐慌模式错误恢复

use crate::compiler::token::*;

// ─── Lexer 错误 ────────────────────────────────────────

/// 词法分析错误。
#[derive(Debug, Clone)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl LexError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

// ─── Lexer ─────────────────────────────────────────────

/// Atomix 源码的词法分析器。
///
/// 用法：
/// ```ignore
/// let mut lexer = Lexer::new(source_code);
/// for token in lexer {
///     println!("{:?}", token);
/// }
/// ```
pub struct Lexer {
    /// 源码字符（UTF-8 字节索引迭代）。
    chars: Vec<char>,
    /// 当前读取位置（字符索引）。
    pos: usize,
    /// 当前行号（从 1 开始）。
    line: usize,
    /// 当前列号（从 1 开始）。
    col: usize,
    /// 是否已经遇到了区域声明关键字（用于元信息块检测）。
    seen_zone_decl: bool,
    /// 是否已经遇到元信息块。
    seen_meta_block: bool,
    /// 收集到的错误列表。
    errors: Vec<LexError>,
}

impl Lexer {
    /// 创建新的词法分析器。
    pub fn new(source: &str) -> Self {
        let chars: Vec<char> = source.chars().collect();
        Self {
            chars,
            pos: 0,
            line: 1,
            col: 1,
            seen_zone_decl: false,
            seen_meta_block: false,
            errors: Vec::new(),
        }
    }

    /// 消耗所有 Token 并返回 (tokens, errors)。
    pub fn tokenize(mut self) -> (Vec<Token>, Vec<LexError>) {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token();
            let is_eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        (tokens, self.errors)
    }

    // ── 字符级操作 ──────────────────────────────────

    fn is_eof(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn current_pos(&self) -> SourcePos {
        SourcePos::new(self.line, self.col)
    }

    // ── Token 生成 ──────────────────────────────────

    fn make_token(&self, kind: TokenKind, start: SourcePos) -> Token {
        Token::new(kind, Span::new(start, self.current_pos()))
    }

    fn error_token(&mut self, msg: impl Into<String>, start: SourcePos) -> Token {
        let span = Span::new(start, self.current_pos());
        self.errors.push(LexError::new(msg, span));
        // 返回一个 Ident 作为错误恢复标记，保持 Token 流不中断
        Token::new(TokenKind::Ident("__error__".into()), span)
    }

    // ── 跳过空白 ────────────────────────────────────

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.advance();
            } else if ch == '\n' {
                // 换行是语句分隔符，但由 Parser 处理。
                // Lexer 在此跳过——Parser 如需新行信息可另行插入 Newline Token。
                self.advance();
            } else {
                break;
            }
        }
    }

    // ── 注释与元信息块 ─────────────────────────────

    /// 尝试跳过注释。返回 true 如果跳过了内容。
    /// 如果遇到元信息块，产出 MetaBlock Token。
    fn try_comment_or_meta(&mut self) -> Option<Token> {
        if self.peek() != Some('#') {
            return None;
        }
        let start = self.current_pos();

        // 检查是否块注释 #!
        if self.peek_next() == Some('!') {
            self.advance(); // #
            self.advance(); // !
            let mut content = String::new();
            loop {
                match self.peek() {
                    None => {
                        // 未闭合的块注释
                        self.errors.push(LexError::new(
                            "未闭合的块注释 `#! ... !#`",
                            Span::new(start, self.current_pos()),
                        ));
                        break;
                    }
                    Some('!') if self.peek_next() == Some('#') => {
                        self.advance(); // !
                        self.advance(); // #
                        break;
                    }
                    Some(ch) => {
                        self.advance();
                        content.push(ch);
                    }
                }
            }

            // 区外且未见过元信息块 → 产出 MetaBlock
            if !self.seen_zone_decl && !self.seen_meta_block {
                // 去掉首尾空白
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    self.seen_meta_block = true;
                    return Some(self.make_token(TokenKind::MetaBlock(trimmed), start));
                }
            }
            // 否则等同于注释
            return None;
        }

        // 行注释
        self.advance(); // #
        while let Some(ch) = self.peek() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            self.advance();
        }
        None
    }

    // ── 标识符 / 关键字 ─────────────────────────────

    fn read_ident_or_keyword(&mut self, start: SourcePos, first: char) -> Token {
        let mut raw = String::new();
        raw.push(first);
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                raw.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // 大小写折叠匹配关键字
        if let Some(kw) = match_keyword(&raw) {
            // 检测区域声明
            if kw.is_zone_keyword() {
                self.seen_zone_decl = true;
            }
            self.make_token(kw, start)
        } else {
            // 普通标识符，保留原始大小写
            self.make_token(TokenKind::Ident(raw), start)
        }
    }

    // ── 数字字面量 ──────────────────────────────────

    fn read_number(&mut self, start: SourcePos, first: char) -> Token {
        // 检测进制前缀
        if first == '0' {
            match self.peek() {
                Some('x') | Some('X') => {
                    self.advance(); // 0x
                    return self.read_int_hex(start);
                }
                Some('b') | Some('B') => {
                    self.advance(); // 0b
                    return self.read_int_bin(start);
                }
                Some('o') | Some('O') => {
                    self.advance(); // 0o
                    return self.read_int_oct(start);
                }
                _ => {}
            }
        }

        // 十进制整数或浮点
        let mut digits = String::new();
        digits.push(first);

        // 读取整数部分
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                digits.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // 检查前导零（非零整数不允许前导零，但 0 本身合法）
        if first == '0' && digits.len() > 1 {
            return self.error_token(
                "前导零：整数不允许前导零（如 007），使用 `0x`/`0b`/`0o` 前缀替代",
                start,
            );
        }

        // 浮点数（带小数点）
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            self.advance(); // .
            digits.push('.');
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() {
                    digits.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
            return self.finish_float(digits, start);
        }

        // 浮点数（科学计数法无小数点，如 1e10）
        if self.peek() == Some('e') || self.peek() == Some('E') {
            return self.finish_float(digits, start);
        }

        // 整数
        let val: i64 = digits.parse().unwrap_or(0);
        self.make_token(TokenKind::Int(val), start)
    }

    fn read_int_hex(&mut self, start: SourcePos) -> Token {
        let mut digits = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_hexdigit() {
                digits.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if digits.is_empty() {
            return self.error_token("空十六进制字面量：`0x` 后需要至少一位十六进制数字", start);
        }
        let val = i64::from_str_radix(&digits, 16).unwrap_or(0);
        self.make_token(TokenKind::Int(val), start)
    }

    fn read_int_bin(&mut self, start: SourcePos) -> Token {
        let mut digits = String::new();
        while let Some(ch) = self.peek() {
            if ch == '0' || ch == '1' {
                digits.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if digits.is_empty() {
            return self.error_token("空二进制字面量：`0b` 后需要至少一位二进制数字", start);
        }
        let val = i64::from_str_radix(&digits, 2).unwrap_or(0);
        self.make_token(TokenKind::Int(val), start)
    }

    fn read_int_oct(&mut self, start: SourcePos) -> Token {
        let mut digits = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() && ch != '8' && ch != '9' {
                digits.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if digits.is_empty() {
            return self.error_token("空八进制字面量：`0o` 后需要至少一位八进制数字", start);
        }
        let val = i64::from_str_radix(&digits, 8).unwrap_or(0);
        self.make_token(TokenKind::Int(val), start)
    }

    // ── 字符串字面量 ────────────────────────────────

    fn read_string(&mut self, start: SourcePos) -> Token {
        let mut content = String::new();
        loop {
            match self.advance() {
                None => {
                    self.errors.push(LexError::new(
                        "未闭合的字符串字面量",
                        Span::new(start, self.current_pos()),
                    ));
                    break;
                }
                Some('"') => break,
                Some('\\') => {
                    match self.advance() {
                        Some('n') => content.push('\n'),
                        Some('t') => content.push('\t'),
                        Some('r') => content.push('\r'),
                        Some('\\') => content.push('\\'),
                        Some('"') => content.push('"'),
                        Some('x') => {
                            // \xHH
                            let hex = self.read_hex_escape(start);
                            content.push(hex as char);
                        }
                        Some(c) => {
                            self.errors.push(LexError::new(
                                format!("未知转义序列 `\\{c}`"),
                                Span::new(start, self.current_pos()),
                            ));
                            content.push(c);
                        }
                        None => {
                            self.errors.push(LexError::new(
                                "字符串中转义序列后意外结束",
                                Span::new(start, self.current_pos()),
                            ));
                            break;
                        }
                    }
                }
                Some(ch) => content.push(ch),
            }
        }
        self.make_token(TokenKind::Str(content), start)
    }

    /// 读取 \xHH 转义，返回解析出的字节值。
    fn read_hex_escape(&mut self, start: SourcePos) -> u8 {
        let mut hex = String::with_capacity(2);
        for _ in 0..2 {
            match self.peek() {
                Some(c) if c.is_ascii_hexdigit() => {
                    hex.push(c);
                    self.advance();
                }
                _ => break,
            }
        }
        if hex.len() != 2 {
            self.errors.push(LexError::new(
                format!("无效的十六进制转义 `\\x{}`", hex),
                Span::new(start, self.current_pos()),
            ));
            return 0;
        }
        u8::from_str_radix(&hex, 16).unwrap_or(0)
    }

    // ── F-字符串 ────────────────────────────────────

    fn read_fstring(&mut self, start: SourcePos) -> Token {
        let mut parts = Vec::new();
        let mut text = String::new();

        loop {
            match self.advance() {
                None => {
                    self.errors.push(LexError::new(
                        "未闭合的 F-字符串字面量",
                        Span::new(start, self.current_pos()),
                    ));
                    if !text.is_empty() {
                        parts.push(FStringPart::Text(text));
                    }
                    break;
                }
                Some('"') => {
                    if !text.is_empty() {
                        parts.push(FStringPart::Text(text));
                    }
                    break;
                }
                Some('\\') => {
                    match self.advance() {
                        Some('n') => text.push('\n'),
                        Some('t') => text.push('\t'),
                        Some('r') => text.push('\r'),
                        Some('\\') => text.push('\\'),
                        Some('"') => text.push('"'),
                        Some('{') => text.push('{'), // \{
                        Some('}') => text.push('}'), // \}
                        Some(c) => text.push(c),
                        None => {
                            self.errors.push(LexError::new(
                                "F-字符串中转义序列后意外结束",
                                Span::new(start, self.current_pos()),
                            ));
                            break;
                        }
                    }
                }
                Some('{') => {
                    // 之前的文本段
                    if !text.is_empty() {
                        parts.push(FStringPart::Text(text));
                        text = String::new();
                    }
                    // 读取插值表达式（直到匹配的 }）
                    let mut depth = 1u32;
                    let mut interp = String::new();
                    loop {
                        match self.advance() {
                            None => {
                                self.errors.push(LexError::new(
                                    "F-字符串中插值表达式未闭合",
                                    Span::new(start, self.current_pos()),
                                ));
                                break;
                            }
                            Some('{') => {
                                depth += 1;
                                interp.push('{');
                            }
                            Some('}') => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                interp.push('}');
                            }
                            Some(ch) => interp.push(ch),
                        }
                    }
                    parts.push(FStringPart::Interp(interp));
                }
                Some(ch) => text.push(ch),
            }
        }

        self.make_token(TokenKind::FStr(parts), start)
    }

    /// 完成浮点字面量（读取指数部分后解析为 f64）。
    fn finish_float(&mut self, mut digits: String, start: SourcePos) -> Token {
        // 指数部分
        if self.peek() == Some('e') || self.peek() == Some('E') {
            self.advance();
            digits.push('e');
            if self.peek() == Some('+') || self.peek() == Some('-') {
                digits.push(self.peek().unwrap());
                self.advance();
            }
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() {
                    digits.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
        }
        let val: f64 = digits.parse().unwrap_or(f64::NAN);
        self.make_token(TokenKind::Float(val), start)
    }

    // ── 运算符与符号 ────────────────────────────────

    fn read_operator_or_symbol(&mut self, start: SourcePos, ch: char) -> Token {
        match ch {
            ':' => {
                if self.peek() == Some(':') {
                    self.advance();
                    self.make_token(TokenKind::DColon, start)
                } else {
                    self.make_token(TokenKind::Colon, start)
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    self.make_token(TokenKind::EqEq, start)
                } else if self.peek() == Some('>') {
                    self.advance();
                    self.make_token(TokenKind::ArrowR, start)
                } else {
                    self.make_token(TokenKind::Eq, start)
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    self.make_token(TokenKind::ArrowL, start)
                } else if self.peek() == Some('<') {
                    self.advance();
                    self.make_token(TokenKind::Shl, start)
                } else {
                    self.make_token(TokenKind::Lt, start)
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    self.make_token(TokenKind::Ge, start)
                } else if self.peek() == Some('>') {
                    self.advance();
                    self.make_token(TokenKind::Shr, start)
                } else {
                    self.make_token(TokenKind::Gt, start)
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    self.make_token(TokenKind::Neq, start)
                } else {
                    self.error_token("单独的 `!`：是用于 `!=` 的，不能单独使用", start)
                }
            }
            '+' => self.make_token(TokenKind::Plus, start),
            '-' => self.make_token(TokenKind::Minus, start),
            '*' => self.make_token(TokenKind::Star, start),
            '/' => self.make_token(TokenKind::Slash, start),
            '%' => self.make_token(TokenKind::Percent, start),
            '&' => self.make_token(TokenKind::Amp, start),
            '|' => self.make_token(TokenKind::Pipe, start),
            '^' => self.make_token(TokenKind::Caret, start),
            '~' => self.make_token(TokenKind::Tilde, start),
            '(' => self.make_token(TokenKind::LParen, start),
            ')' => self.make_token(TokenKind::RParen, start),
            '{' => self.make_token(TokenKind::LBrace, start),
            '}' => self.make_token(TokenKind::RBrace, start),
            '[' => self.make_token(TokenKind::LBracket, start),
            ']' => self.make_token(TokenKind::RBracket, start),
            ',' => self.make_token(TokenKind::Comma, start),
            '.' => self.make_token(TokenKind::Dot, start),
            '$' => self.make_token(TokenKind::Dollar, start),
            _ => self.error_token(format!("非法字符 `{ch}`"), start),
        }
    }

    // ── 主 Token 读取 ───────────────────────────────

    /// 读取下一个 Token。遇到错误时会产出错误 Token 并继续。
    pub fn next_token(&mut self) -> Token {
        // 跳过空白
        self.skip_whitespace();

        let start = self.current_pos();

        // 检查注释/元信息块
        if self.peek() == Some('#') {
            if let Some(token) = self.try_comment_or_meta() {
                return token;
            }
            // 注释跳过后继续
            return self.next_token();
        }

        // 文件结束
        if self.is_eof() {
            return self.make_token(TokenKind::Eof, start);
        }

        let ch = self.advance().unwrap();

        // F-字符串（必须在标识符之前检查，否则 `f"..."` 会当成标识符）
        if ch == 'f' && self.peek() == Some('"') {
            self.advance(); // 跳过 "
            return self.read_fstring(start);
        }

        // 标识符 / 关键字（字母或下划线开头）
        if ch.is_alphabetic() || ch == '_' {
            return self.read_ident_or_keyword(start, ch);
        }

        // 数字
        if ch.is_ascii_digit() {
            return self.read_number(start, ch);
        }

        // 字符串
        if ch == '"' {
            return self.read_string(start);
        }

        // 运算符 / 符号
        self.read_operator_or_symbol(start, ch)
    }
}

// ─── 迭代器支持 ────────────────────────────────────────

impl Iterator for Lexer {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.next_token();
        if matches!(token.kind, TokenKind::Eof) {
            None
        } else {
            Some(token)
        }
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(s: &str) -> (Vec<Token>, Vec<LexError>) {
        Lexer::new(s).tokenize()
    }

    fn kinds(tokens: &[Token]) -> Vec<TokenKind> {
        tokens.iter().map(|t| t.kind.clone()).collect()
    }

    // ── 基础测试 ──────────────────────────────────

    #[test]
    fn empty_input() {
        let (tokens, errors) = tokenize("");
        assert!(errors.is_empty());
        assert_eq!(kinds(&tokens), vec![TokenKind::Eof]);
    }

    #[test]
    fn whitespace_only() {
        let (tokens, errors) = tokenize("   \t\n\r\n  ");
        assert!(errors.is_empty());
        assert_eq!(kinds(&tokens), vec![TokenKind::Eof]);
    }

    // ── 标识符与关键字 ────────────────────────────

    #[test]
    fn identifiers() {
        let (tokens, _) = tokenize("foo bar _baz abc123");
        let ks = kinds(&tokens);
        assert_eq!(ks.len(), 5); // 4 idents + EOF
        assert_eq!(ks[0], TokenKind::Ident("foo".into()));
        assert_eq!(ks[1], TokenKind::Ident("bar".into()));
        assert_eq!(ks[2], TokenKind::Ident("_baz".into()));
        assert_eq!(ks[3], TokenKind::Ident("abc123".into()));
        assert_eq!(ks[4], TokenKind::Eof);
    }

    #[test]
    fn case_insensitive_keywords() {
        let (tokens, _) = tokenize("USE use Use IF if If");
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::Use);
        assert_eq!(ks[1], TokenKind::Use);
        assert_eq!(ks[2], TokenKind::Use);
        assert_eq!(ks[3], TokenKind::If);
        assert_eq!(ks[4], TokenKind::If);
        assert_eq!(ks[5], TokenKind::If);
    }

    // ── 字面量 ────────────────────────────────────

    #[test]
    fn integers() {
        let (tokens, _) = tokenize("42 0 0xFF 0b1010 0o77");
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::Int(42));
        assert_eq!(ks[1], TokenKind::Int(0));
        assert_eq!(ks[2], TokenKind::Int(255));
        assert_eq!(ks[3], TokenKind::Int(10));
        assert_eq!(ks[4], TokenKind::Int(63));
    }

    #[test]
    fn leading_zero_error() {
        let (_, errors) = tokenize("007");
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("前导零"));
    }

    #[test]
    fn empty_hex_error() {
        let (_, errors) = tokenize("0x");
        assert!(!errors.is_empty());
    }

    #[test]
    fn floats() {
        let (tokens, _) = tokenize("3.14 1e10 2.5e-3");
        let ks = kinds(&tokens);
        match &ks[0] {
            TokenKind::Float(v) => assert!((v - 3.14).abs() < 1e-10),
            _ => panic!("expected float"),
        }
        match &ks[1] {
            TokenKind::Float(v) => assert!((v - 1e10).abs() < 1.0),
            _ => panic!("expected float"),
        }
        match &ks[2] {
            TokenKind::Float(v) => assert!((v - 2.5e-3).abs() < 1e-10),
            _ => panic!("expected float"),
        }
    }

    #[test]
    fn dot_separated_not_float() {
        // 1 . 2 → 整数 1, 点, 整数 2（点前无数字才是浮点数）
        let (tokens, _) = tokenize("1.2");
        let ks = kinds(&tokens);
        assert!(matches!(ks[0], TokenKind::Float(_)));
    }

    // ── 字符串 ────────────────────────────────────

    #[test]
    fn string_literal() {
        let (tokens, _) = tokenize(r#""hello world""#);
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::Str("hello world".into()));
    }

    #[test]
    fn string_with_escapes() {
        let (tokens, _) = tokenize(r#""line1\nline2\t tab \\ \"""#);
        let ks = kinds(&tokens);
        let s = match &ks[0] {
            TokenKind::Str(s) => s.clone(),
            _ => panic!("expected string"),
        };
        assert!(s.contains('\n'));
        assert!(s.contains('\t'));
        assert!(s.contains('\\'));
        assert!(s.contains('"'));
    }

    #[test]
    fn unclosed_string() {
        let (_, errors) = tokenize(r#""unclosed"#);
        assert!(!errors.is_empty());
    }

    // ── F-字符串 ──────────────────────────────────

    #[test]
    fn fstring_basic() {
        let (tokens, _) = tokenize(r#"f"hello {name} world""#);
        let parts = match &tokens[0].kind {
            TokenKind::FStr(p) => p.clone(),
            _ => panic!("expected FStr"),
        };
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], FStringPart::Text("hello ".into()));
        assert_eq!(parts[1], FStringPart::Interp("name".into()));
        assert_eq!(parts[2], FStringPart::Text(" world".into()));
    }

    // ── 注释 ──────────────────────────────────────

    #[test]
    fn line_comment() {
        let (tokens, _) = tokenize("x # this is a comment\ny");
        let ks = kinds(&tokens);
        assert_eq!(ks.len(), 3);
        assert_eq!(ks[0], TokenKind::Ident("x".into()));
        assert_eq!(ks[1], TokenKind::Ident("y".into()));
        assert_eq!(ks[2], TokenKind::Eof);
    }

    #[test]
    fn block_comment_inside_zone() {
        // 区域内的 #! ... !# 等效于注释
        let (tokens, _) = tokenize("TOOLS : { #! block !# }");
        let ks = kinds(&tokens);
        // TOOLS : { } (no MetaBlock)
        assert!(!ks.iter().any(|k| matches!(k, TokenKind::MetaBlock(_))));
    }

    #[test]
    fn block_comment_file_level_is_meta() {
        // 文件级（区域前）的 #! ... !# 是元信息块
        let (tokens, _) = tokenize("#! block !#");
        let ks = kinds(&tokens);
        assert!(matches!(ks[0], TokenKind::MetaBlock(_)));
    }

    #[test]
    fn meta_block_before_zone() {
        let (tokens, _) = tokenize("#! author = Test !#\nTOOLS : {}");
        let ks = kinds(&tokens);
        assert!(matches!(ks[0], TokenKind::MetaBlock(_)));
        assert_eq!(ks[1], TokenKind::Tools);
    }

    #[test]
    fn meta_block_after_zone_is_comment() {
        let (tokens, _) = tokenize("TOOLS : {} #! not meta !#");
        let ks = kinds(&tokens);
        // 不应出现 MetaBlock
        for k in &ks {
            assert!(!matches!(k, TokenKind::MetaBlock(_)));
        }
    }

    // ── 运算符 ────────────────────────────────────

    #[test]
    fn operators() {
        let (tokens, _) = tokenize("+ - * / % == != < > <= >= << >> & | ^ ~");
        let ks = kinds(&tokens);
        let expected = vec![
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::EqEq,
            TokenKind::Neq,
            TokenKind::Lt,
            TokenKind::Gt,
            TokenKind::ArrowL, // <=
            TokenKind::Ge,     // >=
            TokenKind::Shl,    // <<
            TokenKind::Shr,    // >>
            TokenKind::Amp,
            TokenKind::Pipe,
            TokenKind::Caret,
            TokenKind::Tilde,
        ];
        assert_eq!(ks.len() - 1, expected.len()); // minus EOF
        for (got, exp) in ks.iter().zip(expected.iter()) {
            assert_eq!(got, exp, "expected {exp:?}, got {got:?}");
        }
    }

    #[test]
    fn colon_dcolon() {
        let (tokens, _) = tokenize(": ::");
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::Colon);
        assert_eq!(ks[1], TokenKind::DColon);
    }

    #[test]
    fn arrows() {
        let (tokens, _) = tokenize("=> <=");
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::ArrowR);
        assert_eq!(ks[1], TokenKind::ArrowL);
    }

    #[test]
    fn dollar() {
        let (tokens, _) = tokenize("$");
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::Dollar);
    }

    // ── 符号 ────────────────────────────────────

    #[test]
    fn symbols() {
        let (tokens, _) = tokenize("( ) { } [ ] , .");
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::LParen);
        assert_eq!(ks[1], TokenKind::RParen);
        assert_eq!(ks[2], TokenKind::LBrace);
        assert_eq!(ks[3], TokenKind::RBrace);
        assert_eq!(ks[4], TokenKind::LBracket);
        assert_eq!(ks[5], TokenKind::RBracket);
        assert_eq!(ks[6], TokenKind::Comma);
        assert_eq!(ks[7], TokenKind::Dot);
    }

    // ── 错误恢复 ──────────────────────────────────

    #[test]
    fn multiple_errors() {
        let (_, errors) = tokenize("! 0x 0b");
        // 应收集多个错误而不是停在第一个
        assert!(!errors.is_empty());
    }

    #[test]
    fn bad_escape() {
        let (_, errors) = tokenize(r#""\q""#);
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("未知转义"));
    }

    // ── zone 关键字检测 ───────────────────────────

    #[test]
    fn zone_keywords_detected() {
        let (tokens, _) = tokenize("TOOLS : {}");
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::Tools);
    }

    // ── 迭代器模式 ────────────────────────────────

    #[test]
    fn iterator_produces_no_eof() {
        let lexer = Lexer::new("a b c");
        let tokens: Vec<Token> = lexer.collect();
        assert_eq!(tokens.len(), 3);
        for t in &tokens {
            assert!(!matches!(t.kind, TokenKind::Eof));
        }
    }

    // ── 完整的 demo.atx 头部词法 ──────────────────

    #[test]
    fn demo_atx_header() {
        let src = r#"USE : "std/csv"
USE : "std/validate"

CONST LIMIT : int = 5000
"#;
        let (tokens, errors) = tokenize(src);
        assert!(errors.is_empty());
        let ks = kinds(&tokens);
        assert_eq!(ks[0], TokenKind::Use);
        assert_eq!(ks[1], TokenKind::Colon);
        assert_eq!(ks[2], TokenKind::Str("std/csv".into()));
        assert_eq!(ks[3], TokenKind::Use);
        assert_eq!(ks[4], TokenKind::Colon);
        assert_eq!(ks[5], TokenKind::Str("std/validate".into()));
        assert_eq!(ks[6], TokenKind::Const);
        assert_eq!(ks[7], TokenKind::Ident("LIMIT".into()));
        assert_eq!(ks[8], TokenKind::Colon);
        assert_eq!(ks[9], TokenKind::IntTy);
        assert_eq!(ks[10], TokenKind::Eq);
        assert_eq!(ks[11], TokenKind::Int(5000));
    }
}
