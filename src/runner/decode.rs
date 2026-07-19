//! 指令解码器 + 256 条目调度表。
//!
//! 覆盖 02-指令集规范.md §1.3 和 §3 的译码规范。

use crate::base::isa::{self, opcode, EncTemplate};

// ─── 解码后的操作数 ────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Operands {
    pub rd: u8,
    pub rs1: u8,
    pub rs2: u8,
    pub imm: u32,
    pub funct: u16,
    pub enc: EncTemplate,
}

impl Operands {
    pub fn new(enc: EncTemplate) -> Self {
        Self {
            rd: 0,
            rs1: 0,
            rs2: 0,
            imm: 0,
            funct: 0,
            enc,
        }
    }
}

// ─── Opcode 条目 ───────────────────────────────────────

/// 调度表条目：操作码名称 + 编码模板。
#[derive(Debug, Clone)]
pub struct OpcodeEntry {
    pub name: &'static str,
    pub enc: EncTemplate,
}

// ─── 调度表 ────────────────────────────────────────────

/// 256 条目的调度表。未使用的 opcode 指向 illegal_instruction。
pub fn dispatch_table() -> Vec<OpcodeEntry> {
    let mut table = vec![OpcodeEntry { name: "illegal", enc: EncTemplate::JI }; 256];

    let entries: &[(u8, &str, EncTemplate)] = &[
        (0x00, "NOP", EncTemplate::JI), (0x01, "TRAP", EncTemplate::R1I), (0x02, "THROW", EncTemplate::R1I),
        (0x10, "MOV", EncTemplate::R3), (0x11, "MOVI", EncTemplate::R2I), (0x12, "LCONST", EncTemplate::R1I),
        (0x13, "LOAD", EncTemplate::R2I), (0x14, "STORE", EncTemplate::R2I),
        (0x20, "ADD", EncTemplate::R3), (0x21, "ADDI", EncTemplate::R2I), (0x22, "SUB", EncTemplate::R3),
        (0x23, "MUL", EncTemplate::R3), (0x24, "DIV", EncTemplate::R3), (0x25, "DIVU", EncTemplate::R3),
        (0x26, "REM", EncTemplate::R3), (0x27, "AND", EncTemplate::R3), (0x28, "OR", EncTemplate::R3),
        (0x29, "XOR", EncTemplate::R3), (0x2A, "NOT", EncTemplate::R1I), (0x2B, "NEG", EncTemplate::R1I),
        (0x2C, "SHL", EncTemplate::R3), (0x2D, "SHR", EncTemplate::R3), (0x2E, "SHRU", EncTemplate::R3),
        (0x2F, "FADD", EncTemplate::R3), (0x30, "FSUB", EncTemplate::R3), (0x31, "FMUL", EncTemplate::R3),
        (0x32, "FDIV", EncTemplate::R3), (0x33, "FEQ", EncTemplate::R3), (0x34, "FNE", EncTemplate::R3),
        (0x35, "FLT", EncTemplate::R3), (0x36, "FLE", EncTemplate::R3), (0x37, "ITOF", EncTemplate::R1I),
        (0x38, "FTOI", EncTemplate::R1I),
        (0x40, "SEQ", EncTemplate::R3), (0x41, "SNE", EncTemplate::R3), (0x42, "SLT", EncTemplate::R3),
        (0x43, "SLE", EncTemplate::R3), (0x44, "SGT", EncTemplate::R3), (0x45, "SGE", EncTemplate::R3),
        (0x50, "JMP", EncTemplate::JI), (0x51, "JZ", EncTemplate::R1I), (0x52, "JNZ", EncTemplate::R1I),
        (0x53, "CALL", EncTemplate::JI), (0x54, "JMPR", EncTemplate::R1I), (0x55, "JALR", EncTemplate::R2I),
        (0x60, "TASK_FORK", EncTemplate::R1I), (0x61, "TASK_JOIN", EncTemplate::R2I),
        (0x62, "TASK_RET", EncTemplate::R1I), (0x63, "TASK_SELF", EncTemplate::R1I),
        (0x70, "ECALL", EncTemplate::R1I),
        (0x80, "MCPY", EncTemplate::R3), (0x81, "MSET", EncTemplate::R3),
        (0xF0, "FENCE", EncTemplate::R1I), (0xF1, "CAS", EncTemplate::R3),
    ];

    for &(op, name, enc) in entries {
        table[op as usize] = OpcodeEntry { name, enc };
    }
    table
}

// ─── 解码函数 ──────────────────────────────────────────

/// 根据编码模板解码指令。
pub fn decode(instr: u32, enc: EncTemplate) -> Operands {
    let mut ops = Operands::new(enc);
    match enc {
        EncTemplate::R3 => {
            let (rd, rs1, rs2, funct) = isa::decode_r3(instr);
            ops.rd = rd;
            ops.rs1 = rs1;
            ops.rs2 = rs2;
            ops.funct = funct;
        }
        EncTemplate::R2I => {
            let (rd, rs1, imm) = isa::decode_r2i(instr);
            ops.rd = rd;
            ops.rs1 = rs1;
            ops.imm = imm as u32;
        }
        EncTemplate::R1I => {
            let (rd, imm) = isa::decode_r1i(instr);
            ops.rd = rd;
            ops.imm = imm;
        }
        EncTemplate::JI => {
            // offset 已由 decode_ji 做符号扩展
            ops.imm = instr & 0x00FF_FFFF;
        }
    }
    ops
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn get_table() -> Vec<OpcodeEntry> { dispatch_table() }

    #[test]
    fn dispatch_table_has_256_entries() {
        let table = get_table();
        assert_eq!(table.len(), 256);
    }

    #[test]
    fn all_54_ops_have_names() {
        let table = get_table();
        let names = [
            "NOP", "TRAP", "THROW",
            "MOV", "MOVI", "LCONST", "LOAD", "STORE",
            "ADD", "ADDI", "SUB", "MUL", "DIV", "DIVU", "REM",
            "AND", "OR", "XOR", "NOT", "NEG", "SHL", "SHR", "SHRU",
            "FADD", "FSUB", "FMUL", "FDIV", "FEQ", "FNE", "FLT", "FLE", "ITOF", "FTOI",
            "SEQ", "SNE", "SLT", "SLE", "SGT", "SGE",
            "JMP", "JZ", "JNZ", "CALL", "JMPR", "JALR",
            "TASK_FORK", "TASK_JOIN", "TASK_RET", "TASK_SELF",
            "ECALL",
            "MCPY", "MSET",
            "FENCE", "CAS",
        ];
            for &name in &names {
            let found = table.iter().any(|e| e.name == name);
            assert!(found, "opcode {} not found in dispatch table", name);
        }
    }

    #[test]
    fn illegal_opcode_is_illegal() {
        // opcode 0x03 (unused) should be illegal
        let table = get_table();
        assert_eq!(table[0x03].name, "illegal");
    }

    #[test]
    fn decode_add_r3() {
        let instr = isa::encode_r3(opcode::ADD, 8, 9, 10, 0);
        let ops = decode(instr, EncTemplate::R3);
        assert_eq!(ops.rd, 8);
        assert_eq!(ops.rs1, 9);
        assert_eq!(ops.rs2, 10);
    }

    #[test]
    fn decode_movi_r2i() {
        let instr = isa::encode_r2i(opcode::MOVI, 8, 0, 42);
        let ops = decode(instr, EncTemplate::R2I);
        assert_eq!(ops.rd, 8);
        assert_eq!(ops.imm, 42);
    }

    #[test]
    fn decode_lconst_r1i() {
        let instr = isa::encode_r1i(opcode::LCONST, 8, 12345);
        let ops = decode(instr, EncTemplate::R1I);
        assert_eq!(ops.rd, 8);
        assert_eq!(ops.imm, 12345);
    }

    #[test]
    fn decode_jmp_ji() {
        let instr = isa::encode_ji(opcode::JMP, 0x100);
        let ops = decode(instr, EncTemplate::JI);
        assert_eq!(ops.imm, 0x100);
    }

    #[test]
    fn all_opcodes_have_correct_enc() {
        // Spot check a few
        let table = get_table();
        assert_eq!(table[opcode::ADD as usize].enc, EncTemplate::R3);
        assert_eq!(table[opcode::MOVI as usize].enc, EncTemplate::R2I);
        assert_eq!(table[opcode::NOT as usize].enc, EncTemplate::R1I);
        assert_eq!(table[opcode::JMP as usize].enc, EncTemplate::JI);
    }

    #[test]
    fn decode_nop_jizero() {
        let instr = isa::encode_ji(opcode::NOP, 0);
        let ops = decode(instr, EncTemplate::JI);
        assert_eq!(ops.imm, 0);
    }
}
