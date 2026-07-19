//! Runner 配置 — 加载 runner.toml / atomix.toml 中的运行时配置。
//!
//! 覆盖 P3-RUN-004。

use serde::Deserialize;

/// Runner 完整配置。
#[derive(Debug, Clone, Deserialize)]
pub struct RunnerConfig {
    #[serde(default)]
    pub runner: RunnerSection,
    #[serde(default)]
    pub resources: ResourceSection,
    #[serde(default)]
    pub coefficients: CoefficientSection,
    #[serde(default)]
    pub per_task: PerTaskSection,
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

fn default_listen() -> String { "0.0.0.0:9000".into() }
fn default_task_dir() -> String { "/var/atomix/tasks".into() }
fn default_state_dir() -> String { "/var/atomix/state".into() }
fn default_resource() -> String { "auto".into() }
fn default_alpha_cpu() -> f64 { 0.75 }
fn default_alpha_mem() -> f64 { 0.50 }
fn default_alpha_io() -> f64 { 0.50 }
fn default_alpha_net() -> f64 { 0.60 }
fn default_cpu_per_task() -> f64 { 0.25 }
fn default_mem_per_task() -> f64 { 16.0 }
fn default_iops_per_task() -> f64 { 100.0 }
fn default_net_per_task() -> f64 { 1.0 }

impl RunnerConfig {
    /// 加载配置文件。path 为 None 时使用默认值。
    pub fn load(path: Option<&str>) -> Result<Self, String> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("读取配置文件失败: {}", e))?;
        toml::from_str(&content)
            .map_err(|e| format!("解析配置文件失败: {}", e))
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
            s if s == "auto" => hardware,
            s if s.ends_with('%') => {
                let pct: f64 = s[..s.len()-1].parse().unwrap_or(100.0);
                hardware * pct / 100.0
            }
            s if s.ends_with("MB") || s.ends_with("mb") => {
                let val: f64 = s[..s.len()-2].trim().parse().unwrap_or(0.0);
                val
            }
            s => {
                s.parse::<f64>().unwrap_or(hardware)
            }
        }
    }
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            runner: RunnerSection::default(),
            resources: ResourceSection::default(),
            coefficients: CoefficientSection::default(),
            per_task: PerTaskSection::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = RunnerConfig::default();
        assert_eq!(config.runner.listen, "0.0.0.0:9000");
        assert_eq!(config.coefficients.alpha_cpu, 0.75);
        assert_eq!(config.per_task.memory, 16.0);
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
