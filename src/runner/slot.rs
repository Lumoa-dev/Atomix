//! 槽位管理 — 墙式预分配的虚地址空间槽位。
//!
//! 覆盖 P3-SLOT-001 墙式预分配、P3-SLOT-003 水位线检测。

/// 单个内存槽位。
#[derive(Debug, Clone)]
pub struct Slot {
    /// 槽位 ID。
    pub id: usize,
    /// 虚地址起始。
    pub base: u64,
    /// 槽位大小（字节）。
    pub size: u64,
    /// 是否已分配（有任务驻留）。
    pub allocated: bool,
}

/// 槽位管理器。基于 N_batch 预分配等大的虚地址槽位。
#[derive(Debug, Clone)]
pub struct SlotManager {
    /// 所有槽位。
    pub slots: Vec<Slot>,
    /// 总内存池大小（字节）。
    pub total_pool: u64,
    /// 每个槽位的虚地址大小。
    pub slot_size: u64,
    /// 安全冗余比例。
    pub safety_margin: f64,
    /// 滑道倍数。
    pub slipway_multiplier: f64,
}

impl SlotManager {
    /// 根据总内存和 N_batch 创建槽位管理器。
    pub fn new(total_mem_mb: f64, n_batch: u32, safety_margin: f64, slipway_mul: f64) -> Self {
        let total_bytes = (total_mem_mb * 1024.0 * 1024.0) as u64;
        let effective = (total_bytes as f64 * (1.0 - safety_margin)) as u64;
        let slot_count = (n_batch as f64 + slipway_mul) as u64;
        let slot_size = if slot_count > 0 { effective / slot_count } else { effective };

        let mut slots = Vec::with_capacity(n_batch as usize);
        for i in 0..n_batch {
            slots.push(Slot {
                id: i as usize,
                base: (i as u64) * slot_size,
                size: slot_size,
                allocated: false,
            });
        }

        Self {
            slots,
            total_pool: total_bytes,
            slot_size,
            safety_margin,
            slipway_multiplier: slipway_mul,
        }
    }

    /// 分配一个空闲槽位，返回 Slot 的可变引用。
    pub fn allocate(&mut self) -> Option<&mut Slot> {
        let slot = self.slots.iter_mut().find(|s| !s.allocated)?;
        slot.allocated = true;
        Some(slot)
    }

    /// 释放指定槽位。
    pub fn free(&mut self, slot_id: usize) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == slot_id) {
            slot.allocated = false;
        }
    }

    /// 当前已分配的槽位数。
    pub fn allocated_count(&self) -> usize {
        self.slots.iter().filter(|s| s.allocated).count()
    }

    /// 空闲槽位数。
    pub fn free_count(&self) -> usize {
        self.slots.iter().filter(|s| !s.allocated).count()
    }

    /// 是否还有空闲槽位。
    pub fn has_free(&self) -> bool {
        self.free_count() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_creation_basic() {
        // 1024MB 内存，N_batch=4，safety=0.15，slipway=1.5
        // effective = 1024 * 0.85 = 870.4 MB
        // slot_count = 4 + 1.5 = 5.5
        // slot_size = 870.4 / 5.5 ≈ 158.25 MB
        let sm = SlotManager::new(1024.0, 4, 0.15, 1.5);
        assert_eq!(sm.slots.len(), 4);
        assert!(sm.slot_size > 0);
        assert!(sm.slot_size < (1024u64 * 1024 * 1024));
    }

    #[test]
    fn allocate_and_free() {
        let mut sm = SlotManager::new(1024.0, 4, 0.15, 1.5);
        assert_eq!(sm.free_count(), 4);
        assert!(sm.has_free());

        let slot = sm.allocate();
        assert!(slot.is_some());
        assert!(slot.unwrap().allocated);
        assert_eq!(sm.allocated_count(), 1);
        assert_eq!(sm.free_count(), 3);

        sm.free(0);
        assert_eq!(sm.allocated_count(), 0);
    }

    #[test]
    fn allocate_all_slots() {
        let mut sm = SlotManager::new(1024.0, 2, 0.15, 1.5);
        assert!(sm.allocate().is_some());
        assert!(sm.allocate().is_some());
        // No more slots
        assert!(sm.allocate().is_none());
    }

    #[test]
    fn slot_sizes_equal() {
        let sm = SlotManager::new(1024.0, 4, 0.15, 1.5);
        let sizes: Vec<u64> = sm.slots.iter().map(|s| s.size).collect();
        assert!(sizes.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn single_slot() {
        let sm = SlotManager::new(1024.0, 1, 0.0, 0.0);
        assert_eq!(sm.slots.len(), 1);
    }
}
