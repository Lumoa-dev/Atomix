//! 反汇编器 — 将 .text 段的 32 位指令字解码为可读的汇编文本。
//!
//! 完全复用现有组件：
//! - `runner::decode::dispatch_table()`  → opcode → (mnemonic, enc_template)
//! - `runner::decode::decode(instr, enc)` → u32 → Operands
//! - `base::isa::reg_name(r)`            → 寄存器编号 → 名称
//! - `base::isa::*` encode/decode 函数    → 四种编码模板

use crate::base::isa;
use crate::runner::decode;

/// 寄存器名的简写格式（大写，用于反汇编输出）。
fn reg(r: u8) -> String {
    let name = isa::reg_name(r as usize);
    if name == "?" {
        format!("R{}", r)
    } else {
        name.to_uppercase()
    }
}

/// 格式化一条指令为可读的汇编文本。
///
/// 格式示例：
/// ```text
/// 0x0000:  ADDI   R1, R0, 0        // sp = 0
/// 0x0004:  MOVI   R4, 0, 42        // a0 = 42
/// 0x0008:  ECALL  0, 15            // print
/// 0x000C:  TRAP   0                // halt
/// ```
pub fn format_instruction(pc: usize, instr: u32) -> String {
    let opcode = (instr >> 24) as u8;
    let table = decode::dispatch_table();
    let entry = &table[opcode as usize];
    let ops = decode::decode(instr, entry.enc);
    let mnemonic = entry.name;

    let operands = match entry.enc {
        isa::EncTemplate::R3 => {
            // MOV   rd, rs1, rs2
            // ADD   rd, rs1, rs2
            format!("{}, {}, {}", reg(ops.rd), reg(ops.rs1), reg(ops.rs2))
        }
        isa::EncTemplate::R2I => {
            // MOVI  rd, rs1, imm    (实际上 MOVI 使用 rd=dest, rs1=ignored, imm=value)
            // LOAD  rd, [rs1+imm]
            // STORE [rd+imm], rs1
            match mnemonic {
                "MOVI" => format!("{}, {}", reg(ops.rd), ops.imm as i16),
                "LOAD" => format!("{}, [{}+{}]", reg(ops.rd), reg(ops.rs1), ops.imm as i16),
                "STORE" => format!("[{}+{}], {}", reg(ops.rd), ops.imm as i16, reg(ops.rs1)),
                _ => format!("{}, {}, {}", reg(ops.rd), reg(ops.rs1), ops.imm as i16),
            }
        }
        isa::EncTemplate::R1I => {
            // ECALL rd, imm   (rd 通常为 0)
            // LCONST rd, imm
            // TRAP  imm
            match mnemonic {
                "TRAP" => format!("{}", ops.imm as i16),
                "ECALL" => {
                    let syscall_name = syscall_name(ops.imm);
                    format!("{}, {}    ; {}", ops.rd, ops.imm, syscall_name)
                }
                "TASK_FORK" | "TASK_JOIN" | "TASK_RET" | "TASK_SELF" => {
                    format!("{}, {}", reg(ops.rd), ops.imm as i16)
                }
                _ => format!("{}, {}", reg(ops.rd), ops.imm as i16),
            }
        }
        isa::EncTemplate::JI => {
            // JMP offset
            // TRAP (offset=0 = halt)
            if mnemonic == "illegal" {
                format!("; illegal instruction {:#010x}", instr)
            } else {
                let offset = ops.imm as i32;
                let target = (pc as i32).wrapping_add(offset);
                format!("{:#x}", target)
            }
        }
    };

    format!("{:#06x}:  {:<8} {}", pc, mnemonic, operands)
}

/// 反汇编一段指令序列。
pub fn disassemble_range(text: &[u32], start: usize, count: usize) -> Vec<String> {
    let end = (start + count).min(text.len());
    (start..end)
        .map(|i| format_instruction(i, text[i]))
        .collect()
}

/// 反汇编全部指令。
pub fn disassemble_all(text: &[u32]) -> Vec<String> {
    disassemble_range(text, 0, text.len())
}

/// ECALL 系统调用名称映射。
fn syscall_name(imm: u32) -> &'static str {
    match imm {
        0 => "ALLOC",
        1 => "FREE",
        2 => "TCP_CONNECT",
        3 => "TCP_SEND",
        4 => "TCP_RECV",
        5 => "TCP_LISTEN",
        6 => "TCP_ACCEPT",
        7 => "TCP_CLOSE",
        8 => "DNS_LOOKUP",
        9 => "FS_OPEN",
        10 => "FS_READ",
        11 => "FS_WRITE",
        12 => "FS_CLOSE",
        13 => "FS_SEEK",
        14 => "FS_STAT",
        15 => "PRINT",
        16 => "LEN",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::isa::{self, opcode, reg};

    #[test]
    fn format_movi() {
        // MOVI a0, 0, 42
        let instr = isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42);
        let s = format_instruction(0x0000, instr);
        assert!(s.contains("MOVI"));
        assert!(s.contains("A0"));
        assert!(s.contains("42"));
    }

    #[test]
    fn format_add() {
        // ADD t0, t1, t2
        let instr = isa::encode_r3(opcode::ADD, reg::T0 as u8, reg::T1 as u8, reg::T2 as u8, 0);
        let s = format_instruction(0x0004, instr);
        assert!(s.contains("ADD"));
        assert!(s.contains("T0"));
        assert!(s.contains("T1"));
        assert!(s.contains("T2"));
    }

    #[test]
    fn format_ecall_print() {
        // ECALL 0, 15 (PRINT)
        let instr = isa::encode_r1i(opcode::ECALL, 0, 15);
        let s = format_instruction(0x0008, instr);
        assert!(s.contains("ECALL"));
        assert!(s.contains("PRINT"));
    }

    #[test]
    fn format_jmp() {
        // JMP offset=8
        let instr = isa::encode_ji(opcode::JMP, 8);
        let s = format_instruction(0x0010, instr);
        assert!(s.contains("JMP"));
        assert!(s.contains("0x18")); // target = 0x10 + 8 = 0x18
    }

    #[test]
    fn format_trap_halt() {
        // TRAP 0 (halt)
        let instr = isa::encode_ji(opcode::TRAP, 0);
        let s = format_instruction(0x0000, instr);
        assert!(s.contains("TRAP"));
    }

    #[test]
    fn disassemble_multiple() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_r1i(opcode::ECALL, 0, 15), // print
            isa::encode_ji(opcode::TRAP, 0),       // halt
        ];
        let lines = disassemble_all(&text);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("MOVI"));
        assert!(lines[1].contains("ECALL"));
        assert!(lines[2].contains("TRAP"));
    }
}
