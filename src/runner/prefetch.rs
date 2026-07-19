//! 预载机制 — 异步加载下一任务的 .atxe 到磁盘。
//!
//! 覆盖设计文档 §3.4（预载机制）和 §6.3（预载调度算法）。

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
}

impl Default for Prefetcher {
    fn default() -> Self {
        Self {
            network_rtt_ms: 50.0,  // 默认 50ms
            avg_exec_time_ms: 500.0, // 默认 500ms
            depth: 1,
            min_depth: 1,
            max_depth: 3,
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
        self.depth = (raw as u32)
            .clamp(self.min_depth, self.max_depth);
    }

    /// 判断是否需要为指定 Executor 触发预载。
    ///
    /// 当剩余执行时间 > 网络延迟 × 1.5 时，返回 true。
    ///
    /// # 参数
    /// - `remaining_instrs`: Executor 当前任务剩余指令数
    /// - `avg_ipc_rate_ns`: 每条指令平均执行时间（ns），默认 ~1ns
    pub fn should_prefetch(&self, remaining_instrs: u64, avg_ipc_rate_ns: f64) -> bool {
        let remaining_time_ms = remaining_instrs as f64 * avg_ipc_rate_ns / 1_000_000.0;
        remaining_time_ms > self.network_rtt_ms * 1.5
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
        assert_eq!(p.network_rtt_ms, 50.0);
    }

    #[test]
    fn prefetcher_recalculates_depth() {
        let mut p = Prefetcher::new();
        // avg_exec=500ms, rtt=50ms → depth = ceil(500/50) = 10 → clamped to 3
        p.set_avg_exec_time(500.0);
        p.set_network_rtt(50.0);
        assert_eq!(p.depth, 3);

        // avg_exec=100ms, rtt=50ms → depth = ceil(100/50) = 2
        p.set_avg_exec_time(100.0);
        assert_eq!(p.depth, 2);

        // avg_exec=30ms, rtt=50ms → depth = ceil(30/50) = 1
        p.set_avg_exec_time(30.0);
        assert_eq!(p.depth, 1);
    }

    #[test]
    fn prefetcher_should_prefetch() {
        let p = Prefetcher::new();
        // remaining=100000 instrs, rate=1ns → 100μs = 0.1ms
        // network_rtt=50ms, 0.1ms < 75ms → false
        assert!(!p.should_prefetch(100000, 1.0));

        // remaining=100_000_000 instrs, rate=1ns → 100ms
        // 100ms > 75ms → true
        assert!(p.should_prefetch(100_000_000, 1.0));
    }

    #[test]
    fn prefetcher_estimate_remaining() {
        let p = Prefetcher::new();
        let time = p.estimate_remaining_time(1_000_000, 1.0);
        assert!((time - 1.0).abs() < 0.001, "time={}ms", time);
    }

    #[test]
    fn prefetcher_setters_clamp() {
        let mut p = Prefetcher::new();
        p.set_network_rtt(0.0); // 应 clamp 到 1.0
        assert!(p.network_rtt_ms >= 1.0);
    }
}
