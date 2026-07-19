//! Executor 定义 — VM 执行体，一个 Executor = 一个 VmState = 一个线程（1:1:1）。
//!
//! 覆盖设计文档 §2（Executor 定义）。

use crate::base::isa::reg;
use crate::runner::event::{EventChannel, ExecutorEvent, ExecutorStats};
use crate::runner::task::{Task, TaskId, TaskStatus};
use crate::runner::VmState;
use crate::runner::VmStateKind;
use std::sync::mpsc::Receiver;

/// 发送给 Executor 线程的命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorCommand {
    /// 运行指定任务的一个时间片。
    RunQuantum {
        /// 时间片大小（指令数）。
        quantum: u32,
    },
    /// 停止 Executor 线程。
    Halt,
}

/// Executor 是持有 VmState 并驱动其执行指令的执行体。
///
/// 每个 Executor：
/// - 独占一个 VmState（load 时从 Task 移入，unload 时移回）
/// - 通过 run_quantum 驱动执行
/// - 正常执行时 Runtime 零介入
/// - 通过事件通道上报事件
#[derive(Debug)]
pub struct Executor {
    /// 持有的 VM 状态（None = 空闲）。
    pub vm: Option<VmState>,
    /// 运行时统计。
    pub stats: ExecutorStats,
    /// 事件上报槽位索引。
    pub event_idx: usize,
    /// 当前加载的任务 ID（None = 空闲）。
    pub task_id: Option<TaskId>,
    /// 当前分配的槽位 ID。
    pub slot_id: Option<u16>,
    /// 心跳间隔（quantum 数，0 = 禁用）。
    pub heartbeat_interval: u32,
    /// 距上次心跳的 quantum 数。
    heartbeat_counter: u32,
    /// 当前任务的 VmState 是否已被取走。
    pub vm_taken: bool,
}

impl Executor {
    /// 创建一个新的 Executor。
    pub fn new(event_idx: usize) -> Self {
        Self {
            vm: None,
            stats: ExecutorStats::default(),
            event_idx,
            task_id: None,
            slot_id: None,
            heartbeat_interval: 0,
            heartbeat_counter: 0,
            vm_taken: false,
        }
    }

    /// 将任务的 VmState 加载到 Executor 中。
    pub fn load(&mut self, task: &mut Task) {
        self.vm = task.vm.take();
        self.task_id = Some(task.id);
        if let Some(ref mut vm) = self.vm {
            vm.task_id = task.id;
            vm.pc = task.entry_offset;
            vm.quantum = 0;
            vm.state = VmStateKind::Running;
        }
        task.status = TaskStatus::Running;
        self.stats = ExecutorStats::default();
        self.vm_taken = false;
    }

    /// 将 VmState 从 Executor 卸回任务。
    pub fn unload(&mut self, task: &mut Task) {
        task.total_instrs = self.stats.total_instrs;
        task.quantum_instrs = 0;

        if let Some(ref vm) = self.vm {
            task.join_waiting_for = vm.join_waiting_for;
            match &vm.state {
                VmStateKind::Halted => {
                    task.status = TaskStatus::Done;
                    task.return_value = vm.read_reg(reg::A0);
                }
                VmStateKind::Error(_) => {
                    task.status = TaskStatus::Error;
                }
                VmStateKind::Suspended => {
                    task.status = TaskStatus::Suspended;
                }
                _ => {
                    task.status = TaskStatus::Ready;
                }
            }
        }

        task.vm = self.vm.take();
        self.task_id = None;
        self.vm_taken = false;
    }

    /// 取走 vm.pending_child（TASK_FORK 产生的子任务 VmState）。
    pub fn take_pending_child(&mut self) -> Option<Box<VmState>> {
        let child = self.vm.as_mut().and_then(|vm| vm.pending_child.take());
        if child.is_some() {
            self.vm_taken = true;
        }
        child
    }

    /// 强制取走整个 VmState（多线程模式下 Runtime 取回 VmState 用）。
    pub fn take_vm(&mut self) -> Option<VmState> {
        let vm = self.vm.take();
        self.vm_taken = true;
        vm
    }

    /// 执行一个时间片（最多 `quantum` 条指令），操作自身持有的 VmState。
    pub fn run_quantum(&mut self, quantum: u32) -> (u64, Option<ExecutorEvent>) {
        let vm = match self.vm.as_mut() {
            Some(vm) if vm.is_running() => vm,
            _ => return (0, None),
        };

        let task_id = vm.task_id;
        let mut count: u64 = 0;

        for _ in 0..quantum {
            if !vm.is_running() {
                break;
            }
            let should_continue = crate::runner::execute::execute_instruction(vm);
            count += 1;
            self.stats.total_instrs += 1;
            if !should_continue {
                break;
            }
        }

        self.stats.pc = vm.pc as u32;
        self.stats.memory_usage = vm.memory.data.len() as u64;
        self.stats.total_quantums += 1;

        // 主事件
        let event = match &vm.state {
            VmStateKind::Running => {
                if vm.quantum >= quantum {
                    Some(ExecutorEvent::Yield { task_id })
                } else {
                    None
                }
            }
            VmStateKind::Halted => {
                let retval = vm.read_reg(reg::A0);
                Some(ExecutorEvent::TaskDone { task_id, retval })
            }
            VmStateKind::Error(_) => {
                Some(ExecutorEvent::TaskError { task_id, errcode: 1 })
            }
            VmStateKind::Suspended => {
                if vm.memory.is_over_watermark() {
                    Some(ExecutorEvent::Oom {
                        task_id,
                        memory_usage: vm.memory.data.len() as u64,
                    })
                } else {
                    None
                }
            }
        };

        // 心跳通过 channel 直接上报，不占用 event 返回
        (count, event)
    }

    /// 将事件上报到 EventChannel。
    pub fn post_event(&self, channel: &EventChannel, event: ExecutorEvent) {
        channel.post(self.event_idx, event);
    }

    /// 检查 Executor 是否空闲。
    pub fn is_idle(&self) -> bool {
        self.task_id.is_none()
    }
}

/// Executor 线程主循环。
///
/// 接收 `ExecutorCommand`，执行 quantum，上报事件到 `EventChannel`。
/// 在 Runtime 的线程池中使用。
pub fn executor_main(
    mut executor: Executor,
    rx: Receiver<ExecutorCommand>,
    event_channel: &EventChannel,
) {
    // 心跳事件的上报直接在这里做，不依赖 run_quantum 的返回值
    loop {
        match rx.recv() {
            Ok(ExecutorCommand::RunQuantum { quantum }) => {
                // 执行一个 quantum
                let (_instr_count, event) = executor.run_quantum(quantum);

                // 上报主事件
                if let Some(ev) = event {
                    executor.post_event(event_channel, ev);
                }

                // 任务结束或挂起后，等待 Runtime 取走 VmState
                if executor.vm.is_none() {
                    continue;
                }
            }
            Ok(ExecutorCommand::Halt) | Err(_) => break,
        }
    }
}
