"""
主仿真循环
==========
离散时间步进仿真，模拟完整的任务生命周期：
任务到达 → 任务池 → 批次调度 → 槽位分配 → 执行 → 完成回收 → 指标记录

v0.3 新增算法：
- 虚→实物理内存分配
- 负载均衡 Executor 分发
- 预载调度
- 死区合并 defrag
- 编译器内存预测 (compiler_peak_mb)
- 回归模型反馈
"""

import time as time_mod
from typing import List, Dict, Optional, Tuple
import numpy as np

from sim.config import (
    HardwareConfig, AlgorithmConfig, ArrivalConfig, TaskProfile,
    SimulationConfig, LoadBalanceConfig, PrefetchConfig, DefragConfig,
    RegressionConfig, ColdStartConfig,
)
from sim.hardware_model import HardwareModel
from sim.task_generator import TaskGenerator, Task, TaskPool
from sim.slot_manager import SlotManager
from sim.executor import ExecutorSync
from sim.adaptive_controller import AdaptiveResourceController, ControlDecision
from sim.metrics import MetricsCollector
from sim.load_balancer import LoadBalancer, PrefetchScheduler, DefragManager


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
                 sim_config: SimulationConfig,
                 load_balance_config: Optional[LoadBalanceConfig] = None,
                 prefetch_config: Optional[PrefetchConfig] = None,
                 defrag_config: Optional[DefragConfig] = None,
                 regression_config: Optional[RegressionConfig] = None,
                 cold_start_config: Optional[ColdStartConfig] = None):
        self.hw_config = hw_config
        self.algo_config = algo_config
        self.task_profiles = task_profiles
        self.arrival = arrival
        self.sim_config = sim_config

        # v0.3 算法配置（可选，默认启用）
        self.load_balance_config = load_balance_config or LoadBalanceConfig()
        self.prefetch_config = prefetch_config or PrefetchConfig()
        self.defrag_config = defrag_config or DefragConfig()
        self.regression_config = regression_config or RegressionConfig()
        self.cold_start_config = cold_start_config or ColdStartConfig()

        # 组件
        self.hardware: Optional[HardwareModel] = None
        self.task_generator: Optional[TaskGenerator] = None
        self.task_pool: Optional[TaskPool] = None
        self.slot_manager: Optional[SlotManager] = None
        self.executor: Optional[ExecutorSync] = None
        self.controller: Optional[AdaptiveResourceController] = None
        self.metrics: Optional[MetricsCollector] = None

        # v0.3 新增组件
        self.load_balancer: Optional[LoadBalancer] = None
        self.prefetch_scheduler: Optional[PrefetchScheduler] = None
        self.defrag_manager: Optional[DefragManager] = None

        # 预载追踪
        self._prefetched_ids: set = set()

        # 大任务积压（因槽位不够大无法分配的任务）
        self._large_task_backlog: list = []

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

        # ── v0.3 新组件 ──

        # 负载均衡器（初始 N_batch = 4，后续 resize）
        n_batch_init = 4
        self.load_balancer = LoadBalancer(n_executors=n_batch_init)
        self.executor.load_balancer = self.load_balancer

        # 预载调度器
        self.prefetch_scheduler = PrefetchScheduler(
            network_rtt_ms=self.prefetch_config.network_rtt_ms,
            max_depth=self.prefetch_config.max_depth,
        )

        # 死区合并管理器
        self.defrag_manager = DefragManager(
            frag_threshold=self.defrag_config.frag_threshold,
            min_dead_slots=self.defrag_config.min_dead_slots,
            roi_min_ratio=self.defrag_config.roi_min_ratio,
        )

        # 指标
        self.metrics = MetricsCollector()

        # 初始槽位布局
        initial_stats = self.task_pool.get_stats()
        decision = self.controller.update(initial_stats, 0.0)
        self.slot_manager.layout(decision.n_batch, decision.slipway_multiplier)
        # 同步负载均衡器到初始 N_batch
        if self.load_balance_config.enabled:
            self.load_balancer.resize(decision.n_batch)

    def run(self) -> MetricsCollector:
        """运行仿真（v0.3 修订版：集成负载均衡、预载、defrag、回归反馈）"""
        self.setup()
        self._running = True

        dt_sec = self.sim_config.time_step_sec
        dt_ms = dt_sec * 1000.0
        total_steps = int(self.sim_config.duration_sec / dt_sec)
        record_interval = max(1, int(0.2 / dt_sec))  # 每 0.2s 记录一次（原 0.1s，数据量减半）

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

            # ── 2. 自适应控制器更新（v0.3：传递 compiler_peak_mb）──
            pool_stats = self.task_pool.get_stats()
            avg_compiler_peak = self._get_avg_compiler_peak()
            decision = self.controller.update(pool_stats, self.sim_time,
                                              compiler_peak_mb=avg_compiler_peak)

            # ── 3. 调整负载均衡器大小（N_batch 变化时）──
            if self.load_balance_config.enabled:
                if decision.n_batch != self.load_balancer.n:
                    self.load_balancer.resize(decision.n_batch)

            # ── 4. 槽位布局调整 ──
            if decision.n_batch != len(self.slot_manager.slots):
                self.slot_manager.layout(decision.n_batch, decision.slipway_multiplier)

            # ── 5. 填充空槽位（v0.3：使用负载均衡分配 executor）──
            self._fill_empty_slots()

            # ── 6. 执行一个时间步 ──
            self.executor.set_sim_time(self.sim_time)
            completed = self.executor.step(dt_ms, self.sim_time)

            # ── 7. 完成的任务 → 释放 + 记录 + 回归样本 ──
            for task in completed:
                self.task_pool.complete(task)
                self.metrics.record_completion(task)
                self.controller.record_peak_mem(task.peak_mem_mb)
                # v0.3：收集回归样本（compiler_peak_mb → actual_peak_mb）
                if task.compiler_peak_mb > 0:
                    self.controller.record_completion(
                        task.compiler_peak_mb, task.peak_mem_mb
                    )

            # ── 8. 记录指标 ──
            if step % record_interval == 0:
                self.metrics.record(
                    self.sim_time, self.task_pool, self.slot_manager,
                    self.executor, self.controller, self.hardware
                )

            # ── 9. 死区合并（周期性执行）──
            if self.defrag_config.enabled and step % 100 == 0 and step > 0:
                self._defrag_step()

            # ── 10. 预载检查 ──
            if self.prefetch_config.enabled:
                self._prefetch_step()

            # ── 11. 进度输出 ──
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
        """从任务池拉取任务填充空槽位（v0.3：负载均衡分发 + compiler_peak_mb 分配）"""
        max_to_fill = self.slot_manager.empty_slots

        for _ in range(max_to_fill):
            if not self.task_pool._queue:
                break

            # peek 队首任务，不弹出（分配成功后再 pop）
            tid = self.task_pool._queue[0]
            task = self.task_pool._tasks.get(tid)
            if task is None:
                self.task_pool._queue.pop(0)
                continue

            slot = self.slot_manager.allocate(task.task_id, task.mem_mb,
                                               compiler_peak_mb=task.compiler_peak_mb)
            if slot is None:
                # 塞不下 → 移到队尾，尝试下一个
                self.task_pool._queue.pop(0)
                self.task_pool._queue.append(tid)
                continue

            # 分配成功 → 正式弹出并分发
            self.task_pool._queue.pop(0)
            task.status = "RUNNING"

            # v0.3：使用负载均衡器选择 Executor
            if self.load_balance_config.enabled and self.load_balancer is not None:
                executor_id = self.load_balancer.assign(task.task_id)
            else:
                executor_id = 0

            self.executor.dispatch(task, slot, executor_id=executor_id)

    # ── v0.3 新增方法 ──────────────────────────

    def _get_avg_compiler_peak(self) -> float:
        """获取任务池中排队任务的平均 compiler_peak_mb"""
        values = []
        # 取队列中前 N 个任务（不阻塞）
        sample_ids = self.task_pool._queue[:50]
        for tid in sample_ids:
            task = self.task_pool._tasks.get(tid)
            if task and task.compiler_peak_mb > 0:
                values.append(task.compiler_peak_mb)
        return float(np.mean(values)) if values else 0.0

    def _defrag_step(self):
        """运行一次死区合并评估并执行"""
        if self.defrag_manager is None or self.slot_manager is None:
            return
        self.slot_manager.defrag_step(self.defrag_manager)

    def _prefetch_step(self):
        """
        预载检查：为每个运行中的 Executor 评估是否需要预载下一任务。

        在仿真中预载是瞬时的（一旦决策即认为已就绪）。
        """
        if self.prefetch_scheduler is None or self.executor is None:
            return

        for task_id, (task, slot, remaining_ms, executor_id) in self.executor.running.items():
            # 取该 executor 的下一个任务
            next_ids = self.task_pool.peek_next(1)
            next_task_id = next_ids[0] if next_ids else None

            decision = self.prefetch_scheduler.evaluate(
                executor_id,
                remaining_instrs=remaining_ms * 1000,  # ms → instrs (1 instr/μs)
                next_task_id=next_task_id,
                task_on_disk=lambda tid: tid in self._prefetched_ids,
            )
            if decision is not None:
                # 仿真中预载是瞬时的
                self._prefetched_ids.add(decision.task_id)
                self.prefetch_scheduler.record_hit()

    def _try_dispatch_large_tasks(self):
        """
        尝试从大任务积压中派发任务。

        当槽位布局发生变化（如 n_batch 增大）后，之前因 size 不够
        而无法分配的任务可能现在可以分配了。
        """
        if not self._large_task_backlog:
            return

        remaining = []
        for tid in self._large_task_backlog:
            task = self.task_pool._tasks.get(tid)
            if task is None or task.status != "QUEUED":
                continue
            slot = self.slot_manager.allocate(task.task_id, task.mem_mb,
                                               compiler_peak_mb=task.compiler_peak_mb)
            if slot is not None:
                if self.load_balance_config.enabled and self.load_balancer is not None:
                    executor_id = self.load_balancer.assign(task.task_id)
                else:
                    executor_id = 0
                self.executor.dispatch(task, slot, executor_id=executor_id)
            else:
                remaining.append(tid)
        self._large_task_backlog = remaining

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
