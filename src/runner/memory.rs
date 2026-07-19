//! 沙箱线性内存 — 边界检查的内存访问抽象。
//!
//! 覆盖 执行器设计.md §3 和执行器设计.md §8 的沙箱规范。

use std::collections::HashMap;

/// 沙箱内存。提供带边界检查的读写操作。
///
/// 支持物理内存按需分配（设计文档 §4.5）：
/// - 初始容量 = 编译预测峰值 × 修正系数
/// - 运行时超出则触发 OOM 流程
/// - `physical_size` 跟踪实际分配的物理内存
#[derive(Debug, Clone)]
pub struct SandboxMemory {
    /// 线性地址空间。
    pub data: Vec<u8>,
    /// 已分配的物理内存大小（字节）。等于 data.len()。
    pub physical_size: u64,
    /// 最大可扩展大小（字节）。
    pub max_size: u64,
    /// 可执行区域起始（.text 映射）。
    pub text_start: u64,
    /// 可执行区域大小。
    pub text_size: u64,
    /// 栈起始。
    pub stack_base: u64,
    /// 栈大小。
    pub stack_size: u64,
    /// 堆区起始地址（紧接 .rodata 之后）。
    pub heap_base: u64,
    /// 堆分配记录（地址 → 大小）。
    allocations: HashMap<u64, u64>,
    /// 水位线：触发 OOM 预警的阈值（字节）。
    pub watermark_high: u64,
    /// 当前堆使用量（相对于 heap_base 的偏移）。
    pub usage: u64,
}

impl SandboxMemory {
    /// 创建指定大小的沙箱内存，默认 heap_base = 64（跳过零地址防止混淆 OOM）。
    ///
    /// 物理内存初始分配 size 大小。如需惰性分配，使用 `reserve`。
    pub fn new(size: usize) -> Self {
        let effective = std::cmp::max(size, 8192);
        let stack = std::cmp::min(4096, effective / 4);
        let stack_base = (effective as u64) - stack as u64;
        Self {
            data: vec![0u8; effective],
            physical_size: effective as u64,
            max_size: effective as u64,
            text_start: 0,
            text_size: 0,
            stack_base,
            stack_size: stack as u64,
            heap_base: 64, // 跳过零地址
            allocations: HashMap::new(),
            watermark_high: (effective as u64) * 75 / 100,
            usage: 0,
        }
    }

    /// 惰性分配：初始只保留必要空间，按需扩展。
    ///
    /// 参数：
    /// - `initial_mb`: 初始分配大小（MB），通常 = compiler_peak × 修正系数
    /// - `max_mb`: 最大可扩展大小（MB），通常 = slot_size
    pub fn reserve(initial_mb: f64, max_mb: f64) -> Self {
        let initial = (initial_mb.max(1.0) * 1024.0 * 1024.0) as usize;
        let max = (max_mb.max(1.0) * 1024.0 * 1024.0) as usize;
        let effective = initial.max(8192);
        let stack = std::cmp::min(4096, effective / 4);
        let stack_base = (effective as u64) - stack as u64;
        Self {
            data: vec![0u8; effective],
            physical_size: effective as u64,
            max_size: max as u64,
            text_start: 0,
            text_size: 0,
            stack_base,
            stack_size: stack as u64,
            heap_base: 64,
            allocations: HashMap::new(),
            watermark_high: (effective as u64) * 75 / 100,
            usage: 0,
        }
    }

    /// 扩展物理内存（触发 OOM 流程前调用）。
    /// 返回 true 表示扩展成功，false 表示达到上限（OOM）。
    pub fn grow(&mut self, additional: u64) -> bool {
        let new_size = self.data.len().saturating_add(additional as usize);
        if new_size as u64 > self.max_size {
            return false; // 达到上限
        }
        self.data.resize(new_size, 0);
        self.physical_size = new_size as u64;
        self.watermark_high = (new_size as u64) * 75 / 100;
        true
    }

    /// 从指定地址读取 64 位值。
    pub fn read_u64(&self, addr: u64) -> Option<u64> {
        let start = addr as usize;
        let end = start.wrapping_add(8);
        if end > self.data.len() || end < start {
            return None;
        }
        let bytes: [u8; 8] = self.data[start..end].try_into().ok()?;
        Some(u64::from_le_bytes(bytes))
    }

    /// 从指定地址读取 32 位值。
    pub fn read_u32(&self, addr: u64) -> Option<u32> {
        let start = addr as usize;
        let end = start + 4;
        if end <= self.data.len() {
            let bytes: [u8; 4] = self.data[start..end].try_into().ok()?;
            Some(u32::from_le_bytes(bytes))
        } else {
            None
        }
    }

    /// 写入 64 位值到指定地址。
    pub fn write_u64(&mut self, addr: u64, val: u64) -> bool {
        let start = addr as usize;
        let end = start.wrapping_add(8);
        if end > self.data.len() || end < start {
            return false;
        }
        self.data[start..end].copy_from_slice(&val.to_le_bytes());
        true
    }

    /// 写入 32 位值到指定地址。
    pub fn write_u32(&mut self, addr: u64, val: u32) -> bool {
        let start = addr as usize;
        let end = start + 4;
        if end <= self.data.len() {
            self.data[start..end].copy_from_slice(&val.to_le_bytes());
            true
        } else {
            false
        }
    }

    /// 读取单个字节。
    pub fn read_u8(&self, addr: u64) -> Option<u8> {
        let idx = addr as usize;
        if idx < self.data.len() {
            Some(self.data[idx])
        } else {
            None
        }
    }

    /// 写入单个字节。
    pub fn write_u8(&mut self, addr: u64, val: u8) -> bool {
        let idx = addr as usize;
        if idx < self.data.len() {
            self.data[idx] = val;
            true
        } else {
            false
        }
    }

    /// 分配堆内存（简易 bump allocator）。
    /// 返回分配的地址，OOM 时返回 u64::MAX。
    pub fn alloc(&mut self, size: u64) -> u64 {
        let addr = self.heap_base + self.usage;
        let end = addr.saturating_add(size);
        let max_addr = self.stack_base;
        if end > max_addr {
            return u64::MAX; // OOM
        }
        self.allocations.insert(addr, size);
        self.usage = end - self.heap_base;
        addr
    }

    /// 释放堆内存。
    pub fn free(&mut self, addr: u64) {
        if let Some(size) = self.allocations.remove(&addr) {
            let _ = size;
            // 简易实现：不回收
        }
    }

    /// 检查是否超过水位线。
    pub fn is_over_watermark(&self) -> bool {
        self.usage >= self.watermark_high
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_u64() {
        let mut mem = SandboxMemory::new(1024);
        assert!(mem.write_u64(8, 0xDEAD_BEEF));
        assert_eq!(mem.read_u64(8), Some(0xDEAD_BEEF));
    }

    #[test]
    fn out_of_bounds_read() {
        let mem = SandboxMemory::new(64);
        // 有效范围内
        assert!(mem.read_u64(0).is_some());
        // 超出范围（地址 > data.len() - 8）
        assert_eq!(mem.read_u64(usize::MAX as u64), None);
    }

    #[test]
    fn out_of_bounds_write() {
        let mut mem = SandboxMemory::new(64);
        assert!(!mem.write_u64(usize::MAX as u64 - 7, 42));
    }

    #[test]
    fn alloc_and_usage() {
        let mut mem = SandboxMemory::new(65536);
        let addr = mem.alloc(1024);
        assert_ne!(addr, u64::MAX, "allocation should succeed");
        assert!(mem.usage > 0);
        // 释放（bump allocator 不回收，但不崩溃）
        mem.free(addr);
    }

    #[test]
    fn watermark_check() {
        let mut mem = SandboxMemory::new(1000);
        mem.watermark_high = 500;
        mem.usage = 600;
        assert!(mem.is_over_watermark());
    }

    #[test]
    fn read_write_u8() {
        let mut mem = SandboxMemory::new(16);
        assert!(mem.write_u8(5, 0xAB));
        assert_eq!(mem.read_u8(5), Some(0xAB));
    }
}
