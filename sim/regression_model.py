"""
线性回归内存预测模型
====================
根据编译预测的 peak_mb 和实际观测到的 actual_peak_mb 做线性回归，
校准内存分配精度。

公式: actual_peak = α × compiler_peak + β

使用条件: 样本数 ≥ MIN_SAMPLES 且 r² ≥ MIN_R_SQUARED
否则退回保守估计 (compiler_peak × SAFETY_MULTIPLIER)
"""

import numpy as np
from typing import List, Tuple, Optional
from dataclasses import dataclass, field


# 默认阈值
MIN_SAMPLES = 50
MIN_R_SQUARED = 0.6
SAFETY_MULTIPLIER = 1.5
RETRAIN_INTERVAL = 200


@dataclass
class RegressionModel:
    """
    线性回归模型，拟合 actual_peak = α × compiler_peak + β

    持久化字段（可序列化为 JSON）:
    """
    alpha: float = 1.0          # 斜率
    beta: float = 0.0           # 截距
    r_squared: float = 0.0      # 拟合优度
    sample_count: int = 0       # 训练样本数
    last_trained_at: int = 0    # 上次训练时的样本数

    # 运行时滑动均值（少量样本时用）
    delta_ema: float = 1.0      # actual/compiler 的指数滑动平均

    # ── 训练样本缓存 ──
    # 存最近的 (compiler_peak, actual_peak) 对，上限 2000 条
    _samples: list = field(default_factory=list)

    def predict(self, compiler_peak_mb: float) -> float:
        """
        预测实际峰值内存。

        回归模型不可用（样本不足/r²太低）时退回 conservative 估计。
        """
        if self.sample_count < MIN_SAMPLES or self.r_squared < MIN_R_SQUARED:
            # 冷启动阶段：用 EMA 修正
            return compiler_peak_mb * max(self.delta_ema, SAFETY_MULTIPLIER)

        predicted = self.alpha * compiler_peak_mb + self.beta

        # 安全钳制：不能低于编译预测的一半，不能高于 3 倍
        lower = compiler_peak_mb * 0.5
        upper = compiler_peak_mb * 3.0
        return max(lower, min(upper, predicted))

    def add_sample(self, compiler_peak_mb: float, actual_peak_mb: float):
        """添加一个训练样本并更新 EMA。"""
        ratio = actual_peak_mb / max(compiler_peak_mb, 0.1)
        # EMA 更新 (α = 0.1)
        self.delta_ema = 0.9 * self.delta_ema + 0.1 * ratio
        self.sample_count += 1

        # 缓存样本用于后续训练（最多 2000 条）
        self._samples.append((compiler_peak_mb, actual_peak_mb))
        if len(self._samples) > 2000:
            self._samples.pop(0)

    def should_retrain(self) -> bool:
        """是否需要重新训练。"""
        return (self.sample_count >= MIN_SAMPLES
                and self.sample_count - self.last_trained_at >= RETRAIN_INTERVAL
                and len(self._samples) >= MIN_SAMPLES)

    def try_train(self):
        """
        如果条件满足，用缓存的样本执行 OLS 训练。
        返回 True 表示执行了训练。
        """
        if not self.should_retrain():
            return False
        self.train(self._samples)
        return True

    def train(self, samples: List[Tuple[float, float]]):
        """
        用 OLS 训练回归模型。

        samples: [(compiler_peak, actual_peak), ...]
        """
        if len(samples) < MIN_SAMPLES:
            return

        xs = np.array([s[0] for s in samples], dtype=np.float64)
        ys = np.array([s[1] for s in samples], dtype=np.float64)

        n = len(xs)
        mean_x = np.mean(xs)
        mean_y = np.mean(ys)

        num = np.sum((xs - mean_x) * (ys - mean_y))
        den = np.sum((xs - mean_x) ** 2)

        if abs(den) < 1e-10:
            return  # 除零保护

        self.alpha = num / den
        self.beta = mean_y - self.alpha * mean_x

        # 计算 r²
        ss_res = np.sum((ys - (self.alpha * xs + self.beta)) ** 2)
        ss_tot = np.sum((ys - mean_y) ** 2)
        self.r_squared = 1.0 - ss_res / ss_tot if ss_tot > 0 else 0.0

        self.last_trained_at = self.sample_count

    def to_dict(self) -> dict:
        return {
            "alpha": self.alpha,
            "beta": self.beta,
            "r_squared": self.r_squared,
            "sample_count": self.sample_count,
            "delta_ema": self.delta_ema,
        }

    @classmethod
    def from_dict(cls, d: dict) -> 'RegressionModel':
        return cls(
            alpha=d.get("alpha", 1.0),
            beta=d.get("beta", 0.0),
            r_squared=d.get("r_squared", 0.0),
            sample_count=d.get("sample_count", 0),
            delta_ema=d.get("delta_ema", 1.0),
        )
