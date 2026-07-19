"""
预定义仿真场景
==============
6个场景覆盖不同负载模式，用于算法对比。
"""

from sim.config import (
    HardwareConfig, ArrivalConfig, SimulationConfig,
    TaskProfile, DEFAULT_TASK_PROFILES
)
from sim.adaptive_controller import AdaptiveResourceController


# ── 场景 1：稳态混合负载 ──────────────────

SCENARIO_STEADY = {
    "name": "稳态混合负载",
    "description": "恒定速率到达的4象限混合任务，验证基准吞吐量和稳定性",
    "hardware": HardwareConfig(
        cpu_cores=16, mem_total_mb=16384, mem_free_mb=8192,
        iops_avail=30000, net_avail_mbps=1000,
    ),
    "profiles": DEFAULT_TASK_PROFILES,
    "arrival": ArrivalConfig(rate_per_sec=15.0),
    "simulation": SimulationConfig(
        duration_sec=120.0, warmup_sec=10.0, seed=42
    ),
}


# ── 场景 2：突发冲击 ──────────────────────

SCENARIO_BURST = {
    "name": "突发冲击",
    "description": "10s处突发5倍任务涌入持续15s，测试积压处理能力",
    "hardware": HardwareConfig(
        cpu_cores=16, mem_total_mb=16384, mem_free_mb=8192,
        iops_avail=30000, net_avail_mbps=1000,
    ),
    "profiles": DEFAULT_TASK_PROFILES,
    "arrival": ArrivalConfig(
        rate_per_sec=10.0,
        burst_multiplier=5.0,
        burst_duration_sec=15.0,
        burst_start_sec=10.0,
    ),
    "simulation": SimulationConfig(
        duration_sec=90.0, warmup_sec=5.0, seed=123
    ),
}


# ── 场景 3：内存压力 ──────────────────────

# 偏向大内存任务
MEM_PRESSURE_PROFILES = [
    TaskProfile("小快", 0.10, (1, 50),     (1, 8),    (10, 50),    (0.01, 0.1)),
    TaskProfile("小慢", 0.10, (200, 5000),  (1, 8),    (10, 50),    (0.01, 0.1)),
    TaskProfile("大快", 0.40, (10, 100),    (100, 800), (100, 500),  (0.1, 1.0)),
    TaskProfile("大慢", 0.40, (500, 10000), (100, 800), (100, 1000), (0.5, 5.0)),
]

SCENARIO_MEM_PRESSURE = {
    "name": "内存压力",
    "description": "90%任务为大内存类型（100-800MB），测试OOM反馈回路和滑道效果",
    "hardware": HardwareConfig(
        cpu_cores=16, mem_total_mb=16384, mem_free_mb=8192,
        iops_avail=30000, net_avail_mbps=1000,
    ),
    "profiles": MEM_PRESSURE_PROFILES,
    "arrival": ArrivalConfig(rate_per_sec=8.0),
    "simulation": SimulationConfig(
        duration_sec=120.0, warmup_sec=10.0, seed=456
    ),
}


# ── 场景 4：CPU 压力 ──────────────────────

CPU_PRESSURE_PROFILES = [
    TaskProfile("小快", 0.60, (10, 200),   (1, 16),   (10, 50),    (0.01, 0.1)),
    TaskProfile("小慢", 0.25, (2000, 8000), (1, 16),   (10, 50),    (0.01, 0.1)),
    TaskProfile("大快", 0.10, (50, 300),    (50, 200), (100, 500),  (0.1, 1.0)),
    TaskProfile("大慢", 0.05, (2000, 5000), (50, 200), (100, 500),  (0.1, 1.0)),
]

SCENARIO_CPU_PRESSURE = {
    "name": "CPU压力",
    "description": "大量CPU密集型任务涌入，测试槽位利用率和吞吐量上限",
    "hardware": HardwareConfig(
        cpu_cores=16, mem_total_mb=16384, mem_free_mb=8192,
        iops_avail=30000, net_avail_mbps=1000,
    ),
    "profiles": CPU_PRESSURE_PROFILES,
    "arrival": ArrivalConfig(rate_per_sec=30.0),
    "simulation": SimulationConfig(
        duration_sec=120.0, warmup_sec=10.0, seed=789
    ),
}


# ── 场景 5：震荡测试 ──────────────────────

# 模拟负载在轻/重之间快速切换（用多个突发段模拟）
SCENARIO_OSCILLATION = {
    "name": "震荡测试",
    "description": "负载在轻重之间快速切换，测试算法稳定性和振荡抑制",
    "hardware": HardwareConfig(
        cpu_cores=16, mem_total_mb=16384, mem_free_mb=8192,
        iops_avail=30000, net_avail_mbps=1000,
    ),
    "profiles": DEFAULT_TASK_PROFILES,
    "arrival": ArrivalConfig(rate_per_sec=20.0),  # 通过仿真中动态调速率模拟
    "simulation": SimulationConfig(
        duration_sec=150.0, warmup_sec=10.0, seed=101
    ),
}


# ── 场景 6：长跑稳定性 ────────────────────

SCENARIO_MARATHON = {
    "name": "长跑稳定性",
    "description": "模拟长时间运行（等效24h压缩），检测漂移/退化/内存泄漏",
    "hardware": HardwareConfig(
        cpu_cores=16, mem_total_mb=16384, mem_free_mb=8192,
        iops_avail=30000, net_avail_mbps=1000,
    ),
    "profiles": DEFAULT_TASK_PROFILES,
    "arrival": ArrivalConfig(rate_per_sec=12.0),
    "simulation": SimulationConfig(
        duration_sec=300.0, warmup_sec=20.0, seed=202
    ),
}


# ── 场景 7：不平衡负载 ────────────────────

SCENARIO_UNBALANCED = {
    "name": "不平衡负载",
    "description": "任务执行时间差异极大（10ms-20000ms），测试负载均衡效果",
    "hardware": HardwareConfig(cpu_cores=8, mem_total_mb=8192, mem_free_mb=4096),
    "profiles": [
        TaskProfile("极快", 0.50, (1, 10), (1, 4), (10, 30), (0.01, 0.05)),
        TaskProfile("中等", 0.30, (100, 500), (8, 32), (30, 100), (0.05, 0.5)),
        TaskProfile("极慢", 0.20, (5000, 20000), (16, 128), (50, 200), (0.1, 1.0)),
    ],
    "arrival": ArrivalConfig(rate_per_sec=20.0),
    "simulation": SimulationConfig(duration_sec=120.0, warmup_sec=10.0, seed=303),
}


# ── 场景 8：冷启动预测误差 ─────────────────

SCENARIO_COLD_START_ERROR = {
    "name": "冷启动预测误差",
    "description": "编译器预测初始不准确（偏差2-3倍），前10个任务预测差后逐步改善，测试冷启动协议适应性",
    "hardware": HardwareConfig(
        cpu_cores=8, mem_total_mb=8192, mem_free_mb=4096,
        compiler_peak_mb=100.0,  # 初始预测峰值（与实际偏差2-3倍）
    ),
    "profiles": [
        TaskProfile("小内存", 0.40, (10, 100), (10, 50), (10, 50), (0.01, 0.1)),
        TaskProfile("中内存", 0.35, (50, 500), (50, 200), (30, 100), (0.05, 0.5)),
        TaskProfile("大内存", 0.25, (100, 2000), (200, 500), (50, 200), (0.1, 1.0)),
    ],
    "arrival": ArrivalConfig(rate_per_sec=5.0),  # 低速率渐变
    "simulation": SimulationConfig(duration_sec=180.0, warmup_sec=10.0, seed=404),
}


# ── 场景 9：高碎片回收 ────────────────────

SCENARIO_HIGH_FRAGMENTATION = {
    "name": "高碎片回收",
    "description": "高内存压力场景，任务内存范围宽（50-500MB），槽位紧张迫使频繁OOM，测试死区碎片回收与ROI评估效果",
    "hardware": HardwareConfig(
        cpu_cores=8, mem_total_mb=8192, mem_free_mb=4096,
        safety_margin=0.10,  # 更小的安全冗余以增加内存压力
    ),
    "profiles": [
        TaskProfile("小快", 0.20, (10, 100), (50, 150), (10, 50), (0.01, 0.1)),
        TaskProfile("小慢", 0.15, (200, 2000), (50, 150), (10, 50), (0.01, 0.1)),
        TaskProfile("大快", 0.35, (10, 100), (150, 400), (100, 500), (0.1, 1.0)),
        TaskProfile("大慢", 0.30, (200, 5000), (150, 500), (100, 500), (0.1, 1.0)),
    ],
    "arrival": ArrivalConfig(rate_per_sec=10.0),
    "simulation": SimulationConfig(duration_sec=120.0, warmup_sec=10.0, seed=505),
}


# ── 场景 10：大批量小任务预载 ──────────────

SCENARIO_BATCH_PREFETCH = {
    "name": "大批量小任务预载",
    "description": "极高到达率（50任务/秒），每个任务极小（1-50ms CPU, 1-8MB内存），测试预载调度器是否能跟上吞吐压力",
    "hardware": HardwareConfig(
        cpu_cores=16, mem_total_mb=16384, mem_free_mb=8192,
        iops_avail=50000, net_avail_mbps=2000,
    ),
    "profiles": [
        TaskProfile("微型", 0.70, (1, 20), (1, 4), (5, 20), (0.01, 0.05)),
        TaskProfile("小型", 0.30, (10, 50), (2, 8), (10, 30), (0.01, 0.1)),
    ],
    "arrival": ArrivalConfig(rate_per_sec=50.0),
    "simulation": SimulationConfig(duration_sec=120.0, warmup_sec=10.0, seed=606),
}


# ── 所有场景列表 ──────────────────────────

ALL_SCENARIOS = [
    SCENARIO_STEADY,
    SCENARIO_BURST,
    SCENARIO_MEM_PRESSURE,
    SCENARIO_CPU_PRESSURE,
    SCENARIO_OSCILLATION,
    SCENARIO_MARATHON,
    SCENARIO_UNBALANCED,
    SCENARIO_COLD_START_ERROR,
    SCENARIO_HIGH_FRAGMENTATION,
    SCENARIO_BATCH_PREFETCH,
]


def get_algorithm_variants(hw: HardwareConfig):
    """获取所有算法变体"""
    return AdaptiveResourceController.generate_variants(hw)
