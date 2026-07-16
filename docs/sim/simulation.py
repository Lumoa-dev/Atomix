"""
主仿真循环
==========
离散时间步进仿真，模拟完整的任务生命周期：
任务到达 → 任务池 → 批次调度 → 槽位分配 → 执行 → 完成回收 → 指标记录
"""

import time as time_mod
from typing import List, Dict, Optional, Tuple
import numpy as np

from sim.config import (
    HardwareConfig, AlgorithmConfig, ArrivalConfig, TaskProfile,
    SimulationConfig
)
from sim.hardware_model import HardwareModel
from sim.task_generator import TaskGenerator, Task, TaskPool
from sim.slot_manager import SlotManager
from sim.executor import ExecutorSync
from sim.adaptive_controller import AdaptiveResourceController, ControlDecision
from sim.metrics import MetricsCollector


class Simulation:
    """
    离散时间步进仿真引擎。

    每个时间步：
    1. 生成新任务 → 放入任务池
    2. 自适应控制器计算 N_batch
    3. 槽位管理器布局
    4. 填充空槽位（从任务池拉任务）
    5. 执行一个时间步（所有运行中的任务前进 dt）
    6. 完成的任务释放槽位和资源
    7. 记录指标
    """

    def __init__(self, hw_config: HardwareConfig, algo_config: AlgorithmConfig,
                 task_profiles: List[TaskProfile], arrival: ArrivalConfig,
                 sim_config: SimulationConfig):
        self.hw_config = hw_config
        self.algo_config = algo_config
        self.task_profiles = task_profiles
        self.arrival = arrival
        self.sim_config = sim_config

        # 组件
        self.hardware: Optional[HardwareModel] = None
        self.task_generator: Optional[TaskGenerator] = None
        self.task_pool: Optional[TaskPool] = None
        self.slot_manager: Optional[SlotManager] = None
        self.executor: Optional[ExecutorSync] = None
        self.controller: Optional[AdaptiveResourceController] = None
        self.metrics: Optional[MetricsCollector] = None

        # 仿真状态
        self.sim_time: float = 0.0
        self._running: bool = False

    def setup(self):
        """初始化所有组件"""
        # 硬件
        self.hardware = HardwareModel(self.hw_config)

        # 任务生成器
        self.task_generator = TaskGenerator(self.task_profiles, self.arrival,
                                            seed=self.sim_config.seed)

        # 任务池
        self.task_pool = TaskPool()

        # 槽位管理器
        total_task_mem = self.hw_config.effective_mem * (1 - self.hw_config.safety_margin)
        self.slot_manager = SlotManager(total_task_mem, self.hw_config.safety_margin)

        # 自适应控制器
        self.controller = AdaptiveResourceController(self.hw_config, self.algo_config)

        # 执行器
        self.executor = ExecutorSync(self.hardware, self.slot_manager)
        self.executor.task_pool = self.task_pool

        # 回调
        self.executor.on_task_complete = self._on_task_complete
        self.executor.on_task_oom = self._on_task_oom

        # 指标
        self.metrics = MetricsCollector()

        # 初始槽位布局
        initial_stats = self.task_pool.get_stats()
        decision = self.controller.update(initial_stats, 0.0)
        self.slot_manager.layout(decision.n_batch, decision.slipway_multiplier)

    def run(self) -> MetricsCollector:
        """运行仿真"""
        self.setup()
        self._running = True

        dt_sec = self.sim_config.time_step_sec
        dt_ms = dt_sec * 1000.0
        total_steps = int(self.sim_config.duration_sec / dt_sec)
        record_interval = max(1, int(0.1 / dt_sec))  # 每 0.1s 记录一次

        last_progress_time = time_mod.time()

        for step in range(total_steps):
            if not self._running:
                break

            self.sim_time = step * dt_sec

            # ── 1. 生成新任务 ──
            new_tasks = self.task_generator.generate_until(self.sim_time)
            if new_tasks:
                self.task_pool.push_batch(new_tasks)
                for _ in new_tasks:
                    self.metrics.record_arrival()

            # ── 2. 自适应控制器更新 ──
            pool_stats = self.task_pool.get_stats()
            decision = self.controller.update(pool_stats, self.sim_time)

            # ── 3. 槽位布局调整 ──
            if decision.n_batch != len(self.slot_manager.slots):
                self.slot_manager.layout(decision.n_batch, decision.slipway_multiplier)

            # ── 4. 填充空槽位 ──
            self._fill_empty_slots()

            # ── 5. 执行一个时间步 ──
            self.executor.set_sim_time(self.sim_time)
            completed = self.executor.step(dt_ms, self.sim_time)

            # ── 6. 完成的任务 → 释放 + 记录
            for task in completed:
                self.task_pool.complete(task)
                self.metrics.record_completion(task)
                # 反馈峰值内存
                self.controller.record_peak_mem(task.peak_mem_mb)

            # ── 7. 记录指标 ──
            if step % record_interval == 0:
                self.metrics.record(
                    self.sim_time, self.task_pool, self.slot_manager,
                    self.executor, self.controller, self.hardware
                )

            # ── 8. 进度输出 ──
            if self.sim_config.verbose:
                now = time_mod.time()
                if now - last_progress_time >= self.sim_config.progress_interval_sec:
                    progress = (step / total_steps) * 100
                    print(f"  [{self.algo_config.label()}] "
                          f"t={self.sim_time:.1f}s ({progress:.0f}%) "
                          f"pool={self.task_pool.pool_depth} "
                          f"N_batch={decision.n_batch} "
                          f"done={self.metrics.total_tasks_completed} "
                          f"OOM={self.metrics.total_ooms}")
                    last_progress_time = now

        # 最终记录
        self.metrics.record(
            self.sim_time, self.task_pool, self.slot_manager,
            self.executor, self.controller, self.hardware
        )

        return self.metrics

    def _fill_empty_slots(self):
        """从任务池拉取任务填充空槽位"""
        max_to_fill = self.slot_manager.empty_slots

        for _ in range(max_to_fill):
            task = self.task_pool.pop()
            if task is None:
                break

            slot = self.slot_manager.allocate(task.task_id, task.mem_mb)
            if slot is None:
                self.task_pool.requeue(task)
                break

            self.executor.dispatch(task, slot)

    def _on_task_complete(self, task: Task):
        """任务完成回调"""
        pass

    def _on_task_oom(self, task: Task, slot):
        """任务 OOM 回调"""
        self.metrics.record_oom()
        self.controller.record_oom(self.sim_time)

    def stop(self):
        self._running = False


def run_single_simulation(hw: HardwareConfig, algo: AlgorithmConfig,
                          profiles: List[TaskProfile], arrival: ArrivalConfig,
                          sim_cfg: SimulationConfig) -> Tuple[str, MetricsCollector]:
    """运行单次仿真，返回 (算法标签, 指标)"""
    sim = Simulation(hw, algo, profiles, arrival, sim_cfg)
    metrics = sim.run()
    return algo.label(), metrics


def run_scenario(scenario_name: str,
                 hw: HardwareConfig,
                 algo_variants: List[Tuple[str, AdaptiveResourceController]],
                 profiles: List[TaskProfile],
                 arrival: ArrivalConfig,
                 sim_cfg: SimulationConfig) -> Dict[str, MetricsCollector]:
    """
    运行一个场景的所有算法变体。

    返回 {label: MetricsCollector}
    """
    results = {}

    print(f"\n{'='*60}")
    print(f"Scenario: {scenario_name}")
    print(f"Duration: {sim_cfg.duration_sec}s, Arrival rate: {arrival.rate_per_sec}/s")
    print(f"Hardware: {hw.cpu_cores}C/{hw.mem_free_mb:.0f}MB")
    print(f"Variants: {len(algo_variants)}")
    print(f"{'='*60}")

    for label, controller in algo_variants:
        print(f"\n--- {label} ---")
        t_start = time_mod.time()

        sim = Simulation(hw, controller.algo, profiles, arrival, sim_cfg)
        # 直接注入已有的 controller（避免重复创建）
        sim.controller = controller
        sim.setup()
        # 重新关联 controller 到 simulation
        sim.controller = controller
        sim.executor.on_task_complete = sim._on_task_complete
        sim.executor.on_task_oom = sim._on_task_oom

        metrics = sim.run()
        results[label] = metrics

        t_elapsed = time_mod.time() - t_start
        summary = metrics.summary()
        print(f"  Done in {t_elapsed:.1f}s | "
              f"Throughput: {summary.get('throughput_per_sec', 0):.1f}/s | "
              f"OOM rate: {summary.get('oom_rate', 0)*100:.1f}% | "
              f"Avg latency: {summary.get('avg_latency_ms', 0):.0f}ms | "
              f"Avg N_batch: {summary.get('avg_n_batch', 0):.1f}")

    return results
