"""
Atomix 仿真配置系统
====================
统一管理硬件参数、任务参数、算法参数和场景参数。
"""

from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple
import numpy as np


# ────────────────────────────────────────────────────────────
# 硬件配置
# ────────────────────────────────────────────────────────────

@dataclass
class HardwareConfig:
    """模拟硬件资源（你的机器：16核 / 16GB，仿真用 ~50%）"""
    cpu_cores: int = 16         # 物理核数
    mem_total_mb: float = 16384 # 总内存 MB
    mem_free_mb: float = 8192   # 空闲内存（~50%）
    iops_avail: float = 30000   # 可用 IOPS（NVMe 级别）
    net_avail_mbps: float = 1000 # 网络带宽 Mbps（千兆）

    # 保留系数 (α)
    alpha_cpu: float = 0.75
    alpha_mem: float = 0.50
    alpha_io: float = 0.50
    alpha_net: float = 0.60

    # 每任务资源估算（初始值）
    cpu_per_task: float = 0.25
    mem_per_task_mb: float = 16.0
    iops_per_task: float = 100.0
    net_per_task_mbps: float = 1.0

    # 安全冗余
    safety_margin: float = 0.15  # β_safety: 15% 安全冗余

	# 时间片
    quantum_instrs: int = 1000

    # 编译器预测（仿真中模拟生成）
    compiler_peak_mb: float = 0.0  # 0 表示使用真实值作为预测

    @property
    def effective_cpu(self) -> float:
        return self.cpu_cores * self.alpha_cpu

    @property
    def effective_mem(self) -> float:
        return self.mem_free_mb * self.alpha_mem

    @property
    def effective_iops(self) -> float:
        return self.iops_avail * self.alpha_io

    @property
    def effective_net(self) -> float:
        return self.net_avail_mbps * self.alpha_net

    def compute_hard_ceiling(self) -> int:
        """计算硬上限 H = min(C, M, I, N)"""
        C = self.effective_cpu / self.cpu_per_task
        M = self.effective_mem / self.mem_per_task_mb
        I = self.effective_iops / self.iops_per_task
        N = self.effective_net / self.net_per_task_mbps
        return max(1, int(np.floor(min(C, M, I, N))))

    def compute_slot_memory(self, n_batch: int) -> float:
        """计算每槽位虚地址大小（含安全冗余）"""
        total_pool = self.effective_mem * (1 - self.safety_margin)
        return total_pool / n_batch


# ────────────────────────────────────────────────────────────
# 任务类型配置
# ────────────────────────────────────────────────────────────

@dataclass
class TaskProfile:
    """任务特征描述"""
    name: str
    weight: float           # 在混合负载中的占比
    cpu_ms_range: Tuple[float, float]   # CPU 耗时范围
    mem_mb_range: Tuple[float, float]   # 内存占用范围
    iops_range: Tuple[float, float]     # IOPS 需求范围
    net_mbps_range: Tuple[float, float] # 网络带宽需求范围

    def sample(self, rng: np.random.Generator) -> Dict[str, float]:
        return {
            "cpu_ms": rng.uniform(*self.cpu_ms_range),
            "mem_mb": rng.uniform(*self.mem_mb_range),
            "iops": rng.uniform(*self.iops_range),
            "net_mbps": rng.uniform(*self.net_mbps_range),
        }


# 四个象限的默认任务特征
DEFAULT_TASK_PROFILES = [
    TaskProfile("小快", 0.40, (1, 50),     (1, 8),    (10, 50),    (0.01, 0.1)),
    TaskProfile("小慢", 0.25, (200, 5000),  (1, 8),    (10, 50),    (0.01, 0.1)),
    TaskProfile("大快", 0.15, (10, 100),    (50, 500), (100, 500),  (0.1, 1.0)),
    TaskProfile("大慢", 0.20, (500, 10000), (50, 500), (100, 1000), (0.5, 5.0)),
]


# ────────────────────────────────────────────────────────────
# 任务到达配置
# ────────────────────────────────────────────────────────────

@dataclass
class ArrivalConfig:
    """任务到达模型配置"""
    rate_per_sec: float = 10.0      # 平均到达率（泊松 λ）
    burst_multiplier: float = 1.0   # 突发倍数
    burst_duration_sec: float = 0.0 # 突发持续时间（0=不突发）
    burst_start_sec: float = 10.0   # 突发开始时间


# ────────────────────────────────────────────────────────────
# 算法变体标识
# ────────────────────────────────────────────────────────────

from enum import Enum, auto


class SmoothingMethod(Enum):
    """因子平滑化方法"""
    DISCRETE_SEGMENTED = auto()   # 原文档分段表
    LINEAR_INTERP = auto()        # 分段间线性插值
    SIGMOID_SMOOTH = auto()       # 连续 sigmoid
    ADAPTIVE_SMOOTH = auto()      # sigmoid + 在线调参


class MergeStrategy(Enum):
    """因子合并策略"""
    MULTIPLICATIVE = auto()       # β × λ × σ × γ
    MIN_BOTTLENECK = auto()       # min(β, λ, σ, γ)
    WEIGHTED_GEOMEAN = auto()     # exp(Σ w_i × ln(f_i))


class OOMFeedback(Enum):
    """OOM 反馈策略"""
    HARD_MULTIPLY = auto()        # ≥3次 → α_mem × 0.8
    AIMD = auto()                 # 加法增加/乘法减少
    AIMD_HYSTERESIS = auto()      # AIMD + 滞回区


class SlipwayStrategy(Enum):
    """滑道大小策略"""
    FIXED_1_5X = auto()           # 固定 1.5×
    DYNAMIC_ELASTIC = auto()      # 根据 OOM 频率动态调整
    PERCENTILE_P95 = auto()       # 基于历史内存 P95


@dataclass
class AlgorithmConfig:
    """算法配置组合"""
    smoothing: SmoothingMethod = SmoothingMethod.SIGMOID_SMOOTH
    merge: MergeStrategy = MergeStrategy.MULTIPLICATIVE
    oom_feedback: OOMFeedback = OOMFeedback.AIMD_HYSTERESIS
    slipway: SlipwayStrategy = SlipwayStrategy.DYNAMIC_ELASTIC

    # 滑道固定尺寸倍数
    slipway_multiplier: float = 1.5

    # OOM 反馈参数
    oom_threshold_count: int = 3          # 时间窗口内触发阈值
    oom_window_sec: float = 60.0          # 时间窗口
    oom_alpha_multiplier: float = 0.8     # 乘法因子（HardMultiply）
    aimd_increase: float = 0.02           # AIMD 加法增量
    aimd_decrease_factor: float = 0.75    # AIMD 乘法减少因子
    hysteresis_low: float = 2             # 滞回下限（低于此值才恢复增加）
    hysteresis_high: float = 5            # 滞回上限（高于此值才触发减少）

    # 因子平滑化参数
    sigmoid_steepness: float = 5.0        # sigmoid 陡峭度

    # 滑动窗口大小
    window_size: int = 100                # 统计滑动窗口（任务数）

    def label(self) -> str:
        """简短标签，用于图表"""
        sm = {SmoothingMethod.DISCRETE_SEGMENTED: "Disc",
              SmoothingMethod.LINEAR_INTERP: "Lin",
              SmoothingMethod.SIGMOID_SMOOTH: "Sig",
              SmoothingMethod.ADAPTIVE_SMOOTH: "Adp"}
        mg = {MergeStrategy.MULTIPLICATIVE: "Mul",
              MergeStrategy.MIN_BOTTLENECK: "Min",
              MergeStrategy.WEIGHTED_GEOMEAN: "WGM"}
        om = {OOMFeedback.HARD_MULTIPLY: "Hard",
              OOMFeedback.AIMD: "AIMD",
              OOMFeedback.AIMD_HYSTERESIS: "AimdH"}
        sw = {SlipwayStrategy.FIXED_1_5X: "1.5x",
              SlipwayStrategy.DYNAMIC_ELASTIC: "Dyn",
              SlipwayStrategy.PERCENTILE_P95: "P95"}
        return f"{sm[self.smoothing]}_{mg[self.merge]}_{om[self.oom_feedback]}_{sw[self.slipway]}"

    def full_label(self) -> str:
        return f"Smooth={self.smoothing.name} Merge={self.merge.name} OOM={self.oom_feedback.name} Slip={self.slipway.name}"


# ────────────────────────────────────────────────────────────
# 新增算法配置
# ────────────────────────────────────────────────────────────

@dataclass
class ColdStartConfig:
    """冷启动协议配置"""
    enabled: bool = True
    bootstrap_n_batch: int = 1
    warmup_threshold: int = 5        # 阶段 1 → 2 的任务数
    accumulate_threshold: int = 50   # 阶段 2 → 3 的任务数
    safety_multiplier: float = 1.5   # 冷启动时预测峰值安全系数


@dataclass
class LoadBalanceConfig:
    """负载均衡配置"""
    enabled: bool = True
    method: str = "weighted_least"   # "round_robin" | "weighted_least"
    anti_skew_threshold: float = 0.1  # 负载差距在此比例内视为"接近"


@dataclass
class PrefetchConfig:
    """预载调度配置"""
    enabled: bool = True
    network_rtt_ms: float = 50.0     # 网络延迟估计
    prefetch_threshold_ratio: float = 1.5  # 剩余时间 > RTT × 此值才预载
    max_depth: int = 3               # 最大预载深度


@dataclass
class DefragConfig:
    """死区合并配置"""
    enabled: bool = True
    frag_threshold: float = 0.30     # 碎片率超过此值触发评估
    min_dead_slots: int = 2          # 最少死区数量
    roi_min_ratio: float = 2.0       # ROI 最低比率


@dataclass
class RegressionConfig:
    """线性回归模型配置"""
    enabled: bool = True
    min_samples: int = 50            # 最少样本数
    min_r_squared: float = 0.6       # 最低 r²
    retrain_interval: int = 200      # 重新训练间隔
    safety_multiplier: float = 1.5   # 回归不可用时的安全系数


# ────────────────────────────────────────────────────────────
# 仿真配置
# ────────────────────────────────────────────────────────────

@dataclass
class SimulationConfig:
    """仿真运行配置"""
    duration_sec: float = 120.0       # 仿真时长
    time_step_sec: float = 0.02       # 时间步长（0.01→0.02 步数减半，精度损失可忽略）
    warmup_sec: float = 10.0          # 预热时间（不计入统计）
    seed: int = 42                    # 随机种子

    # 输出
    report_dir: str = "sim/reports"
    verbose: bool = True
    progress_interval_sec: float = 5.0

    # CSV 原始数据
    export_csv: bool = True           # 是否输出 CSV
    csv_dir: str = "sim/reports/csv"  # CSV 输出目录

    # 全参数扫描
    full_scan: bool = False           # 是否跑全参数扫描
    scan_params: dict = field(default_factory=lambda: {
        "cpu_cores": [4, 8, 16],
        "mem_free_mb": [2048, 4096, 8192],
        "arrival_rate": [5, 10, 20],
        "sigmoid_steepness": [3, 5, 8],
        "aimd_increase": [0.01, 0.02, 0.05],
    })


# ────────────────────────────────────────────────────────────
# 场景配置
# ────────────────────────────────────────────────────────────

@dataclass
class ScenarioConfig:
    """场景完整配置"""
    name: str
    description: str
    hardware: HardwareConfig = field(default_factory=HardwareConfig)
    task_profiles: List[TaskProfile] = field(default_factory=lambda: DEFAULT_TASK_PROFILES)
    arrival: ArrivalConfig = field(default_factory=ArrivalConfig)
    simulation: SimulationConfig = field(default_factory=SimulationConfig)

    # 新增算法配置
    cold_start: ColdStartConfig = field(default_factory=ColdStartConfig)
    load_balance: LoadBalanceConfig = field(default_factory=LoadBalanceConfig)
    prefetch: PrefetchConfig = field(default_factory=PrefetchConfig)
    defrag: DefragConfig = field(default_factory=DefragConfig)
    regression: RegressionConfig = field(default_factory=RegressionConfig)

    # 要测试的算法变体列表
    algorithm_variants: List[AlgorithmConfig] = field(default_factory=lambda: [
        AlgorithmConfig(smoothing=SmoothingMethod.DISCRETE_SEGMENTED,
                        merge=MergeStrategy.MULTIPLICATIVE,
                        oom_feedback=OOMFeedback.HARD_MULTIPLY,
                        slipway=SlipwayStrategy.FIXED_1_5X),
        AlgorithmConfig(smoothing=SmoothingMethod.SIGMOID_SMOOTH,
                        merge=MergeStrategy.MULTIPLICATIVE,
                        oom_feedback=OOMFeedback.AIMD_HYSTERESIS,
                        slipway=SlipwayStrategy.DYNAMIC_ELASTIC),
    ])
