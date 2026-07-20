//! 表达式求值器 — 在调试会话中解析并计算表达式。
//!
//! 语法 (递归下降):
//!
//! ```text
//! expr     → or_expr
//! or_expr  → xor_expr ('|' xor_expr)*
//! xor_expr → and_expr ('^' and_expr)*
//! and_expr → cmp_expr ('&' cmp_expr)*
//! cmp_expr → add_expr (('=='|'!='|'<'|'>'|'<='|'>=') add_expr)*
//! add_expr → mul_expr (('+'|'-') mul_expr)*
//! mul_expr → unary_expr (('*'|'/') unary_expr)*
//! unary_expr → '-' unary_expr | '*' unary_expr | primary
//! primary  → NUMBER | REG | '(' expr ')'
//! NUMBER   → 十进制 | 0x 十六进制
//! REG      → a0..a3 | t0..t5 | sp | fp | ra | zero | pc | r0..r15
//! ```

use crate::runner::VmState;

// ─── 词法分析 ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(u64),
    Reg(String),
    Plus,
    Minus,
    Star,
    Slash,
    Amp,
    Pipe,
    Caret,
    EqEq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    LParen,
    RParen,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    let reg_names: &[(&str, &str)] = &[
        ("a0", "a0"), ("a1", "a1"), ("a2", "a2"), ("a3", "a3"),
        ("t0", "t0"), ("t1", "t1"), ("t2", "t2"), ("t3", "t3"),
        ("t4", "t4"), ("t5", "t5"),
        ("sp", "sp"), ("fp", "fp"), ("ra", "ra"), ("zero", "zero"), ("pc", "pc"),
    ];

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch.is_ascii_digit() {
            // 解析数字（十六进制或十进制）
            let mut s = String::new();
            if ch == '0' {
                s.push(chars.next().unwrap());
                if chars.peek() == Some(&'x') || chars.peek() == Some(&'X') {
                    s.push(chars.next().unwrap());
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_hexdigit() {
                            s.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    let val = u64::from_str_radix(&s[2..], 16)
                        .map_err(|_| format!("无效的十六进制数: {}", s))?;
                    tokens.push(Token::Number(val));
                    continue;
                }
            }
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    s.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            let val = s.parse::<u64>()
                .map_err(|_| format!("无效的数字: {}", s))?;
            tokens.push(Token::Number(val));
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || c == '_' {
                    s.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            let lower = s.to_lowercase();
            // 检查是否为寄存器名
            let is_reg = reg_names.iter().any(|(name, _)| *name == lower);
            // 也接受 r0..r15 格式
            let is_rn = lower.starts_with('r') && lower[1..].parse::<u8>().ok().map_or(false, |n| n < 16);
            if is_reg || is_rn {
                tokens.push(Token::Reg(lower));
            } else {
                return Err(format!("未知的标识符: {}", s));
            }
        } else {
            match ch {
                '+' => { chars.next(); tokens.push(Token::Plus); }
                '-' => { chars.next(); tokens.push(Token::Minus); }
                '*' => { chars.next(); tokens.push(Token::Star); }
                '/' => { chars.next(); tokens.push(Token::Slash); }
                '&' => { chars.next(); tokens.push(Token::Amp); }
                '|' => { chars.next(); tokens.push(Token::Pipe); }
                '^' => { chars.next(); tokens.push(Token::Caret); }
                '(' => { chars.next(); tokens.push(Token::LParen); }
                ')' => { chars.next(); tokens.push(Token::RParen); }
                '=' => {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(Token::EqEq);
                    } else {
                        return Err("意外的 '=', 你是否想用 '=='?".to_string());
                    }
                }
                '!' => {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(Token::Ne);
                    } else {
                        return Err("意外的 '!', 你是否想用 '!='?".to_string());
                    }
                }
                '<' => {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(Token::Le);
                    } else {
                        tokens.push(Token::Lt);
                    }
                }
                '>' => {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(Token::Ge);
                    } else {
                        tokens.push(Token::Gt);
                    }
                }
                _ => return Err(format!("意外的字符: '{}'", ch)),
            }
        }
    }
    Ok(tokens)
}

// ─── 表达式求值 ────────────────────────────────────────

/// 在给定的 VM 状态下计算表达式。
pub fn eval_expr(input: &str, vm: &VmState) -> Result<u64, String> {
    let tokens = tokenize(input)?;
    let mut pos = 0;
    let result = parse_or(&tokens, &mut pos, vm)?;
    if pos < tokens.len() {
        return Err(format!("表达式结束后有多余的 token"));
    }
    Ok(result)
}

fn peek(tokens: &[Token], pos: usize) -> Option<&Token> {
    tokens.get(pos)
}

fn advance<'a>(tokens: &'a [Token], pos: &mut usize) -> Option<&'a Token> {
    let t = tokens.get(*pos);
    *pos += 1;
    t
}

fn expect(tokens: &[Token], pos: &mut usize, expected: &Token) -> Result<(), String> {
    match tokens.get(*pos) {
        Some(t) if t == expected => {
            *pos += 1;
            Ok(())
        }
        Some(t) => Err(format!("期望 {:?}, 得到 {:?}", expected, t)),
        None => Err(format!("期望 {:?}, 但表达式已结束", expected)),
    }
}

// 优先级: | < ^ < & < == != < < > <= >= < + - < * / < unary
fn parse_or(tokens: &[Token], pos: &mut usize, vm: &VmState) -> Result<u64, String> {
    let mut left = parse_xor(tokens, pos, vm)?;
    while peek(tokens, *pos) == Some(&Token::Pipe) {
        advance(tokens, pos);
        let right = parse_xor(tokens, pos, vm)?;
        left |= right;
    }
    Ok(left)
}

fn parse_xor(tokens: &[Token], pos: &mut usize, vm: &VmState) -> Result<u64, String> {
    let mut left = parse_and(tokens, pos, vm)?;
    while peek(tokens, *pos) == Some(&Token::Caret) {
        advance(tokens, pos);
        let right = parse_and(tokens, pos, vm)?;
        left ^= right;
    }
    Ok(left)
}

fn parse_and(tokens: &[Token], pos: &mut usize, vm: &VmState) -> Result<u64, String> {
    let mut left = parse_cmp(tokens, pos, vm)?;
    while peek(tokens, *pos) == Some(&Token::Amp) {
        advance(tokens, pos);
        let right = parse_cmp(tokens, pos, vm)?;
        left &= right;
    }
    Ok(left)
}

fn parse_cmp(tokens: &[Token], pos: &mut usize, vm: &VmState) -> Result<u64, String> {
    let left = parse_add(tokens, pos, vm)?;
    if let Some(op) = peek(tokens, *pos) {
        match op {
            Token::EqEq => {
                advance(tokens, pos);
                let right = parse_add(tokens, pos, vm)?;
                return Ok(if left == right { 1 } else { 0 });
            }
            Token::Ne => {
                advance(tokens, pos);
                let right = parse_add(tokens, pos, vm)?;
                return Ok(if left != right { 1 } else { 0 });
            }
            Token::Lt => {
                advance(tokens, pos);
                let right = parse_add(tokens, pos, vm)?;
                return Ok(if left < right { 1 } else { 0 });
            }
            Token::Gt => {
                advance(tokens, pos);
                let right = parse_add(tokens, pos, vm)?;
                return Ok(if left > right { 1 } else { 0 });
            }
            Token::Le => {
                advance(tokens, pos);
                let right = parse_add(tokens, pos, vm)?;
                return Ok(if left <= right { 1 } else { 0 });
            }
            Token::Ge => {
                advance(tokens, pos);
                let right = parse_add(tokens, pos, vm)?;
                return Ok(if left >= right { 1 } else { 0 });
            }
            _ => {}
        }
    }
    Ok(left)
}

fn parse_add(tokens: &[Token], pos: &mut usize, vm: &VmState) -> Result<u64, String> {
    let mut left = parse_mul(tokens, pos, vm)?;
    while let Some(op) = peek(tokens, *pos) {
        match op {
            Token::Plus => {
                advance(tokens, pos);
                let right = parse_mul(tokens, pos, vm)?;
                left = left.wrapping_add(right);
            }
            Token::Minus => {
                advance(tokens, pos);
                let right = parse_mul(tokens, pos, vm)?;
                left = left.wrapping_sub(right);
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_mul(tokens: &[Token], pos: &mut usize, vm: &VmState) -> Result<u64, String> {
    let mut left = parse_unary(tokens, pos, vm)?;
    while let Some(op) = peek(tokens, *pos) {
        match op {
            Token::Star => {
                advance(tokens, pos);
                let right = parse_unary(tokens, pos, vm)?;
                left = left.wrapping_mul(right);
            }
            Token::Slash => {
                advance(tokens, pos);
                let right = parse_unary(tokens, pos, vm)?;
                if right == 0 {
                    return Err("除以零".to_string());
                }
                left = left.wrapping_div(right);
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_unary(tokens: &[Token], pos: &mut usize, vm: &VmState) -> Result<u64, String> {
    if let Some(op) = peek(tokens, *pos) {
        match op {
            Token::Minus => {
                advance(tokens, pos);
                let val = parse_unary(tokens, pos, vm)?;
                return Ok((!val).wrapping_add(1)); // 补码取负
            }
            Token::Star => {
                advance(tokens, pos);
                let addr = parse_unary(tokens, pos, vm)?;
                // 从 VM 沙箱内存读取 u64
                return vm.memory.read_u64(addr)
                    .ok_or_else(|| format!("无法读取地址 {:#x}", addr));
            }
            _ => {}
        }
    }
    parse_primary(tokens, pos, vm)
}

fn parse_primary(tokens: &[Token], pos: &mut usize, vm: &VmState) -> Result<u64, String> {
    match peek(tokens, *pos) {
        Some(Token::Number(n)) => {
            let val = *n;
            advance(tokens, pos);
            Ok(val)
        }
        Some(Token::Reg(name)) => {
            let val = get_reg_value(name, vm)?;
            advance(tokens, pos);
            Ok(val)
        }
        Some(Token::LParen) => {
            advance(tokens, pos);
            let val = parse_or(tokens, pos, vm)?;
            expect(tokens, pos, &Token::RParen)?;
            Ok(val)
        }
        Some(t) => Err(format!("意外的 token: {:?}", t)),
        None => Err("表达式不完整，期望一个值或表达式".to_string()),
    }
}

/// 获取寄存器值。
fn get_reg_value(name: &str, vm: &VmState) -> Result<u64, String> {
    let idx = match name {
        "zero" | "r0" => 0,
        "sp" | "r1" => 1,
        "fp" | "r2" => 2,
        "ra" | "r3" => 3,
        "a0" | "r4" => 4,
        "a1" | "r5" => 5,
        "a2" | "r6" => 6,
        "a3" | "r7" => 7,
        "t0" | "r8" => 8,
        "t1" | "r9" => 9,
        "t2" | "r10" => 10,
        "t3" | "r11" => 11,
        "t4" | "r12" => 12,
        "t5" | "r13" => 13,
        "task_id" | "r14" => 14,
        "tmp" | "r15" => 15,
        "pc" => {
            return Ok(vm.pc as u64);
        }
        _ => return Err(format!("未知寄存器: {}", name)),
    };
    Ok(vm.read_reg(idx))
}

/// 格式化结果输出。
pub fn format_result(val: u64) -> String {
    if val < 10_000 {
        format!("{} ({:#x})", val, val)
    } else {
        format!("{:#x} ({})", val, val as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::{AtxeBinary, Header};
    use crate::base::isa::{self, opcode, reg};

    fn make_test_vm() -> VmState {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let header = Header::new(0, text.len() as u16);
        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata: vec![],
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let mut vm = VmState::from_atxe(&binary).unwrap();
        // 设置一些已知值
        vm.write_reg(reg::A0, 42);
        vm.write_reg(reg::T0, 100);
        vm.write_reg(reg::T1, 50);
        vm.write_reg(reg::SP, 0x1000);
        vm.pc = 0x0042;
        vm
    }

    #[test]
    fn eval_number() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("42", &vm).unwrap(), 42);
        assert_eq!(eval_expr("0xFF", &vm).unwrap(), 255);
        assert_eq!(eval_expr("0", &vm).unwrap(), 0);
    }

    #[test]
    fn eval_register() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("a0", &vm).unwrap(), 42);
        assert_eq!(eval_expr("t0", &vm).unwrap(), 100);
        assert_eq!(eval_expr("sp", &vm).unwrap(), 0x1000);
        assert_eq!(eval_expr("pc", &vm).unwrap(), 0x0042);
        assert_eq!(eval_expr("r4", &vm).unwrap(), 42); // a0 = R4
    }

    #[test]
    fn eval_add() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("a0 + t0", &vm).unwrap(), 142);
        assert_eq!(eval_expr("a0 + 8", &vm).unwrap(), 50);
        assert_eq!(eval_expr("t0 + t1", &vm).unwrap(), 150);
    }

    #[test]
    fn eval_sub_mul_div() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("t0 - t1", &vm).unwrap(), 50);
        assert_eq!(eval_expr("t0 * 2", &vm).unwrap(), 200);
        assert_eq!(eval_expr("t0 / 3", &vm).unwrap(), 33);
    }

    #[test]
    fn eval_comparison() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("a0 == 42", &vm).unwrap(), 1);
        assert_eq!(eval_expr("a0 == 99", &vm).unwrap(), 0);
        assert_eq!(eval_expr("a0 != 99", &vm).unwrap(), 1);
        assert_eq!(eval_expr("a0 < 100", &vm).unwrap(), 1);
        assert_eq!(eval_expr("a0 > 100", &vm).unwrap(), 0);
        assert_eq!(eval_expr("t0 >= 100", &vm).unwrap(), 1);
        assert_eq!(eval_expr("t0 <= 50", &vm).unwrap(), 0);
    }

    #[test]
    fn eval_bitwise() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("0xFF & 0x0F", &vm).unwrap(), 0x0F);
        assert_eq!(eval_expr("0xF0 | 0x0F", &vm).unwrap(), 0xFF);
        assert_eq!(eval_expr("0xFF ^ 0x0F", &vm).unwrap(), 0xF0);
    }

    #[test]
    fn eval_parentheses() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("(a0 + t0) * 2", &vm).unwrap(), 284);
        assert_eq!(eval_expr("2 * (a0 + t0)", &vm).unwrap(), 284);
        assert_eq!(eval_expr("(a0)", &vm).unwrap(), 42);
    }

    #[test]
    fn eval_unary_minus() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("-a0", &vm).unwrap() as i64, -42i64);
    }

    #[test]
    fn eval_deref() {
        let mut vm = make_test_vm();
        let addr = vm.memory.alloc(16);
        assert!(vm.memory.write_u64(addr, 0xDEAD_BEEF));
        assert_eq!(eval_expr(&format!("*{}", addr), &vm).unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn eval_memory_chain() {
        let mut vm = make_test_vm();
        let addr = vm.memory.alloc(16);
        vm.memory.write_u64(addr, 0x42);
        assert_eq!(eval_expr(&format!("*{} == 0x42", addr), &vm).unwrap(), 1);
    }

    #[test]
    fn eval_errors() {
        let vm = make_test_vm();
        assert!(eval_expr("", &vm).is_err());
        assert!(eval_expr("a0 +", &vm).is_err());
        assert!(eval_expr("unknown", &vm).is_err());
        assert!(eval_expr("1/0", &vm).is_err());
    }

    #[test]
    fn eval_hex() {
        let vm = make_test_vm();
        assert_eq!(eval_expr("0x1000", &vm).unwrap(), 0x1000);
        assert_eq!(eval_expr("0xFF + 1", &vm).unwrap(), 256);
    }

    #[test]
    fn eval_complex() {
        let mut vm = make_test_vm();
        vm.write_reg(reg::A1, 10);
        vm.write_reg(reg::A2, 3);
        // (a0 + a1) * (a2 + 1) = (42+10) * (3+1) = 52 * 4 = 208
        assert_eq!(eval_expr("(a0 + a1) * (a2 + 1)", &vm).unwrap(), 208);
    }

    #[test]
    fn format_result_small() {
        assert_eq!(format_result(42), "42 (0x2a)");
    }

    #[test]
    fn format_result_large() {
        assert_eq!(format_result(0xDEAD_BEEF), "0xdeadbeef (3735928559)");
    }
}
