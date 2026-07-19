"""
指标采集与聚合
==============
实时采集仿真运行时的所有指标，支持时间序列和统计汇总。
"""

import dataclasses
from dataclasses import dataclass, field
from typing import List, Dict, Tuple
import numpy as np
from collections import deque


@dataclass
class TimeSeriesPoint:
    """单个时间点的快照"""
    sim_time: float

    # 批次管理
    n_batch: int
    hard_ceiling: int
    pool_depth: int
    pooled_total: int

    # 槽位
    slots_occupied: int
    slots_empty: int
    slots_dead: int
    slot_size_mb: float
    slipway_multiplier: float

    # 执行器
    tasks_running: int
    tasks_completed_cum: int
    ooms_cum: int

    # 资源
    cpu_util: float
    mem_util: float
    mem_avail_mb: float

    # 因子（来自控制器）
    beta: float = 1.0
    lambda_speed: float = 1.0
    sigma_volume: float = 1.0
    gamma_variance: float = 1.0
    merged_factor: float = 1.0
    alpha_mem: float = 0.5

    # 任务统计
    avg_task_latency_ms: float = 0.0
    p50_latency_ms: float = 0.0
    p99_latency_ms: float = 0.0

    # v0.3 新架构指标
    theta_confidence: float = 1.0
    cold_start_phase: str = "stable"
    balance_metric: float = 1.0
    prefetch_hit_rate: float = 0.0
    defrag_merges: int = 0
    regression_r2: float = 0.0


class MetricsCollector:
    """指标采集器"""

    def __init__(self):
        self.time_series: List[TimeSeriesPoint] = []

        # 延迟样本
        self._latency_samples: deque = deque(maxlen=10000)
        self._task_completion_times: deque = deque(maxlen=10000)

        # 滑动窗口 OOM 计数
        self._oom_events: deque = deque(maxlen=1000)

        # 累积计数
        self.total_tasks_completed: int = 0
        self.total_ooms: int = 0
        self.total_arrivals: int = 0

    def export_csv(self, filepath: str):
        """Export all time series data as CSV."""
        import csv as csv_mod
        import os
        os.makedirs(os.path.dirname(filepath), exist_ok=True)

        with open(filepath, 'w', newline='') as f:
            if not self.time_series:
                return
            first = self.time_series[0]
            fields = [f.name for f in dataclasses.fields(first)]
            writer = csv_mod.DictWriter(f, fieldnames=fields)
            writer.writeheader()
            for point in self.time_series:
                writer.writerow(dataclasses.asdict(point))

    def record(self, sim_time: float, pool, slot_manager, executor, controller, hardware):
        """记录一个时间点的快照"""
        slot_snap = slot_manager.snapshot()
        exec_snap = executor.snapshot()
        hw_snap = hardware.snapshot(sim_time)
        pool_stats = pool.get_stats()

        # 延迟统计
        latencies = list(self._latency_samples)[-100:]
        avg_latency = np.mean(latencies) if latencies else 0.0
        p50 = np.percentile(latencies, 50) if latencies else 0.0
        p99 = np.percentile(latencies, 99) if len(latencies) >= 100 else 0.0

        decision = controller.last_decision

        point = TimeSeriesPoint(
            sim_time=sim_time,
            n_batch=decision.n_batch if decision else 0,
            hard_ceiling=decision.hard_ceiling if decision else 0,
            pool_depth=pool_stats["pool_depth"],
            pooled_total=pool.total_tasks,
            slots_occupied=slot_snap["n_occupied"],
            slots_empty=slot_snap["n_empty"],
            slots_dead=slot_snap["n_dead"],
            slot_size_mb=slot_snap["slot_size_mb"],
            slipway_multiplier=decision.slipway_multiplier if decision else 1.5,
            tasks_running=exec_snap["running"],
            tasks_completed_cum=self.total_tasks_completed,
            ooms_cum=self.total_ooms,
            cpu_util=hw_snap.cpu_util,
            mem_util=hw_snap.mem_util,
            mem_avail_mb=hw_snap.mem_avail_mb,
            beta=decision.beta if decision else 1.0,
            lambda_speed=decision.lambda_speed if decision else 1.0,
            sigma_volume=decision.sigma_volume if decision else 1.0,
            gamma_variance=decision.gamma_variance if decision else 1.0,
            merged_factor=decision.merged_factor if decision else 1.0,
            alpha_mem=decision.alpha_mem_current if decision else 0.5,
            avg_task_latency_ms=avg_latency,
            p50_latency_ms=p50,
            p99_latency_ms=p99,
            # v0.3 新架构指标
            theta_confidence=decision.theta_confidence if (decision and hasattr(decision, 'theta_confidence')) else 1.0,
            cold_start_phase=getattr(controller, 'cold_start_phase', 'stable') if decision else 'stable',
            balance_metric=getattr(controller, 'balance_metric', 1.0),
            prefetch_hit_rate=getattr(controller, 'prefetch_hit_rate', 0.0),
            defrag_merges=getattr(controller, 'defrag_merges', 0),
            regression_r2=controller.regression.r_squared if hasattr(controller, 'regression') else 0.0,
        )
        self.time_series.append(point)

    def record_latency(self, latency_sec: float):
        self._latency_samples.append(latency_sec * 1000)  # 转毫秒

    def record_oom(self):
        self.total_ooms += 1

    def record_arrival(self):
        self.total_arrivals += 1

    def record_completion(self, task):
        self.total_tasks_completed += 1
        if task.finish_time > task.arrive_time:
            self.record_latency(task.latency_sec)

    def summary(self) -> dict:
        """生成汇总统计"""
        if not self.time_series:
            return {}

        ts = self.time_series

        # 吞吐量（任务/秒）
        duration = ts[-1].sim_time - ts[0].sim_time
        throughput = self.total_tasks_completed / duration if duration > 0 else 0

        # OOM 率
        oom_rate = self.total_ooms / max(1, self.total_tasks_completed)

        # 平均槽位利用率
        avg_utilization = np.mean([p.slots_occupied / max(1, p.n_batch) for p in ts if p.n_batch > 0])

        # 平均 N_batch
        avg_n_batch = np.mean([p.n_batch for p in ts if p.n_batch > 0])

        # 延迟
        latencies = list(self._latency_samples)
        if latencies:
            avg_latency = np.mean(latencies)
            p50 = np.percentile(latencies, 50)
            p95 = np.percentile(latencies, 95)
            p99 = np.percentile(latencies, 99)
        else:
            avg_latency = p50 = p95 = p99 = 0.0

        # 因子稳定性（因子波动标准差）
        beta_std = np.std([p.beta for p in ts]) if ts else 0.0
        merged_std = np.std([p.merged_factor for p in ts]) if ts else 0.0

        # v0.3 新汇总指标
        load_balance_metric = np.mean([p.balance_metric for p in ts]) if ts else 1.0
        avg_prefetch_hit_rate = np.mean([p.prefetch_hit_rate for p in ts]) if ts else 0.0
        regression_r2 = ts[-1].regression_r2 if ts else 0.0
        cold_start_phase = ts[-1].cold_start_phase if ts else "stable"
        defrag_merges_total = sum(p.defrag_merges for p in ts) if ts else 0

        return {
            "duration_sec": duration,
            "total_arrivals": self.total_arrivals,
            "total_completed": self.total_tasks_completed,
            "total_ooms": self.total_ooms,
            "throughput_per_sec": throughput,
            "oom_rate": oom_rate,
            "avg_utilization": avg_utilization,
            "avg_n_batch": avg_n_batch,
            "avg_latency_ms": avg_latency,
            "p50_latency_ms": p50,
            "p95_latency_ms": p95,
            "p99_latency_ms": p99,
            "beta_stability": beta_std,
            "merged_stability": merged_std,
            "load_balance_metric": load_balance_metric,
            "avg_prefetch_hit_rate": avg_prefetch_hit_rate,
            "regression_r2": regression_r2,
            "cold_start_phase": cold_start_phase,
            "defrag_merges_total": defrag_merges_total,
        }

    def get_arrays(self) -> Dict[str, np.ndarray]:
        """将时间序列转为 numpy 数组（用于可视化）"""
        if not self.time_series:
            return {}

        return {
            "time": np.array([p.sim_time for p in self.time_series]),
            "n_batch": np.array([p.n_batch for p in self.time_series]),
            "hard_ceiling": np.array([p.hard_ceiling for p in self.time_series]),
            "pool_depth": np.array([p.pool_depth for p in self.time_series]),
            "slots_occupied": np.array([p.slots_occupied for p in self.time_series]),
            "slots_empty": np.array([p.slots_empty for p in self.time_series]),
            "slots_dead": np.array([p.slots_dead for p in self.time_series]),
            "tasks_completed_cum": np.array([p.tasks_completed_cum for p in self.time_series]),
            "ooms_cum": np.array([p.ooms_cum for p in self.time_series]),
            "cpu_util": np.array([p.cpu_util for p in self.time_series]),
            "mem_util": np.array([p.mem_util for p in self.time_series]),
            "mem_avail_mb": np.array([p.mem_avail_mb for p in self.time_series]),
            "beta": np.array([p.beta for p in self.time_series]),
            "lambda_speed": np.array([p.lambda_speed for p in self.time_series]),
            "sigma_volume": np.array([p.sigma_volume for p in self.time_series]),
            "gamma_variance": np.array([p.gamma_variance for p in self.time_series]),
            "merged_factor": np.array([p.merged_factor for p in self.time_series]),
            "alpha_mem": np.array([p.alpha_mem for p in self.time_series]),
            "avg_latency_ms": np.array([p.avg_task_latency_ms for p in self.time_series]),
            "p50_latency_ms": np.array([p.p50_latency_ms for p in self.time_series]),
            "p99_latency_ms": np.array([p.p99_latency_ms for p in self.time_series]),
            "slot_size_mb": np.array([p.slot_size_mb for p in self.time_series]),
            "slipway_multiplier": np.array([p.slipway_multiplier for p in self.time_series]),
            # v0.3 新架构指标
            "theta_confidence": np.array([p.theta_confidence for p in self.time_series]),
            "cold_start_phase": [p.cold_start_phase for p in self.time_series],
            "balance_metric": np.array([p.balance_metric for p in self.time_series]),
            "prefetch_hit_rate": np.array([p.prefetch_hit_rate for p in self.time_series]),
            "defrag_merges": np.array([p.defrag_merges for p in self.time_series]),
            "regression_r2": np.array([p.regression_r2 for p in self.time_series]),
        }
