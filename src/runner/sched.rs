//! 任务调度器 — 多任务执行循环。
//!
//! 覆盖 P3-SCH-001 执行引擎主循环、P3-SCH-002 时间片与抢占、
//! P3-SCH-003 上下文切换。

use crate::base::ir::AtxeBinary;
use crate::base::isa::reg;
use crate::runner::VmState;
use crate::runner::batch::BatchManager;
use crate::runner::executor::Executor;
use crate::runner::loader::parse_task_section;
use crate::runner::pool::TaskPool;
use crate::runner::slot::SlotManager;
use crate::runner::task::{Task, TaskId, TaskStatus};

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
    /// 批次管理器。
    pub batch: BatchManager,
    /// 槽位管理器。
    pub slot_manager: SlotManager,
    /// 冷启动模式（N_batch 从 2 开始爬坡）。
    pub cold_start: bool,
    /// 冷启动计数器。
    pub cold_start_count: u32,
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
                vm: Some(vm),
                return_value: 0,
                total_instrs: 0,
                quantum_instrs: 0,
                join_waiting_for: None,
            };
            let mut batch = BatchManager::new(4.0, 1024.0);
            let n_batch = batch.compute_decision().n_batch;
            return Ok(Self {
                pool: TaskPool::new(vec![task]),
                quantum: 1000,
                total_instrs: 0,
                next_task_id: 1,
                batch,
                slot_manager: SlotManager::new(1024.0, n_batch.max(2), 0.15, 1.5),
                cold_start: true,
                cold_start_count: 0,
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
                vm: Some(vm),
                return_value: 0,
                total_instrs: 0,
                quantum_instrs: 0,
                join_waiting_for: None,
            });
        }

        let max_id = tasks.iter().map(|t| t.id).max().unwrap_or(0);
        let mut batch = BatchManager::new(4.0, 1024.0);
        let n_batch = batch.compute_decision().n_batch;
        Ok(Self {
            pool: TaskPool::new(tasks),
            quantum: 1000,
            total_instrs: 0,
            next_task_id: max_id + 1,
            batch,
            slot_manager: SlotManager::new(1024.0, n_batch.max(2), 0.15, 1.5),
            cold_start: true,
            cold_start_count: 0,
        })
    }

    /// 运行所有任务直到全部完成或出错。
    /// 使用依赖图层级调度：按拓扑层级从深到浅分批执行。
    pub fn run_all(&mut self) -> Result<(), String> {
        // 计算拓扑层级
        let levels = self.pool.compute_levels();
        if levels.is_empty() {
            return Ok(());
        }

        for (_level, task_ids) in &levels {
            // 激活当前层级所有任务
            self.pool.activate_ready_tasks();

            // 更新 N_batch
            let n_ready = task_ids.len() as f64;
            self.batch
                .set_pool_depth(n_ready + self.pool.len() as f64 * 0.5);
            let decision = self.batch.compute_decision();
            let n_batch = if self.cold_start {
                if self.cold_start_count >= 8 {
                    self.cold_start = false;
                    decision.n_batch as usize
                } else {
                    2
                }
            } else {
                decision.n_batch as usize
            };

            // 将当前层级所有任务设为 Ready
            for &id in task_ids {
                if let Some(task) = self.pool.get_mut(id)
                    && task.status == TaskStatus::Init
                {
                    task.status = TaskStatus::Ready;
                }
            }

            // 循环执行当前层级任务直到全部完成
            let level_complete = |pool: &TaskPool, ids: &[TaskId]| -> bool {
                ids.iter()
                    .all(|id| pool.get(*id).is_some_and(|t| t.status.is_terminal()))
            };

            while !level_complete(&self.pool, task_ids) {
                // 检查 OOM-Suspended 任务并扩容
                self.recover_oom_tasks();

                let ready = self.pool.ready_tasks();
                if ready.is_empty() {
                    if self.pool.has_suspended() {
                        continue;
                    }
                    break;
                }

                for task_id in ready.iter().take(n_batch) {
                    self.execute_quantum(*task_id);
                }
            }
        }

        // 处理动态创建的任务（TASK_FORK 产生的非层级任务）
        // 使用传统的 flat ready-task 循环
        loop {
            self.pool.activate_ready_tasks();
            self.recover_oom_tasks();
            if self.pool.all_done() {
                break;
            }
            let ready = self.pool.ready_tasks();
            if ready.is_empty() {
                if self.pool.has_suspended() {
                    continue;
                }
                break;
            }
            for task_id in ready.iter().take(4) {
                self.execute_quantum(*task_id);
            }
        }

        // 检查是否有任务出错
        for (id, status, _, _) in self.pool.results() {
            if status == TaskStatus::Error {
                let task = self.pool.get(id).unwrap();
                let msg = task
                    .vm
                    .as_ref()
                    .map(|vm| format!("{:?}", vm.state))
                    .unwrap_or_else(|| "unknown".into());
                return Err(format!("任务 {} 执行出错: {}", id, msg));
            }
        }

        Ok(())
    }

    /// 检查所有 Suspended 状态的任务，如果是因为 OOM（join_waiting_for == None），
    /// 则扩容内存后恢复为 Ready。
    fn recover_oom_tasks(&mut self) {
        // 收集所有 OOM-Suspended 任务的 ID
        let oom_tasks: Vec<TaskId> = self
            .pool
            .all_tasks()
            .iter()
            .filter(|t| t.status == TaskStatus::Suspended && t.join_waiting_for.is_none())
            .map(|t| t.id)
            .collect();

        for id in oom_tasks {
            if let Some(task) = self.pool.get_mut(id) {
                if let Some(ref mut vm) = task.vm {
                    // 扩容 1.5 倍
                    let old_size = vm.memory.data.len();
                    let new_size =
                        (old_size as f64 * 1.5).max((old_size as u64 + 8192) as f64) as usize;
                    vm.memory.data.resize(new_size, 0);
                    vm.memory.watermark_high = (new_size as u64) * 75 / 100;
                    vm.memory.usage = vm
                        .memory
                        .usage
                        .min((new_size as u64).saturating_sub(vm.memory.heap_base) * 50 / 100);
                    vm.state = crate::runner::VmStateKind::Running;
                }
                task.status = TaskStatus::Ready;
            }
        }
    }

    /// 执行一个任务的一个时间片。
    ///
    /// 内部使用 `Executor::load/run_quantum/unload` 模式。
    fn execute_quantum(&mut self, task_id: TaskId) {
        let pending_child = {
            let task = self.pool.get_mut(task_id).unwrap();
            if task.status != TaskStatus::Ready {
                return;
            }

            // 将任务加载到 Executor（vm 移入 executor）
            let mut executor = Executor::new(0);
            executor.load(task);

            // 执行一个时间片
            let (instr_count, _event) = executor.run_quantum(self.quantum);

            // 同步统计
            task.total_instrs += instr_count;
            task.quantum_instrs += instr_count;
            self.total_instrs += instr_count;

            // 取走 pending_child（TASK_FORK 产生）
            let child = executor.take_pending_child();

            // 卸回 VmState（自动同步 join_waiting_for 和任务状态）
            executor.unload(task);

            child
        }; // task borrow dropped here

        // 处理 pending_child（box deref）
        if let Some(child_vm) = pending_child {
            let child_id = child_vm.task_id;
            let new_task = Task {
                id: child_id,
                entry_offset: child_vm.pc,
                status: TaskStatus::Ready,
                deps: Vec::new(),
                vm: Some(*child_vm),
                return_value: 0,
                total_instrs: 0,
                quantum_instrs: 0,
                join_waiting_for: None,
            };
            self.pool.add_task(new_task);
            if child_id >= self.next_task_id {
                self.next_task_id = child_id.wrapping_add(1);
            }
        }

        // 处理完成任务（task 已被 unload，status 已更新）
        let (is_done, retval) = {
            let task = self.pool.get(task_id).unwrap();
            (task.status == TaskStatus::Done || task.status == TaskStatus::Error, task.return_value)
        };

        if is_done {
            self.pool.wake_joiners(task_id, retval);
            let wall_time_ms = (self.quantum as f64) * 0.001;
            self.batch.update_stats(wall_time_ms, 16.0, 0.0);
            if self.cold_start {
                self.cold_start_count += 1;
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
        let bytes = make_multi_task_atxe(vec![task0, task1], vec![(0, 0, vec![]), (1, 2, vec![0])]);
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
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 10), // 0: a0=10
            isa::encode_r1i(opcode::TASK_FORK, reg::T0 as u8, 1), // 1: fork id=1
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 20), // 2: a0=20
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0), // 3: ret
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
            isa::encode_r2i(opcode::MOVI, reg::T0 as u8, 0, 0), // 0: t0=0
            isa::encode_r1i(opcode::TASK_FORK, reg::T1 as u8, 1), // 1: fork id=1, handle→t1
            // child code (pc=2):
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 77), // 2: a0=77
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0), // 3: ret→77
            // parent resume (pc=4):
            isa::encode_r2i(opcode::TASK_JOIN, reg::T2 as u8, reg::T1 as u8, 0), // 4: join t1→t2
            isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T2 as u8, 0, 0),     // 5: a0=t2
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),                 // 6: ret
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

    // ── 层级调度测试 ──────────────────────────

    #[test]
    fn level_scheduling_three_levels() {
        // 3 层依赖的任务：
        // Level 0: task 0 (no deps), task 1 (no deps)
        // Level 1: task 2 (deps: 0, 1)
        // Level 2: task 3 (deps: 2)
        let t0 = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 10),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let t1 = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 20),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let t2 = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 30),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let t3 = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 40),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let bytes = make_multi_task_atxe(
            vec![t0, t1, t2, t3],
            vec![
                (0, 0, vec![]),
                (1, 2, vec![]),
                (2, 4, vec![0, 1]),
                (3, 6, vec![2]),
            ],
        );
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut sched = Scheduler::from_atxe(&binary).unwrap();
        sched.run_all().unwrap();

        let results = sched.pool.results();
        assert_eq!(results.len(), 4);
        for (_, status, _, _) in &results {
            assert_eq!(*status, TaskStatus::Done);
        }
        // Verify values
        assert_eq!(results.iter().find(|(id, ..)| *id == 0).unwrap().2, 10);
        assert_eq!(results.iter().find(|(id, ..)| *id == 1).unwrap().2, 20);
        assert_eq!(results.iter().find(|(id, ..)| *id == 2).unwrap().2, 30);
        assert_eq!(results.iter().find(|(id, ..)| *id == 3).unwrap().2, 40);
    }
}
