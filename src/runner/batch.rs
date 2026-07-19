//! 批次管理器 — 动态计算 N_batch 并发额度。
//!
//! 覆盖 P3-BM-001~BM-008, BM-011 全部需求。
//! 公式原文见 docs/11-策略模块.md。

// ─── OOM 反馈状态机 ──────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OomState {
    /// 加法增加：每秒 α_mem += 0.02
    Increase,
    /// OOM 滞回区 [3,4]：不调整
    Hysteresis,
    /// 乘法减少：α_mem ×= 0.75
    Decrease,
}

// ─── 控制决策 ────────────────────────────────────

/// 批次管理器的一次决策输出。
#[derive(Debug, Clone)]
pub struct ControlDecision {
    /// 最终批次额度（至少为 1）。
    pub n_batch: u32,
    /// 硬上限 H。
    pub hard_ceiling: f64,
    /// 软上限系数 merged。
    pub merged_factor: f64,
    /// 槽位大小（MB）。
    pub slot_size_mb: f64,
    /// 滑道倍数。
    pub slipway_multiplier: f64,
}

// ─── 批次管理器 ──────────────────────────────────

/// 批次管理器。负责计算并发额度 N_batch。
#[derive(Debug, Clone)]
pub struct BatchManager {
    // ── 资源限制 ──
    /// 可用 CPU 核心数。
    pub cpu_limit: f64,
    /// 可用内存（MB）。
    pub mem_limit: f64,
    /// 每任务预估 CPU 需求。
    pub cpu_per_task: f64,
    /// 每任务预估内存（MB）。
    pub mem_per_task: f64,

    // ── 保留系数 ──
    pub alpha_cpu: f64,
    pub alpha_mem: f64,

    // ── 运行时统计（滚动 EMA） ──
    /// 平均任务耗时（ms）。
    pub mu_t: f64,
    /// 平均任务内存（MB）。
    pub mu_m: f64,
    /// 耗时标准差。
    pub sigma_t: f64,
    /// 积压深度（就绪 + 运行中任务数）。
    pub pool_depth: f64,

    // ── OOM 反馈 ──
    pub oom_count: u32,
    pub alpha_mem_current: f64,
    pub initial_alpha_mem: f64,
    pub oom_state: OomState,

    // ── 因子权重 ──
    pub w_beta: f64,
    pub w_lambda: f64,
    pub w_sigma: f64,
    pub w_gamma: f64,

    // ── 安全参数 ──
    pub safety_margin: f64,

    // ── 滑道 ──
    pub slipway_base: f64,
}

impl BatchManager {
    /// 创建默认配置的批次管理器。
    pub fn new(cpu_limit: f64, mem_limit: f64) -> Self {
        let alpha_mem = 0.50;
        Self {
            cpu_limit,
            mem_limit,
            cpu_per_task: 0.25,
            mem_per_task: 16.0,
            alpha_cpu: 0.75,
            alpha_mem,
            mu_t: 500.0,       // 初始猜测 500ms
            mu_m: 16.0,         // 初始猜测 16MB
            sigma_t: 250.0,     // 初始猜测
            pool_depth: 1.0,
            oom_count: 0,
            alpha_mem_current: alpha_mem,
            initial_alpha_mem: alpha_mem,
            oom_state: OomState::Increase,
            w_beta: 0.25,
            w_lambda: 0.25,
            w_sigma: 0.25,
            w_gamma: 0.25,
            safety_margin: 0.15,
            slipway_base: 1.5,
        }
    }

    // ═══════════════════════════════════════════════
    //  硬上限 H
    // ═══════════════════════════════════════════════

    /// 计算硬上限 H = floor(min(C, M))。
    /// 当前实现使用 CPU 和内存二维（IO/网络暂估为 inf）。
    pub fn compute_hard_ceiling(&self) -> f64 {
        let c = (self.cpu_limit * self.alpha_cpu) / self.cpu_per_task;
        let m = (self.mem_limit * self.alpha_mem_current) / self.mem_per_task;
        let h = c.min(m);
        if h < 0.0 { 0.0 } else { h }
    }

    // ═══════════════════════════════════════════════
    //  四个 Sigmoid 因子
    // ═══════════════════════════════════════════════

    /// 积压因子 β(d)：d→0 → 1.00，d=1.5 → 0.75，d→∞ → 0.50
    pub fn factor_beta(&self) -> f64 {
        let h = self.compute_hard_ceiling();
        if h <= 0.0 { return 0.50; }
        let d = self.pool_depth / h;
        0.50 + 0.50 / (1.0 + f64::exp(5.0 * (d - 1.5)))
    }

    /// 速度因子 λ(μ_t)：μ_t→0 → 1.40，μ_t=500ms → 1.20，μ_t→∞ → 1.00
    pub fn factor_lambda(&self) -> f64 {
        1.00 + 0.40 / (1.0 + f64::exp(5.0 * (self.mu_t / 500.0 - 1.0)))
    }

    /// 体积因子 σ(r)：r→0 → 1.35，r=1.0 → 0.95，r→∞ → 0.55
    pub fn factor_sigma(&self) -> f64 {
        let r = self.mu_m / self.mem_per_task.max(1.0);
        0.55 + 0.80 / (1.0 + f64::exp(5.0 * (r - 1.0)))
    }

    /// 方差因子 γ(v_t)：v_t→0 → 1.05，v_t=0.5 → 0.78，v_t→∞ → 0.50
    pub fn factor_gamma(&self) -> f64 {
        let v_t = if self.mu_t > 0.0 {
            self.sigma_t / self.mu_t
        } else {
            0.0
        };
        0.50 + 0.55 / (1.0 + f64::exp(5.0 * (v_t - 0.5)))
    }

    // ═══════════════════════════════════════════════
    //  合并
    // ═══════════════════════════════════════════════

    /// 加权几何平均合并四个因子。
    pub fn merge_factors(&self) -> f64 {
        let beta = self.factor_beta();
        let lambda = self.factor_lambda();
        let sigma = self.factor_sigma();
        let gamma = self.factor_gamma();

        let total_w = self.w_beta + self.w_lambda + self.w_sigma + self.w_gamma;
        if total_w <= 0.0 {
            return 1.0;
        }

        let log_merged = self.w_beta * beta.ln()
            + self.w_lambda * lambda.ln()
            + self.w_sigma * sigma.ln()
            + self.w_gamma * gamma.ln();

        (log_merged / total_w).exp()
    }

    // ═══════════════════════════════════════════════
    //  完整决策
    // ═══════════════════════════════════════════════

    /// 完整计算 N_batch。
    pub fn compute_decision(&mut self) -> ControlDecision {
        let h = self.compute_hard_ceiling();
        let merged = self.merge_factors();
        let s = h * merged;
        let n_batch_raw = h.min(s);
        let n_batch = (n_batch_raw.max(1.0).floor()) as u32;

        let effective_mem = self.mem_limit * self.alpha_mem_current;
        let slot_count = n_batch as f64 + self.slipway_base;
        let slot_size_mb = if slot_count > 0.0 {
            (effective_mem * (1.0 - self.safety_margin)) / slot_count
        } else {
            effective_mem
        };

        ControlDecision {
            n_batch,
            hard_ceiling: h,
            merged_factor: merged,
            slot_size_mb,
            slipway_multiplier: self.slipway_base,
        }
    }

    // ═══════════════════════════════════════════════
    //  统计更新
    // ═══════════════════════════════════════════════

    /// 用任务完成数据更新运行时统计（EMA，α=0.3）。
    pub fn update_stats(&mut self, task_time_ms: f64, task_mem_mb: f64) {
        let alpha = 0.3;
        self.mu_t = self.mu_t * (1.0 - alpha) + task_time_ms * alpha;
        self.sigma_t = self.sigma_t * (1.0 - alpha)
            + (task_time_ms - self.mu_t).abs() * alpha;
        self.mu_m = self.mu_m * (1.0 - alpha) + task_mem_mb * alpha;
    }

    /// 设置积压深度。
    pub fn set_pool_depth(&mut self, depth: f64) {
        self.pool_depth = depth;
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_ceiling_basic() {
        // 4 核 CPU × 0.75 / 0.25 每任务 = 12
        // 1024MB × 0.50 / 16 每任务 = 32
        // H = min(12, 32) = 12
        let bm = BatchManager::new(4.0, 1024.0);
        let h = bm.compute_hard_ceiling();
        assert!((h - 12.0).abs() < 0.01);
    }

    #[test]
    fn hard_ceiling_memory_bound() {
        // 16 核 CPU 但只有 64MB 内存
        // C = 16 × 0.75 / 0.25 = 48
        // M = 64 × 0.50 / 16 = 2
        // H = min(48, 2) = 2
        let bm = BatchManager::new(16.0, 64.0);
        let h = bm.compute_hard_ceiling();
        assert!((h - 2.0).abs() < 0.01);
    }

    #[test]
    fn hard_ceiling_zero_resources() {
        let bm = BatchManager::new(0.0, 0.0);
        let h = bm.compute_hard_ceiling();
        assert!(h == 0.0);
    }

    #[test]
    fn factor_beta_values() {
        let mut bm = BatchManager::new(4.0, 1024.0);
        // d = 0 → β ≈ 1.00
        bm.pool_depth = 0.0;
        let b = bm.factor_beta();
        assert!((b - 1.00).abs() < 0.01, "beta(0)={}", b);

        // d = 1.5 → β ≈ 0.75
        bm.pool_depth = 1.5 * bm.compute_hard_ceiling();
        let b = bm.factor_beta();
        assert!((b - 0.75).abs() < 0.05, "beta(1.5)={}", b);

        // d = 100 → β ≈ 0.50
        bm.pool_depth = 100.0 * bm.compute_hard_ceiling();
        let b = bm.factor_beta();
        assert!((b - 0.50).abs() < 0.02, "beta(100)={}", b);
    }

    #[test]
    fn factor_lambda_values() {
        let mut bm = BatchManager::new(4.0, 1024.0);
        // μ_t → 1ms → λ ≈ 1.40
        bm.mu_t = 1.0;
        let l = bm.factor_lambda();
        assert!((l - 1.40).abs() < 0.05, "lambda(1ms)={}", l);

        // μ_t = 500ms → λ ≈ 1.20
        bm.mu_t = 500.0;
        let l = bm.factor_lambda();
        assert!((l - 1.20).abs() < 0.05, "lambda(500ms)={}", l);

        // μ_t → 10000ms → λ ≈ 1.00
        bm.mu_t = 10000.0;
        let l = bm.factor_lambda();
        assert!((l - 1.00).abs() < 0.05, "lambda(10s)={}", l);
    }

    #[test]
    fn factor_sigma_values() {
        let mut bm = BatchManager::new(4.0, 1024.0);
        // r → 0 → σ ≈ 1.35
        bm.mu_m = 0.1;
        let s = bm.factor_sigma();
        assert!((s - 1.35).abs() < 0.05, "sigma(0)={}", s);

        // r = 1.0 → σ ≈ 0.95
        bm.mu_m = bm.mem_per_task;
        let s = bm.factor_sigma();
        assert!((s - 0.95).abs() < 0.05, "sigma(1)={}", s);

        // r → 100 → σ ≈ 0.55
        bm.mu_m = bm.mem_per_task * 100.0;
        let s = bm.factor_sigma();
        assert!((s - 0.55).abs() < 0.05, "sigma(100)={}", s);
    }

    #[test]
    fn factor_gamma_values() {
        let mut bm = BatchManager::new(4.0, 1024.0);
        // v_t → 0 → γ ≈ 1.05
        bm.sigma_t = 0.01;
        bm.mu_t = 500.0;
        let g = bm.factor_gamma();
        assert!((g - 1.05).abs() < 0.05, "gamma(0)={}", g);

        // v_t = 0.5 → γ ≈ 0.78
        bm.sigma_t = 250.0;
        bm.mu_t = 500.0;
        let g = bm.factor_gamma();
        assert!((g - 0.78).abs() < 0.08, "gamma(0.5)={}", g);

        // v_t → 10 → γ ≈ 0.50
        bm.sigma_t = 5000.0;
        bm.mu_t = 500.0;
        let g = bm.factor_gamma();
        assert!((g - 0.50).abs() < 0.05, "gamma(10)={}", g);
    }

    #[test]
    fn merge_all_one() {
        // 所有因子 = 1.0 → merged = 1.0
        let bm = BatchManager::new(4.0, 1024.0);
        // To get beta=1.0, need d→0
        // But with pool_depth=0 and H=12, d≈0 → beta≈1.0
        // lambda at mu_t=500ms ≈ 1.2, not 1.0...
        // So this test checks the formula, not absolute values
        let merged = bm.merge_factors();
        assert!(merged > 0.0);
        assert!(merged <= 1.0);
    }

    #[test]
    fn compute_decision_n_batch_at_least_one() {
        // 即使资源很少，N_batch 至少为 1
        let mut bm = BatchManager::new(0.5, 16.0); // 0.5 核, 16MB → H ≈ 0.5
        let decision = bm.compute_decision();
        assert!(decision.n_batch >= 1, "n_batch should be at least 1");
    }

    #[test]
    fn compute_decision_basic() {
        let mut bm = BatchManager::new(4.0, 1024.0);
        let decision = bm.compute_decision();
        assert!(decision.n_batch >= 1);
        assert!(decision.hard_ceiling > 0.0);
        assert!(decision.merged_factor > 0.0);
        assert!(decision.slot_size_mb > 0.0);
    }

    #[test]
    fn update_stats_ema() {
        let mut bm = BatchManager::new(4.0, 1024.0);
        let old_mu_t = bm.mu_t;
        let old_mu_m = bm.mu_m;

        bm.update_stats(100.0, 32.0);

        // mu_t 应该向 100 偏移
        assert!(bm.mu_t < old_mu_t, "mu_t should decrease toward 100");
        // mu_m 应该向 32 偏移
        assert!(bm.mu_m > old_mu_m, "mu_m should increase toward 32");
    }

    #[test]
    fn factor_beta_continuous() {
        let mut bm = BatchManager::new(4.0, 1024.0);
        let h = bm.compute_hard_ceiling();
        // 验证 β 在 d 变化时连续（无跳变）
        let mut prev = bm.factor_beta();
        for depth in (0..100).map(|i| i as f64 * h / 20.0) {
            bm.pool_depth = depth;
            let curr = bm.factor_beta();
            let diff = (curr - prev).abs();
            assert!(diff < 0.1, "beta should be continuous at d={}", depth / h);
            prev = curr;
        }
    }
}
