//! 表达式编译 — AST 表达式 → IR 指令序列。
//!
//! 覆盖 04-编译管线.md §5.2 的表达式到 IR 映射规则。

use crate::base::isa::{opcode, reg};
use crate::compiler::ast::{BinOp, Expr, UnOp};
use crate::compiler::codegen::instr::{InstrEmitter, VReg, vreg_to_preg};

/// 下一次可用的虚拟寄存器编号。
static mut NEXT_VREG: VReg = 0;

/// 分配一个虚拟寄存器。
pub fn alloc_vreg() -> VReg {
    let v = unsafe { NEXT_VREG };
    unsafe { NEXT_VREG += 1 };
    v
}

/// 重置虚拟寄存器分配器。
pub fn reset_vreg() {
    unsafe { NEXT_VREG = 0 };
}

// ─── 常量池 ────────────────────────────────────────────

/// 常量池条目。
#[derive(Debug, Clone)]
pub enum ConstEntry {
    Str(String),
    Float(f64),
    /// 大整数（超出 MOVI/LCONST 范围）
    BigInt(i64),
}

/// 常量池管理者。负责向 .rodata 段发射常量并记录偏移。
#[derive(Debug, Clone)]
pub struct ConstPool {
    pub data: Vec<u8>,
    /// 字符串 → 偏移
    str_offsets: std::collections::HashMap<String, usize>,
    /// 浮点值（按位表示）→ 偏移
    float_offsets: std::collections::HashMap<u64, usize>,
    /// 大整数值 → 偏移（64 位）
    big_int_offsets: std::collections::HashMap<i64, usize>,
}

impl ConstPool {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            str_offsets: std::collections::HashMap::new(),
            float_offsets: std::collections::HashMap::new(),
            big_int_offsets: std::collections::HashMap::new(),
        }
    }

    /// 确保 8 字节对齐。
    fn align8(&mut self) {
        while !self.data.len().is_multiple_of(8) {
            self.data.push(0);
        }
    }

    /// 添加字符串常量，返回在 .rodata 中的字节偏移。
    pub fn add_str(&mut self, s: &str) -> usize {
        if let Some(&off) = self.str_offsets.get(s) {
            return off;
        }
        self.align8();
        let off = self.data.len();
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0); // null terminator
        self.str_offsets.insert(s.to_string(), off);
        off
    }

    /// 添加浮点常量，返回在 .rodata 中的字节偏移。
    pub fn add_float(&mut self, val: f64) -> usize {
        let bits = val.to_bits();
        if let Some(&off) = self.float_offsets.get(&bits) {
            return off;
        }
        self.align8();
        let off = self.data.len();
        self.data.extend_from_slice(&bits.to_le_bytes());
        self.float_offsets.insert(bits, off);
        off
    }

    /// 添加 64 位大整数常量（超出 MOVI/LCONST 范围），返回 .rodata 字节偏移。
    pub fn add_big_int(&mut self, val: i64) -> usize {
        if let Some(&off) = self.big_int_offsets.get(&val) {
            return off;
        }
        self.align8();
        let off = self.data.len();
        self.data.extend_from_slice(&val.to_le_bytes());
        self.big_int_offsets.insert(val, off);
        off
    }

    /// 当前 .rodata 大小（字节）。
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Default for ConstPool {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 表达式编译 ────────────────────────────────────────

/// 编译表达式，返回结果所在的虚拟寄存器。
pub fn compile_expr(emit: &mut InstrEmitter, pool: &mut ConstPool, expr: &Expr) -> VReg {
    match expr {
        Expr::Int(n) => compile_int(emit, pool, *n),
        Expr::Float(n) => compile_float(emit, pool, *n),
        Expr::Str(s) => compile_str(emit, pool, s),
        Expr::Bool(b) => {
            let rd = alloc_vreg();
            emit.emit_movi(vreg_to_preg(rd), if *b { 1 } else { 0 });
            rd
        }
        Expr::Ident(name) => {
            // 标识符：从符号表查找寄存器。当前直接分配新寄存器。
            // 真正的寄存器分配在 Phase 2 中完成。
            let rd = alloc_vreg();
            let _ = name;
            rd
        }
        Expr::Binary { op, lhs, rhs } => compile_binary(emit, pool, *op, lhs, rhs),
        Expr::Unary { op, expr: inner } => compile_unary(emit, pool, *op, inner),
        Expr::List(items) => {
            // 列表字面量：依次编译每个元素
            for item in items {
                compile_expr(emit, pool, item);
            }
            alloc_vreg()
        }
        Expr::Tuple(items) => {
            for item in items {
                compile_expr(emit, pool, item);
            }
            alloc_vreg()
        }
        Expr::Dollar | Expr::DollarKey(_) => {
            // `$` 管道变量：编译期标记，运行时通过寄存器传递
            alloc_vreg()
        }
        Expr::CrossRef { domain, name } => {
            let rd = alloc_vreg();
            let _ = domain;
            let _ = name;
            rd
        }
        Expr::Index { target, index } => {
            compile_expr(emit, pool, target);
            compile_expr(emit, pool, index);
            alloc_vreg()
        }
        Expr::Dot { target, field } => {
            compile_expr(emit, pool, target);
            let _ = field;
            alloc_vreg()
        }
        Expr::DoFn { .. } => alloc_vreg(),
        Expr::Call { name, args } => compile_call(emit, pool, name, args),
        Expr::FStr(parts) => {
            // F-字符串：拼接所有片段
            for part in parts {
                match part {
                    crate::compiler::ast::FStringFragment::Text(t) => {
                        compile_str(emit, pool, t);
                    }
                    crate::compiler::ast::FStringFragment::Interp(e) => {
                        compile_expr(emit, pool, e);
                    }
                }
            }
            alloc_vreg()
        }
        Expr::Dict(entries) => {
            for (k, v) in entries {
                compile_expr(emit, pool, k);
                compile_expr(emit, pool, v);
            }
            alloc_vreg()
        }
    }
}

// ─── 具体表达式编译 ────────────────────────────────────

/// 编译整数字面量。
fn compile_int(emit: &mut InstrEmitter, pool: &mut ConstPool, n: i64) -> VReg {
    let rd = alloc_vreg();
    let preg = vreg_to_preg(rd);
    if n >= 0 && n <= u16::MAX as i64 {
        // 16 位无符号：单条 MOVI
        emit.emit_movi(preg, n as u16);
    } else if (-(1 << 19)..(1 << 19)).contains(&n) {
        // 20 位有符号：单条 LCONST
        emit.emit_r1i(opcode::LCONST, preg, n as u32);
    } else {
        // 完整 64 位：存入 .rodata，通过 LOAD 加载
        let off = pool.add_big_int(n);
        emit.emit_r2i(opcode::LOAD, preg, reg::SP as u8, off as u16);
    }
    rd
}

/// 编译浮点字面量。
fn compile_float(emit: &mut InstrEmitter, pool: &mut ConstPool, n: f64) -> VReg {
    let off = pool.add_float(n);
    let rd = alloc_vreg();
    let preg = vreg_to_preg(rd);
    // LOAD rd, [sp + offset] — 实际使用 .rodata 基址
    emit.emit_r2i(opcode::LOAD, preg, reg::SP as u8, off as u16);
    rd
}

/// 编译字符串字面量。
fn compile_str(emit: &mut InstrEmitter, pool: &mut ConstPool, s: &str) -> VReg {
    let off = pool.add_str(s);
    let rd = alloc_vreg();
    let preg = vreg_to_preg(rd);
    emit.emit_r2i(opcode::LOAD, preg, reg::SP as u8, off as u16);
    rd
}

/// 编译二元运算。
fn compile_binary(
    emit: &mut InstrEmitter,
    pool: &mut ConstPool,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
) -> VReg {
    let lr = compile_expr(emit, pool, lhs);
    let rr = compile_expr(emit, pool, rhs);
    let rd = alloc_vreg();
    let d = vreg_to_preg(rd);
    let s1 = vreg_to_preg(lr);
    let s2 = vreg_to_preg(rr);

    let (ocode, use_r3) = match op {
        BinOp::Add => (opcode::ADD, true),
        BinOp::Sub => (opcode::SUB, true),
        BinOp::Mul => (opcode::MUL, true),
        BinOp::Div => (opcode::DIV, true),
        BinOp::Mod => (opcode::REM, true),
        BinOp::And => (opcode::AND, true),
        BinOp::Or => (opcode::OR, true),
        BinOp::Eq => (opcode::SEQ, true),
        BinOp::Ne => (opcode::SNE, true),
        BinOp::Lt => (opcode::SLT, true),
        BinOp::Gt => (opcode::SGT, true),
        BinOp::Le => (opcode::SLE, true),
        BinOp::Ge => (opcode::SGE, true),
        BinOp::BitAnd => (opcode::AND, true),
        BinOp::BitOr => (opcode::OR, true),
        BinOp::BitXor => (opcode::XOR, true),
        BinOp::Shl => (opcode::SHL, true),
        BinOp::Shr => (opcode::SHR, true),
    };

    if use_r3 {
        emit.emit_r3(ocode, d, s1, s2, 0);
    }
    rd
}

/// 编译一元运算。
fn compile_unary(emit: &mut InstrEmitter, pool: &mut ConstPool, op: UnOp, expr: &Expr) -> VReg {
    let inner = compile_expr(emit, pool, expr);
    let rd = alloc_vreg();
    let d = vreg_to_preg(rd);
    let s = vreg_to_preg(inner);

    match op {
        UnOp::Neg => {
            emit.emit_mov(d, s);
            emit.emit_r1i(opcode::NEG, d, 0);
        }
        UnOp::Not => {
            emit.emit_r3(opcode::SEQ, d, s, reg::ZERO as u8, 0);
        }
        UnOp::BitNot => {
            emit.emit_mov(d, s);
            emit.emit_r1i(opcode::NOT, d, 0);
        }
    }
    rd
}

/// 编译函数调用表达式。
fn compile_call(emit: &mut InstrEmitter, pool: &mut ConstPool, name: &str, args: &[Expr]) -> VReg {
    // 检查是否为内置函数（编译期 IR 展开）
    if let Some(builtin) = crate::compiler::builtins::lookup(name) {
        let arg_vregs: Vec<VReg> = args
            .iter()
            .map(|arg| compile_expr(emit, pool, arg))
            .collect();
        return (builtin.expand)(emit, &arg_vregs);
    }

    // 参数传入 R4-R7
    for (i, arg) in args.iter().enumerate() {
        if i >= 4 {
            break; // 最多 4 个参数
        }
        let arg_reg = compile_expr(emit, pool, arg);
        let preg = match i {
            0 => reg::A0 as u8,
            1 => reg::A1 as u8,
            2 => reg::A2 as u8,
            3 => reg::A3 as u8,
            _ => unreachable!(),
        };
        emit.emit_mov(preg, vreg_to_preg(arg_reg));
    }
    // CALL offset 由链接阶段填充
    emit.emit_ji(opcode::CALL, 0);
    // 返回值在 R4
    let rd = alloc_vreg();
    emit.emit_mov(vreg_to_preg(rd), reg::A0 as u8);
    rd
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(expr: Expr) -> (Vec<u32>, ConstPool) {
        reset_vreg();
        let mut emit = InstrEmitter::new();
        let mut pool = ConstPool::new();
        compile_expr(&mut emit, &mut pool, &expr);
        (emit.text, pool)
    }

    #[test]
    fn int_literal() {
        let (text, _) = compile(Expr::Int(42));
        assert!(!text.is_empty());
        let instr = text[0];
        let op = (instr >> 24) as u8;
        assert_eq!(op, opcode::MOVI);
    }

    #[test]
    fn float_literal_in_rodata() {
        let (text, pool) = compile(Expr::Float(3.14));
        assert_eq!(pool.len(), 8); // one f64
        assert!(!text.is_empty());
    }

    #[test]
    fn string_literal_in_rodata() {
        let (text, pool) = compile(Expr::Str("hello".into()));
        assert!(pool.len() >= 6); // "hello\0"
        assert!(!text.is_empty());
    }

    #[test]
    fn bool_literal() {
        let (text, _) = compile(Expr::Bool(true));
        assert_eq!((text[0] >> 24) as u8, opcode::MOVI);
        // Should be MOVI rd, 1
    }

    #[test]
    fn binary_add() {
        let expr = Expr::Binary {
            op: BinOp::Add,
            lhs: Box::new(Expr::Int(2)),
            rhs: Box::new(Expr::Int(3)),
        };
        let (text, _) = compile(expr);
        // Expect: MOVI t0, 2; MOVI t1, 3; ADD t2, t0, t1
        assert!(text.len() >= 3);
        assert_eq!((text[2] >> 24) as u8, opcode::ADD);
    }

    #[test]
    fn binary_arithmetic_sequence() {
        let expr = Expr::Binary {
            op: BinOp::Mul,
            lhs: Box::new(Expr::Binary {
                op: BinOp::Add,
                lhs: Box::new(Expr::Int(2)),
                rhs: Box::new(Expr::Int(3)),
            }),
            rhs: Box::new(Expr::Int(4)),
        };
        let (text, _) = compile(expr);
        // 2+3 → ADD, then ×4 → MUL
        assert!(text.len() >= 4);
    }

    #[test]
    fn comparison_to_bool() {
        let expr = Expr::Binary {
            op: BinOp::Eq,
            lhs: Box::new(Expr::Int(1)),
            rhs: Box::new(Expr::Int(2)),
        };
        let (text, _) = compile(expr);
        assert_eq!((text[2] >> 24) as u8, opcode::SEQ);
    }

    #[test]
    fn unary_not() {
        let expr = Expr::Unary {
            op: UnOp::Not,
            expr: Box::new(Expr::Bool(true)),
        };
        let (text, _) = compile(expr);
        // Bool → MOVI, then NOT → SEQ rd, rs, zero
        assert_eq!((text[1] >> 24) as u8, opcode::SEQ);
    }

    #[test]
    fn const_pool_deduplication() {
        let mut pool = ConstPool::new();
        let off1 = pool.add_str("hello");
        let off2 = pool.add_str("hello");
        assert_eq!(off1, off2);
        assert_eq!(pool.len(), 6); // "hello\0" without trailing alignment padding
    }

    #[test]
    fn multiple_constants_in_pool() {
        let mut pool = ConstPool::new();
        pool.add_float(1.0);
        pool.add_str("test");
        assert!(pool.len() >= 13); // 8 (float) + 5 ("test\0")
    }

    #[test]
    fn big_int_literal_in_rodata() {
        // 超过 LCONST 范围（20 位有符号）→ 存入 .rodata
        let val = 0x1_0000_0000u64 as i64; // 4GB+1, 超出 32 位
        let (text, pool) = compile(Expr::Int(val));
        assert_eq!(pool.len(), 8); // one i64 = 8 bytes
        assert!(!text.is_empty());
        // 应为 LOAD 指令
        assert_eq!((text[0] >> 24) as u8, opcode::LOAD);
    }

    #[test]
    fn big_int_negative_in_rodata() {
        // 负数超出 LCONST 范围
        let val = -100_000_000i64;
        let (text, pool) = compile(Expr::Int(val));
        assert_eq!(pool.len(), 8);
        assert!(!text.is_empty());
    }

    #[test]
    fn big_int_deduplication() {
        let mut pool = ConstPool::new();
        let off1 = pool.add_big_int(0x1_0000_0000i64);
        let off2 = pool.add_big_int(0x1_0000_0000i64);
        assert_eq!(off1, off2);
        assert_eq!(pool.data.len(), 8); // single copy
    }
}
