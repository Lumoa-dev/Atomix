"""
硬件资源模型
============
模拟 CPU、内存、IO、网络四条资源维度。
实时追踪使用量、剩余量、利用率。
"""

from dataclasses import dataclass, field
import time
from sim.config import HardwareConfig


@dataclass
class ResourceSnapshot:
    """某一时刻的资源快照"""
    timestamp: float = 0.0

    # 可用量
    cpu_avail: float = 0.0
    mem_avail_mb: float = 0.0
    iops_avail: float = 0.0
    net_avail_mbps: float = 0.0

    # 已用量
    cpu_used: float = 0.0
    mem_used_mb: float = 0.0
    iops_used: float = 0.0
    net_used_mbps: float = 0.0

    @property
    def cpu_util(self) -> float:
        total = self.cpu_avail + self.cpu_used
        return self.cpu_used / total if total > 0 else 0.0

    @property
    def mem_util(self) -> float:
        total = self.mem_avail_mb + self.mem_used_mb
        return self.mem_used_mb / total if total > 0 else 0.0


class HardwareModel:
    """硬件资源模型——追踪资源使用和剩余"""

    def __init__(self, config: HardwareConfig):
        self.config = config

        # 当前可用资源
        self.cpu_avail = config.effective_cpu
        self.mem_avail_mb = config.effective_mem
        self.iops_avail = config.effective_iops
        self.net_avail_mbps = config.effective_net

        # 总容量（用于利用率计算）
        self.cpu_total = config.effective_cpu
        self.mem_total_mb = config.effective_mem
        self.iops_total = config.effective_iops
        self.net_total_mbps = config.effective_net

        # 历史快照
        self.snapshots: list = []

    def allocate(self, cpu: float, mem_mb: float, iops: float, net_mbps: float) -> bool:
        """尝试分配资源。成功返回 True，失败返回 False。"""
        if (cpu > self.cpu_avail or mem_mb > self.mem_avail_mb
                or iops > self.iops_avail or net_mbps > self.net_avail_mbps):
            return False

        self.cpu_avail -= cpu
        self.mem_avail_mb -= mem_mb
        self.iops_avail -= iops
        self.net_avail_mbps -= net_mbps
        return True

    def release(self, cpu: float, mem_mb: float, iops: float, net_mbps: float):
        """释放资源"""
        self.cpu_avail = min(self.cpu_total, self.cpu_avail + cpu)
        self.mem_avail_mb = min(self.mem_total_mb, self.mem_avail_mb + mem_mb)
        self.iops_avail = min(self.iops_total, self.iops_avail + iops)
        self.net_avail_mbps = min(self.net_total_mbps, self.net_avail_mbps + net_mbps)

    def snapshot(self, sim_time: float) -> ResourceSnapshot:
        """获取当前资源快照"""
        s = ResourceSnapshot(
            timestamp=sim_time,
            cpu_avail=self.cpu_avail,
            mem_avail_mb=self.mem_avail_mb,
            iops_avail=self.iops_avail,
            net_avail_mbps=self.net_avail_mbps,
            cpu_used=self.cpu_total - self.cpu_avail,
            mem_used_mb=self.mem_total_mb - self.mem_avail_mb,
            iops_used=self.iops_total - self.iops_avail,
            net_used_mbps=self.net_total_mbps - self.net_avail_mbps,
        )
        self.snapshots.append(s)
        return s

    def update_effective_mem(self, new_alpha_mem: float):
        """更新内存保留系数（OOM 反馈用）"""
        self.config.alpha_mem = new_alpha_mem
        new_effective = self.config.mem_free_mb * new_alpha_mem
        # 平滑过渡：不突然收回已分配的内存
        self.mem_total_mb = new_effective
        self.mem_avail_mb = max(0, new_effective - (self.mem_total_mb - self.mem_avail_mb))
