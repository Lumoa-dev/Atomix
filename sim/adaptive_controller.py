"""
自适应资源控制器 — 策略模块
============================
实现 N_batch 计算的完整算法链路，支持多种变体组合。

算法链路：
  输入（硬件快照 + 任务统计 + 事件计数）
    → 因子平滑化（4种方法）
    → 因子合并（3种策略）
    → OOM 反馈调节（3种策略）
    → 滑道调整（3种策略）
  → 输出（N_batch, slot_size, slipway_ratio）
"""

from typing import List, Dict, Tuple, Optional
from dataclasses import dataclass, field
import numpy as np

from sim.config import (
    HardwareConfig, AlgorithmConfig,
    SmoothingMethod, MergeStrategy, OOMFeedback, SlipwayStrategy,
    ColdStartConfig, RegressionConfig
)
from sim.regression_model import RegressionModel


# ═══════════════════════════════════════════════════════════════
# 因子平滑化函数
# ═══════════════════════════════════════════════════════════════

class FactorSmoother:
    """因子平滑化 —— 4种变体"""

    def __init__(self, method: SmoothingMethod, config: AlgorithmConfig):
        self.method = method
        self.config = config
        self.k = config.sigmoid_steepness

        # β 自适应平滑独立陡峭度（不串扰 λ/σ/γ 的 sigmoid）
        self._beta_k = config.sigmoid_steepness

        # 自适应平滑的历史记录
        self._beta_history: List[float] = []
        self._lambda_history: List[float] = []
        self._sigma_history: List[float] = []
        self._gamma_history: List[float] = []

    def beta(self, d: float) -> float:
        """积压因子 β(d)：d = pool_depth / N_batch"""
        if self.method == SmoothingMethod.DISCRETE_SEGMENTED:
            return self._beta_discrete(d)
        elif self.method == SmoothingMethod.LINEAR_INTERP:
            return self._beta_linear(d)
        elif self.method == SmoothingMethod.SIGMOID_SMOOTH:
            return self._beta_sigmoid(d)
        elif self.method == SmoothingMethod.ADAPTIVE_SMOOTH:
            return self._beta_adaptive(d)
        return 1.0

    def _beta_discrete(self, d: float) -> float:
        if d < 1.0:   return 1.00
        if d < 2.0:   return 0.85
        if d < 3.0:   return 0.70
        return 0.50

    def _beta_linear(self, d: float) -> float:
        """分段间线性插值，消除硬跳变"""
        if d <= 0.5:  return 1.00
        if d <= 1.0:  return 1.00 - (d - 0.5) * 0.30 / 0.5    # 1.00 → 0.85
        if d <= 2.0:  return 0.85 - (d - 1.0) * 0.15 / 1.0    # 0.85 → 0.70
        if d <= 3.0:  return 0.70 - (d - 2.0) * 0.20 / 1.0    # 0.70 → 0.50
        return max(0.30, 0.50 - (d - 3.0) * 0.05)

    def _beta_sigmoid(self, d: float) -> float:
        """S 型平滑函数，无硬边界"""
        # 中心在 d=1.5，值在 0.5~1.0 之间
        return 0.50 + 0.50 / (1.0 + np.exp(self.k * (d - 1.5)))

    def _beta_adaptive(self, d: float) -> float:
        """Sigmoid + 根据历史在线调整陡峭度（独立参数，不串扰其他因子）"""
        val = 0.50 + 0.50 / (1.0 + np.exp(self._beta_k * (d - 1.5)))
        self._beta_history.append(val)
        if len(self._beta_history) > 100:
            self._beta_history.pop(0)
        # 如果历史波动大，降低陡峭度 → 更平滑
        if len(self._beta_history) >= 10:
            std = np.std(self._beta_history)
            if std > 0.1:
                self._beta_k = max(2.0, self._beta_k * 0.95)
            else:
                self._beta_k = min(10.0, self._beta_k * 1.05)
        return val

    def lambda_speed(self, mu_t_ms: float) -> float:
        """速度因子 λ(μ_t)：mu_t = 平均任务耗时 (ms)"""
        if self.method == SmoothingMethod.DISCRETE_SEGMENTED:
            return self._lambda_discrete(mu_t_ms)
        elif self.method == SmoothingMethod.LINEAR_INTERP:
            return self._lambda_linear(mu_t_ms)
        elif self.method == SmoothingMethod.SIGMOID_SMOOTH:
            return self._lambda_sigmoid(mu_t_ms)
        elif self.method == SmoothingMethod.ADAPTIVE_SMOOTH:
            return self._lambda_adaptive(mu_t_ms)
        return 1.0

    def _lambda_discrete(self, mu_t_ms: float) -> float:
        if mu_t_ms < 50:       return 1.40
        if mu_t_ms < 200:      return 1.20
        if mu_t_ms < 1000:     return 1.10
        return 1.00

    def _lambda_linear(self, mu_t_ms: float) -> float:
        if mu_t_ms <= 20:   return 1.40
        if mu_t_ms <= 50:   return 1.40 - (mu_t_ms - 20) * 0.20 / 30
        if mu_t_ms <= 200:  return 1.20 - (mu_t_ms - 50) * 0.10 / 150
        if mu_t_ms <= 1000: return 1.10 - (mu_t_ms - 200) * 0.10 / 800
        return 1.00

    def _lambda_sigmoid(self, mu_t_ms: float) -> float:
        return 1.00 + 0.40 / (1.0 + np.exp(self.k * (mu_t_ms / 500.0 - 1.0)))

    def _lambda_adaptive(self, mu_t_ms: float) -> float:
        return self._lambda_sigmoid(mu_t_ms)

    def sigma_volume(self, r: float) -> float:
        """体积因子 σ(r)：r = μ_m / MEM_per_task 实际与预估比值"""
        if self.method == SmoothingMethod.DISCRETE_SEGMENTED:
            return self._sigma_discrete(r)
        elif self.method == SmoothingMethod.LINEAR_INTERP:
            return self._sigma_linear(r)
        elif self.method == SmoothingMethod.SIGMOID_SMOOTH:
            return self._sigma_sigmoid(r)
        elif self.method == SmoothingMethod.ADAPTIVE_SMOOTH:
            return self._sigma_adaptive(r)
        return 1.0

    def _sigma_discrete(self, r: float) -> float:
        if r < 0.3:    return 1.30
        if r < 0.6:    return 1.15
        if r < 1.5:    return 1.00
        if r < 3.0:    return 0.80
        return 0.60

    def _sigma_linear(self, r: float) -> float:
        if r <= 0.15:  return 1.30
        if r <= 0.3:   return 1.30 - (r - 0.15) * 0.15 / 0.15
        if r <= 0.6:   return 1.15 - (r - 0.3) * 0.15 / 0.3
        if r <= 1.5:   return 1.00 - (r - 0.6) * 0.20 / 0.9
        if r <= 3.0:   return 0.80 - (r - 1.5) * 0.20 / 1.5
        return max(0.40, 0.60 - (r - 3.0) * 0.05)

    def _sigma_sigmoid(self, r: float) -> float:
        """中心在 r=1.0（正常），区间 0.55~1.35"""
        return 0.55 + 0.80 / (1.0 + np.exp(self.k * (r - 1.0)))

    def _sigma_adaptive(self, r: float) -> float:
        return self._sigma_sigmoid(r)

    def gamma_variance(self, v_t: float) -> float:
        """方差因子 γ(v_t)：v_t = σ_t / μ_t 耗时变异系数"""
        if self.method == SmoothingMethod.DISCRETE_SEGMENTED:
            return self._gamma_discrete(v_t)
        elif self.method == SmoothingMethod.LINEAR_INTERP:
            return self._gamma_linear(v_t)
        elif self.method == SmoothingMethod.SIGMOID_SMOOTH:
            return self._gamma_sigmoid(v_t)
        elif self.method == SmoothingMethod.ADAPTIVE_SMOOTH:
            return self._gamma_adaptive(v_t)
        return 1.0

    def _gamma_discrete(self, v_t: float) -> float:
        if v_t < 0.3:   return 1.00
        if v_t < 0.6:   return 0.85
        if v_t < 1.0:   return 0.70
        return 0.55

    def _gamma_linear(self, v_t: float) -> float:
        if v_t <= 0.15:  return 1.00
        if v_t <= 0.3:   return 1.00 - (v_t - 0.15) * 0.15 / 0.15
        if v_t <= 0.6:   return 0.85 - (v_t - 0.3) * 0.15 / 0.3
        if v_t <= 1.0:   return 0.70 - (v_t - 0.6) * 0.15 / 0.4
        return max(0.35, 0.55 - (v_t - 1.0) * 0.05)

    def _gamma_sigmoid(self, v_t: float) -> float:
        """中心在 v_t=0.5，区间 0.50~1.05"""
        return 0.50 + 0.55 / (1.0 + np.exp(self.k * (v_t - 0.5)))

    def _gamma_adaptive(self, v_t: float) -> float:
        return self._gamma_sigmoid(v_t)


# ═══════════════════════════════════════════════════════════════
# 因子合并策略
# ═══════════════════════════════════════════════════════════════

def merge_factors(factors: List[float], strategy: MergeStrategy,
                  weights: Optional[List[float]] = None) -> float:
    """
    合并多个因子为单一乘数。

    factors: [β, λ, σ, γ]
    strategy: 合并策略
    weights: 可选权重（用于 WeightedGeoMean）
    """
    if not factors:
        return 1.0

    if strategy == MergeStrategy.MULTIPLICATIVE:
        return np.prod(factors)

    elif strategy == MergeStrategy.MIN_BOTTLENECK:
        return min(factors)

    elif strategy == MergeStrategy.WEIGHTED_GEOMEAN:
        if weights is None:
            weights = [0.25] * len(factors)
        # 归一化
        w = np.array(weights) / np.sum(weights)
        log_factors = np.log(np.maximum(factors, 0.01))
        return np.exp(np.dot(w, log_factors))

    return 1.0


# ═══════════════════════════════════════════════════════════════
# OOM 反馈控制器
# ═══════════════════════════════════════════════════════════════

class OOMFeedbackController:
    """OOM 反馈 —— 3种变体"""

    def __init__(self, config: AlgorithmConfig, initial_alpha_mem: float):
        self.config = config
        self.alpha_mem = initial_alpha_mem
        self.initial_alpha = initial_alpha_mem

        # OOM 事件历史（时间戳列表）
        self.oom_events: List[float] = []

        # AIMD 状态
        self._aimd_state = "INCREASE"  # INCREASE / DECREASE
        self._last_adjust_time: float = 0.0
        self._consecutive_ok_windows: int = 0

    def record_oom(self, sim_time: float):
        """记录一次 OOM 事件"""
        self.oom_events.append(sim_time)
        # 清理窗口外事件
        cutoff = sim_time - self.config.oom_window_sec
        self.oom_events = [t for t in self.oom_events if t > cutoff]

    def recent_oom_count(self, sim_time: float) -> int:
        """时间窗口内的 OOM 次数"""
        cutoff = sim_time - self.config.oom_window_sec
        return sum(1 for t in self.oom_events if t > cutoff)

    def update(self, sim_time: float) -> float:
        """
        更新 alpha_mem 并返回新值。
        每步调用一次。
        """
        method = self.config.oom_feedback

        if method == OOMFeedback.HARD_MULTIPLY:
            self._update_hard_multiply(sim_time)
        elif method == OOMFeedback.AIMD:
            self._update_aimd(sim_time)
        elif method == OOMFeedback.AIMD_HYSTERESIS:
            self._update_aimd_hysteresis(sim_time)

        # 钳制在合理范围
        self.alpha_mem = max(0.10, min(self.initial_alpha * 1.5, self.alpha_mem))

        return self.alpha_mem

    def _update_hard_multiply(self, sim_time: float):
        """当前文档方案：≥3次 OOM → α_mem × 0.8；60s 无 OOM → ×1.1"""
        count = self.recent_oom_count(sim_time)

        if count >= self.config.oom_threshold_count:
            self.alpha_mem *= self.config.oom_alpha_multiplier
            # 清空窗口内事件（避免重复触发）
            cutoff = sim_time - self.config.oom_window_sec
            self.oom_events = [t for t in self.oom_events if t <= cutoff]

        elif count == 0 and self.alpha_mem < self.initial_alpha:
            # 逐步恢复
            self._consecutive_ok_windows += 1
            if self._consecutive_ok_windows >= 60:  # 60秒
                self.alpha_mem = min(self.initial_alpha, self.alpha_mem * 1.1)
                self._consecutive_ok_windows = 0
        else:
            self._consecutive_ok_windows = 0

    def _update_aimd(self, sim_time: float):
        """AIMD：OOM → 乘法减少；正常 → 加法增加"""
        count = self.recent_oom_count(sim_time)

        if count >= self.config.oom_threshold_count:
            # 乘法减少 (Multiplicative Decrease)
            self.alpha_mem *= self.config.aimd_decrease_factor
            # 清空窗口
            cutoff = sim_time - self.config.oom_window_sec
            self.oom_events = [t for t in self.oom_events if t <= cutoff]
            self._aimd_state = "DECREASE"
        elif count == 0 and self.alpha_mem < self.initial_alpha:
            # 加法增加 (Additive Increase)
            self.alpha_mem += self.config.aimd_increase
            self._aimd_state = "INCREASE"

    def _update_aimd_hysteresis(self, sim_time: float):
        """
        AIMD + 滞回区：
        - OOM > hysteresis_high → MD（乘法减少）
        - OOM < hysteresis_low → AI（加法增加）
        - 在中间 → 保持不变（滞回区）
        """
        count = self.recent_oom_count(sim_time)

        if count >= self.config.hysteresis_high:
            self.alpha_mem *= self.config.aimd_decrease_factor
            cutoff = sim_time - self.config.oom_window_sec
            self.oom_events = [t for t in self.oom_events if t <= cutoff]
            self._aimd_state = "DECREASE"
        elif count <= self.config.hysteresis_low and self.alpha_mem < self.initial_alpha:
            # 在滞回下限以下且未恢复到初始值 → 缓慢加法增加
            self.alpha_mem += self.config.aimd_increase * 0.5  # 恢复速度减半
            self._aimd_state = "INCREASE"
        # else: 在滞回区内 → 不调整


# ═══════════════════════════════════════════════════════════════
# 滑道大小计算
# ═══════════════════════════════════════════════════════════════

def compute_slipway_multiplier(strategy: SlipwayStrategy, config: AlgorithmConfig,
                               peak_mem_samples: List[float],
                               oom_rate: float,
                               default_slot_size: float) -> float:
    """
    计算滑道倍数。

    peak_mem_samples: 历史内存峰值样本
    oom_rate: 最近 OOM 频率
    default_slot_size: 当前槽位大小
    """
    if strategy == SlipwayStrategy.FIXED_1_5X:
        return config.slipway_multiplier

    elif strategy == SlipwayStrategy.PERCENTILE_P95:
        if len(peak_mem_samples) >= 10:
            p95 = np.percentile(peak_mem_samples, 95)
            ratio = p95 / default_slot_size if default_slot_size > 0 else 1.5
            # 至少 1.2x，最多 3.0x
            return max(1.2, min(3.0, ratio))
        return config.slipway_multiplier

    elif strategy == SlipwayStrategy.DYNAMIC_ELASTIC:
        base = config.slipway_multiplier
        # OOM 频率高 → 加大滑道
        if oom_rate > 0.05:     # >5% OOM
            base = min(3.0, base * 1.3)
        elif oom_rate > 0.02:   # 2-5% OOM
            base = min(3.0, base * 1.1)
        elif oom_rate < 0.005:  # <0.5% OOM → 可以收缩滑道
            base = max(1.2, base * 0.95)

        # P95 修正
        if len(peak_mem_samples) >= 10:
            p95_ratio = np.percentile(peak_mem_samples, 95) / default_slot_size if default_slot_size > 0 else 1.0
            base = max(base, p95_ratio * 0.8)  # 滑道至少覆盖 P95 的 80%

        return max(1.2, min(3.0, base))

    return 1.5


# ═══════════════════════════════════════════════════════════════
# 主控制器
# ═══════════════════════════════════════════════════════════════

@dataclass
class ControlDecision:
    """控制器输出"""
    n_batch: int
    hard_ceiling: int
    soft_ceiling: float
    slot_size_mb: float
    slipway_multiplier: float

    # 各因子的值（用于调试/可视化）
    beta: float = 1.0
    lambda_speed: float = 1.0
    sigma_volume: float = 1.0
    gamma_variance: float = 1.0
    theta_confidence: float = 1.0   # 新增：置信因子 θ
    merged_factor: float = 1.0
    alpha_mem_current: float = 0.5

    # 各维度上限
    C: float = 0.0
    M: float = 0.0
    I: float = 0.0
    N: float = 0.0

    # 冷启动阶段
    cold_start_phase: str = "stable"  # bootstrap / warmup / accumulate / stable


class AdaptiveResourceController:
    """
    自适应资源控制器主类（v0.3 修订版）。

    变更：
    - 新增置信因子 θ（基于回归模型的 r²）
    - 5 因子合并（β, λ, σ, γ, θ）
    - 集成线性回归模型
    - 四阶段冷启动协议

    接口：
    - update(stats) → ControlDecision
    - record_oom(time) → None
    - record_completion(task) → None（收集回归样本）
    """

    def __init__(self, hw_config: HardwareConfig, algo_config: AlgorithmConfig,
                 cold_start_config: Optional['ColdStartConfig'] = None,
                 regression_config: Optional['RegressionConfig'] = None):
        self.hw = hw_config
        self.algo = algo_config
        self.cs_cfg = cold_start_config or ColdStartConfig()
        self.reg_cfg = regression_config or RegressionConfig()

        # 因子平滑器
        self.smoother = FactorSmoother(algo_config.smoothing, algo_config)

        # OOM 反馈控制器
        self.oom_controller = OOMFeedbackController(algo_config, hw_config.alpha_mem)

        # 历史内存峰值样本
        self.peak_mem_samples: List[float] = []

        # 线性回归模型
        self.regression = RegressionModel()

        # 冷启动状态
        self.cold_start_phase = "bootstrap"
        self.completed_count = 0
        self.delta_ema = 1.0  # actual/compiler 的滑动平均

        self.last_decision: Optional[ControlDecision] = None

    def record_oom(self, sim_time: float):
        self.oom_controller.record_oom(sim_time)

    def record_peak_mem(self, peak_mb: float):
        self.peak_mem_samples.append(peak_mb)
        if len(self.peak_mem_samples) > 500:
            self.peak_mem_samples.pop(0)

    def record_completion(self, compiler_peak_mb: float, actual_peak_mb: float):
        """记录任务完成，收集回归样本并更新冷启动状态。"""
        self.completed_count += 1

        # 回归样本
        if compiler_peak_mb > 0 and self.reg_cfg.enabled:
            self.regression.add_sample(compiler_peak_mb, actual_peak_mb)
            # 攒够样本就训练一次，之后每 RETRAIN_INTERVAL 条再训练
            if self.regression.try_train():
                pass  # 训练成功，r² 已更新

        # 冷启动状态机
        if self.cold_start_phase == "bootstrap":
            self.delta_ema = actual_peak_mb / max(compiler_peak_mb, 0.1)
            if self.completed_count >= 1:
                self.cold_start_phase = "warmup"

        elif self.cold_start_phase == "warmup":
            ratio = actual_peak_mb / max(compiler_peak_mb, 0.1)
            self.delta_ema = 0.7 * self.delta_ema + 0.3 * ratio
            if self.completed_count >= self.cs_cfg.warmup_threshold:
                self.cold_start_phase = "accumulate"

        elif self.cold_start_phase == "accumulate":
            if self.completed_count >= self.reg_cfg.min_samples:
                self.cold_start_phase = "stable"
            # delta_ema 持续更新（在回归不可用时使用）

        # stable 阶段不需要额外操作

    def _get_effective_mem_per_task(self, compiler_peak_mb: float = 0,
                                     observed_avg_mem_mb: float = 0) -> float:
        """
        获取当前使用的每任务内存预估值。

        优先级：
        1. 回归模型可用 → 用回归预测（基于 compiler_peak_mb）
        2. 有实际观测 avg_mem_mb → 用观测值（冷启动阶段加安全系数）
        3. 啥都没有 → 退回硬件配置的默认值（16MB）
        """
        # 优先级 1：回归模型
        if self.reg_cfg.enabled and compiler_peak_mb > 0:
            if self.regression.r_squared >= self.reg_cfg.min_r_squared:
                predicted = self.regression.predict(compiler_peak_mb)
                # 冷启动额外安全系数
                if self.cold_start_phase == "bootstrap":
                    return predicted * 1.5
                elif self.cold_start_phase == "warmup":
                    return predicted * 1.3
                elif self.cold_start_phase == "accumulate":
                    return predicted * 1.1
                return predicted

        # 优先级 2：实际观测
        if observed_avg_mem_mb > 0:
            multiplier = 1.0
            if self.cold_start_phase == "bootstrap":
                multiplier = 1.5
            elif self.cold_start_phase == "warmup":
                multiplier = 1.3
            return observed_avg_mem_mb * multiplier

        # 优先级 3：退回默认值
        return self.hw.mem_per_task_mb

    def _compute_confidence_factor(self) -> float:
        """
        计算置信因子 θ。

        θ(r²) = 0.70 + 0.30 × r²

        行为：
          r² → 0: θ → 0.70（回归不可靠，收缩 N_batch）
          r² → 1: θ → 1.00（回归可靠，不收缩）
        """
        if not self.reg_cfg.enabled:
            return 1.0

        r2 = self.regression.r_squared

        if self.regression.sample_count < self.reg_cfg.min_samples:
            # 样本不足，保守
            progress = self.regression.sample_count / self.reg_cfg.min_samples
            return 0.70 + 0.30 * progress

        return 0.70 + 0.30 * max(0.0, min(1.0, r2))

    def update(self, stats: dict, sim_time: float,
               compiler_peak_mb: float = 0) -> ControlDecision:
        """
        主更新函数。

        stats 应包含：
        - pool_depth: int
        - avg_cpu_ms: float
        - avg_mem_mb: float
        - cv_cpu: float  (σ_t / μ_t)
        - mem_per_task_mb: float (预估值)
        """
        pool_depth = stats.get("pool_depth", 0)
        avg_cpu_ms = stats.get("avg_cpu_ms", 16.0)
        avg_mem_mb = stats.get("avg_mem_mb", 16.0)
        cv_cpu = stats.get("cv_cpu", 0.0)

        # 使用回归校正后的 MEM_per_task（传入实际观测 avg_mem_mb 作为 fallback）
        mem_per_task_mb = self._get_effective_mem_per_task(compiler_peak_mb, avg_mem_mb)

        # 1. 计算硬上限
        C = self.hw.effective_cpu / self.hw.cpu_per_task
        M = self.hw.effective_mem / mem_per_task_mb
        I_val = self.hw.effective_iops / self.hw.iops_per_task
        N_val = self.hw.effective_net / self.hw.net_per_task_mbps
        H = max(1, int(np.floor(min(C, M, I_val, N_val))))

        # 2. 更新 OOM 反馈 → 获得当前 alpha_mem
        alpha_mem = self.oom_controller.update(sim_time)

        # 如果有调整，重新计算 M
        if abs(alpha_mem - self.hw.alpha_mem) > 0.001:
            self.hw.alpha_mem = alpha_mem
            M = self.hw.effective_mem / mem_per_task_mb
            H = max(1, int(np.floor(min(C, M, I_val, N_val))))

        # 3. 计算各因子（5 因子：β, λ, σ, γ, θ）
        d = pool_depth / H if H > 0 else 0.0
        r = avg_mem_mb / mem_per_task_mb if mem_per_task_mb > 0 else 1.0

        beta_val = self.smoother.beta(d)
        lambda_val = self.smoother.lambda_speed(avg_cpu_ms)
        sigma_val = self.smoother.sigma_volume(r)
        gamma_val = self.smoother.gamma_variance(cv_cpu)
        theta_val = self._compute_confidence_factor()

        # 5 因子合并（加权几何平均，θ 权重最高）
        five_factors = [beta_val, lambda_val, sigma_val, gamma_val, theta_val]
        five_weights = [0.20, 0.20, 0.20, 0.15, 0.25]  # θ 权重 0.25

        merged = merge_factors(five_factors, MergeStrategy.WEIGHTED_GEOMEAN, five_weights)

        # 4. 软上限
        S = H * merged
        S = max(1.0, S)

        # 5. N_batch（冷启动阶段限制）
        N_batch_raw = max(1, int(np.floor(min(H, S))))
        if self.cold_start_phase == "bootstrap":
            N_batch = min(N_batch_raw, self.cs_cfg.bootstrap_n_batch)
        elif self.cold_start_phase == "warmup":
            N_batch = min(N_batch_raw, 2)
        else:
            N_batch = N_batch_raw

        # 6. 滑道倍数
        oom_count = self.oom_controller.recent_oom_count(sim_time)
        oom_rate = oom_count / max(1, stats.get("completed", 1))
        default_slot_size = self.hw.effective_mem * (1 - self.hw.safety_margin) / N_batch
        slipway_m = compute_slipway_multiplier(
            self.algo.slipway, self.algo,
            self.peak_mem_samples, oom_rate, default_slot_size
        )

        # 7. 槽位大小（使用回归校正后的每任务内存）
        slot_size = mem_per_task_mb * (1 + slipway_m * 0.1)  # 滑道额外预留

        decision = ControlDecision(
            n_batch=N_batch,
            hard_ceiling=H,
            soft_ceiling=S,
            slot_size_mb=slot_size,
            slipway_multiplier=slipway_m,
            beta=beta_val,
            lambda_speed=lambda_val,
            sigma_volume=sigma_val,
            gamma_variance=gamma_val,
            theta_confidence=theta_val,
            merged_factor=merged,
            alpha_mem_current=alpha_mem,
            C=C, M=M, I=I_val, N=N_val,
            cold_start_phase=self.cold_start_phase,
        )
        self.last_decision = decision
        return decision

    @staticmethod
    def generate_variants(hw_config: HardwareConfig) -> List[Tuple[str, 'AdaptiveResourceController']]:
        """
        生成所有算法变体组合，用于对比测试。
        返回 [("label", controller), ...]
        """
        # 只生成有代表性的组合（不是全部 4×3×3×3=108 种）
        variants = []

        # 基准：原文档方案
        variants.append(("Baseline (Disc+Mul+Hard+1.5x)", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.DISCRETE_SEGMENTED,
                merge=MergeStrategy.MULTIPLICATIVE,
                oom_feedback=OOMFeedback.HARD_MULTIPLY,
                slipway=SlipwayStrategy.FIXED_1_5X,
            )
        )))

        # 只改平滑
        variants.append(("Sigmoid Only", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.SIGMOID_SMOOTH,
                merge=MergeStrategy.MULTIPLICATIVE,
                oom_feedback=OOMFeedback.HARD_MULTIPLY,
                slipway=SlipwayStrategy.FIXED_1_5X,
            )
        )))

        # 只改合并
        variants.append(("MinBottleneck", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.DISCRETE_SEGMENTED,
                merge=MergeStrategy.MIN_BOTTLENECK,
                oom_feedback=OOMFeedback.HARD_MULTIPLY,
                slipway=SlipwayStrategy.FIXED_1_5X,
            )
        )))

        # 只改 OOM
        variants.append(("AIMD+Hysteresis", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.DISCRETE_SEGMENTED,
                merge=MergeStrategy.MULTIPLICATIVE,
                oom_feedback=OOMFeedback.AIMD_HYSTERESIS,
                slipway=SlipwayStrategy.FIXED_1_5X,
            )
        )))

        # 组合：Sigmoid + AIMD_Hysteresis
        variants.append(("Sig+AimdH", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.SIGMOID_SMOOTH,
                merge=MergeStrategy.MULTIPLICATIVE,
                oom_feedback=OOMFeedback.AIMD_HYSTERESIS,
                slipway=SlipwayStrategy.FIXED_1_5X,
            )
        )))

        # 组合：LinearInterp + WeightedGeoMean
        variants.append(("Lin+WGM+AimdH", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.LINEAR_INTERP,
                merge=MergeStrategy.WEIGHTED_GEOMEAN,
                oom_feedback=OOMFeedback.AIMD_HYSTERESIS,
                slipway=SlipwayStrategy.DYNAMIC_ELASTIC,
            )
        )))

        # 全优化组合
        variants.append(("FullOpt", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.SIGMOID_SMOOTH,
                merge=MergeStrategy.WEIGHTED_GEOMEAN,
                oom_feedback=OOMFeedback.AIMD_HYSTERESIS,
                slipway=SlipwayStrategy.DYNAMIC_ELASTIC,
            )
        )))

        # 全自适应
        variants.append(("FullAdaptive", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.ADAPTIVE_SMOOTH,
                merge=MergeStrategy.WEIGHTED_GEOMEAN,
                oom_feedback=OOMFeedback.AIMD_HYSTERESIS,
                slipway=SlipwayStrategy.PERCENTILE_P95,
            )
        )))

        # v0.3 全优化（含置信因子 + 回归 + 冷启动 + 5 因子）
        variants.append(("FullOpt_v0.3", AdaptiveResourceController(
            hw_config,
            AlgorithmConfig(
                smoothing=SmoothingMethod.SIGMOID_SMOOTH,
                merge=MergeStrategy.WEIGHTED_GEOMEAN,
                oom_feedback=OOMFeedback.AIMD_HYSTERESIS,
                slipway=SlipwayStrategy.DYNAMIC_ELASTIC,
            )
        )))

        return variants
