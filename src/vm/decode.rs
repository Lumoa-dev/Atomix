//! 指令解码器 + 256 条目调度表。
//!
//! 覆盖 02-指令集规范.md §1.3 和 §3 的译码规范。

use crate::base::isa::{self, opcode};

// ─── 编码模板 ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncType {
    /// R3: [rd:4][rs1:4][rs2:4][funct:12]
    R3,
    /// R2I: [rd:4][rs1:4][imm:16]
    R2I,
    /// R1I: [rd:4][imm:20]
    R1I,
    /// JI: [offset:24]
    JI,
}

// ─── 解码后的操作数 ────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Operands {
    pub rd: u8,
    pub rs1: u8,
    pub rs2: u8,
    pub imm: u32,
    pub funct: u16,
    pub enc: EncType,
}

impl Operands {
    pub fn new(enc: EncType) -> Self {
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
    pub enc: EncType,
}

// ─── 调度表 ────────────────────────────────────────────

/// 256 条目的调度表。未使用的 opcode 指向 illegal_instruction。
pub fn dispatch_table() -> Vec<OpcodeEntry> {
    let mut table = vec![OpcodeEntry { name: "illegal", enc: EncType::JI }; 256];

    let entries: &[(u8, &str, EncType)] = &[
        (0x00, "NOP", EncType::JI), (0x01, "TRAP", EncType::R1I), (0x02, "THROW", EncType::R1I),
        (0x10, "MOV", EncType::R3), (0x11, "MOVI", EncType::R2I), (0x12, "LCONST", EncType::R1I),
        (0x13, "LOAD", EncType::R2I), (0x14, "STORE", EncType::R2I),
        (0x20, "ADD", EncType::R3), (0x21, "ADDI", EncType::R2I), (0x22, "SUB", EncType::R3),
        (0x23, "MUL", EncType::R3), (0x24, "DIV", EncType::R3), (0x25, "DIVU", EncType::R3),
        (0x26, "REM", EncType::R3), (0x27, "AND", EncType::R3), (0x28, "OR", EncType::R3),
        (0x29, "XOR", EncType::R3), (0x2A, "NOT", EncType::R1I), (0x2B, "NEG", EncType::R1I),
        (0x2C, "SHL", EncType::R3), (0x2D, "SHR", EncType::R3), (0x2E, "SHRU", EncType::R3),
        (0x2F, "FADD", EncType::R3), (0x30, "FSUB", EncType::R3), (0x31, "FMUL", EncType::R3),
        (0x32, "FDIV", EncType::R3), (0x33, "FEQ", EncType::R3), (0x34, "FNE", EncType::R3),
        (0x35, "FLT", EncType::R3), (0x36, "FLE", EncType::R3), (0x37, "ITOF", EncType::R1I),
        (0x38, "FTOI", EncType::R1I),
        (0x40, "SEQ", EncType::R3), (0x41, "SNE", EncType::R3), (0x42, "SLT", EncType::R3),
        (0x43, "SLE", EncType::R3), (0x44, "SGT", EncType::R3), (0x45, "SGE", EncType::R3),
        (0x50, "JMP", EncType::JI), (0x51, "JZ", EncType::R1I), (0x52, "JNZ", EncType::R1I),
        (0x53, "CALL", EncType::JI), (0x54, "JMPR", EncType::R1I), (0x55, "JALR", EncType::R2I),
        (0x60, "TASK_FORK", EncType::R1I), (0x61, "TASK_JOIN", EncType::R2I),
        (0x62, "TASK_RET", EncType::R1I), (0x63, "TASK_SELF", EncType::R1I),
        (0x70, "ECALL", EncType::R1I),
        (0x80, "MCPY", EncType::R3), (0x81, "MSET", EncType::R3),
        (0xF0, "FENCE", EncType::R1I), (0xF1, "CAS", EncType::R3),
    ];

    for &(op, name, enc) in entries {
        table[op as usize] = OpcodeEntry { name, enc };
    }
    table
}

// ─── 解码函数 ──────────────────────────────────────────

/// 根据编码模板解码指令。
pub fn decode(instr: u32, enc: EncType) -> Operands {
    let mut ops = Operands::new(enc);
    match enc {
        EncType::R3 => {
            let (rd, rs1, rs2, funct) = isa::decode_r3(instr);
            ops.rd = rd;
            ops.rs1 = rs1;
            ops.rs2 = rs2;
            ops.funct = funct;
        }
        EncType::R2I => {
            let (rd, rs1, imm) = isa::decode_r2i(instr);
            ops.rd = rd;
            ops.rs1 = rs1;
            ops.imm = imm as u32;
        }
        EncType::R1I => {
            let (rd, imm) = isa::decode_r1i(instr);
            ops.rd = rd;
            ops.imm = imm;
        }
        EncType::JI => {
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
        let ops = decode(instr, EncType::R3);
        assert_eq!(ops.rd, 8);
        assert_eq!(ops.rs1, 9);
        assert_eq!(ops.rs2, 10);
    }

    #[test]
    fn decode_movi_r2i() {
        let instr = isa::encode_r2i(opcode::MOVI, 8, 0, 42);
        let ops = decode(instr, EncType::R2I);
        assert_eq!(ops.rd, 8);
        assert_eq!(ops.imm, 42);
    }

    #[test]
    fn decode_lconst_r1i() {
        let instr = isa::encode_r1i(opcode::LCONST, 8, 12345);
        let ops = decode(instr, EncType::R1I);
        assert_eq!(ops.rd, 8);
        assert_eq!(ops.imm, 12345);
    }

    #[test]
    fn decode_jmp_ji() {
        let instr = isa::encode_ji(opcode::JMP, 0x100);
        let ops = decode(instr, EncType::JI);
        assert_eq!(ops.imm, 0x100);
    }

    #[test]
    fn all_opcodes_have_correct_enc() {
        // Spot check a few
        let table = get_table();
        assert_eq!(table[opcode::ADD as usize].enc, EncType::R3);
        assert_eq!(table[opcode::MOVI as usize].enc, EncType::R2I);
        assert_eq!(table[opcode::NOT as usize].enc, EncType::R1I);
        assert_eq!(table[opcode::JMP as usize].enc, EncType::JI);
    }

    #[test]
    fn decode_nop_jizero() {
        let instr = isa::encode_ji(opcode::NOP, 0);
        let ops = decode(instr, EncType::JI);
        assert_eq!(ops.imm, 0);
    }
}
