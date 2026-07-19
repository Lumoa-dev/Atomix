//! 预载机制 — 异步加载下一任务的 .atxe 到磁盘。
//!
//! 覆盖设计文档 §3.4（预载机制）和 §6.3（预载调度算法）。

use std::collections::VecDeque;

/// 预载队列 — 已预加载的任务 ID 列表。
#[derive(Debug, Clone)]
pub struct PrefetchQueue {
    /// 内部队列。
    queue: VecDeque<u16>,
}

impl PrefetchQueue {
    /// 创建空队列。
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    /// 入队一个预加载的任务。
    pub fn push(&mut self, task_id: u16) {
        if !self.queue.contains(&task_id) {
            self.queue.push_back(task_id);
        }
    }

    /// 出队一个已预加载的任务。
    pub fn pop(&mut self) -> Option<u16> {
        self.queue.pop_front()
    }

    /// 队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 队列长度。
    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

/// 预载调度器。
///
/// 在 Runtime 主循环中决策：对每个活跃 Executor，
/// 估算其剩余执行时间。如果剩余时间 > 网络延迟 × 1.5，
/// 异步拉取下一个任务的 .atxe。
///
/// 预载深度动态调整：
/// ```
/// prefetch_depth = clamp(ceil(avg_exec_time_ms / network_latency_ms), 1, 3)
/// ```
pub struct Prefetcher {
    /// 网络往返延迟估计（ms）。
    pub network_rtt_ms: f64,
    /// 平均执行时间（ms），由 Runtime 更新。
    pub avg_exec_time_ms: f64,
    /// 当前预载深度。
    pub depth: u32,
    /// 最小预载深度。
    pub min_depth: u32,
    /// 最大预载深度。
    pub max_depth: u32,
    /// 预载队列。
    pub queue: PrefetchQueue,
    /// 预载阈值倍数（剩余时间 > RTT × 倍数时触发）。
    pub threshold_multiplier: f64,
}

impl Default for Prefetcher {
    fn default() -> Self {
        Self {
            network_rtt_ms: 50.0,
            avg_exec_time_ms: 500.0,
            depth: 1,
            min_depth: 1,
            max_depth: 3,
            queue: PrefetchQueue::new(),
            threshold_multiplier: 1.5,
        }
    }
}

impl Prefetcher {
    /// 创建预载调度器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 更新网络延迟估计。
    pub fn set_network_rtt(&mut self, rtt_ms: f64) {
        self.network_rtt_ms = rtt_ms.max(1.0);
        self.recalc_depth();
    }

    /// 更新平均执行时间。
    pub fn set_avg_exec_time(&mut self, exec_time_ms: f64) {
        self.avg_exec_time_ms = exec_time_ms.max(1.0);
        self.recalc_depth();
    }

    /// 重新计算预载深度。
    fn recalc_depth(&mut self) {
        let raw = (self.avg_exec_time_ms / self.network_rtt_ms).ceil();
        self.depth = (raw as u32).clamp(self.min_depth, self.max_depth);
    }

    /// 判断是否需要为指定 Executor 触发预载。
    pub fn should_prefetch(&self, remaining_instrs: u64, avg_ipc_rate_ns: f64) -> bool {
        let remaining_time_ms = remaining_instrs as f64 * avg_ipc_rate_ns / 1_000_000.0;
        remaining_time_ms > self.network_rtt_ms * self.threshold_multiplier
    }

    /// 获取当前预载深度。
    pub fn prefetch_depth(&self) -> u32 {
        self.depth
    }

    /// 估算剩余执行时间（ms）。
    pub fn estimate_remaining_time(&self, remaining_instrs: u64, avg_ipc_rate_ns: f64) -> f64 {
        remaining_instrs as f64 * avg_ipc_rate_ns / 1_000_000.0
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefetcher_default_depth() {
        let p = Prefetcher::new();
        assert_eq!(p.depth, 1);
    }

    #[test]
    fn prefetcher_recalculates_depth() {
        let mut p = Prefetcher::new();
        p.set_avg_exec_time(500.0);
        p.set_network_rtt(50.0);
        assert_eq!(p.depth, 3);

        p.set_avg_exec_time(100.0);
        assert_eq!(p.depth, 2);

        p.set_avg_exec_time(30.0);
        assert_eq!(p.depth, 1);
    }

    #[test]
    fn prefetcher_should_prefetch() {
        let p = Prefetcher::new();
        assert!(!p.should_prefetch(100000, 1.0));
        assert!(p.should_prefetch(100_000_000, 1.0));
    }

    #[test]
    fn prefetch_queue_basic() {
        let mut q = PrefetchQueue::new();
        assert!(q.is_empty());
        q.push(1);
        q.push(2);
        assert_eq!(q.len(), 2);
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
        assert!(q.is_empty());
    }

    #[test]
    fn prefetch_queue_dedup() {
        let mut q = PrefetchQueue::new();
        q.push(1);
        q.push(1); // 重复
        assert_eq!(q.len(), 1);
    }
}
