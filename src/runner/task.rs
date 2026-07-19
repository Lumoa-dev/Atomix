//! 任务定义 — 调度器的基本工作单元。
//!
//! 覆盖 P3-PL-001 任务池基本设计、P3-PL-003 TaskContext 结构体。

use crate::runner::VmState;

/// 任务 ID 类型。
pub type TaskId = u16;

/// 任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// 初始状态，尚未就绪。
    Init,
    /// 依赖已满足，可被调度执行。
    Ready,
    /// 正在执行中。
    Running,
    /// 阻塞（等待子任务或 IO）。
    Suspended,
    /// 正常完成（TASK_RET 或根任务结束）。
    Done,
    /// 异常终止。
    Error,
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Done | TaskStatus::Error)
    }
}

/// 调度器中的任务条目。
#[derive(Debug, Clone)]
pub struct Task {
    /// 唯一标识。
    pub id: TaskId,
    /// 在 .text 中的入口指令偏移。
    pub entry_offset: usize,
    /// 当前状态。
    pub status: TaskStatus,
    /// 依赖的父任务 ID 列表（这些任务完成后本任务才可执行）。
    pub deps: Vec<TaskId>,
    /// 任务私有的 VM 状态（寄存器、PC、内存等）。
    pub vm: VmState,
    /// TASK_RET 的返回值（当 status == Done 时有效）。
    pub return_value: u64,
    /// 已执行的总指令数。
    pub total_instrs: u64,
    /// 当前时间片内已执行的指令数。
    pub quantum_instrs: u64,
}
