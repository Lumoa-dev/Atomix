"""
负载均衡器 + 预载调度器 + 死区合并管理器
==========================================
三个算法在仿真中的模拟实现。

负载均衡：加权最少任务 + 抗偏斜，2 线程轮询
预载调度：根据执行进度和网络延迟异步拉取 .atxe
死区合并：ROI 评估选择最优时机做 defrag
"""

from typing import List, Dict, Optional, Tuple, Callable
from dataclasses import dataclass, field
import numpy as np
from enum import Enum


# ═══════════════════════════════════════════════════════════════
# 负载均衡
# ═══════════════════════════════════════════════════════════════

@dataclass
class ExecutorLoad:
    """单个 Executor 的负载信息"""
    executor_id: int
    running_tasks: int = 0          # 当前运行的任务数
    remaining_instrs: float = 0.0  # 剩余指令估计
    pending_io: int = 0             # 阻塞 IO 数
    is_idle: bool = True

    @property
    def load_score(self) -> float:
        """综合负载评分"""
        if self.is_idle:
            return 0.0
        return self.remaining_instrs + self.pending_io * 1000  # IO 权重大


class LoadBalancer:
    """
    负载均衡器。

    策略：
    - N_batch = 2 → 轮询
    - N_batch ≥ 3 → 加权最少负载 + 抗偏斜
    """

    def __init__(self, n_executors: int = 4):
        self.n = n_executors
        self.executors = [ExecutorLoad(executor_id=i) for i in range(n_executors)]
        self._round_robin_counter = 0

    def resize(self, n: int):
        """调整 Executor 数量（N_batch 变化时）"""
        if n == self.n:
            return
        old = self.executors
        self.n = n
        self.executors = [ExecutorLoad(executor_id=i) for i in range(n)]
        # 保留旧负载信息
        for i in range(min(n, len(old))):
            self.executors[i] = old[i]

    def assign(self, task_id: int, estimated_instrs: float = 1000) -> int:
        """
        为任务分配一个 Executor。

        返回 executor_id。
        """
        if self.n == 2:
            return self._round_robin()

        # 优先 idle
        idle = [e for e in self.executors if e.is_idle]
        if idle:
            selected = idle[np.random.randint(len(idle))]
            self._update_load(selected.executor_id, estimated_instrs)
            return selected.executor_id

        # 选负载最低的
        loads = [(e.executor_id, e.load_score) for e in self.executors]
        min_load = min(l[1] for l in loads)

        # 如果多个负载接近（差距 < 10%），随机选一个（抗偏斜）
        threshold = max(min_load * 0.1, 1.0)
        candidates = [l[0] for l in loads if abs(l[1] - min_load) <= threshold]
        selected = candidates[np.random.randint(len(candidates))]

        self._update_load(selected, estimated_instrs)
        return selected

    def _round_robin(self) -> int:
        selected = self._round_robin_counter % self.n
        self._round_robin_counter += 1
        return selected

    def _update_load(self, executor_id: int, instrs: float):
        self.executors[executor_id].running_tasks += 1
        self.executors[executor_id].remaining_instrs += instrs
        self.executors[executor_id].is_idle = False

    def task_done(self, executor_id: int, instrs: float = 1000):
        """任务完成，更新负载"""
        if executor_id < len(self.executors):
            self.executors[executor_id].running_tasks = max(0, self.executors[executor_id].running_tasks - 1)
            self.executors[executor_id].remaining_instrs = max(0, self.executors[executor_id].remaining_instrs - instrs)
            if self.executors[executor_id].running_tasks == 0:
                self.executors[executor_id].is_idle = True

    def get_load_balance_metric(self) -> float:
        """
        负载均衡度量。
        返回变异系数 (CV) 的倒数，越高越均衡。
        1.0 = 完全均衡，0.0 = 极不均衡。
        """
        scores = [e.load_score for e in self.executors]
        if not scores or max(scores) == 0:
            return 1.0
        cv = np.std(scores) / np.mean(scores) if np.mean(scores) > 0 else 0.0
        return max(0.0, min(1.0, 1.0 - cv))

    def snapshot(self) -> dict:
        return {
            "n_executors": self.n,
            "loads": [e.load_score for e in self.executors],
            "balance_metric": self.get_load_balance_metric(),
        }


# ═══════════════════════════════════════════════════════════════
# 预载调度器
# ═══════════════════════════════════════════════════════════════

@dataclass
class PrefetchDecision:
    """预载决策"""
    task_id: int
    executor_id: int
    reason: str  # "network_latency" | "disk_load" | "sequential"


class PrefetchScheduler:
    """
    预载调度器。

    当 Executor 的剩余执行时间 > 网络延迟 × 1.5 时，
    提前从网络拉取下一个任务的 .atxe 到磁盘。
    """

    def __init__(self, network_rtt_ms: float = 50.0,
                 avg_instr_rate_mhz: float = 1.0,  # 1 instr/μs → 1M instr/s
                 max_depth: int = 3):
        self.network_rtt_ms = network_rtt_ms
        self.avg_instr_rate = avg_instr_rate_mhz  # instr per μs
        self.max_depth = max_depth

        # 预载命中统计
        self.hits: int = 0      # 预载了且用上了
        self.misses: int = 0    # 预载了但没用上（任务变更）
        self.not_attempted: int = 0  # 没预载（来不及）

    def evaluate(self, executor_id: int,
                 remaining_instrs: float,
                 next_task_id: Optional[int],
                 task_on_disk: Callable[[int], bool]) -> Optional[PrefetchDecision]:
        """
        评估是否需要为指定 Executor 预载下一任务。

        返回 None = 不需要预载。
        """
        if next_task_id is None:
            return None

        # 如果任务已在磁盘上，不需要预载
        if task_on_disk(next_task_id):
            return None

        remaining_time_us = remaining_instrs / self.avg_instr_rate
        remaining_time_ms = remaining_time_us / 1000.0

        if remaining_time_ms > self.network_rtt_ms * 1.5:
            return PrefetchDecision(
                task_id=next_task_id,
                executor_id=executor_id,
                reason="network_latency"
            )

        return None

    def record_hit(self):
        self.hits += 1

    def record_miss(self):
        self.misses += 1

    def record_not_attempted(self):
        self.not_attempted += 1

    @property
    def hit_rate(self) -> float:
        total = self.hits + self.misses + self.not_attempted
        return self.hits / total if total > 0 else 0.0

    def snapshot(self) -> dict:
        return {
            "hits": self.hits,
            "misses": self.misses,
            "not_attempted": self.not_attempted,
            "hit_rate": self.hit_rate,
        }


# ═══════════════════════════════════════════════════════════════
# 死区合并管理器
# ═══════════════════════════════════════════════════════════════

@dataclass
class DefragAction:
    """死区合并动作"""
    dead_slot_id: int
    target_slot_id: int
    merged_size_mb: float
    roi_ratio: float  # benefit / cost


class DefragManager:
    """
    死区合并管理器。

    评估碎片率和 ROI，在适当时机执行合并。
    """

    def __init__(self, frag_threshold: float = 0.30,
                 min_dead_slots: int = 2,
                 roi_min_ratio: float = 2.0):
        self.frag_threshold = frag_threshold
        self.min_dead_slots = min_dead_slots
        self.roi_min_ratio = roi_min_ratio

        # 统计
        self.merges_performed: int = 0
        self.total_recovered_mb: float = 0.0
        self.evaluations: int = 0

    def evaluate(self, slots: List[dict]) -> List[DefragAction]:
        """
        评估当前槽位布局，返回建议的合并动作列表。

        slots: [{"id": int, "status": str, "size_mb": float,
                 "task_id": Optional[int]}, ...]
        """
        self.evaluations += 1
        actions = []

        dead_slots = [s for s in slots if s["status"] == "DEAD"]
        free_slots = [s for s in slots if s["status"] == "EMPTY"]

        if len(dead_slots) < self.min_dead_slots:
            return actions

        total = sum(s["size_mb"] for s in slots)
        dead_total = sum(s["size_mb"] for s in dead_slots)
        free_total = sum(s["size_mb"] for s in free_slots)
        fragmentation = (dead_total + free_total) / total if total > 0 else 0.0

        if fragmentation < self.frag_threshold:
            return actions

        # 枚举可合并的死区
        for ds in dead_slots:
            idx = ds["id"]
            # 检查左邻居
            if idx > 0 and slots[idx - 1]["status"] in ("EMPTY", "DEAD"):
                cost = self._estimate_cost(ds)
                benefit = self._estimate_benefit(ds, slots[idx - 1])
                roi = benefit / cost if cost > 0 else float('inf')
                if roi >= self.roi_min_ratio:
                    actions.append(DefragAction(
                        dead_slot_id=idx,
                        target_slot_id=idx - 1,
                        merged_size_mb=ds["size_mb"] + slots[idx - 1]["size_mb"],
                        roi_ratio=roi,
                    ))

            # 检查右邻居
            if idx < len(slots) - 1 and slots[idx + 1]["status"] in ("EMPTY", "DEAD"):
                cost = self._estimate_cost(ds)
                benefit = self._estimate_benefit(ds, slots[idx + 1])
                roi = benefit / cost if cost > 0 else float('inf')
                if roi >= self.roi_min_ratio:
                    actions.append(DefragAction(
                        dead_slot_id=idx,
                        target_slot_id=idx + 1,
                        merged_size_mb=ds["size_mb"] + slots[idx + 1]["size_mb"],
                        roi_ratio=roi,
                    ))

        return actions

    def record_merge(self, recovered_mb: float):
        self.merges_performed += 1
        self.total_recovered_mb += recovered_mb

    def _estimate_cost(self, dead_slot: dict) -> float:
        """估计合并成本（迁移时间 + 调度开销）"""
        # 如果有任务在死区上，需要迁移
        has_task = dead_slot.get("task_id") is not None and dead_slot["task_id"] >= 0
        base = 1.0  # 基础调度开销
        if has_task:
            base += dead_slot["size_mb"] * 0.1  # 迁移成本随大小增加
        return base

    def _estimate_benefit(self, dead_slot: dict, neighbor: dict) -> float:
        """估计合并收益（回收空间 × 预期驻留时间）"""
        recovered = dead_slot["size_mb"] + neighbor["size_mb"]
        # 预期驻留时间估算（简化为固定值）
        expected_idle_time = 10.0  # 秒
        return recovered * expected_idle_time

    def snapshot(self) -> dict:
        return {
            "merges_performed": self.merges_performed,
            "total_recovered_mb": self.total_recovered_mb,
            "evaluations": self.evaluations,
        }
