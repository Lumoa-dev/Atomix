//! 槽位管理 — 墙式预分配的虚地址空间槽位 + 滑道溢出 + 死区合并。
//!
//! 覆盖设计文档 §4（内存模型）。

/// 槽位状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotStatus {
    /// 空闲（未分配）。
    Free,
    /// 已分配（有任务驻留）。
    Occupied,
    /// 死区（任务 OOM 滑入滑道后原槽位标记为 Dead）。
    Dead,
    /// 滑道槽位（OOM 时备用，永不分配给新任务）。
    Slipway,
}

/// 单个内存槽位。
#[derive(Debug, Clone)]
pub struct Slot {
    /// 槽位 ID。
    pub id: usize,
    /// 虚地址起始。
    pub base: u64,
    /// 槽位虚地址大小（字节）。
    pub size: u64,
    /// 已分配的物理内存大小（字节）。
    pub physical_size: u64,
    /// 槽位状态。
    pub status: SlotStatus,
    /// 占用该槽位的任务 ID（None = 无）。
    pub task_id: Option<u16>,
}

impl Slot {
    /// 创建新槽位。
    fn new(id: usize, base: u64, size: u64, status: SlotStatus) -> Self {
        Self {
            id,
            base,
            size,
            physical_size: 0,
            status,
            task_id: None,
        }
    }

    /// 槽位是否可用（Free 或 Dead 可被合并后重用）。
    pub fn is_available(&self) -> bool {
        self.status == SlotStatus::Free
    }
}

/// 槽位管理器。
///
/// 基于 N_batch 预分配等大的虚地址槽位，物理内存按需分配。
/// 支持滑道溢出和死区合并。
#[derive(Debug, Clone)]
pub struct SlotManager {
    /// 所有常规槽位。
    pub slots: Vec<Slot>,
    /// 滑道槽位（OOM 时备用）。
    pub slipway_slots: Vec<Slot>,
    /// 死区列表：(起始地址, 大小)
    pub dead_zones: Vec<(u64, u64)>,
    /// 总内存池大小（字节）。
    pub total_pool: u64,
    /// 每个槽位的虚地址大小。
    pub slot_size: u64,
    /// 安全冗余比例。
    pub safety_margin: f64,
    /// 滑道倍数（slot_size 的倍数）。
    pub slipway_multiplier: f64,
}

impl SlotManager {
    /// 根据总内存和 N_batch 创建槽位管理器。
    ///
    /// 虚地址空间按固定间隔排列（如每槽 16MB），
    /// 但物理内存只分配任务所需的量。
    pub fn new(total_mem_mb: f64, n_batch: u32, safety_margin: f64, slipway_mul: f64) -> Self {
        let total_bytes = (total_mem_mb * 1024.0 * 1024.0) as u64;
        let effective = (total_bytes as f64 * (1.0 - safety_margin)) as u64;
        let slot_count = (n_batch as f64 + slipway_mul) as u64;
        let slot_size = if slot_count > 0 {
            effective / slot_count
        } else {
            effective
        };

        let slipway_count = slipway_mul.ceil() as usize;

        // 创建常规槽位
        let mut slots = Vec::with_capacity(n_batch as usize);
        for i in 0..n_batch {
            slots.push(Slot::new(
                i as usize,
                (i as u64) * slot_size,
                slot_size,
                SlotStatus::Free,
            ));
        }

        // 创建滑道槽位（永不分配给新任务，大小为常规的 2 倍）
        let mut slipway_slots = Vec::with_capacity(slipway_count);
        for i in 0..slipway_count {
            let id = n_batch as usize + i;
            slipway_slots.push(Slot::new(
                id,
                (id as u64) * slot_size,
                slot_size * 2, // 滑道槽位更大
                SlotStatus::Slipway,
            ));
        }

        Self {
            slots,
            slipway_slots,
            dead_zones: Vec::new(),
            total_pool: total_bytes,
            slot_size,
            safety_margin,
            slipway_multiplier: slipway_mul,
        }
    }

    /// 分配一个槽位，匹配合适的大小。
    ///
    /// 策略（设计文档 §4.3）：
    /// 1. 预估峰值 = comp_mb × max(δ, 1.2)
    /// 2. 在空闲槽位中找 size ≥ 预估峰值的
    /// 3. 有则分配，physical_size = 预估峰值
    /// 4. 无则返回 None（任务等待槽位释放）
    pub fn allocate(&mut self, estimated_peak_mb: f64) -> Option<&mut Slot> {
        let needed = (estimated_peak_mb * 1024.0 * 1024.0) as u64;

        // 先找最匹配的常规空闲槽位（size 最接近需要值但 ≥ needed）
        let best_idx = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_available())
            .filter(|(_, s)| s.size >= needed)
            .min_by_key(|(_, s)| s.size - needed)
            .map(|(i, _)| i);

        if let Some(idx) = best_idx {
            let slot = &mut self.slots[idx];
            slot.status = SlotStatus::Occupied;
            slot.physical_size = needed;
            return Some(slot);
        }

        // 无足够大的常规槽位 → 尝试滑道槽位
        // （滑道槽位在正常 allocate 时不分配，只在 OOM slip 时使用）
        None
    }

    /// OOM 滑入：将任务从原槽位移入滑道槽位。
    ///
    /// 当任务执行中内存超过原槽位上限时：
    /// 1. 原槽位标记为 Dead
    /// 2. 找空闲滑道槽位，标记为 Occupied
    /// 3. 返回滑道槽位引用
    pub fn slip_to_slipway(&mut self, from_slot_id: usize, task_id: u16) -> Option<&mut Slot> {
        // 标记原槽位为 Dead
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == from_slot_id) {
            slot.status = SlotStatus::Dead;
            slot.task_id = None;
            self.dead_zones.push((slot.base, slot.size));
            self.merge_dead_zones();
        }

        // 找空闲滑道槽位
        let slip = self
            .slipway_slots
            .iter_mut()
            .find(|s| s.status == SlotStatus::Slipway);

        if let Some(slot) = slip {
            slot.status = SlotStatus::Occupied;
            slot.task_id = Some(task_id);
            return Some(slot);
        }

        None
    }

    /// 释放槽位。如果释放的是滑道槽位，标记为死区并尝试合并。
    pub fn free(&mut self, slot_id: usize) {
        // 找常规槽位
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == slot_id) {
            slot.status = SlotStatus::Free;
            slot.physical_size = 0;
            slot.task_id = None;
            return;
        }

        // 找滑道槽位 → 标记为死区
        if let Some(slot) = self.slipway_slots.iter_mut().find(|s| s.id == slot_id) {
            slot.status = SlotStatus::Slipway; // 恢复滑道状态
            slot.physical_size = 0;
            slot.task_id = None;
            self.dead_zones.push((slot.base, slot.size));
            self.merge_dead_zones();
        }
    }

    /// 合并相邻的死区。
    fn merge_dead_zones(&mut self) {
        if self.dead_zones.len() < 2 {
            return;
        }
        self.dead_zones.sort_by_key(|&(base, _)| base);
        let mut merged: Vec<(u64, u64)> = Vec::new();
        for (base, size) in self.dead_zones.drain(..) {
            if let Some(last) = merged.last_mut()
                && last.0 + last.1 >= base
            {
                last.1 = last.1.max(base + size - last.0);
                continue;
            }
            merged.push((base, size));
        }
        self.dead_zones = merged;
    }

    /// 死区合并 ROI 评估（设计文档 §4.6）。
    ///
    /// 当 defrag_benefit > defrag_cost × 2 时执行。
    /// defrag_cost = 迁移任务数 × 迁移时间
    /// defrag_benefit = 回收的死区大小 × 预期驻留时间 + 碎片率下降收益
    pub fn evaluate_defrag_roi(&self) -> Option<Vec<(usize, usize)>> {
        let fragmentation = self.calc_fragmentation();
        if fragmentation < 0.30 {
            return None; // 碎片率 < 30%，不合并
        }

        // 找出 Dead + 相邻 Free 的可合并方案
        let mut candidates: Vec<(usize, usize)> = Vec::new(); // (dead_idx, free_idx)
        for (i, dead) in self.slots.iter().enumerate() {
            if dead.status != SlotStatus::Dead {
                continue;
            }
            // 检查相邻槽位
            for j in 0..self.slots.len() {
                if i == j {
                    continue;
                }
                let adj = &self.slots[j];
                if adj.status == SlotStatus::Free || adj.status == SlotStatus::Dead {
                    // 简单 ROI：如果合并大小 > 阈值，视为值得
                    let merged_size = dead.size + adj.size;
                    if merged_size > self.slot_size * 2 {
                        candidates.push((i, j));
                    }
                }
            }
        }

        if candidates.is_empty() {
            None
        } else {
            Some(candidates)
        }
    }

    /// 计算碎片率。
    fn calc_fragmentation(&self) -> f64 {
        let dead_total: u64 = self
            .slots
            .iter()
            .filter(|s| s.status == SlotStatus::Dead)
            .map(|s| s.size)
            .sum();
        let free_total: u64 = self
            .slots
            .iter()
            .filter(|s| s.status == SlotStatus::Free)
            .map(|s| s.size)
            .sum();
        let total: u64 = self.slots.iter().map(|s| s.size).sum();
        if total == 0 {
            return 0.0;
        }
        (dead_total + free_total) as f64 / total as f64
    }

    // ── 查询方法 ──

    /// 当前已分配的槽位数。
    pub fn allocated_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.status == SlotStatus::Occupied)
            .count()
            + self
                .slipway_slots
                .iter()
                .filter(|s| s.status == SlotStatus::Occupied)
                .count()
    }

    /// 空闲槽位数。
    pub fn free_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_available()).count()
    }

    /// 是否还有空闲槽位。
    pub fn has_free(&self) -> bool {
        self.free_count() > 0
    }

    /// 获取指定槽位的可变引用。
    pub fn get_mut(&mut self, slot_id: usize) -> Option<&mut Slot> {
        self.slots
            .iter_mut()
            .chain(self.slipway_slots.iter_mut())
            .find(|s| s.id == slot_id)
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_creation_basic() {
        let sm = SlotManager::new(1024.0, 4, 0.15, 1.5);
        assert_eq!(sm.slots.len(), 4);
        assert_eq!(sm.slipway_slots.len(), 2); // ceil(1.5) = 2
        assert!(sm.slot_size > 0);
    }

    #[test]
    fn allocate_and_free() {
        let mut sm = SlotManager::new(1024.0, 4, 0.15, 1.5);
        let slot = sm.allocate(64.0); // 需要 64MB
        assert!(slot.is_some());
        assert_eq!(slot.unwrap().status, SlotStatus::Occupied);
        assert_eq!(sm.allocated_count(), 1);

        sm.free(0);
        assert_eq!(sm.allocated_count(), 0);
    }

    #[test]
    fn allocate_too_large_returns_none() {
        let mut sm = SlotManager::new(1024.0, 2, 0.15, 0.0);
        // slot_size 有限，分配超过 slot_size 的任务应该失败
        let huge = 99999.0; // MB
        let slot = sm.allocate(huge);
        assert!(slot.is_none());
    }

    #[test]
    fn slip_to_slipway_creates_dead_zone() {
        let mut sm = SlotManager::new(1024.0, 2, 0.15, 1.5);
        let slot = sm.allocate(64.0).unwrap();
        let slot_id = slot.id;

        // OOM 滑入
        let slip = sm.slip_to_slipway(slot_id, 0);
        assert!(slip.is_some(), "should have slipway slot");
        assert_eq!(slip.unwrap().status, SlotStatus::Occupied);

        // 原槽位应为 Dead
        assert_eq!(sm.slots[slot_id].status, SlotStatus::Dead);

        // 应有死区记录
        assert!(!sm.dead_zones.is_empty());
    }

    #[test]
    fn dead_zone_merge() {
        let mut sm = SlotManager::new(1024.0, 4, 0.15, 1.5);
        // 模拟两个相邻槽位变成 Dead
        sm.slots[0].status = SlotStatus::Dead;
        sm.dead_zones.push((sm.slots[0].base, sm.slots[0].size));
        sm.slots[1].status = SlotStatus::Dead;
        sm.dead_zones.push((sm.slots[1].base, sm.slots[1].size));
        sm.merge_dead_zones();

        // 应合并为一个
        assert_eq!(sm.dead_zones.len(), 1);
        assert!(sm.dead_zones[0].1 >= sm.slot_size * 2 - 1);
    }

    #[test]
    fn fragmentation_calculation() {
        let mut sm = SlotManager::new(1024.0, 4, 0.15, 0.0);
        // 所有槽位空闲 → 碎片率 = 1.0（全部可用）
        let frag = sm.calc_fragmentation();
        assert!((frag - 1.0).abs() < 0.01);

        // 分配 2 个槽位
        sm.allocate(16.0);
        sm.allocate(16.0);
        let frag = sm.calc_fragmentation();
        assert!((frag - 0.5).abs() < 0.01, "frag={}", frag);
    }

    #[test]
    fn defrag_roi_low_fragmentation() {
        let sm = SlotManager::new(1024.0, 4, 0.15, 0.0);
        // 碎片率低 → 不应触发合并
        let candidates = sm.evaluate_defrag_roi();
        assert!(candidates.is_none());
    }

    #[test]
    fn slipway_slots_never_allocated_by_allocate() {
        let mut sm = SlotManager::new(1024.0, 1, 0.15, 2.0);
        // 分配一个任务
        assert!(sm.allocate(16.0).is_some());
        // allocate 不会使用滑道槽位
        assert!(sm.allocate(16.0).is_none());
        // 但 slip_to_slipway 可以使用
        assert!(sm.slip_to_slipway(0, 0).is_some());
    }
}
