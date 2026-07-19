//! 内置函数注册表。
//!
//! 每个内置函数 = 一个名字 + 一个 IR 展开函数。
//! 加新函数只需在 `all_builtins()` 加一行 + 写展开函数。
//! 不改 lexer、parser、semantic、isa、execute。

use crate::base::isa::{opcode, reg};
use crate::compiler::codegen::expr::alloc_vreg;
use crate::compiler::codegen::instr::{InstrEmitter, VReg, vreg_to_preg};

// ─── 内置函数条目 ──────────────────────────────────────

/// 内置函数注册条目。
pub struct BuiltinEntry {
    pub name: &'static str,
    /// IR 展开函数。
    /// 接收已编译好的参数 VReg，发射 IR 指令序列。
    /// 返回结果的 VReg。
    pub expand: fn(&mut InstrEmitter, &[VReg]) -> VReg,
}

// ─── 注册表 ────────────────────────────────────────────

/// 按名字查找内置函数。
pub fn lookup(name: &str) -> Option<&'static BuiltinEntry> {
    ALL_BUILTINS.iter().find(|e| e.name == name)
}

/// 内置函数列表（静态生命周期，用于 lookup 引用）。
pub static ALL_BUILTINS: &[BuiltinEntry] = &[
    BuiltinEntry {
        name: "int",
        expand: emit_int,
    },
    BuiltinEntry {
        name: "float",
        expand: emit_float,
    },
    BuiltinEntry {
        name: "bool",
        expand: emit_bool,
    },
    BuiltinEntry {
        name: "abs",
        expand: emit_abs,
    },
    BuiltinEntry {
        name: "min",
        expand: emit_min,
    },
    BuiltinEntry {
        name: "max",
        expand: emit_max,
    },
    BuiltinEntry {
        name: "print",
        expand: emit_print,
    },
    BuiltinEntry {
        name: "len",
        expand: emit_len,
    },
];

// ─── IR 展开函数 ───────────────────────────────────────

/// `int(x)` — 浮点转整数：FTOI rd, rs
fn emit_int(emit: &mut InstrEmitter, args: &[VReg]) -> VReg {
    let rd = alloc_vreg();
    if let Some(&arg) = args.first() {
        emit.emit_r1i(opcode::FTOI, vreg_to_preg(rd), vreg_to_preg(arg) as u32);
    }
    rd
}

/// `float(x)` — 整数转浮点：ITOF rd, rs
fn emit_float(emit: &mut InstrEmitter, args: &[VReg]) -> VReg {
    let rd = alloc_vreg();
    if let Some(&arg) = args.first() {
        emit.emit_r1i(opcode::ITOF, vreg_to_preg(rd), vreg_to_preg(arg) as u32);
    }
    rd
}

/// `bool(x)` — 与零不等：SNE rd, rs, zero
fn emit_bool(emit: &mut InstrEmitter, args: &[VReg]) -> VReg {
    let rd = alloc_vreg();
    if let Some(&arg) = args.first() {
        emit.emit_r3(
            opcode::SNE,
            vreg_to_preg(rd),
            vreg_to_preg(arg),
            reg::ZERO as u8,
            0,
        );
    }
    rd
}

/// `abs(x)` — 绝对值：SLT + 条件跳转 + NEG
fn emit_abs(emit: &mut InstrEmitter, args: &[VReg]) -> VReg {
    let rd = alloc_vreg();
    if let Some(&arg) = args.first() {
        let preg = vreg_to_preg(rd);
        let arg_preg = vreg_to_preg(arg);
        let skip_label = format!(".L_abs_skip_{}", rd);

        // rd = arg (先复制)
        emit.emit_mov(preg, arg_preg);
        // SLT tmp, rd, zero → rd < 0 时 tmp = 1
        emit.emit_r3(opcode::SLT, reg::TMP as u8, preg, reg::ZERO as u8, 0);
        // JZ tmp, .skip → tmp = 0 (非负) 时跳过
        emit.emit_jz_to(reg::TMP as u8, &skip_label);
        // NEG rd
        emit.emit_r1i(opcode::NEG, preg, 0);
        emit.emit_label(&skip_label);
        emit.resolve_all();
    }
    rd
}

/// `min(a, b)` — 取小值
fn emit_min(emit: &mut InstrEmitter, args: &[VReg]) -> VReg {
    let rd = alloc_vreg();
    if args.len() >= 2 {
        let a = vreg_to_preg(rd);
        let b = vreg_to_preg(args[1]);
        let skip_label = format!(".L_min_skip_{}", rd);

        // rd = arg0
        emit.emit_mov(a, vreg_to_preg(args[0]));
        // SLT tmp, rd, b → rd < b 时 tmp = 1
        emit.emit_r3(opcode::SLT, reg::TMP as u8, a, b, 0);
        // JNZ tmp, .skip → rd < b，跳过
        emit.emit_jnz_to(reg::TMP as u8, &skip_label);
        // rd = b
        emit.emit_mov(a, b);
        emit.emit_label(&skip_label);
        emit.resolve_all();
    }
    rd
}

/// `max(a, b)` — 取大值
fn emit_max(emit: &mut InstrEmitter, args: &[VReg]) -> VReg {
    let rd = alloc_vreg();
    if args.len() >= 2 {
        let a = vreg_to_preg(rd);
        let b = vreg_to_preg(args[1]);
        let skip_label = format!(".L_max_skip_{}", rd);

        // rd = arg0
        emit.emit_mov(a, vreg_to_preg(args[0]));
        // SGT tmp, rd, b → rd > b 时 tmp = 1
        emit.emit_r3(opcode::SGT, reg::TMP as u8, a, b, 0);
        // JNZ tmp, .skip → rd > b，跳过
        emit.emit_jnz_to(reg::TMP as u8, &skip_label);
        // rd = b
        emit.emit_mov(a, b);
        emit.emit_label(&skip_label);
        emit.resolve_all();
    }
    rd
}

/// `print(x)` — ECALL PRINT
fn emit_print(emit: &mut InstrEmitter, args: &[VReg]) -> VReg {
    let rd = alloc_vreg();
    if let Some(&arg) = args.first() {
        // MOV A0, arg
        emit.emit_mov(reg::A0 as u8, vreg_to_preg(arg));
        // ECALL PRINT (syscall 15)
        emit.emit_r1i(opcode::ECALL, reg::A0 as u8, crate::base::isa::ecall::PRINT);
        // 返回值 → rd
        emit.emit_mov(vreg_to_preg(rd), reg::A0 as u8);
    }
    rd
}

/// `len(x)` — ECALL LEN
fn emit_len(emit: &mut InstrEmitter, args: &[VReg]) -> VReg {
    let rd = alloc_vreg();
    if let Some(&arg) = args.first() {
        emit.emit_mov(reg::A0 as u8, vreg_to_preg(arg));
        emit.emit_r1i(opcode::ECALL, reg::A0 as u8, crate::base::isa::ecall::LEN);
        emit.emit_mov(vreg_to_preg(rd), reg::A0 as u8);
    }
    rd
}
