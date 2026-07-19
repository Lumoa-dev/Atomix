//! Runtime — 监管层。事件驱动的多任务执行引擎。
//!
//! 覆盖设计文档 §1‒§7。
//!
//! Runtime 是 Runner 的核心，负责任务池管理、N_batch 决策、
//! 内存槽位分配、事件处理、回归模型维护。
//!
//! ## 组件关系
//!
//! | 层 | 组件 | 线程归属 |
//! |:---|:-----|:---------|
//! | 监管层 | Runtime | 主线程 |
//! | 执行层 | Executor 线程池（N_batch 个） | 每 Executor 一个线程 |
//! | 存储层 | 磁盘仓库 | 独立 I/O 线程 |

use crate::base::ir::AtxeBinary;
use crate::base::isa::reg;
use crate::runner::batch::BatchManager;
use crate::runner::event::EventChannel;
use crate::runner::executor::Executor;
use crate::runner::loader::parse_task_section;
use crate::runner::pool::TaskPool;
use crate::runner::slot::SlotManager;
use crate::runner::task::{Task, TaskId, TaskStatus};
use crate::runner::VmState;
use crate::runner::VmStateKind;
use crate::runner::load_balancer::LoadBalancer;
use crate::runner::prefetch::Prefetcher;
use crate::runner::regression::RegressionModel;

// ─── 冷启动阶段 ─────────────────────────────────────

/// 冷启动阶段状态机（设计文档 §7.4）。
///
/// | 阶段 | 条件 | N_batch 上限 | MEM 估计 |
/// |:-----|:-----|:-------------|:---------|
/// | Bootstrap | 首任务 | 1 | compiler_peak × 1.5 |
/// | WarmUp | 前 5 个 | min(2, H) | δ × compiler_peak |
/// | Accumulate | 5~50 个 | min(H, S) | δ × compiler_peak |
/// | Stable | ≥50 个 | min(H, S) | 回归修正值 |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColdStartPhase {
    /// Phase 0: 第 1 个任务。
    Bootstrap,
    /// Phase 1: 2~5 个任务。
    WarmUp,
    /// Phase 2: 6~49 个任务。
    Accumulate,
    /// Phase 3: ≥50 个任务且回归就绪。
    Stable,
}

impl ColdStartPhase {
    pub fn next(&self, completed_count: u32, regression_ready: bool) -> Self {
        match self {
            ColdStartPhase::Bootstrap => {
                if completed_count >= 1 {
                    ColdStartPhase::WarmUp
                } else {
                    ColdStartPhase::Bootstrap
                }
            }
            ColdStartPhase::WarmUp => {
                if completed_count >= 5 {
                    ColdStartPhase::Accumulate
                } else {
                    ColdStartPhase::WarmUp
                }
            }
            ColdStartPhase::Accumulate => {
                if completed_count >= 50 && regression_ready {
                    ColdStartPhase::Stable
                } else {
                    ColdStartPhase::Accumulate
                }
            }
            ColdStartPhase::Stable => ColdStartPhase::Stable,
        }
    }
}

// ─── Runtime ─────────────────────────────────────────

/// Runtime — Atomix Runner 的监管层。
///
/// 驱动 Executor 线程池、处理事件、分配任务、管理内存。
pub struct Runtime {
    /// 任务池。
    pub pool: TaskPool,
    /// Executor 列表（N_batch 个）。
    pub executors: Vec<Executor>,
    /// 事件通道。
    pub event_channel: EventChannel,
    /// 批次管理器。
    pub batch: BatchManager,
    /// 槽位管理器。
    pub slot_manager: SlotManager,
    /// 已执行的总指令数。
    pub total_instrs: u64,
    /// 下一个可用的 task_id（TASK_FORK 分配用）。
    pub next_task_id: u16,
    /// 冷启动阶段。
    pub cold_start_phase: ColdStartPhase,
    /// 已完成任务计数。
    pub completed_count: u32,
    /// 时间片大小（指令数）。
    pub quantum: u32,
    /// 负载均衡器。
    pub load_balancer: LoadBalancer,
    /// 预载调度器。
    pub prefetcher: Prefetcher,
    /// 回归样本：(compiler_peak_mb, actual_peak_mb)
    pub regression_samples: Vec<(f64, f64)>,
}

impl Runtime {
    /// 从 .atxe 二进制创建 Runtime。
    pub fn from_atxe(binary: &AtxeBinary) -> Result<Self, String> {
        let entries = parse_task_section(&binary.task_table)?;
        let (tasks, next_id) = if entries.is_empty() {
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
            (vec![task], 1u16)
        } else {
            let mut tasks = Vec::with_capacity(entries.len());
            for entry in &entries {
                let mut vm = VmState::from_atxe(binary)?;
                vm.pc = entry.entry_offset as usize;
                vm.task_id = entry.task_id;

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
            (tasks, max_id + 1)
        };

        let mut batch = BatchManager::new(4.0, 1024.0);
        let decision = batch.compute_decision();
        let n_batch = decision.n_batch.max(2) as usize;

        // 创建 Executor 池（Phase 2: 轻量 Executor，不持有 VmState）
        let executors: Vec<Executor> = (0..n_batch).map(Executor::new).collect();
        let event_channel = EventChannel::new(n_batch);

        Ok(Self {
            pool: TaskPool::new(tasks),
            executors,
            event_channel,
            batch,
            slot_manager: SlotManager::new(1024.0, n_batch as u32, 0.15, 1.5),
            total_instrs: 0,
            next_task_id: next_id,
            cold_start_phase: ColdStartPhase::Bootstrap,
            completed_count: 0,
            quantum: 1000,
            load_balancer: LoadBalancer::new(),
            prefetcher: Prefetcher::new(),
            regression_samples: Vec::new(),
        })
    }

    /// 运行所有任务直到全部完成或出错。
    ///
    /// 使用事件驱动循环：
    /// 1. 轮询所有 Executor 事件
    /// 2. 处理事件（任务完成 → 唤醒 joiners, OOM → 扩容）
    /// 3. 分配就绪任务给空闲 Executor
    /// 4. 无事件时处理非事件任务（预载、死区合并等）
    pub fn run(&mut self) -> Result<(), String> {
        self.pool.activate_ready_tasks();

        // 主循环
        loop {
            // 1. 检查是否全部完成
            if self.pool.all_done() {
                break;
            }

            // 2. 处理 OOM-Suspended 任务
            self.recover_oom_tasks();

            // 3. 获取就绪任务
            let ready = self.pool.ready_tasks();
            if ready.is_empty() {
                if self.pool.has_suspended() {
                    continue;
                }
                break;
            }

            // 4. 计算 N_batch（含冷启动）
            let n_batch = self.current_n_batch() as usize;
            let n_batch = n_batch.min(ready.len());

            // 5. 使用 LoadBalancer 分配任务到 Executor
            let exec_loads = self
                .executors
                .iter()
                .map(|e| crate::runner::load_balancer::ExecutorLoad {
                    idx: e.event_idx,
                    load: e.stats.total_instrs as f64,
                    idle: e.is_idle(),
                })
                .collect::<Vec<_>>();

            let assignment = self
                .load_balancer
                .assign(&ready[..n_batch], &exec_loads, self.executors.len());

            // 6. 执行分配的 quantum
            //    当前为单线程兼容模式，逐个执行
            for (_, exec_idx) in assignment.iter().take(n_batch) {
                // 按分配结果执行（当前简化：直接执行任务）
                let task_id = ready[*exec_idx % ready.len()];
                self.execute_quantum(task_id);
            }
        }

        // 处理动态创建的任务（TASK_FORK 产生的非层级任务）
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

    /// 执行一个任务的一个时间片。
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
        };

        // 处理 pending_child
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
        let (is_done, retval, actual_peak, compiler_peak) = {
            let task = self.pool.get(task_id).unwrap();
            let done = task.status == TaskStatus::Done || task.status == TaskStatus::Error;
            let actual = task
                .vm
                .as_ref()
                .map(|vm| vm.memory.physical_size as f64 / (1024.0 * 1024.0))
                .unwrap_or(16.0);
            let compiler = self.batch.compiler_peak_current;
            (done, task.return_value, actual, compiler)
        };

        if is_done {
            self.pool.wake_joiners(task_id, retval);
            let wall_time_ms = (self.quantum as f64) * 0.001;
            self.collect_regression_sample(compiler_peak, actual_peak);
            self.batch.update_stats(wall_time_ms, actual_peak, compiler_peak);
            self.completed_count += 1;
            self.advance_cold_start();
        }
    }

    /// 收集回归样本并触发训练。
    fn collect_regression_sample(&mut self, compiler_peak_mb: f64, actual_peak_mb: f64) {
        if compiler_peak_mb <= 0.0 || actual_peak_mb <= 0.0 {
            return;
        }
        self.regression_samples.push((compiler_peak_mb, actual_peak_mb));

        // 检查是否需要训练
        if self.regression_samples.len() as u64 >= RegressionModel::min_samples()
            && self.batch.regression.should_retrain()
        {
            self.batch.regression.train(&self.regression_samples);
        }
    }

    /// 检查并恢复 OOM-Suspended 任务。
    fn recover_oom_tasks(&mut self) {
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
                    vm.state = VmStateKind::Running;
                }
                task.status = TaskStatus::Ready;
            }
        }
    }

    /// 计算当前 N_batch（考虑冷启动阶段）。
    fn current_n_batch(&mut self) -> u32 {
        let h = self.batch.compute_hard_ceiling() as u32;
        match self.cold_start_phase {
            ColdStartPhase::Bootstrap => 1u32.min(h),
            ColdStartPhase::WarmUp => 2u32.min(h),
            ColdStartPhase::Accumulate | ColdStartPhase::Stable => {
                let decision = self.batch.compute_decision();
                decision.n_batch.min(h.max(1))
            }
        }
    }

    /// 推进冷启动阶段。
    fn advance_cold_start(&mut self) {
        let regression_ready = false; // Phase 4 会接入真实回归模型
        let next = self
            .cold_start_phase
            .next(self.completed_count, regression_ready);
        self.cold_start_phase = next;
    }

    /// 获取任务执行结果。
    pub fn results(&self) -> Vec<(TaskId, TaskStatus, u64, u64)> {
        self.pool.results()
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::Header;
    use crate::base::isa::{self, opcode};

    fn make_multi_task_atxe(
        texts: Vec<Vec<u32>>,
        entries: Vec<(u16, u32, Vec<u16>)>,
    ) -> Vec<u8> {
        let mut all_text = Vec::new();
        for t in &texts {
            all_text.extend_from_slice(t);
        }

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
    fn runtime_run_single_task() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let bytes = make_multi_task_atxe(vec![text], vec![(0, 0, vec![])]);
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut rt = Runtime::from_atxe(&binary).unwrap();
        rt.run().unwrap();

        let results = rt.results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, TaskStatus::Done);
        assert_eq!(results[0].2, 42);
    }

    #[test]
    fn runtime_two_tasks_sequential() {
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
        let mut rt = Runtime::from_atxe(&binary).unwrap();
        rt.run().unwrap();

        let results = rt.results();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, TaskStatus::Done);
        assert_eq!(results[1].1, TaskStatus::Done);
        assert_eq!(results[0].2, 10);
        assert_eq!(results[1].2, 20);
    }

    #[test]
    fn runtime_cold_start_phases() {
        let mut rt = Runtime::from_atxe(
            &AtxeBinary::from_bytes(&make_multi_task_atxe(
                vec![vec![
                    isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 1),
                    isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
                ]],
                vec![(0, 0, vec![])],
            )).unwrap()
        ).unwrap();

        assert_eq!(rt.cold_start_phase, ColdStartPhase::Bootstrap);
        rt.run().unwrap();
        // 完成 1 个任务后应进入 WarmUp
        assert_eq!(rt.cold_start_phase, ColdStartPhase::WarmUp);
    }

    #[test]
    fn runtime_cold_start_accumulate() {
        // 创建足够多的任务来推进到 Accumulate
        let mut texts = Vec::new();
        let mut entries = Vec::new();
        for i in 0..8 {
            texts.push(vec![
                isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, i as u16),
                isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
            ]);
            entries.push((i, i as u32 * 2, vec![]));
        }
        let bytes = make_multi_task_atxe(texts, entries);
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut rt = Runtime::from_atxe(&binary).unwrap();
        assert_eq!(rt.cold_start_phase, ColdStartPhase::Bootstrap);
        rt.run().unwrap();
        // 完成 8 个 → 应进入 Accumulate
        assert_eq!(rt.cold_start_phase, ColdStartPhase::Accumulate);
    }

    #[test]
    fn runtime_current_n_batch() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 0),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let bytes = make_multi_task_atxe(vec![text], vec![(0, 0, vec![])]);
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut rt = Runtime::from_atxe(&binary).unwrap();

        // Bootstrap: N_batch 应为 1
        rt.cold_start_phase = ColdStartPhase::Bootstrap;
        assert_eq!(rt.current_n_batch(), 1);

        // WarmUp: N_batch 应为 2
        rt.cold_start_phase = ColdStartPhase::WarmUp;
        assert_eq!(rt.current_n_batch(), 2);

        // Accumulate: N_batch 来自 BatchManager
        rt.cold_start_phase = ColdStartPhase::Accumulate;
        let nb = rt.current_n_batch();
        assert!(nb >= 1, "N_batch should be >= 1, got {}", nb);
    }
}
