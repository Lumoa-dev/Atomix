//! Atomix ISA — opcode constants, encoding templates, register model.
//! Source of truth shared by compiler, VM, debugger, disassembler.

// ─── Opcodes ──────────────────────────────────────────────────────

pub mod opcode {
    // System / Control (0x00–0x0F)
    pub const NOP: u8 = 0x00;
    pub const TRAP: u8 = 0x01;
    pub const THROW: u8 = 0x02;

    // Data Movement (0x10–0x1F)
    pub const MOV: u8 = 0x10;
    pub const MOVI: u8 = 0x11;
    pub const LCONST: u8 = 0x12;
    pub const LOAD: u8 = 0x13;
    pub const STORE: u8 = 0x14;

    // Integer Arithmetic (0x20–0x2E)
    pub const ADD: u8 = 0x20;
    pub const ADDI: u8 = 0x21;
    pub const SUB: u8 = 0x22;
    pub const MUL: u8 = 0x23;
    pub const DIV: u8 = 0x24;
    pub const DIVU: u8 = 0x25;
    pub const REM: u8 = 0x26;
    pub const AND: u8 = 0x27;
    pub const OR: u8 = 0x28;
    pub const XOR: u8 = 0x29;
    pub const NOT: u8 = 0x2A;
    pub const NEG: u8 = 0x2B;
    pub const SHL: u8 = 0x2C;
    pub const SHR: u8 = 0x2D;
    pub const SHRU: u8 = 0x2E;

    // Floating Point (0x2F–0x38)
    pub const FADD: u8 = 0x2F;
    pub const FSUB: u8 = 0x30;
    pub const FMUL: u8 = 0x31;
    pub const FDIV: u8 = 0x32;
    pub const FEQ: u8 = 0x33;
    pub const FNE: u8 = 0x34;
    pub const FLT: u8 = 0x35;
    pub const FLE: u8 = 0x36;
    pub const ITOF: u8 = 0x37;
    pub const FTOI: u8 = 0x38;

    // Compare / Set (0x40–0x45)
    pub const SEQ: u8 = 0x40;
    pub const SNE: u8 = 0x41;
    pub const SLT: u8 = 0x42;
    pub const SLE: u8 = 0x43;
    pub const SGT: u8 = 0x44;
    pub const SGE: u8 = 0x45;

    // Control Flow (0x50–0x55)
    pub const JMP: u8 = 0x50;
    pub const JZ: u8 = 0x51;
    pub const JNZ: u8 = 0x52;
    pub const CALL: u8 = 0x53;
    pub const JMPR: u8 = 0x54;
    pub const JALR: u8 = 0x55;

    // Concurrency (0x60–0x63)
    pub const TASK_FORK: u8 = 0x60;
    pub const TASK_JOIN: u8 = 0x61;
    pub const TASK_RET: u8 = 0x62;
    pub const TASK_SELF: u8 = 0x63;

    // System Call (0x70)
    pub const ECALL: u8 = 0x70;

    // Memory Operations (0x80–0x81)
    pub const MCPY: u8 = 0x80;
    pub const MSET: u8 = 0x81;

    // Atomic / Memory Barrier (0xF0–0xF1)
    pub const FENCE: u8 = 0xF0;
    pub const CAS: u8 = 0xF1;
}

// ─── Encoding Templates ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncTemplate {
    /// R3: [opcode:8][rd:4][rs1:4][rs2:4][funct:12]
    R3,
    /// R2I: [opcode:8][rd:4][rs1:4][imm:16]
    R2I,
    /// R1I: [opcode:8][rd:4][imm:20]
    R1I,
    /// JI: [opcode:8][offset:24]
    JI,
}

// ─── Register Model ───────────────────────────────────────────────

pub const REG_COUNT: usize = 16;

pub mod reg {
    pub const ZERO: usize = 0; // r0 — hardwired zero
    pub const SP: usize = 1; // r1 — stack pointer
    pub const FP: usize = 2; // r2 — frame pointer
    pub const RA: usize = 3; // r3 — return address
    pub const A0: usize = 4; // r4 — argument / return value 0
    pub const A1: usize = 5; // r5
    pub const A2: usize = 6; // r6
    pub const A3: usize = 7; // r7
    pub const T0: usize = 8; // r8 — temporary
    pub const T1: usize = 9;
    pub const T2: usize = 10;
    pub const T3: usize = 11;
    pub const T4: usize = 12;
    pub const T5: usize = 13;
    pub const TASK_ID: usize = 14; // r14 — read-only task id
    pub const TMP: usize = 15; // r15 — temporary
}

/// Human-readable register name for disassembly.
pub fn reg_name(r: usize) -> &'static str {
    match r {
        0 => "zero",
        1 => "sp",
        2 => "fp",
        3 => "ra",
        4 => "a0",
        5 => "a1",
        6 => "a2",
        7 => "a3",
        8 => "t0",
        9 => "t1",
        10 => "t2",
        11 => "t3",
        12 => "t4",
        13 => "t5",
        14 => "task_id",
        15 => "tmp",
        _ => "?",
    }
}

// ─── Encode / Decode ─────────────────────────────────────────────

/// Pack an R3-format instruction.
#[inline]
pub fn encode_r3(opcode: u8, rd: u8, rs1: u8, rs2: u8, funct: u16) -> u32 {
    ((opcode as u32) << 24)
        | ((rd as u32 & 0x0F) << 20)
        | ((rs1 as u32 & 0x0F) << 16)
        | ((rs2 as u32 & 0x0F) << 12)
        | (funct as u32 & 0x0FFF)
}

/// Decode an R3-format instruction.
#[inline]
pub fn decode_r3(instr: u32) -> (u8, u8, u8, u16) {
    // returns (rd, rs1, rs2, funct)
    (
        ((instr >> 20) & 0x0F) as u8,
        ((instr >> 16) & 0x0F) as u8,
        ((instr >> 12) & 0x0F) as u8,
        (instr & 0x0FFF) as u16,
    )
}

/// Pack an R2I-format instruction.
#[inline]
pub fn encode_r2i(opcode: u8, rd: u8, rs1: u8, imm: u16) -> u32 {
    ((opcode as u32) << 24)
        | ((rd as u32 & 0x0F) << 20)
        | ((rs1 as u32 & 0x0F) << 16)
        | (imm as u32 & 0xFFFF)
}

/// Decode an R2I-format instruction.
#[inline]
pub fn decode_r2i(instr: u32) -> (u8, u8, u16) {
    // returns (rd, rs1, imm)
    (
        ((instr >> 20) & 0x0F) as u8,
        ((instr >> 16) & 0x0F) as u8,
        (instr & 0xFFFF) as u16,
    )
}

/// Pack an R1I-format instruction.
#[inline]
pub fn encode_r1i(opcode: u8, rd: u8, imm: u32) -> u32 {
    ((opcode as u32) << 24) | ((rd as u32 & 0x0F) << 20) | (imm & 0x000F_FFFF)
}

/// Decode an R1I-format instruction.
#[inline]
pub fn decode_r1i(instr: u32) -> (u8, u32) {
    // returns (rd, imm)
    (((instr >> 20) & 0x0F) as u8, instr & 0x000F_FFFF)
}

/// Pack a JI-format instruction.
#[inline]
pub fn encode_ji(opcode: u8, offset: u32) -> u32 {
    ((opcode as u32) << 24) | (offset & 0x00FF_FFFF)
}

/// Decode a JI-format instruction (signed 24-bit offset).
#[inline]
pub fn decode_ji(instr: u32) -> i32 {
    let raw = (instr & 0x00FF_FFFF) as i32;
    // sign-extend from 24 bits
    (raw << 8) >> 8
}

// ─── Profile ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Default. Must run inside atomix-runner.
    Runner,
    /// Requires an RTOS abstraction layer.
    Embedded,
    /// Pure compute closure. No ECALL / TASK_* / TRAP(HALT).
    Bare,
}

impl Profile {
    pub fn from_flags(flags: u16) -> Self {
        match (flags >> 3) & 0x03 {
            0b00 => Profile::Runner,
            0b01 => Profile::Embedded,
            0b10 => Profile::Bare,
            _ => Profile::Runner, // reserved → default
        }
    }

    pub fn to_flags_bits(self) -> u16 {
        match self {
            Profile::Runner => 0b00 << 3,
            Profile::Embedded => 0b01 << 3,
            Profile::Bare => 0b10 << 3,
        }
    }
}

// ─── Fence Modes ──────────────────────────────────────────────────

pub mod fence {
    pub const FULL: u32 = 0;
    pub const ACQUIRE: u32 = 1;
    pub const RELEASE: u32 = 2;
    pub const IO: u32 = 3;
}

// ─── ECALL Syscall Numbers ────────────────────────────────────────

pub mod ecall {
    pub const ALLOC: u32 = 0;
    pub const FREE: u32 = 1;
    pub const TCP_CONNECT: u32 = 2;
    pub const TCP_SEND: u32 = 3;
    pub const TCP_RECV: u32 = 4;
    pub const TCP_LISTEN: u32 = 5;
    pub const TCP_ACCEPT: u32 = 6;
    pub const TCP_CLOSE: u32 = 7;
    pub const DNS_LOOKUP: u32 = 8;
    pub const FS_OPEN: u32 = 9;
    pub const FS_READ: u32 = 10;
    pub const FS_WRITE: u32 = 11;
    pub const FS_CLOSE: u32 = 12;
    pub const FS_SEEK: u32 = 13;
    pub const FS_STAT: u32 = 14;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r3_roundtrip() {
        let instr = encode_r3(opcode::ADD, reg::T0 as u8, reg::A0 as u8, reg::A1 as u8, 0);
        let (rd, rs1, rs2, funct) = decode_r3(instr);
        assert_eq!((instr >> 24) as u8, opcode::ADD);
        assert_eq!(rd, reg::T0 as u8);
        assert_eq!(rs1, reg::A0 as u8);
        assert_eq!(rs2, reg::A1 as u8);
        assert_eq!(funct, 0);
    }

    #[test]
    fn r2i_roundtrip() {
        let instr = encode_r2i(opcode::ADDI, reg::T1 as u8, reg::ZERO as u8, 42);
        let (rd, rs1, imm) = decode_r2i(instr);
        assert_eq!(rd, reg::T1 as u8);
        assert_eq!(rs1, reg::ZERO as u8);
        assert_eq!(imm, 42);
    }

    #[test]
    fn ji_sign_extend() {
        let instr = encode_ji(opcode::JMP, (-8i32) as u32 & 0x00FF_FFFF);
        assert_eq!(decode_ji(instr), -8);
    }

    #[test]
    fn profile_encode() {
        let flags = Profile::Embedded.to_flags_bits();
        assert_eq!(Profile::from_flags(flags), Profile::Embedded);
    }
}
