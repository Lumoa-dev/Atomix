"""
执行器模型
==========
模拟多线程槽位执行：N_batch 个 worker，每个绑定一个槽位。
任务执行期间消耗 CPU/IO/NET 资源，完成后释放。

使用 Python threading 模拟并发执行。
"""

import threading
import time
import queue
from typing import Dict, List, Optional, Callable
from dataclasses import dataclass, field
import numpy as np

from sim.config import HardwareConfig
from sim.hardware_model import HardwareModel
from sim.task_generator import Task, TaskPool
from sim.slot_manager import SlotManager, Slot, SlotStatus


@dataclass
class ExecutorStats:
    """执行器统计"""
    tasks_completed: int = 0
    tasks_oom: int = 0
    tasks_blocked: int = 0
    total_context_switches: int = 0
    total_instrs_executed: int = 0


class Executor:
    """多线程执行器 —— 模拟 N_batch 个 worker 线程"""

    def __init__(self, hardware: HardwareModel, slot_manager: SlotManager):
        self.hardware = hardware
        self.slot_manager = slot_manager

        self.stats = ExecutorStats()

        # 任务池引用（由 Simulation 注入）
        self.task_pool: Optional[TaskPool] = None

        # Worker 线程
        self.workers: List[threading.Thread] = []
        self._stop_event = threading.Event()
        self._task_queues: List[queue.Queue] = []  # 每个 worker 一个任务队列

        # 回调
        self.on_task_complete: Optional[Callable] = None
        self.on_task_oom: Optional[Callable] = None

        # 当前仿真时间
        self.sim_time: float = 0.0

        # 每指令执行时间 (ms) —— 用于时间片模拟
        self.instr_time_ms: float = 0.001  # 1μs per instruction at 1M instr/s

    def start(self, n_workers: int):
        """启动 worker 线程池"""
        self._stop_event.clear()
        self._task_queues = [queue.Queue() for _ in range(n_workers)]

        for i in range(n_workers):
            t = threading.Thread(target=self._worker_loop, args=(i,), daemon=True)
            self.workers.append(t)
            t.start()

    def stop(self):
        """停止所有 worker"""
        self._stop_event.set()
        for q in self._task_queues:
            q.put(None)  # 发送停止信号
        for t in self.workers:
            t.join(timeout=1.0)
        self.workers.clear()
        self._task_queues.clear()

    def dispatch(self, task: Task, slot: Slot):
        """将任务分配到指定 worker 的槽位"""
        task.slot_id = slot.slot_id
        task.mem_addr = slot.start_addr
        task.mem_size = slot.size_mb
        task.status = "RUNNING"
        task.dispatched = True

        # 分配硬件资源
        cpu_alloc = self.hardware.config.cpu_per_task
        success = self.hardware.allocate(
            cpu=cpu_alloc,
            mem_mb=task.mem_mb,
            iops=task.iops,
            net_mbps=task.net_mbps,
        )

        if not success:
            # 资源不足，任务挂起
            task.status = "SUSPENDED"
            return

        # 放入 worker 队列（按 slot_id 取模分配 worker）
        worker_idx = slot.slot_id % len(self._task_queues)
        self._task_queues[worker_idx].put((task, slot))

    def set_sim_time(self, t: float):
        self.sim_time = t

    def _worker_loop(self, worker_id: int):
        """Worker 线程主循环"""
        q = self._task_queues[worker_id]

        while not self._stop_event.is_set():
            try:
                item = q.get(timeout=0.1)
            except queue.Empty:
                continue

            if item is None:  # 停止信号
                break

            task, slot = item
            self._execute_task(task, slot)
            q.task_done()

    def _execute_task(self, task: Task, slot: Slot):
        """执行单个任务"""
        if task.status == "SUSPENDED":
            return

        task.start_time = self.sim_time

        # 模拟 CPU 执行时间
        cpu_sec = task.cpu_ms / 1000.0

        # 模拟时间片
        quantum_sec = self.hardware.config.quantum_instrs * self.instr_time_ms / 1000.0
        remaining = cpu_sec

        context_switches = 0
        while remaining > 0 and not self._stop_event.is_set():
            chunk = min(remaining, quantum_sec)
            time.sleep(chunk * 0.001)  # 压缩时间（仿真加速）
            remaining -= chunk
            context_switches += 1
            self.stats.total_instrs_executed += self.hardware.config.quantum_instrs

            # 模拟期间检测 OOM
            if task.peak_mem_mb > slot.size_mb * 1.2:  # 超过槽位 120% 触发扩容
                # 尝试俄罗斯方块扩容
                if self.slot_manager.expand_task(slot, additional_mb=task.peak_mem_mb - slot.size_mb):
                    task.oom_count += 1
                    self.stats.tasks_oom += 1
                    if self.on_task_oom:
                        self.on_task_oom(task, slot)
                else:
                    # 滑道满，任务挂起
                    task.status = "SUSPENDED"
                    self.stats.tasks_blocked += 1
                    return

        # 更新峰值内存
        if task.peak_mem_mb == 0:
            task.peak_mem_mb = task.mem_mb

        # 任务完成
        task.finish_time = self.sim_time + cpu_sec
        task.status = "DONE"
        self.stats.tasks_completed += 1
        self.stats.total_context_switches += context_switches

        # 释放硬件资源
        self.hardware.release(
            cpu=self.hardware.config.cpu_per_task,
            mem_mb=task.mem_mb,
            iops=task.iops,
            net_mbps=task.net_mbps,
        )

        # 释放槽位
        if slot.expanded_to_addr >= 0:
            self.slot_manager.shrink_expansion(slot)
        self.slot_manager.release(slot)

        # 回调
        if self.on_task_complete:
            self.on_task_complete(task)


class ExecutorSync:
    """
    同步执行器（单步仿真用）
    ========================
    用时间步进代替真实线程，更适合离散事件仿真。
    每步执行一定量的指令，模拟时间片调度。
    """

    def __init__(self, hardware: HardwareModel, slot_manager: SlotManager):
        self.hardware = hardware
        self.slot_manager = slot_manager

        # 运行中的任务 {task_id: (Task, Slot, remaining_cpu_ms)}
        self.running: Dict[int, tuple] = {}

        self.stats = ExecutorStats()
        self.task_pool: Optional[TaskPool] = None

        # 回调
        self.on_task_complete: Optional[Callable] = None
        self.on_task_oom: Optional[Callable] = None

    def dispatch(self, task: Task, slot: Slot):
        """分配任务到槽位"""
        task.slot_id = slot.slot_id
        task.mem_addr = slot.start_addr
        task.mem_size = slot.size_mb
        task.status = "RUNNING"
        task.dispatched = True
        task.start_time = task.start_time or 0.0  # 保留已记录的开始时间

        # 分配硬件资源
        self.hardware.allocate(
            cpu=self.hardware.config.cpu_per_task,
            mem_mb=task.mem_mb,
            iops=task.iops,
            net_mbps=task.net_mbps,
        )

        self.running[task.task_id] = (task, slot, task.cpu_ms)

    def set_sim_time(self, t: float):
        """更新仿真时间（兼容接口）"""
        pass

    def step(self, dt_ms: float, sim_time: float) -> List[Task]:
        """
        执行一个时间步。

        dt_ms: 时间步长（毫秒）
        sim_time: 当前仿真时间（秒）

        返回：本步完成的任务列表
        """
        completed = []
        oom_tasks = []

        for task_id in list(self.running.keys()):
            task, slot, remaining_ms = self.running[task_id]

            if task.status == "SUSPENDED":
                continue

            # 执行 dt_ms 毫秒
            executed = min(remaining_ms, dt_ms)
            remaining_ms -= executed
            self.stats.total_instrs_executed += int(executed / self._instr_time_ms())

            # 模拟内存使用波动（在接近峰值附近波动）
            if remaining_ms > 0 and task.mem_mb > 0:
                # 模拟内存逐步增长到峰值
                progress = 1.0 - (remaining_ms / task.cpu_ms)
                current_mem = task.mem_mb * (0.3 + 0.7 * progress)  # 内存从 30%→100% 增长
                task.peak_mem_mb = max(task.peak_mem_mb, current_mem)

                # 检测 OOM
                if current_mem > slot.size_mb * 1.2:
                    additional = current_mem - slot.size_mb
                    if self.slot_manager.expand_task(slot, additional):
                        task.oom_count += 1
                        self.stats.tasks_oom += 1
                        if self.on_task_oom:
                            self.on_task_oom(task, slot)
                    else:
                        task.status = "SUSPENDED"
                        self.stats.tasks_blocked += 1
                        continue

            if remaining_ms <= 0:
                # 任务完成
                task.finish_time = sim_time + dt_ms / 1000.0
                task.status = "DONE"
                task.peak_mem_mb = max(task.peak_mem_mb, task.mem_mb)
                self.stats.tasks_completed += 1

                # 释放资源
                self.hardware.release(
                    cpu=self.hardware.config.cpu_per_task,
                    mem_mb=task.mem_mb,
                    iops=task.iops,
                    net_mbps=task.net_mbps,
                )

                # 释放槽位
                if slot.expanded_to_addr >= 0:
                    self.slot_manager.shrink_expansion(slot)
                self.slot_manager.release(slot)

                del self.running[task_id]
                completed.append(task)

                if self.on_task_complete:
                    self.on_task_complete(task)
            else:
                self.running[task_id] = (task, slot, remaining_ms)

        return completed

    def _instr_time_ms(self) -> float:
        """每指令执行时间（毫秒）"""
        return 0.001  # 1 M instr/s → 1μs/instr → 0.001ms/instr

    @property
    def running_count(self) -> int:
        return sum(1 for _, (t, _, _) in self.running.items() if t.status == "RUNNING")

    @property
    def suspended_count(self) -> int:
        return sum(1 for _, (t, _, _) in self.running.items() if t.status == "SUSPENDED")

    def resume_suspended(self, sim_time: float):
        """尝试恢复挂起的任务"""
        for task_id, (task, slot, remaining_ms) in list(self.running.items()):
            if task.status == "SUSPENDED":
                # 检查是否有空闲槽位可以容纳
                task.status = "RUNNING"

    def snapshot(self) -> dict:
        return {
            "running": self.running_count,
            "suspended": self.suspended_count,
            "completed": self.stats.tasks_completed,
            "ooms": self.stats.tasks_oom,
            "blocked": self.stats.tasks_blocked,
        }
