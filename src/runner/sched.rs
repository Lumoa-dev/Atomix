//! 任务调度器 — 多任务执行循环。
//!
//! 覆盖 P3-SCH-001 执行引擎主循环、P3-SCH-002 时间片与抢占、
//! P3-SCH-003 上下文切换。

use crate::base::ir::AtxeBinary;
use crate::base::isa::reg;
use crate::runner::execute;
use crate::runner::loader::parse_task_section;
use crate::runner::pool::TaskPool;
use crate::runner::task::{Task, TaskId, TaskStatus};
use crate::runner::VmState;

/// 调度器。管理多个任务的分时执行。
pub struct Scheduler {
    /// 任务池。
    pub pool: TaskPool,
    /// 默认时间片大小（指令数）。
    pub quantum: u32,
    /// 已执行的总指令数。
    pub total_instrs: u64,
}

impl Scheduler {
    /// 从 .atxe 二进制创建调度器，自动解析 .task 段并为每个任务创建 VmState。
    pub fn from_atxe(binary: &AtxeBinary) -> Result<Self, String> {
        let entries = parse_task_section(&binary.task_table)?;

        if entries.is_empty() {
            // 没有 .task 段时，创建一个默认的根任务
            let vm = VmState::from_atxe(binary)?;
            let task = Task {
                id: 0,
                entry_offset: vm.pc,
                status: TaskStatus::Ready,
                deps: Vec::new(),
                vm,
                return_value: 0,
                total_instrs: 0,
                quantum_instrs: 0,
            };
            return Ok(Self {
                pool: TaskPool::new(vec![task]),
                quantum: 1000,
                total_instrs: 0,
            });
        }

        let mut tasks = Vec::with_capacity(entries.len());
        for entry in &entries {
            let mut vm = VmState::from_atxe(binary)?;
            vm.pc = entry.entry_offset as usize;
            vm.task_id = entry.task_id;

            // 初始状态：有依赖的为 Init，无依赖的为 Ready
            let status = if entry.dep_list.is_empty() {
                TaskStatus::Ready
            } else {
                TaskStatus::Init
            };

            tasks.push(Task {
                id: entry.task_id,
                entry_offset: entry.entry_offset as usize,
                status,
                deps: entry.dep_list.clone(),
                vm,
                return_value: 0,
                total_instrs: 0,
                quantum_instrs: 0,
            });
        }

        Ok(Self {
            pool: TaskPool::new(tasks),
            quantum: 1000,
            total_instrs: 0,
        })
    }

    /// 运行所有任务直到全部完成或出错。
    /// 返回成功时是所有任务完成，返回错误是某个任务出错。
    pub fn run_all(&mut self) -> Result<(), String> {
        loop {
            if self.pool.all_done() {
                break;
            }

            // 激活依赖已满足的 Init 任务
            self.pool.activate_ready_tasks();

            let ready = self.pool.ready_tasks();
            if ready.is_empty() {
                // 没有就绪任务但也不是全部完成 → 有任务阻塞等待中
                // 第一版：没有阻塞任务（TASK_JOIN 暂不阻塞），所以这种情况不应发生
                break;
            }

            for task_id in ready {
                self.execute_quantum(task_id);
            }
        }

        // 检查是否有任务出错
        for (id, status, _, _) in self.pool.results() {
            if status == TaskStatus::Error {
                let task = self.pool.get(id).unwrap();
                return Err(format!("任务 {} 执行出错: {:?}", id, task.vm.state));
            }
        }

        Ok(())
    }

    /// 执行一个任务的一个时间片。
    fn execute_quantum(&mut self, task_id: TaskId) {
        let task = self.pool.get_mut(task_id).unwrap();
        if task.status != TaskStatus::Ready {
            return;
        }
        task.status = TaskStatus::Running;
        task.quantum_instrs = 0;

        // 执行最多 quantum 条指令
        let budget = self.quantum;
        for _ in 0..budget {
            let was_running = task.vm.is_running();
            if !was_running {
                break;
            }

            let should_continue = execute::execute_instruction(&mut task.vm);
            task.total_instrs += 1;
            task.quantum_instrs += 1;
            self.total_instrs += 1;

            if !should_continue {
                break; // 指令要求让出（HALT、error、quantum 耗尽等）
            }
        }

        // 检查执行结果
        match &task.vm.state {
            crate::runner::VmStateKind::Halted => {
                task.status = TaskStatus::Done;
                task.return_value = task.vm.read_reg(reg::A0);
            }
            crate::runner::VmStateKind::Error(_) => {
                task.status = TaskStatus::Error;
            }
            crate::runner::VmStateKind::Suspended => {
                task.status = TaskStatus::Suspended;
            }
            _ => {
                // Running → 时间片用完，重新入队
                task.status = TaskStatus::Ready;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::{AtxeBinary, Header};
    use crate::base::isa::{self, opcode};
    use crate::compiler::codegen::assembly;

    /// 创建一个含有多任务 .task 段的最小 .atxe。
    fn make_multi_task_atxe(
        texts: Vec<Vec<u32>>,
        entries: Vec<(u16, u32, Vec<u16>)>, // (task_id, entry_offset, deps)
    ) -> Vec<u8> {
        let mut all_text = Vec::new();
        for t in &texts {
            all_text.extend_from_slice(t);
        }

        // 构建 .task 段
        let mut task_data = Vec::new();
        for (id, entry, deps) in &entries {
            task_data.extend_from_slice(&id.to_le_bytes());
            task_data.extend_from_slice(&entry.to_le_bytes());
            task_data.extend_from_slice(&(deps.len() as u16).to_le_bytes());
            for dep in deps {
                task_data.extend_from_slice(&dep.to_le_bytes());
            }
        }

        let header = Header::new(0, 6);
        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text: all_text,
            rodata: vec![],
            task_table: task_data,
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        binary.to_bytes()
    }

    #[test]
    fn scheduler_run_single_task() {
        // 单个任务：MOVI a0, 42; TASK_RET a0
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        // 只有一个 task 0，无依赖
        let bytes = make_multi_task_atxe(vec![text], vec![(0, 0, vec![])]);
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut sched = Scheduler::from_atxe(&binary).unwrap();
        sched.run_all().unwrap();

        let results = sched.pool.results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, TaskStatus::Done);
        assert_eq!(results[0].2, 42); // return_value
    }

    #[test]
    fn scheduler_two_tasks_sequential() {
        // Task 0: MOVI a0, 10; TASK_RET a0 (entry=0)
        // Task 1: MOVI a0, 20; TASK_RET a0 (entry=2, dep on task 0)
        let task0 = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 10),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let task1 = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 20),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let bytes = make_multi_task_atxe(
            vec![task0, task1],
            vec![(0, 0, vec![]), (1, 2, vec![0])],
        );
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut sched = Scheduler::from_atxe(&binary).unwrap();
        sched.run_all().unwrap();

        let results = sched.pool.results();
        assert_eq!(results.len(), 2);
        // Both should be Done
        assert_eq!(results[0].1, TaskStatus::Done);
        assert_eq!(results[1].1, TaskStatus::Done);
        assert_eq!(results[0].2, 10);
        assert_eq!(results[1].2, 20);
    }

    #[test]
    fn scheduler_task_error_propagates() {
        // Task: DIV by zero → error
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 1),
            isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 0),
            isa::encode_r3(opcode::DIV, reg::A2 as u8, reg::A0 as u8, reg::A1 as u8, 0),
        ];
        let bytes = make_multi_task_atxe(vec![text], vec![(0, 0, vec![])]);
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut sched = Scheduler::from_atxe(&binary).unwrap();
        let result = sched.run_all();
        assert!(result.is_err());
    }

    #[test]
    fn scheduler_without_task_section() {
        // 没有 .task 段的情况：创建一个默认根任务
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 99),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
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
        let mut sched = Scheduler::from_atxe(&binary).unwrap();
        sched.run_all().unwrap();
        let results = sched.pool.results();
        assert_eq!(results[0].2, 99);
    }
}
