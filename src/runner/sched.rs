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
    /// 下一个可用的 task_id（TASK_FORK 分配用）。
    pub next_task_id: u16,
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
                next_task_id: 1,
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

        let max_id = tasks.iter().map(|t| t.id).max().unwrap_or(0);
        Ok(Self {
            pool: TaskPool::new(tasks),
            quantum: 1000,
            total_instrs: 0,
            next_task_id: max_id + 1,
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
                if self.pool.all_done() {
                    break;
                }
                if !self.pool.has_suspended() {
                    // 没有阻塞任务也不是全部完成 → 死锁或异常
                    break;
                }
                // 有 Suspended 任务在等 child 完成 → 继续循环（等下一轮唤醒）
                continue;
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
        // 先提取所需数据再释放 task 借用，避免与 self.pool 的方法冲突
        let (completed_id, retval, pending_child) = {
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
                    break;
                }
            }

            // 检查执行结果
            let completed = match &task.vm.state {
                crate::runner::VmStateKind::Halted => {
                    task.status = TaskStatus::Done;
                    task.return_value = task.vm.read_reg(reg::A0);
                    Some(task.id)
                }
                crate::runner::VmStateKind::Error(_) => {
                    task.status = TaskStatus::Error;
                    Some(task.id)
                }
                crate::runner::VmStateKind::Suspended => {
                    task.status = TaskStatus::Suspended;
                    None
                }
                _ => {
                    task.status = TaskStatus::Ready;
                    None
                }
            };
            let ret = task.return_value;
            let child = task.vm.pending_child.take();
            (completed, ret, child)
        }; // task borrow dropped here

        // 唤醒 joiners（此时无 task 借用，可安全借用 pool）
        if let Some(done_id) = completed_id {
            self.pool.wake_joiners(done_id, retval);
        }

        // 处理 pending_child（box deref）
        if let Some(child_vm) = pending_child {
            let child_id = child_vm.task_id;
            let new_task = Task {
                id: child_id,
                entry_offset: child_vm.pc,
                status: TaskStatus::Ready,
                deps: Vec::new(),
                vm: *child_vm,
                return_value: 0,
                total_instrs: 0,
                quantum_instrs: 0,
            };
            self.pool.add_task(new_task);
            if child_id >= self.next_task_id {
                self.next_task_id = child_id.wrapping_add(1);
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

    // ── TASK_FORK / TASK_JOIN 测试 ──────────────

    #[test]
    fn fork_one_child() {
        // 父任务: MOVI t0=1; TASK_FORK t0=child_id; MOVI a0=0; TASK_RET a0
        // 子任务: MOVI a0=42; TASK_RET a0
        // TASK_FORK imm=1 → 子任务 ID=1，从父任务 pc+1 开始
        // 父任务指令 (entry=0):
        // 0: MOVI T0, 1          — 随便放个数
        // 1: TASK_FORK T0, 1     — fork 子任务 ID=1，handle→T0
        // 2: MOVI A0, 0          — 父任务自己的返回值
        // 3: TASK_RET A0
        // 子任务指令 (entry=1?? 不，fork从pc+1开始)
        // 实际上 TASK_FORK 执行后，子任务从 pc+1 开始执行，
        // 但父任务也会继续执行 pc+2。
        // 所以父子任务的指令流会交叉。
        //
        // 简化设计：让父子任务执行相同的代码路径
        // MOVI A0, 42; TASK_RET A0
        // 父任务 fork 后，子任务也从当前位置开始执行相同的指令
        
        // 最简单的 fork 测试：父任务 fork child，child 自动运行然后完成
        // 但父子共享同一份代码——fork 时 child.pc = parent.pc + 1
        // 所以如果父任务指令是:
        // 0: MOVI A0, 10     (父任务设置返回值)
        // 1: TASK_FORK 0, 1  (fork child id=1)
        // 2: MOVI A0, 20     (父任务修改返回值)
        // 3: TASK_RET A0     (父任务返回)
        //
        // 子任务从 pc=2 (parent.pc+1=1+1=2) 开始：
        // 2: MOVI A0, 20
        // 3: TASK_RET A0     → 子任务返回 20
        
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 10),  // 0: a0=10
            isa::encode_r1i(opcode::TASK_FORK, reg::T0 as u8, 1), // 1: fork id=1
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 20),  // 2: a0=20
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),  // 3: ret
        ];
        let bytes = make_multi_task_atxe(vec![text], vec![(0, 0, vec![])]);
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut sched = Scheduler::from_atxe(&binary).unwrap();
        sched.run_all().unwrap();
        
        // 应有 2 个任务：父(0) + child(1)
        let results = sched.pool.results();
        assert_eq!(results.len(), 2, "should have parent + child");
        // 两个任务都应该 Done
        for (_, status, _, _) in &results {
            assert_eq!(*status, TaskStatus::Done, "all tasks should be Done");
        }
    }

    #[test]
    fn fork_then_join_child() {
        // 父任务：fork 一个 child，然后 JOIN 等它完成，读到返回值
        // 指令序列：
        // 0: MOVI A0, 10       (无用)
        // 1: TASK_FORK t0, 1   (fork child id=1, handle→t0)
        // 2: TASK_JOIN t1, t0  (等待 child 完成，返回值→t1)
        // 3: MOV A0, t1        (把 child 返回值设为自己的返回值)
        // 4: TASK_RET a0
        //
        // child 从 pc=2 (parent fork 时的 pc+1 = 1+1) 开始：
        // 2: TASK_JOIN t1, t0  — child 没有 join_waiting_for，所以 join 的是... 
        //     这个问题！child 也执行了 TASK_JOIN，但 t0 里存的是 0（没意义），
        //     join_waiting_for = Some(0) 但 id=0 的任务还在运行，会死锁。
        //
        // 需要让 child 不走 TASK_JOIN 路径。
        // 方案：child 和 parent 用不同的代码路径。
        // 但 fork 时 child.pc = parent.pc+1 = 1+1 = 2，即从 TASK_JOIN 开始执行。
        // 
        // 不行，这样父子共享代码会出问题。
        //
        // 换个思路：把 child 的入口放在另一个位置。
        // TASK_FORK 的 ops.imm 是 task_id=1。
        // 我们构造 .task 段，让 task_id=1 的 entry_offset 指向 child 的代码。
        // 但当前 TASK_FORK 实现是 child.pc = parent.pc + 1，不查 .task 段。
        //
        // 最简测试：不用 JOIN，只验证 fork 产生子任务并行执行。
        // 或者在 fork 前先跳转到子程序，然后 fork，让 child 从子程序开始。
        
        // 简化：让父子代码路径独立。
        // 父：MOVI t0, 42; TASK_FORK t1, 1; TASK_JOIN t2, t1; MOV a0, t2; TASK_RET a0
        // 子做独立的事情（MOVI a0, 99; TASK_RET a0）
        // 但子从 pc+1 开始，所以需要安排指令布局：
        //
        // 0: MOVI t0, 0           (占位)
        // 1: TASK_FORK t1, 1      (fork child, handle→t1, child.pc=2)
        // 2: ... child code ...   (child 从这里开始)
        // 2: MOVI a0, 77          (子任务返回值)
        // 3: TASK_RET a0          (子任务返回)
        // 4: ... parent continue ...
        // 4: TASK_JOIN t2, t1     (父任务等 child, handle=t1, 返回值→t2)
        // 5: MOV a0, t2           (父任务返回值=child返回值)
        // 6: TASK_RET a0          (父任务返回)

        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::T0 as u8, 0, 0),   // 0: t0=0
            isa::encode_r1i(opcode::TASK_FORK, reg::T1 as u8, 1), // 1: fork id=1, handle→t1
            // child code (pc=2):
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 77),  // 2: a0=77
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),  // 3: ret→77
            // parent resume (pc=4):
            isa::encode_r2i(opcode::TASK_JOIN, reg::T2 as u8, reg::T1 as u8, 0), // 4: join t1→t2
            isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T2 as u8, 0, 0),     // 5: a0=t2
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),                  // 6: ret
        ];
        let bytes = make_multi_task_atxe(vec![text], vec![(0, 0, vec![])]);
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut sched = Scheduler::from_atxe(&binary).unwrap();
        sched.run_all().unwrap();

        let results = sched.pool.results();
        // Find parent (task 0) and child (task 1)
        let parent = results.iter().find(|(id, _, _, _)| *id == 0).unwrap();
        let child = results.iter().find(|(id, _, _, _)| *id == 1).unwrap();
        assert_eq!(parent.1, TaskStatus::Done, "parent should be Done");
        assert_eq!(child.1, TaskStatus::Done, "child should be Done");
        assert_eq!(child.2, 77, "child return value");
        // Parent's return value should equal child's return value
        // (parent does MOV a0, t2 where t2 = TASK_JOIN return value)
        assert_eq!(parent.2, 77, "parent should read child's return value");
    }
}
