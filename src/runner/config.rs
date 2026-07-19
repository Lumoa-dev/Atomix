//! Runner 配置 — 加载 runner.toml / atomix.toml 中的运行时配置。
//!
//! 覆盖 P3-RUN-004。

use serde::Deserialize;

/// Runner 完整配置。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RunnerConfig {
    #[serde(default)]
    pub runner: RunnerSection,
    #[serde(default)]
    pub resources: ResourceSection,
    #[serde(default)]
    pub coefficients: CoefficientSection,
    #[serde(default)]
    pub per_task: PerTaskSection,
    #[serde(default)]
    pub executor: ExecutorSection,
    #[serde(default)]
    pub memory: MemorySection,
    #[serde(default)]
    pub regression: RegressionSection,
    #[serde(default)]
    pub scheduler: SchedulerSection,
}

/// `[runner]` 段。
#[derive(Debug, Clone, Deserialize)]
pub struct RunnerSection {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_task_dir")]
    pub task_dir: String,
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
}

impl Default for RunnerSection {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            task_dir: default_task_dir(),
            state_dir: default_state_dir(),
        }
    }
}

/// `[resources]` 段。
#[derive(Debug, Clone, Deserialize)]
pub struct ResourceSection {
    #[serde(default = "default_resource")]
    pub cpu: String,
    #[serde(default = "default_resource")]
    pub memory: String,
    #[serde(default = "default_resource")]
    pub iops: String,
    #[serde(default = "default_resource")]
    pub network: String,
}

impl Default for ResourceSection {
    fn default() -> Self {
        Self {
            cpu: default_resource(),
            memory: default_resource(),
            iops: default_resource(),
            network: default_resource(),
        }
    }
}

/// `[coefficients]` 段。
#[derive(Debug, Clone, Deserialize)]
pub struct CoefficientSection {
    #[serde(default = "default_alpha_cpu")]
    pub alpha_cpu: f64,
    #[serde(default = "default_alpha_mem")]
    pub alpha_mem: f64,
    #[serde(default = "default_alpha_io")]
    pub alpha_io: f64,
    #[serde(default = "default_alpha_net")]
    pub alpha_net: f64,
}

impl Default for CoefficientSection {
    fn default() -> Self {
        Self {
            alpha_cpu: default_alpha_cpu(),
            alpha_mem: default_alpha_mem(),
            alpha_io: default_alpha_io(),
            alpha_net: default_alpha_net(),
        }
    }
}

/// `[per_task]` 段。
#[derive(Debug, Clone, Deserialize)]
pub struct PerTaskSection {
    #[serde(default = "default_cpu_per_task")]
    pub cpu: f64,
    #[serde(default = "default_mem_per_task")]
    pub memory: f64,
    #[serde(default = "default_iops_per_task")]
    pub iops: f64,
    #[serde(default = "default_net_per_task")]
    pub network: f64,
}

impl Default for PerTaskSection {
    fn default() -> Self {
        Self {
            cpu: default_cpu_per_task(),
            memory: default_mem_per_task(),
            iops: default_iops_per_task(),
            network: default_net_per_task(),
        }
    }
}

// ─── 默认值 ─────────────────────────────────────

fn default_listen() -> String {
    "0.0.0.0:9000".into()
}
fn default_task_dir() -> String {
    "/var/atomix/tasks".into()
}
fn default_state_dir() -> String {
    "/var/atomix/state".into()
}
fn default_resource() -> String {
    "auto".into()
}
fn default_alpha_cpu() -> f64 {
    0.75
}
fn default_alpha_mem() -> f64 {
    0.50
}
fn default_alpha_io() -> f64 {
    0.50
}
fn default_alpha_net() -> f64 {
    0.60
}
fn default_cpu_per_task() -> f64 {
    0.25
}
fn default_mem_per_task() -> f64 {
    16.0
}
fn default_iops_per_task() -> f64 {
    100.0
}
fn default_net_per_task() -> f64 {
    1.0
}

// ─── Executor 配置 ──────────────────────────────────

/// `[executor]` 段。
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutorSection {
    /// 每个时间片的指令数。
    #[serde(default = "default_quantum_size")]
    pub quantum_size: u32,
    /// 心跳间隔（quantum 数，0 = 禁用）。
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u32,
}

impl Default for ExecutorSection {
    fn default() -> Self {
        Self {
            quantum_size: default_quantum_size(),
            heartbeat_interval: default_heartbeat_interval(),
        }
    }
}

fn default_quantum_size() -> u32 {
    1000
}
fn default_heartbeat_interval() -> u32 {
    0 // 默认禁用
}

// ─── 内存配置 ──────────────────────────────────────

/// `[memory]` 段。
#[derive(Debug, Clone, Deserialize)]
pub struct MemorySection {
    /// 安全冗余比例。
    #[serde(default = "default_safety_margin")]
    pub safety_margin: f64,
    /// 滑道倍数。
    #[serde(default = "default_slipway_multiplier")]
    pub slipway_multiplier: f64,
    /// 死区合并碎片率阈值（低于此不合并）。
    #[serde(default = "default_defrag_threshold")]
    pub defrag_threshold: f64,
}

impl Default for MemorySection {
    fn default() -> Self {
        Self {
            safety_margin: default_safety_margin(),
            slipway_multiplier: default_slipway_multiplier(),
            defrag_threshold: default_defrag_threshold(),
        }
    }
}

fn default_safety_margin() -> f64 {
    0.15
}
fn default_slipway_multiplier() -> f64 {
    1.5
}
fn default_defrag_threshold() -> f64 {
    0.30
}

// ─── 回归模型配置 ─────────────────────────────────

/// `[regression]` 段。
#[derive(Debug, Clone, Deserialize)]
pub struct RegressionSection {
    /// 最小样本数。
    #[serde(default = "default_min_samples")]
    pub min_samples: u64,
    /// 最小 r²。
    #[serde(default = "default_min_r_squared")]
    pub min_r_squared: f64,
    /// 重新训练间隔（样本数）。
    #[serde(default = "default_retrain_interval")]
    pub retrain_interval: u64,
    /// 安全乘数（回归不可用时的退守值）。
    #[serde(default = "default_safety_multiplier")]
    pub safety_multiplier: f64,
}

impl Default for RegressionSection {
    fn default() -> Self {
        Self {
            min_samples: default_min_samples(),
            min_r_squared: default_min_r_squared(),
            retrain_interval: default_retrain_interval(),
            safety_multiplier: default_safety_multiplier(),
        }
    }
}

fn default_min_samples() -> u64 {
    50
}
fn default_min_r_squared() -> f64 {
    0.6
}
fn default_retrain_interval() -> u64 {
    200
}
fn default_safety_multiplier() -> f64 {
    1.5
}

// ─── 调度器配置 ────────────────────────────────────

/// `[scheduler]` 段。
#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerSection {
    /// 预载阈值（剩余时间 > 网络延迟 × 倍数时触发）。
    #[serde(default = "default_prefetch_threshold")]
    pub prefetch_threshold: f64,
    /// 负载均衡启用。
    #[serde(default = "default_load_balance_enabled")]
    pub load_balance_enabled: bool,
    /// 冷启动 Bootstrap 阶段 N_batch。
    #[serde(default = "default_cold_start_bootstrap")]
    pub cold_start_bootstrap: u32,
    /// 冷启动 WarmUp 阶段任务数阈值。
    #[serde(default = "default_cold_start_warmup_threshold")]
    pub cold_start_warmup_threshold: u32,
    /// 冷启动 Accumulate 阶段任务数阈值。
    #[serde(default = "default_cold_start_accumulate_threshold")]
    pub cold_start_accumulate_threshold: u32,
}

impl Default for SchedulerSection {
    fn default() -> Self {
        Self {
            prefetch_threshold: default_prefetch_threshold(),
            load_balance_enabled: default_load_balance_enabled(),
            cold_start_bootstrap: default_cold_start_bootstrap(),
            cold_start_warmup_threshold: default_cold_start_warmup_threshold(),
            cold_start_accumulate_threshold: default_cold_start_accumulate_threshold(),
        }
    }
}

fn default_prefetch_threshold() -> f64 {
    1.5
}
fn default_load_balance_enabled() -> bool {
    true
}
fn default_cold_start_bootstrap() -> u32 {
    1
}
fn default_cold_start_warmup_threshold() -> u32 {
    5
}
fn default_cold_start_accumulate_threshold() -> u32 {
    50
}

impl RunnerConfig {
    /// 加载配置文件。path 为 None 时使用默认值。
    pub fn load(path: Option<&str>) -> Result<Self, String> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("读取配置文件失败: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("解析配置文件失败: {}", e))
    }

    /// 将 resources 中的百分比/绝对值解析为实际数字。
    /// `hardware` 是硬件检测值，用于百分比计算。
    pub fn resolve_resource(&self, key: &str, hardware: f64) -> f64 {
        let raw = match key {
            "cpu" => &self.resources.cpu,
            "memory" => &self.resources.memory,
            _ => "auto",
        };
        match raw {
            "auto" => hardware,
            s if s.ends_with('%') => {
                let pct: f64 = s[..s.len() - 1].parse().unwrap_or(100.0);
                hardware * pct / 100.0
            }
            s if s.ends_with("MB") || s.ends_with("mb") => {
                let val: f64 = s[..s.len() - 2].trim().parse().unwrap_or(0.0);
                val
            }
            s => s.parse::<f64>().unwrap_or(hardware),
        }
    }
}

// RunnerConfig 使用 #[derive(Default)]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = RunnerConfig::default();
        assert_eq!(config.runner.listen, "0.0.0.0:9000");
        assert_eq!(config.coefficients.alpha_cpu, 0.75);
        assert_eq!(config.per_task.memory, 16.0);
        // 新配置段默认值
        assert_eq!(config.executor.quantum_size, 1000);
        assert_eq!(config.executor.heartbeat_interval, 0);
        assert!((config.memory.safety_margin - 0.15).abs() < 0.001);
        assert!((config.memory.defrag_threshold - 0.30).abs() < 0.001);
        assert_eq!(config.regression.min_samples, 50);
        assert!((config.regression.min_r_squared - 0.6).abs() < 0.001);
        assert_eq!(config.scheduler.cold_start_bootstrap, 1);
        assert_eq!(config.scheduler.cold_start_warmup_threshold, 5);
    }

    #[test]
    fn resolve_resource_auto() {
        let config = RunnerConfig::default();
        let val = config.resolve_resource("cpu", 4.0);
        assert_eq!(val, 4.0);
    }

    #[test]
    fn resolve_resource_percent() {
        let config = RunnerConfig::default();
        // 修改 resources.cpu 为 "50%"
        let mut c = config.clone();
        c.resources.cpu = "50%".into();
        let val = c.resolve_resource("cpu", 8.0);
        assert_eq!(val, 4.0);
    }

    #[test]
    fn resolve_resource_absolute() {
        let config = RunnerConfig::default();
        let mut c = config.clone();
        c.resources.memory = "256MB".into();
        let val = c.resolve_resource("memory", 1024.0);
        assert_eq!(val, 256.0);
    }

    #[test]
    fn load_nonexistent_file() {
        let result = RunnerConfig::load(Some("/nonexistent/path.toml"));
        assert!(result.is_err());
    }
}
