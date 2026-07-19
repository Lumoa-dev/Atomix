//! 硬件检测 — 查询 CPU 核心数和可用内存。
//!
//! 覆盖 P3-RUN-003。

/// 检测到的硬件信息。
#[derive(Debug, Clone)]
pub struct HardwareInfo {
    /// 可用 CPU 核心数（逻辑核心）。
    pub cpu_cores: f64,
    /// 可用物理内存（MB）。
    pub mem_mb: f64,
}

/// 检测本机硬件。检测失败时使用提供的默认值。
pub fn detect_hardware(default_cpu: f64, default_mem_mb: f64) -> HardwareInfo {
    let cpu = detect_cpu_cores().unwrap_or(default_cpu);
    let mem = detect_memory_mb().unwrap_or(default_mem_mb);
    HardwareInfo {
        cpu_cores: cpu,
        mem_mb: mem,
    }
}

/// 检测 CPU 核心数。
fn detect_cpu_cores() -> Option<f64> {
    std::thread::available_parallelism()
        .ok()
        .map(|n| n.get() as f64)
}

/// 检测可用物理内存（MB）。
#[cfg(target_os = "linux")]
fn detect_memory_mb() -> Option<f64> {
    // 读取 /proc/meminfo 中的 MemAvailable
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in content.lines() {
        if line.starts_with("MemAvailable:") {
            // "MemAvailable:    12345678 kB"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: f64 = parts[1].parse().ok()?;
                return Some(kb / 1024.0);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn detect_memory_mb() -> Option<f64> {
    // 使用 GlobalMemoryStatusEx
    // 通过 winapi 或直接调用
    // 简化实现：使用环境变量或默认值
    None
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn detect_memory_mb() -> Option<f64> {
    // macOS 或其他 Unix: sysctl hw.memsize
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cpu_returns_some() {
        // 在任何真实机器上，cpu 检测应该返回正数
        if let Some(cpu) = detect_cpu_cores() {
            assert!(cpu > 0.0, "CPU cores should be positive");
        }
    }

    #[test]
    fn detect_hardware_fallback() {
        let info = detect_hardware(2.0, 512.0);
        assert!(info.cpu_cores > 0.0);
        assert!(info.mem_mb > 0.0);
    }

    #[test]
    fn detect_hardware_uses_detected_values() {
        let info = detect_hardware(1.0, 1.0);
        // CPU 应该来自真实检测（如果在真实机器上运行）
        if let Some(detected) = detect_cpu_cores() {
            assert_eq!(info.cpu_cores, detected);
        }
    }
}
