//! 轻量任务元数据 — 任务描述信息，不含运行时状态。
//!
//! 覆盖设计文档 §3.2（TaskMeta）。

use crate::runner::task::TaskId;

/// 轻量任务元数据，约 65 字节。
///
/// 任务本身（.atxe 二进制）不在内存中。只在磁盘上。
/// 内存中的只有 TaskMeta（每个任务 ~65B）。
///
/// 100 万个任务 → 65 MB（可接受，按需分批加载）
/// 1 亿个任务   → 6.5 GB（需按 backlog 分批加载）
#[derive(Debug, Clone)]
pub struct TaskMeta {
    /// 任务唯一标识。
    pub task_id: TaskId,
    /// 任务名称（最多 32 字节 UTF-8）。
    pub name: [u8; 32],
    /// .atxe 在磁盘仓库中的偏移。
    pub disk_offset: u64,
    /// .atxe 文件大小（字节）。
    pub disk_size: u32,
    /// 运行时 SandboxMemory 基址（运行时填充，每次加载可能不同）。
    pub memory_addr: u64,
    /// 入口 PC 偏移。
    pub entry_point: u32,
    /// 编译器预测峰值（MB）。
    pub compiler_peak_mb: f32,
    /// 实际峰值（执行后填入）。
    pub actual_peak_mb: f32,
}

impl TaskMeta {
    /// 创建一个新的 TaskMeta。
    pub fn new(task_id: TaskId, entry_point: u32) -> Self {
        Self {
            task_id,
            name: [0u8; 32],
            disk_offset: 0,
            disk_size: 0,
            memory_addr: 0,
            entry_point,
            compiler_peak_mb: 0.0,
            actual_peak_mb: 0.0,
        }
    }

    /// 设置任务名称（从字符串截取或填充）。
    pub fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(31);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self.name[len] = 0;
    }

    /// 获取任务名称（以 \0 结尾的 C 风格字符串）。
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(32);
        std::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_meta_new() {
        let meta = TaskMeta::new(42, 0x100);
        assert_eq!(meta.task_id, 42);
        assert_eq!(meta.entry_point, 0x100);
        assert_eq!(meta.disk_offset, 0);
        assert_eq!(meta.disk_size, 0);
        assert_eq!(meta.memory_addr, 0);
    }

    #[test]
    fn task_meta_name() {
        let mut meta = TaskMeta::new(0, 0);
        meta.set_name("hello");
        assert_eq!(meta.name_str(), "hello");
    }

    #[test]
    fn task_meta_name_truncated() {
        let mut meta = TaskMeta::new(0, 0);
        let long = "a".repeat(64);
        meta.set_name(&long);
        // 截断到 31 字节 + null = 32
        assert_eq!(meta.name_str().len(), 31);
    }

    #[test]
    fn task_meta_size() {
        // 验证结构体大小合理（~65 字节）
        let size = std::mem::size_of::<TaskMeta>();
        // task_id(2) + name(32) + disk_offset(8) + disk_size(4)
        // + memory_addr(8) + entry_point(4) + compiler_peak_mb(4) + actual_peak_mb(4)
        // = 66, padding 可能 2 字节对齐
        assert!(
            size <= 72,
            "TaskMeta size {} should be <= 72 bytes",
            size
        );
        assert!(size >= 60, "TaskMeta size {} should be >= 60 bytes", size);
    }
}
