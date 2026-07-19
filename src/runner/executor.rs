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
    RunQuantum { task_id: TaskId },
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
#[derive(Debug, Clone)]
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
        }
    }

    /// 将任务的 VmState 加载到 Executor 中。
    ///
    /// 从 `task.vm` 中 `take()` 出 VmState，Executor 获得所有权。
    /// 调用后 `task.vm == None`。
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
    }

    /// 将 VmState 从 Executor 卸回任务。
    ///
    /// 同步执行统计、join_waiting_for、任务状态到 Task。
    pub fn unload(&mut self, task: &mut Task) {
        // 同步统计
        task.total_instrs = self.stats.total_instrs;
        task.quantum_instrs = 0;

        if let Some(ref vm) = self.vm {
            task.join_waiting_for = vm.join_waiting_for;

            // 根据 VM 状态更新任务状态
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

        // 移回 VmState
        task.vm = self.vm.take();
        self.task_id = None;
    }

    /// 取走 `vm.pending_child`（TASK_FORK 产生的子任务 VmState）。
    /// 在 unload 之前调用。
    pub fn take_pending_child(&mut self) -> Option<Box<VmState>> {
        self.vm.as_mut().and_then(|vm| vm.pending_child.take())
    }

    /// 执行一个时间片（最多 `quantum` 条指令），操作自身持有的 VmState。
    ///
    /// 返回 (指令数, 事件)。
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

        // 更新统计
        self.stats.pc = vm.pc as u32;
        self.stats.memory_usage = vm.memory.data.len() as u64;
        self.stats.total_quantums += 1;

        // 心跳上报
        if self.heartbeat_interval > 0 {
            self.heartbeat_counter += 1;
            if self.heartbeat_counter >= self.heartbeat_interval {
                self.heartbeat_counter = 0;
                // 心跳由调用方通过 EventChannel 上报
            }
        }

        // 生成事件
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

        (count, event)
    }

    /// 将事件上报到 EventChannel（Runtime 消费）。
    pub fn post_event(&self, channel: &EventChannel, event: ExecutorEvent) {
        channel.post(self.event_idx, event);
    }

    /// 检查 Executor 是否空闲（无加载的任务）。
    pub fn is_idle(&self) -> bool {
        self.task_id.is_none()
    }
}

/// Executor 线程主循环。
///
/// 接收 `ExecutorCommand`，执行 quantum，上报事件。
/// 在 Runtime 的线程池中使用。
pub fn executor_main(
    mut executor: Executor,
    rx: Receiver<ExecutorCommand>,
    event_channel: &EventChannel,
) {
    loop {
        match rx.recv() {
            Ok(ExecutorCommand::RunQuantum { task_id }) => {
                let _ = task_id; // 任务已由 Runtime 在 load 阶段移入 executor

                // 执行一个 quantum
                let (instr_count, event) = executor.run_quantum(1000);

                // 上报事件
                if let Some(ev) = event {
                    executor.post_event(event_channel, ev);
                }

                // 如果指令数为 0，说明任务已结束，等待被 unload
                if instr_count == 0 {
                    continue;
                }
            }
            Ok(ExecutorCommand::Halt) | Err(_) => break,
        }
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::{AtxeBinary, Header};
    use crate::base::isa::{self, opcode};

    fn make_test_vm(text: Vec<u32>) -> VmState {
        let header = Header::new(0, 6);
        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata: vec![],
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        VmState::load_atxe(&binary.to_bytes()).unwrap()
    }

    fn make_test_task(id: u16, entry_offset: usize, status: TaskStatus, vm: VmState) -> Task {
        Task {
            id,
            entry_offset,
            status,
            deps: Vec::new(),
            return_value: 0,
            total_instrs: 0,
            quantum_instrs: 0,
            join_waiting_for: None,
            vm: Some(vm),
        }
    }

    #[test]
    fn executor_new_is_idle() {
        let exec = Executor::new(0);
        assert!(exec.is_idle());
        assert!(exec.vm.is_none());
    }

    #[test]
    fn executor_load_unload_roundtrip() {
        let vm = make_test_vm(vec![0]);
        let mut task = make_test_task(1, 0, TaskStatus::Ready, vm);
        let mut exec = Executor::new(0);

        exec.load(&mut task);
        assert!(exec.vm.is_some());
        assert_eq!(exec.task_id, Some(1));
        assert!(task.vm.is_none());
        assert_eq!(task.status, TaskStatus::Running);

        exec.unload(&mut task);
        assert!(exec.vm.is_none());
        assert!(exec.is_idle());
        assert!(task.vm.is_some());
    }

    #[test]
    fn executor_run_single_instruction() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let vm = make_test_vm(text);
        let mut task = make_test_task(0, 0, TaskStatus::Ready, vm);
        let mut exec = Executor::new(0);

        exec.load(&mut task);
        let (count, event) = exec.run_quantum(1000);

        assert!(count > 0, "should have executed");
        let event = event.expect("should produce TaskDone event");
        match event {
            ExecutorEvent::TaskDone { task_id, retval } => {
                assert_eq!(task_id, 0);
                assert_eq!(retval, 42);
            }
            other => panic!("expected TaskDone, got {:?}", other),
        }

        exec.unload(&mut task);
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(task.return_value, 42);
    }

    #[test]
    fn executor_quantum_yield() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 0);
            500
        ];
        let vm = make_test_vm(text);
        let mut task = make_test_task(0, 0, TaskStatus::Ready, vm);
        let mut exec = Executor::new(0);

        exec.load(&mut task);
        let (count, event) = exec.run_quantum(10);
        assert_eq!(count, 10);

        match event {
            Some(ExecutorEvent::Yield { task_id }) => {
                assert_eq!(task_id, 0);
            }
            other => panic!("expected Yield, got {:?}", other),
        }
    }

    #[test]
    fn executor_not_running_returns_zero() {
        let vm = make_test_vm(vec![0]);
        let mut task = make_test_task(0, 0, TaskStatus::Ready, vm);
        let mut exec = Executor::new(0);

        exec.load(&mut task);
        // 手动设置为 Halted（模拟任务执行完毕的情况）
        exec.vm.as_mut().unwrap().state = VmStateKind::Halted;
        let (count, event) = exec.run_quantum(1000);
        assert_eq!(count, 0);
        assert!(event.is_none());
    }

    #[test]
    fn executor_take_pending_child() {
        let vm = make_test_vm(vec![0]);
        let mut task = make_test_task(0, 0, TaskStatus::Ready, vm);
        let mut exec = Executor::new(0);
        exec.load(&mut task);
        assert!(exec.take_pending_child().is_none());
    }
}
