//! 线性回归模型 — 编译器峰值预测 → 实际峰值的 OLS 修正。
//!
//! 覆盖设计文档 §7.3（线性回归修正）。

/// OLS 线性回归模型。
///
/// actual_peak = α × compiler_peak + β + ε
///
/// 使用条件：
/// - 样本数 ≥ MIN_SAMPLES (50)
/// - r² ≥ MIN_R_SQUARED (0.6)
/// - 否则退回到保守估计（×1.5）
#[derive(Debug, Clone)]
pub struct RegressionModel {
    /// 斜率。
    pub alpha: f64,
    /// 截距。
    pub beta: f64,
    /// 拟合优度。
    pub r_squared: f64,
    /// 样本数。
    pub sample_count: u64,
    /// 上次训练时的样本数。
    pub last_trained_at: u64,
}

impl Default for RegressionModel {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            beta: 0.0,
            r_squared: 0.0,
            sample_count: 0,
            last_trained_at: 0,
        }
    }
}

impl RegressionModel {
    /// 最小样本数（低于此值不训练）。
    pub const MIN_SAMPLES: u64 = 50;

    /// 返回最小样本数（公开访问）。
    pub fn min_samples() -> u64 {
        Self::MIN_SAMPLES
    }
    /// 最小 r²（低于此值不启用回归预测）。
    const MIN_R_SQUARED: f64 = 0.6;
    /// 安全乘数（回归不可用时的退守值）。
    const SAFETY_MULTIPLIER: f64 = 1.5;
    /// 每 N 个新样本重新训练一次。
    const RETRAIN_INTERVAL: u64 = 200;

    /// 回归模型是否就绪。
    pub fn is_ready(&self) -> bool {
        self.sample_count >= Self::MIN_SAMPLES && self.r_squared >= Self::MIN_R_SQUARED
    }

    /// 预测实际峰值。
    ///
    /// 回归就绪时：actual = α × compiler_peak + β
    /// 未就绪时：返回 compiler_peak × 1.5（保守估计）
    ///
    /// 结果钳制在 [compiler_peak × 0.5, compiler_peak × 3.0] 之间。
    pub fn predict(&self, compiler_peak: f64) -> f64 {
        if !self.is_ready() {
            return compiler_peak * Self::SAFETY_MULTIPLIER;
        }

        let predicted = self.alpha * compiler_peak + self.beta;
        predicted
            .max(compiler_peak * 0.5)
            .min(compiler_peak * 3.0)
    }

    /// 是否需要重新训练。
    pub fn should_retrain(&self) -> bool {
        self.sample_count > 0
            && self.sample_count - self.last_trained_at >= Self::RETRAIN_INTERVAL
    }

    /// OLS 训练。
    ///
    /// 样本格式：`(compiler_peak_mb, actual_peak_mb)`
    ///
    /// 训练完成后更新 alpha, beta, r_squared, sample_count, last_trained_at。
    pub fn train(&mut self, samples: &[(f64, f64)]) {
        let n = samples.len() as f64;
        self.sample_count = samples.len() as u64;

        if n < Self::MIN_SAMPLES as f64 {
            return;
        }

        let sum_x: f64 = samples.iter().map(|(x, _)| x).sum();
        let sum_y: f64 = samples.iter().map(|(_, y)| y).sum();
        let mean_x = sum_x / n;
        let mean_y = sum_y / n;

        let num: f64 = samples
            .iter()
            .map(|(x, y)| (x - mean_x) * (y - mean_y))
            .sum();
        let den: f64 = samples
            .iter()
            .map(|(x, _)| (x - mean_x).powi(2))
            .sum();

        if den.abs() < 1e-10 {
            return; // 除零保护
        }

        self.alpha = num / den;
        self.beta = mean_y - self.alpha * mean_x;

        // 计算 r²
        let ss_res: f64 = samples
            .iter()
            .map(|(x, y)| (y - (self.alpha * x + self.beta)).powi(2))
            .sum();
        let ss_tot: f64 = samples.iter().map(|(_, y)| (y - mean_y).powi(2)).sum();

        self.r_squared = if ss_tot.abs() < 1e-10 {
            0.0
        } else {
            1.0 - ss_res / ss_tot
        };

        self.sample_count = samples.len() as u64;
        self.last_trained_at = self.sample_count;
    }

    /// 从 CSV 字符串加载样本。
    ///
    /// 格式：每行 `compiler_peak,actual_peak`
    pub fn load_samples_from_csv(content: &str) -> Vec<(f64, f64)> {
        content
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with("compiler") {
                    return None;
                }
                let mut parts = line.splitn(2, ',');
                let x = parts.next()?.trim().parse().ok()?;
                let y = parts.next()?.trim().parse().ok()?;
                Some((x, y))
            })
            .collect()
    }

    /// 序列化模型到 JSON 字符串。
    pub fn to_json(&self) -> String {
        fn fmt_f64(v: f64) -> String {
            if v.fract() == 0.0 {
                format!("{:.1}", v)
            } else {
                format!("{:.6}", v).trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
        format!(
            r#"{{"alpha":{},"beta":{},"r_squared":{},"sample_count":{},"last_trained_at":{}}}"#,
            fmt_f64(self.alpha),
            fmt_f64(self.beta),
            fmt_f64(self.r_squared),
            self.sample_count,
            self.last_trained_at
        )
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_default_not_ready() {
        let model = RegressionModel::default();
        assert!(!model.is_ready());
        // 默认预测应使用安全乘数
        let pred = model.predict(100.0);
        assert!((pred - 150.0).abs() < 0.01, "pred={}", pred);
    }

    #[test]
    fn regression_ols_perfect_fit() {
        // y = 1.2x + 10 (完美线性)
        let samples: Vec<(f64, f64)> = (0..100)
            .map(|i| {
                let x = i as f64 * 10.0;
                (x, 1.2 * x + 10.0)
            })
            .collect();

        let mut model = RegressionModel::default();
        model.train(&samples);

        assert!(model.is_ready());
        assert!((model.alpha - 1.2).abs() < 0.01, "alpha={}", model.alpha);
        assert!((model.beta - 10.0).abs() < 0.5, "beta={}", model.beta);
        assert!((model.r_squared - 1.0).abs() < 0.001, "r2={}", model.r_squared);

        // 预测应与真实值接近
        let pred = model.predict(50.0);
        assert!((pred - 70.0).abs() < 1.0, "pred(50)={}", pred);
    }

    #[test]
    fn regression_ols_noisy_fit() {
        // y ≈ 0.8x + 5 (带噪声)
        let samples: Vec<(f64, f64)> = (0..100)
            .map(|i| {
                let x = i as f64 * 5.0;
                (x, 0.8 * x + 5.0 + (i as f64 - 50.0) * 0.1)
            })
            .collect();

        let mut model = RegressionModel::default();
        model.train(&samples);

        assert!(model.is_ready());
        assert!((model.alpha - 0.8).abs() < 0.1, "alpha={}", model.alpha);
        assert!(model.r_squared > 0.9, "r2={}", model.r_squared);
    }

    #[test]
    fn regression_insufficient_samples() {
        // 只有 10 个样本（低于 MIN_SAMPLES=50）
        let samples: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64, (i * 2) as f64))
            .collect();

        let mut model = RegressionModel::default();
        model.train(&samples);

        // 样本数不足，不应就绪
        assert!(!model.is_ready());
        assert_eq!(model.sample_count, 10);
    }

    #[test]
    fn regression_predict_clamping() {
        let mut model = RegressionModel::default();
        model.alpha = 5.0; // 极端的斜率
        model.beta = 0.0;
        model.r_squared = 0.9;
        model.sample_count = 100;
        model.last_trained_at = 100;

        // 预测应为 clamped: compiler_peak × 3.0
        let pred = model.predict(100.0);
        assert!((pred - 300.0).abs() < 0.01, "pred={}", pred);
    }

    #[test]
    fn regression_load_csv() {
        let csv = "\
compiler_peak,actual_peak
10,15
20,28
30,42
";
        let samples = RegressionModel::load_samples_from_csv(csv);
        assert_eq!(samples.len(), 3);
        assert!((samples[0].0 - 10.0).abs() < 0.01);
        assert!((samples[0].1 - 15.0).abs() < 0.01);
    }

    #[test]
    fn regression_to_json() {
        let mut model = RegressionModel::default();
        model.alpha = 1.2;
        model.beta = 5.0;
        model.r_squared = 0.95;
        model.sample_count = 100;
        let json = model.to_json();
        assert!(json.contains("\"alpha\":1.2"));
        assert!(json.contains("\"beta\":5.0"));
        assert!(json.contains("\"r_squared\":0.95"));
    }

    #[test]
    fn regression_should_retrain() {
        let mut model = RegressionModel::default();
        model.sample_count = 250;
        model.last_trained_at = 50;
        assert!(model.should_retrain());

        model.last_trained_at = 200;
        assert!(!model.should_retrain()); // 距离不足 200
    }

    #[test]
    fn regression_divide_by_zero() {
        let mut model = RegressionModel::default();
        // 所有 x 相同 → den = 0
        let samples: Vec<(f64, f64)> = (0..100).map(|_| (42.0, 100.0)).collect();
        model.train(&samples); // 不应 panic
        assert!(!model.is_ready()); // r² = 0
    }
}
