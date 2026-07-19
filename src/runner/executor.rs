//! Executor 定义 — VM 执行体，一个 Executor = 一个 VmState = 一个线程（1:1:1）。
//!
//! 覆盖设计文档 §2（Executor 定义）。

use crate::base::isa::reg;
use crate::runner::event::{EventChannel, ExecutorEvent, ExecutorStats};
use crate::runner::pool::TaskPool;
use crate::runner::task::{Task, TaskId, TaskStatus};
use crate::runner::VmState;
use crate::runner::VmStateKind;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

/// 发送给 Executor 线程的命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorCommand {
    /// 加载指定任务并执行一个时间片。
    Execute {
        task_id: TaskId,
        quantum: u32,
    },
    /// 停止 Executor 线程。
    Halt,
}

/// Executor 是持有 VmState 并驱动其执行指令的执行体。
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
    heartbeat_counter: u32,
    pub vm_taken: bool,
}

impl Executor {
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
                VmStateKind::Error(_) => task.status = TaskStatus::Error,
                VmStateKind::Suspended => task.status = TaskStatus::Suspended,
                _ => task.status = TaskStatus::Ready,
            }
        }
        task.vm = self.vm.take();
        self.task_id = None;
        self.vm_taken = false;
    }

    pub fn take_pending_child(&mut self) -> Option<Box<VmState>> {
        let child = self.vm.as_mut().and_then(|vm| vm.pending_child.take());
        if child.is_some() {
            self.vm_taken = true;
        }
        child
    }

    pub fn take_vm(&mut self) -> Option<VmState> {
        let vm = self.vm.take();
        self.vm_taken = true;
        vm
    }

    /// 执行一个时间片（最多 `quantum` 条指令）。
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
            VmStateKind::Error(_) => Some(ExecutorEvent::TaskError { task_id, errcode: 1 }),
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
        (count, event)
    }

    pub fn post_event(&self, channel: &EventChannel, event: ExecutorEvent) {
        channel.post(self.event_idx, event);
    }

    pub fn is_idle(&self) -> bool {
        self.task_id.is_none()
    }
}

/// Executor 线程主循环。
///
/// VmState 始终在 TaskPool 中。每次 Execute 命令：
/// 1. 锁定 pool，取出 task 的 VmState
/// 2. 移入 executor，run_quantum
/// 3. 取走 pending_child，加入 pool
/// 4. 卸回 VmState，上报事件
/// 5. 解锁
pub fn executor_main(
    mut executor: Executor,
    rx: Receiver<ExecutorCommand>,
    event_channel: &EventChannel,
    pool: &Mutex<TaskPool>,
) {
    loop {
        match rx.recv() {
            Ok(ExecutorCommand::Execute { task_id, quantum }) => {
                executor.task_id = Some(task_id);

                // 1. 锁定 pool
                let mut guard = pool.lock().unwrap();
                let task = match guard.get_mut(task_id) {
                    Some(t) => t,
                    None => continue,
                };
                if task.status != TaskStatus::Ready && task.status != TaskStatus::Running {
                    continue;
                }

                // 2. load → run
                executor.load(task);
                let (count, _event) = executor.run_quantum(quantum);
                task.total_instrs += count;
                task.quantum_instrs += count;

                // 3. 在 unload 之前捕获返回值（避免 unload 后失去引用）
                let task_status = task.status;
                let task_retval = task.return_value;

                // 4. pending_child（在解锁之前取走）
                let child_vm = executor.take_pending_child();

                // 5. unload
                executor.unload(task);
                // task 借用结束

                // 6. 添加子任务到 pool
                if let Some(cv) = child_vm {
                    let child_id = cv.task_id;
                    let new_task = Task {
                        id: child_id,
                        entry_offset: cv.pc,
                        status: TaskStatus::Ready,
                        deps: Vec::new(),
                        vm: Some(*cv),
                        return_value: 0,
                        total_instrs: 0,
                        quantum_instrs: 0,
                        join_waiting_for: None,
                    };
                    guard.add_task(new_task);
                }
                // guard 释放

                // 7. 上报事件（锁外）
                let event = match task_status {
                    TaskStatus::Done => Some(ExecutorEvent::TaskDone {
                        task_id,
                        retval: task_retval,
                    }),
                    TaskStatus::Error => Some(ExecutorEvent::TaskError {
                        task_id,
                        errcode: 1,
                    }),
                    TaskStatus::Suspended => Some(ExecutorEvent::Oom {
                        task_id,
                        memory_usage: 0,
                    }),
                    _ => Some(ExecutorEvent::Yield { task_id }),
                };
                if let Some(ev) = event {
                    executor.post_event(event_channel, ev);
                }
            }
            Ok(ExecutorCommand::Halt) | Err(_) => break,
        }
    }
}
