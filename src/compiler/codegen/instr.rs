//! 指令发射器和标签管理。
//!
//! 封装 02-指令集规范.md 的指令编码，提供:
//! - emit_opcode_* 系列方法
//! - emit_label / resolve_labels 前向引用处理
//! - 指令偏移跟踪（用于 .exn / .task 段定位）

use crate::base::isa::{self, opcode, reg};

// ─── 指令发射器 ────────────────────────────────────────

/// 指令发射器。持有原始指令字序列和标签映射。
#[derive(Debug, Clone)]
pub struct InstrEmitter {
    /// 已发射的指令字（每条 32 位）。
    pub text: Vec<u32>,
    /// 标签名 → 指令偏移（指令数，非字节数）。
    labels: std::collections::HashMap<String, usize>,
    /// 未解析的前向引用：(标签名, 指令位置, 修复偏移量函数)
    pending: Vec<PendingFixup>,
}

/// 待修复的前向引用。
#[derive(Debug, Clone)]
struct PendingFixup {
    label: String,
    /// 需要修复的指令在 text 中的索引。
    instr_idx: usize,
}

impl InstrEmitter {
    pub fn new() -> Self {
        Self {
            text: Vec::new(),
            labels: std::collections::HashMap::new(),
            pending: Vec::new(),
        }
    }

    /// 当前指令数（text 长度）。
    pub fn instr_count(&self) -> usize {
        self.text.len()
    }

    /// 当前指令偏移（字节）。
    pub fn byte_offset(&self) -> usize {
        self.text.len() * 4
    }

    // ── 标签 ──────────────────────────────────────

    /// 在当前指令位置定义标签。
    pub fn emit_label(&mut self, name: &str) {
        let idx = self.instr_count();
        self.labels.insert(name.to_string(), idx);
    }

    /// 查找已定义的标签位置（指令偏移）。
    pub fn lookup_label(&self, name: &str) -> Option<usize> {
        self.labels.get(name).copied()
    }

    /// 是否为已定义的标签。
    pub fn is_label_defined(&self, name: &str) -> bool {
        self.labels.contains_key(name)
    }

    // ── 指令发射 ──────────────────────────────────

    /// 发射 R3 格式指令。
    pub fn emit_r3(&mut self, op: u8, rd: u8, rs1: u8, rs2: u8, funct: u16) {
        self.text.push(isa::encode_r3(op, rd, rs1, rs2, funct));
    }

    /// 发射 R2I 格式指令。
    pub fn emit_r2i(&mut self, op: u8, rd: u8, rs1: u8, imm: u16) {
        self.text.push(isa::encode_r2i(op, rd, rs1, imm));
    }

    /// 发射 R1I 格式指令。
    pub fn emit_r1i(&mut self, op: u8, rd: u8, imm: u32) {
        self.text.push(isa::encode_r1i(op, rd, imm));
    }

    /// 发射 JI 格式指令（直接偏移）。
    pub fn emit_ji(&mut self, op: u8, offset: u32) {
        self.text.push(isa::encode_ji(op, offset));
    }

    /// 发射带标签的 JMP（后向引用立即解析，前向引用加入等待列表）。
    pub fn emit_jmp_to(&mut self, label: &str) {
        if let Some(target) = self.lookup_label(label) {
            let offset = Self::calc_jmp_offset(self.instr_count(), target);
            self.emit_ji(opcode::JMP, offset as u32);
        } else {
            // 前向引用：先发射 0，稍后修复
            self.pending.push(PendingFixup {
                label: label.to_string(),
                instr_idx: self.instr_count(),
            });
            self.emit_ji(opcode::JMP, 0);
        }
    }

    /// 发射带标签的 JZ（rd == 0 时跳转）。R1I 模板。
    pub fn emit_jz_to(&mut self, rd: u8, label: &str) {
        let instr_idx = self.instr_count();
        if let Some(target) = self.lookup_label(label) {
            let offset = Self::calc_jmp_offset(instr_idx, target);
            self.emit_r1i(opcode::JZ, rd, offset as u32);
        } else {
            self.pending.push(PendingFixup {
                label: label.to_string(),
                instr_idx,
            });
            self.emit_r1i(opcode::JZ, rd, 0);
        }
    }

    /// 发射带标签的 JNZ（rd != 0 时跳转）。R1I 模板。
    pub fn emit_jnz_to(&mut self, rd: u8, label: &str) {
        let instr_idx = self.instr_count();
        if let Some(target) = self.lookup_label(label) {
            let offset = Self::calc_jmp_offset(instr_idx, target);
            self.emit_r1i(opcode::JNZ, rd, offset as u32);
        } else {
            self.pending.push(PendingFixup {
                label: label.to_string(),
                instr_idx,
            });
            self.emit_r1i(opcode::JNZ, rd, 0);
        }
    }

    /// 发射带标签的 CALL。
    pub fn emit_call_to(&mut self, target: usize) {
        let offset = Self::calc_jmp_offset(self.instr_count(), target);
        self.emit_ji(opcode::CALL, offset as u32);
    }

    // ── 便捷指令 ──────────────────────────────────

    /// MOV rd, rs (寄存器到寄存器)
    pub fn emit_mov(&mut self, rd: u8, rs: u8) {
        self.emit_r3(opcode::MOV, rd, rs, 0, 0);
    }

    /// MOVI rd, imm (加载 16 位立即数，零扩展)
    pub fn emit_movi(&mut self, rd: u8, imm: u16) {
        self.emit_r2i(opcode::MOVI, rd, 0, imm);
    }

    /// ADD rd, rs1, rs2
    pub fn emit_add(&mut self, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r3(opcode::ADD, rd, rs1, rs2, 0);
    }

    /// NOP
    pub fn emit_nop(&mut self) {
        self.emit_ji(opcode::NOP, 0);
    }

    // ── 前向引用修复 ──────────────────────────────

    /// 解析所有前向引用（必须在所有标签定义后调用）。
    pub fn resolve_all(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for fixup in pending {
            if let Some(target) = self.lookup_label(&fixup.label) {
                let offset = Self::calc_jmp_offset(fixup.instr_idx, target);
                let instr = &mut self.text[fixup.instr_idx];
                let op = (*instr >> 24) as u8;
                let rd = ((*instr >> 20) & 0x0F) as u8;
                // JMP → JI 模板，JZ/JNZ → R1I 模板
                if op == opcode::JMP {
                    *instr = isa::encode_ji(op, offset as u32);
                } else {
                    *instr = isa::encode_r1i(op, rd, offset as u32);
                }
            } else {
                panic!("未定义的标签: {}", fixup.label);
            }
        }
    }

    /// 计算跳转偏移（以指令数计）。
    /// offset = target - (current + 1) （因为 pc 在取指后已指向下一条）
    fn calc_jmp_offset(from_instr: usize, to_instr: usize) -> i32 {
        to_instr as i32 - from_instr as i32
    }
}

impl Default for InstrEmitter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 寄存器分配骨架 ────────────────────────────────────

/// 虚拟寄存器（在寄存器分配前使用）。
pub type VReg = usize;

/// 物理寄存器编号。
pub type PReg = u8;

/// 将虚拟寄存器映射到物理寄存器。
/// Phase 1 简单实现：直接使用 T0-T5 临时寄存器。
pub fn vreg_to_preg(vreg: VReg) -> PReg {
    if vreg < 6 {
        (reg::T0 + vreg) as PReg
    } else {
        reg::TMP as PReg // 溢出使用 TMP
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::isa::opcode;

    #[test]
    fn emit_and_count() {
        let mut emit = InstrEmitter::new();
        assert_eq!(emit.instr_count(), 0);
        emit.emit_nop();
        assert_eq!(emit.instr_count(), 1);
        emit.emit_movi(reg::T0 as u8, 42);
        assert_eq!(emit.instr_count(), 2);
    }

    #[test]
    fn label_roundtrip() {
        let mut emit = InstrEmitter::new();
        emit.emit_nop();
        emit.emit_label("start");
        assert_eq!(emit.lookup_label("start"), Some(1));
    }

    #[test]
    fn backward_jmp() {
        let mut emit = InstrEmitter::new();
        emit.emit_label("loop");
        emit.emit_nop(); // instr 1
        emit.emit_jmp_to("loop"); // JMP back to instr 0
        emit.resolve_all();

        let instr = emit.text[1]; // JMP at instr 1
        let op = (instr >> 24) as u8;
        assert_eq!(op, opcode::JMP);
        // offset = target(0) - (current+1)(2) = -2
        let offset = isa::decode_ji(instr);
        assert_eq!(offset, -1); // JMP to instr 0 from instr 1 = offset -1
    }

    #[test]
    fn forward_jmp() {
        let mut emit = InstrEmitter::new();
        emit.emit_jmp_to("end"); // forward ref
        emit.emit_nop(); // skipped
        emit.emit_label("end");
        emit.emit_nop();
        emit.resolve_all();

        let instr = emit.text[0]; // JMP at instr 0
        let offset = isa::decode_ji(instr);
        assert_eq!(offset, 2); // JMP to instr 2 from instr 0 = offset 2
    }

    #[test]
    fn multiple_forward_refs() {
        let mut emit = InstrEmitter::new();
        emit.emit_jmp_to("a");
        emit.emit_jmp_to("b");
        emit.emit_label("a");
        emit.emit_nop(); // at instr 2
        emit.emit_label("b");
        emit.emit_nop(); // at instr 3
        emit.resolve_all();

        let offset_a = isa::decode_ji(emit.text[0]); // JMP at instr 0 → "a" at instr 2
        let offset_b = isa::decode_ji(emit.text[1]); // JMP at instr 1 → "b" at instr 3
        assert_eq!(offset_a, 2); // instr 0 → instr 2 = +2
        assert_eq!(offset_b, 2); // instr 1 → instr 3 = +2
    }

    #[test]
    fn encode_decode_consistency() {
        let mut emit = InstrEmitter::new();
        emit.emit_r3(opcode::ADD, reg::T0 as u8, reg::A0 as u8, reg::A1 as u8, 0);
        let instr = emit.text[0];
        let (rd, rs1, rs2, funct) = isa::decode_r3(instr);
        assert_eq!((instr >> 24) as u8, opcode::ADD);
        assert_eq!(rd, reg::T0 as u8);
        assert_eq!(rs1, reg::A0 as u8);
        assert_eq!(rs2, reg::A1 as u8);
        assert_eq!(funct, 0);
    }

    #[test]
    fn movi_and_add_sequence() {
        let mut emit = InstrEmitter::new();
        emit.emit_movi(reg::T0 as u8, 2);
        emit.emit_movi(reg::T1 as u8, 3);
        emit.emit_add(reg::T2 as u8, reg::T0 as u8, reg::T1 as u8);
        assert_eq!(emit.instr_count(), 3);
        // Verify ADD encoding
        let instr = emit.text[2];
        assert_eq!((instr >> 24) as u8, opcode::ADD);
    }
}
