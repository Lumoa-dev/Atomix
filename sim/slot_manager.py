"""
槽位管理器 — 俄罗斯方块内存模型
================================
实现墙式预分配槽位 + 滑道（空白保留区）+ 死区回收 + OOM 扩容。

核心机制：
- N 个固定槽位，每个有虚地址空间
- 始终保留一个 ≥1.5× max_slot 的空白滑道
- 任务 OOM 时滑入滑道，旧槽变死区
- 邻居释放后死区合并回收，重新切分成槽位
"""

from dataclasses import dataclass, field
from typing import List, Optional, Dict, Tuple
from enum import Enum
import numpy as np


class SlotStatus(Enum):
    EMPTY = 0       # 空闲
    OCCUPIED = 1    # 有任务在跑
    DEAD = 2        # 死区（被扩容抛弃的旧槽位）


@dataclass
class Slot:
    """单个槽位（v0.3 架构修订）"""
    slot_id: int
    start_addr: float       # 虚地址起始
    size_mb: float          # 虚地址大小（= max_size，槽位上限）

    status: SlotStatus = SlotStatus.EMPTY
    task_id: int = -1       # 当前占用的任务 ID

    # v0.3：物理内存按需分配（不再是整槽分配）
    physical_size: float = 0.0  # 实际分配的物理内存大小

    # 扩容追踪
    expanded_to_addr: float = -1  # 如果扩容了，指向滑道中的新地址
    original_size_mb: float = 0.0 # 扩容前的大小
    expanded_mb: float = 0.0      # 本次扩容增量（additional_mb）

    def __repr__(self):
        return (f"Slot({self.slot_id}, {self.start_addr:.0f}-{self.start_addr+self.size_mb:.0f}MB, "
                f"{self.status.name}, T{self.task_id})")


@dataclass
class SlipwayState:
    """滑道状态"""
    size_mb: float          # 当前大小
    used_mb: float = 0.0    # 已用量
    expansions: int = 0     # 当前扩容任务数


class SlotManager:
    """俄罗斯方块式槽位管理器"""

    def __init__(self, total_memory_mb: float, safety_margin: float = 0.15):
        """
        total_memory_mb: 任务内存池大小（已减去安全冗余后的）
        safety_margin: 安全冗余比例
        """
        self.total_memory_mb = total_memory_mb
        self.safety_margin = safety_margin

        # 槽位
        self.slots: List[Slot] = []
        self.slot_size_mb: float = 0.0

        # 滑道（空白保留区）
        self.slipway: SlipwayState = SlipwayState(size_mb=0.0)

        # 死区列表（等待回收）
        self.dead_zones: List[Tuple[float, float]] = []  # (start, size)

        # OOM 统计
        self.total_ooms: int = 0
        self.recent_ooms: List[float] = []  # 时间戳列表

        # 每任务内存峰值采样
        self.peak_mem_samples: List[float] = []

    # ── 槽位布局 ──────────────────────────

    def layout(self, n_batch: int, slipway_multiplier: float = 1.5):
        """
        重新规划内存布局。

        布局格式：
        [Slot0][Slot1]...[SlotN][--- 滑道 ---]
        """
        if n_batch <= 0:
            n_batch = 1

        # 计算槽位大小
        pool_for_slots = self.total_memory_mb
        raw_slot_size = pool_for_slots / (n_batch + slipway_multiplier)

        self.slot_size_mb = raw_slot_size

        # 滑道大小
        slipway_size = raw_slot_size * slipway_multiplier

        # 如果布局变了，重建槽位
        if len(self.slots) != n_batch or abs(self.slots[0].size_mb - raw_slot_size) > 0.1 if self.slots else True:
            self.slots = []
            addr = 0.0
            for i in range(n_batch):
                self.slots.append(Slot(
                    slot_id=i,
                    start_addr=addr,
                    size_mb=raw_slot_size,
                ))
                addr += raw_slot_size

            self.slipway = SlipwayState(size_mb=slipway_size)

    # ── 槽位分配（v0.3 虚→实分配）─────────

    def allocate(self, task_id: int, mem_requirement_mb: float,
                 compiler_peak_mb: float = 0) -> Optional[Slot]:
        """
        为任务分配一个空闲槽位（虚地址空间 + 物理内存）。

        v0.3 修改：
        - 使用 compiler_peak_mb（若提供）否则使用 mem_requirement_mb
        - 物理内存 over-allocate 20%（×1.2）作为安全裕度
        - 只分配任务所需的物理内存，不占满整个槽位

        返回槽位或 None（无空闲）。
        """
        # 计算所需物理内存
        required = compiler_peak_mb if compiler_peak_mb > 0 else mem_requirement_mb
        physical_size = required * 1.2  # 安全 over-allocate 20%

        for slot in self.slots:
            if slot.status == SlotStatus.EMPTY and slot.size_mb >= physical_size:
                slot.status = SlotStatus.OCCUPIED
                slot.task_id = task_id
                slot.physical_size = physical_size
                slot.original_size_mb = slot.size_mb
                return slot

        # 尝试回收死区来创建新槽位
        reclaimed = self._try_reclaim_dead_zone()
        if reclaimed is not None and reclaimed.size_mb >= physical_size:
            reclaimed.status = SlotStatus.OCCUPIED
            reclaimed.task_id = task_id
            reclaimed.physical_size = physical_size
            reclaimed.original_size_mb = reclaimed.size_mb
            return reclaimed

        return None

    def release(self, slot: Slot):
        """释放槽位"""
        slot.status = SlotStatus.EMPTY
        slot.task_id = -1
        slot.expanded_to_addr = -1
        slot.original_size_mb = 0.0
        slot.expanded_mb = 0.0

        # 检查相邻死区，尝试合并
        self._try_merge_dead_zones()

    # ── OOM 扩容（俄罗斯方块滑动）─────────

    def expand_task(self, slot: Slot, additional_mb: float) -> bool:
        """
        任务 OOM，尝试滑入滑道。
        成功返回 True，失败（滑道不够）返回 False。
        """
        if self.slipway.used_mb + additional_mb > self.slipway.size_mb:
            # 滑道满了，无法扩容
            return False

        # 记录扩容
        self.total_ooms += 1
        slot.expanded_mb = additional_mb

        # 旧槽位变死区
        slot.status = SlotStatus.DEAD
        slot.expanded_to_addr = self.slipway.size_mb  # 简化：记录使用了滑道

        # 滑道被占用（只记录额外部分）
        self.slipway.used_mb += additional_mb
        self.slipway.expansions += 1

        # 采样峰值
        self.peak_mem_samples.append(slot.original_size_mb + additional_mb)

        return True

    def shrink_expansion(self, slot: Slot):
        """扩容任务完成，释放滑道空间"""
        released = slot.expanded_mb
        if released > 0:
            self.slipway.used_mb = max(0, self.slipway.used_mb - released)
        self.slipway.expansions = max(0, self.slipway.expansions - 1)

    # ── 死区合并（v0.3 defrag 集成）──────

    def snapshot_slots(self) -> List[dict]:
        """
        将当前槽位布局导出为字典列表，供 DefragManager.evaluate() 使用。
        """
        slots_data = []
        for s in self.slots:
            slots_data.append({
                "id": s.slot_id,
                "status": s.status.name,
                "size_mb": s.size_mb,
                "task_id": s.task_id if s.task_id >= 0 else None,
                "physical_size": s.physical_size,
            })
        return slots_data

    def merge_dead_zone(self, dead_slot_id: int, target_slot_id: int):
        """
        合并死区与相邻槽位。

        将 dead_slot 的内容（如果有）合并到 target_slot，
        dead_slot 标记为 EMPTY 且大小置零。
        """
        if dead_slot_id < 0 or dead_slot_id >= len(self.slots):
            return
        if target_slot_id < 0 or target_slot_id >= len(self.slots):
            return

        dead = self.slots[dead_slot_id]
        target = self.slots[target_slot_id]

        # 合并大小
        target.size_mb += dead.size_mb
        target.physical_size += dead.physical_size

        # 死区清零
        dead.status = SlotStatus.EMPTY
        dead.size_mb = 0.0
        dead.physical_size = 0.0
        dead.task_id = -1

    def defrag_step(self, defrag_manager) -> None:
        """
        运行一次碎片整理评估并执行合并动作。

        defrag_manager: DefragManager 实例
        """
        slots_data = self.snapshot_slots()
        actions = defrag_manager.evaluate(slots_data)

        for action in actions:
            self.merge_dead_zone(action.dead_slot_id, action.target_slot_id)
            defrag_manager.record_merge(action.merged_size_mb)

    # ── 死区管理 ──────────────────────────

    def _try_reclaim_dead_zone(self) -> Optional[Slot]:
        """尝试回收死区：找到相邻空闲槽位的死区，合并创建新槽位"""
        for i, slot in enumerate(self.slots):
            if slot.status != SlotStatus.DEAD:
                continue

            # 检查左右邻居是否空闲
            left_empty = (i > 0 and self.slots[i-1].status == SlotStatus.EMPTY)
            right_empty = (i < len(self.slots)-1 and self.slots[i+1].status == SlotStatus.EMPTY)

            if left_empty:
                # 合并死区和左空闲槽位
                self.slots[i-1].size_mb += slot.size_mb
                slot.status = SlotStatus.EMPTY
                slot.task_id = -1
                # 返回合并后的空闲槽位
                return slot

            if right_empty:
                # 合并死区和右空闲槽位
                slot.size_mb += self.slots[i+1].size_mb
                slot.status = SlotStatus.EMPTY
                slot.task_id = -1
                # 移除原空闲槽位
                self.slots[i+1].status = SlotStatus.EMPTY
                self.slots[i+1].task_id = -1
                return slot

        return None

    def _try_merge_dead_zones(self):
        """合并相邻空闲/死区"""
        # 简化：只是清理连续的 EMPTY+DEAD 为 EMPTY
        for i, slot in enumerate(self.slots):
            if slot.status == SlotStatus.DEAD:
                # 如果邻居空闲，吸收
                if i > 0 and self.slots[i-1].status == SlotStatus.EMPTY:
                    self.slots[i-1].size_mb += slot.size_mb
                    # 标记当前死区为已合并（设为空）
                    slot.status = SlotStatus.EMPTY
                    slot.size_mb = 0.0
                elif i < len(self.slots)-1 and self.slots[i+1].status == SlotStatus.EMPTY:
                    self.slots[i+1].size_mb += slot.size_mb
                    slot.status = SlotStatus.EMPTY
                    slot.size_mb = 0.0

    # ── 统计 ──────────────────────────────

    @property
    def empty_slots(self) -> int:
        """空闲槽位数（含死区）"""
        return sum(1 for s in self.slots if s.status in (SlotStatus.EMPTY, SlotStatus.DEAD))

    @property
    def occupied_slots(self) -> int:
        return sum(1 for s in self.slots if s.status == SlotStatus.OCCUPIED)

    @property
    def dead_slots(self) -> int:
        return sum(1 for s in self.slots if s.status == SlotStatus.DEAD)

    @property
    def utilization(self) -> float:
        """槽位利用率"""
        if not self.slots:
            return 0.0
        return self.occupied_slots / len(self.slots)

    def get_effective_slot_memory(self) -> float:
        """获取有效每槽位内存（考虑死区后实际可用）"""
        total = sum(s.size_mb for s in self.slots if s.status != SlotStatus.DEAD)
        n = sum(1 for s in self.slots if s.status != SlotStatus.DEAD)
        return total / n if n > 0 else self.slot_size_mb

    def snapshot(self) -> dict:
        return {
            "n_slots": len(self.slots),
            "n_occupied": self.occupied_slots,
            "n_empty": self.empty_slots,
            "n_dead": self.dead_slots,
            "slot_size_mb": self.slot_size_mb,
            "slipway_size_mb": self.slipway.size_mb,
            "slipway_used_mb": self.slipway.used_mb,
            "slipway_expansions": self.slipway.expansions,
            "total_ooms": self.total_ooms,
            "utilization": self.utilization,
            "effective_slot_mem": self.get_effective_slot_memory(),
        }
