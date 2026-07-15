"""
任务生成器
==========
模拟任务到达：泊松过程 + 4象限任务类型。
生成的任务进入磁盘任务池（以索引形式存在）。
"""

from dataclasses import dataclass, field
from typing import Dict, List, Optional
import numpy as np
from sim.config import ArrivalConfig, TaskProfile


@dataclass
class Task:
    """仿真中的任务实例"""
    task_id: int
    profile_name: str          # 属于哪个象限

    # 任务资源需求（真实值——仿真中我们"知道"但算法不知道）
    cpu_ms: float              # CPU 耗时
    mem_mb: float              # 内存峰值
    iops: float                # IOPS 需求
    net_mbps: float            # 网络带宽需求

    # 生命周期
    arrive_time: float         # 到达时间（进入任务池）
    start_time: float = 0.0    # 开始执行时间
    finish_time: float = 0.0   # 完成时间
    dispatched: bool = False   # 是否已分配到槽位

    # 槽位信息
    slot_id: int = -1
    mem_addr: float = 0.0      # 虚地址起始
    mem_size: float = 0.0      # 实际分配的内存大小

    # 状态
    status: str = "INIT"       # INIT / QUEUED / RUNNING / SUSPENDED / DONE / ERROR
    oom_count: int = 0         # 该任务经历的 OOM 次数
    peak_mem_mb: float = 0.0   # 该任务执行期间内存峰值

    @property
    def latency_sec(self) -> float:
        """从到达到完成的延迟"""
        if self.finish_time > 0:
            return self.finish_time - self.arrive_time
        return 0.0

    @property
    def queue_time_sec(self) -> float:
        """在任务池中的等待时间"""
        if self.start_time > 0:
            return self.start_time - self.arrive_time
        return 0.0

    @property
    def exec_time_sec(self) -> float:
        """实际执行时间"""
        if self.finish_time > 0 and self.start_time > 0:
            return self.finish_time - self.start_time
        return 0.0


class TaskGenerator:
    """任务生成器：按泊松过程生成混合类型任务"""

    def __init__(self, profiles: List[TaskProfile], arrival: ArrivalConfig, seed: int = 42):
        self.profiles = profiles
        self.arrival = arrival
        self.rng = np.random.default_rng(seed)

        # 归一化权重
        weights = [p.weight for p in profiles]
        total = sum(weights)
        self._weights = [w / total for w in weights]

        self._task_counter = 0
        self._next_arrival_time = 0.0

    def reset(self):
        self._task_counter = 0
        self._next_arrival_time = 0.0

    def generate_until(self, sim_time: float) -> List[Task]:
        """生成从当前时间到 sim_time 之间的所有到达任务"""
        tasks = []
        current_rate = self.arrival.rate_per_sec

        # 处理突发
        if (self.arrival.burst_duration_sec > 0
                and sim_time >= self.arrival.burst_start_sec
                and sim_time <= self.arrival.burst_start_sec + self.arrival.burst_duration_sec):
            current_rate *= self.arrival.burst_multiplier

        while self._next_arrival_time <= sim_time:
            task = self._generate_one(self._next_arrival_time)
            tasks.append(task)
            # 泊松间隔
            interval = self.rng.exponential(1.0 / current_rate)
            self._next_arrival_time += interval

        return tasks

    def _generate_one(self, arrive_time: float) -> Task:
        """生成单个任务"""
        # 按权重选择任务类型
        idx = self.rng.choice(len(self.profiles), p=self._weights)
        profile = self.profiles[idx]

        # 采样资源参数
        params = profile.sample(self.rng)

        self._task_counter += 1
        return Task(
            task_id=self._task_counter,
            profile_name=profile.name,
            cpu_ms=params["cpu_ms"],
            mem_mb=params["mem_mb"],
            iops=params["iops"],
            net_mbps=params["net_mbps"],
            arrive_time=arrive_time,
        )

    def generate_batch(self, n: int, arrive_time: float) -> List[Task]:
        """批量生成 n 个任务（用于测试）"""
        tasks = []
        for _ in range(n):
            task = self._generate_one(arrive_time)
            tasks.append(task)
        return tasks


class TaskPool:
    """任务池——磁盘索引，不存储任务本体"""

    def __init__(self):
        self._tasks: Dict[int, Task] = {}     # task_id → Task（索引）
        self._queue: List[int] = []           # 排队中的 task_id（FIFO）
        self._history: List[int] = []         # 已完成 task_id

    def push(self, task: Task):
        """任务进入池"""
        self._tasks[task.task_id] = task
        task.status = "QUEUED"
        self._queue.append(task.task_id)

    def push_batch(self, tasks: List[Task]):
        for t in tasks:
            self.push(t)

    def pop(self) -> Optional[Task]:
        """从队首取出一个任务"""
        if not self._queue:
            return None
        tid = self._queue.pop(0)
        task = self._tasks[tid]
        task.status = "RUNNING"
        return task

    def pop_batch(self, n: int) -> List[Task]:
        """批量取出 n 个任务"""
        tasks = []
        for _ in range(min(n, len(self._queue))):
            t = self.pop()
            if t:
                tasks.append(t)
        return tasks

    def complete(self, task: Task):
        """标记任务完成"""
        task.status = "DONE"
        self._history.append(task.task_id)

    def requeue(self, task: Task):
        """任务未完成，放回队首"""
        task.status = "QUEUED"
        self._queue.insert(0, task.task_id)

    @property
    def pool_depth(self) -> int:
        """当前积压深度（排队中的任务数）"""
        return len(self._queue)

    @property
    def total_tasks(self) -> int:
        return len(self._tasks)

    @property
    def completed_count(self) -> int:
        return len(self._history)

    def get_recent_completed(self, n: int) -> List[Task]:
        """取最近完成的 n 个任务"""
        recent_ids = self._history[-n:]
        return [self._tasks[tid] for tid in recent_ids if tid in self._tasks]

    def get_stats(self) -> dict:
        """获取任务池统计"""
        completed = self.get_recent_completed(min(100, self.completed_count))
        if not completed:
            return {
                "avg_cpu_ms": 16.0,
                "avg_mem_mb": 16.0,
                "std_cpu_ms": 0.0,
                "std_mem_mb": 0.0,
                "cv_cpu": 0.0,
                "pool_depth": self.pool_depth,
                "completed": self.completed_count,
            }

        cpu_times = [t.cpu_ms for t in completed]
        mem_usages = [t.peak_mem_mb if t.peak_mem_mb > 0 else t.mem_mb for t in completed]

        mu_t = np.mean(cpu_times) if cpu_times else 0.0
        sigma_t = np.std(cpu_times) if cpu_times else 0.0
        mu_m = np.mean(mem_usages) if mem_usages else 0.0
        sigma_m = np.std(mem_usages) if mem_usages else 0.0

        return {
            "avg_cpu_ms": mu_t,
            "avg_mem_mb": mu_m,
            "std_cpu_ms": sigma_t,
            "std_mem_mb": sigma_m,
            "cv_cpu": sigma_t / mu_t if mu_t > 0 else 0.0,
            "cv_mem": sigma_m / mu_m if mu_m > 0 else 0.0,
            "pool_depth": self.pool_depth,
            "completed": self.completed_count,
        }
