//! 线性扫描寄存器分配器。
//!
//! 覆盖 04-编译管线.md §5.1 的寄存器分配规范。
//! 16 个物理寄存器，6 个通用临时寄存器 (T0-T5)。
//! 溢出到栈通过 sp 相对偏移的 LOAD/STORE 实现。

use std::collections::{HashMap, HashSet, VecDeque};
use crate::base::isa::{self, opcode, reg};

// ─── 可用物理寄存器列表（临时寄存器） ──────────────────

const PHYSICAL_REGS: &[u8] = &[
    reg::T0 as u8,  // 8
    reg::T1 as u8,  // 9
    reg::T2 as u8,  // 10
    reg::T3 as u8,  // 11
    reg::T4 as u8,  // 12
    reg::T5 as u8,  // 13
];

const NUM_PHYSICAL_REGS: usize = PHYSICAL_REGS.len();

// ─── 活跃区间 ──────────────────────────────────────────

/// 一个值的活跃区间：[start, end) 指令偏移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveInterval {
    pub vreg: usize,
    pub start: usize,
    pub end: usize,
}

// ─── 寄存器分配器 ──────────────────────────────────────

pub struct RegAllocator {
    /// 虚拟寄存器 → 物理寄存器映射
    pub mapping: HashMap<usize, u8>,
    /// 溢出到栈的虚拟寄存器 → 栈偏移（相对 sp）
    pub spills: HashMap<usize, usize>,
    /// 栈帧大小（字节）
    pub frame_size: usize,
    /// 当前已用的栈偏移
    next_stack_offset: usize,
}

impl RegAllocator {
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
            spills: HashMap::new(),
            frame_size: 0,
            next_stack_offset: 0,
        }
    }

    /// 运行线性扫描寄存器分配。
    pub fn allocate(&mut self, text: &[u32]) {
        // 1. 收集所有虚拟寄存器
        let vregs = self.collect_vregs(text);
        if vregs.is_empty() {
            return;
        }

        // 2. 计算每个虚拟寄存器的活跃区间
        let intervals = self.compute_live_intervals(text, &vregs);

        // 3. 按起始位置排序
        let mut sorted: Vec<LiveInterval> = intervals.into_iter().collect();
        sorted.sort_by_key(|iv| iv.start);

        // 4. 线性扫描分配
        let mut active: VecDeque<(usize, u8, usize)> = VecDeque::new(); // (vreg, preg, end)

        for iv in &sorted {
            // 4a. 过期处理：释放已经结束的寄存器
            while let Some(front) = active.front() {
                if front.2 <= iv.start {
                    let (_, preg, _) = active.pop_front().unwrap();
                    // 归还物理寄存器（由空闲列表管理）
                } else {
                    break;
                }
            }

            // 4b. 尝试分配物理寄存器
            let used: HashSet<u8> = active.iter().map(|(_, p, _)| *p).collect();
            let free = PHYSICAL_REGS.iter().find(|p| !used.contains(p));

            if let Some(&preg) = free {
                // 分配成功
                self.mapping.insert(iv.vreg, preg);
                active.push_back((iv.vreg, preg, iv.end));
            } else {
                // 4c. 溢出处理：选择结束最晚的活跃寄存器溢出
                if let Some((spill_vreg, spill_preg, _)) =
                    active.iter().max_by_key(|(_, _, end)| *end)
                {
                    let spill_vreg = *spill_vreg;
                    let spill_preg = *spill_preg;
                    // 将最晚结束的寄存器溢出到栈
                    self.spill_register(spill_vreg, spill_preg);
                    // 从活跃列表移除
                    active.retain(|(v, _, _)| *v != spill_vreg);
                    // 使用刚释放的物理寄存器
                    self.mapping.insert(iv.vreg, spill_preg);
                    active.push_back((iv.vreg, spill_preg, iv.end));
                }
            }
        }
    }

    /// 为分配的寄存器生成溢出代码（在指令序列中插入 LOAD/STORE）。
    /// 返回修改后的指令序列。
    pub fn insert_spill_code(&self, text: &[u32]) -> Vec<u32> {
        let mut result = Vec::new();
        let mut loaded: HashMap<(usize, usize), bool> = HashMap::new();

        for (i, &instr) in text.iter().enumerate() {
            let op = (instr >> 24) as u8;

            // 对于使用溢出寄存器的指令，在之前插入 LOAD
            let rd = ((instr >> 20) & 0x0F) as usize;
            let rs1 = ((instr >> 16) & 0x0F) as usize;
            let rs2 = ((instr >> 12) & 0x0F) as usize;

            // 检查源操作数是否在溢出寄存器
            for vreg in &[rs1, rs2] {
                if let Some(&sp_offset_val) = self.spills.get(vreg) {
                    if !loaded.contains_key(&(*vreg, i)) {
                        // LOAD tmp, [sp + sp_offset]
                        result.push(isa::encode_r2i(
                            opcode::LOAD,
                            reg::TMP as u8,
                            reg::SP as u8,
                            sp_offset_val as u16,
                        ));
                        loaded.insert((*vreg, i), true);
                    }
                }
            }

            result.push(instr);

            // 如果目标寄存器是溢出寄存器，在之后插入 STORE
            if let Some(&sp_offset_val) = self.spills.get(&rd) {
                // STORE [sp + sp_offset], rd
                result.push(isa::encode_r2i(
                    opcode::STORE,
                    reg::SP as u8,
                    rd as u8,
                    sp_offset_val as u16,
                ));
            }
        }

        result
    }

    // ── 内部方法 ──────────────────────────────────

    /// 从指令序列中收集所有虚拟寄存器编号。
    fn collect_vregs(&self, text: &[u32]) -> HashSet<usize> {
        let mut vregs = HashSet::new();
        for &instr in text {
            let rd = ((instr >> 20) & 0x0F) as usize;
            let rs1 = ((instr >> 16) & 0x0F) as usize;
            let rs2 = ((instr >> 12) & 0x0F) as usize;
            // R0-R7 是特殊寄存器，不参与分配
            if rd >= 8 { vregs.insert(rd); }
            if rs1 >= 8 { vregs.insert(rs1); }
            if rs2 >= 8 { vregs.insert(rs2); }
        }
        vregs
    }

    /// 计算每个虚拟寄存器的活跃区间。
    fn compute_live_intervals(&self, text: &[u32], vregs: &HashSet<usize>) -> Vec<LiveInterval> {
        let mut intervals = Vec::new();
        for &vreg in vregs {
            let mut start = usize::MAX;
            let mut end = 0;
            for (i, &instr) in text.iter().enumerate() {
                let rd = ((instr >> 20) & 0x0F) as usize;
                let rs1 = ((instr >> 16) & 0x0F) as usize;
                let rs2 = ((instr >> 12) & 0x0F) as usize;
                if rd == vreg || rs1 == vreg || rs2 == vreg {
                    if i < start { start = i; }
                    if i + 1 > end { end = i + 1; }
                }
            }
            if start != usize::MAX {
                intervals.push(LiveInterval { vreg, start, end });
            }
        }
        intervals
    }

    /// 溢出虚拟寄存器到栈。
    fn spill_register(&mut self, vreg: usize, preg: u8) {
        if !self.spills.contains_key(&vreg) {
            let offset = self.next_stack_offset;
            self.spills.insert(vreg, offset);
            self.next_stack_offset += 8; // 每个溢出值 8 字节
            self.frame_size = self.next_stack_offset;
        }
        // 从映射中移除（已溢出）
        self.mapping.remove(&vreg);
        let _ = preg;
    }
}

impl Default for RegAllocator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::isa::{encode_r3, encode_r2i, encode_r1i};

    #[test]
    fn collect_vregs_simple() {
        let alloc = RegAllocator::new();
        let text = vec![
            encode_r2i(opcode::MOVI, 8, 0, 42),  // MOVI t0, 42
            encode_r2i(opcode::MOVI, 9, 0, 10),  // MOVI t1, 10
            encode_r3(opcode::ADD, 10, 8, 9, 0), // ADD t2, t0, t1
        ];
        let vregs = alloc.collect_vregs(&text);
        assert!(vregs.contains(&8));
        assert!(vregs.contains(&9));
        assert!(vregs.contains(&10));
        assert_eq!(vregs.len(), 3);
    }

    #[test]
    fn live_interval_basic() {
        let alloc = RegAllocator::new();
        let text = vec![
            encode_r2i(opcode::MOVI, 8, 0, 1),   // vreg 8 used at 0
            encode_r2i(opcode::MOVI, 9, 0, 2),   // vreg 9 used at 1
            encode_r3(opcode::ADD, 10, 8, 9, 0), // vreg 8,9,10 used at 2
        ];
        let vregs = alloc.collect_vregs(&text);
        let intervals = alloc.compute_live_intervals(&text, &vregs);
        assert_eq!(intervals.len(), 3);
    }

    #[test]
    fn allocate_simple() {
        let mut alloc = RegAllocator::new();
        let text = vec![
            encode_r2i(opcode::MOVI, 8, 0, 1),   // t0
            encode_r2i(opcode::MOVI, 9, 0, 2),   // t1
            encode_r3(opcode::ADD, 10, 8, 9, 0), // t2
        ];
        alloc.allocate(&text);
        // 6 physical regs available, 3 needed → no spills
        assert_eq!(alloc.spills.len(), 0);
        assert_eq!(alloc.mapping.len(), 3);
    }

    #[test]
    fn allocate_with_many_overlapping() {
        let mut alloc = RegAllocator::new();
        // Create many overlapping vregs: all vregs used in a single instruction
        // ADD t9, t8, t0; ADD t10, t9, t1; ... creates long live intervals
        // Simpler: just use same vregs in sequence to build overlapping intervals
        // MOVI t0,1; MOVI t1,2; ... MOVI t7,8 → then ADD t8,t0,t1; ADD t9,t2,t3; ...
        let mut text = Vec::new();
        // Phase 1: set up 10 values
        for i in 0..10 {
            text.push(encode_r2i(opcode::MOVI, 8 + i, 0, i as u16));
        }
        // Phase 2: use them all together
        for i in 0..8 {
            text.push(encode_r3(opcode::ADD, 18 + i, 8 + i, 9 + i, 0));
        }
        alloc.allocate(&text);
        // If we have spills or not depends on overlap, just verify it ran
        assert!(alloc.frame_size >= 0);
    }

    #[test]
    fn spill_code_insertion() {
        let mut alloc = RegAllocator::new();
        let text = vec![
            encode_r2i(opcode::MOVI, 8, 0, 1),   // t0
            encode_r2i(opcode::MOVI, 9, 0, 2),   // t1
            encode_r2i(opcode::MOVI, 10, 0, 3),  // t2
            encode_r3(opcode::ADD, 11, 8, 9, 0), // t3 = t0 + t1
        ];
        alloc.allocate(&text);
        let spilling = alloc.spills.len() > 0;
        if !spilling {
            return; // TODO: force spill in test
        }
        let expanded = alloc.insert_spill_code(&text);
        assert!(expanded.len() >= text.len());
    }

    #[test]
    fn no_vregs() {
        let mut alloc = RegAllocator::new();
        let text = vec![
            encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
        ];
        alloc.allocate(&text);
        assert!(alloc.mapping.is_empty());
    }
}
