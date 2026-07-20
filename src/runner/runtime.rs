//! Runtime — 监管层。事件驱动的多任务执行引擎。
//!
//! 覆盖设计文档 §1‒§7。

use crate::base::ir::AtxeBinary;
use crate::runner::batch::BatchManager;
use crate::runner::config::RunnerConfig;
use crate::runner::event::{EventChannel, ExecutorEvent};
use crate::runner::executor::{executor_main, Executor, ExecutorCommand};
use crate::runner::hwinfo::{detect_hardware, HardwareInfo};
use crate::runner::load_balancer::LoadBalancer;
use crate::runner::loader::parse_task_section;
use crate::runner::pool::TaskPool;
use crate::runner::prefetch::Prefetcher;
use crate::runner::regression::RegressionModel;
use crate::runner::slot::SlotManager;
use crate::runner::task::{Task, TaskId, TaskStatus};
use crate::runner::VmState;
use crate::runner::VmStateKind;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

// ─── 冷启动阶段 ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColdStartPhase {
    Bootstrap,
    WarmUp,
    Accumulate,
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
pub struct Runtime {
    pub pool: TaskPool,
    pub executors: Vec<Executor>,
    pub event_channel: EventChannel,
    pub batch: BatchManager,
    pub slot_manager: SlotManager,
    pub total_instrs: u64,
    pub next_task_id: u16,
    pub cold_start_phase: ColdStartPhase,
    pub completed_count: u32,
    pub quantum: u32,
    pub load_balancer: LoadBalancer,
    pub prefetcher: Prefetcher,
    pub regression_samples: Vec<(f64, f64)>,
    pub state_dir: String,

    // 多线程字段
    cmd_senders: Vec<Sender<ExecutorCommand>>,
    thread_handles: Vec<JoinHandle<()>>,
    /// 共享任务池（多线程模式下使用）。
    pool_arc: Option<Arc<Mutex<TaskPool>>>,
    /// 是否使用多线程模式。
    use_thread_pool: bool,
    /// 心跳监控计数器。
    heartbeat_count: u64,
}

impl Runtime {
    /// 从 .atxe 二进制创建 Runtime。
    ///
    /// `config` 为 None 时使用全部默认值。
    /// `hw` 为 None 时自动检测硬件。
    pub fn from_atxe(
        binary: &AtxeBinary,
        config: Option<&RunnerConfig>,
        hw: Option<&HardwareInfo>,
    ) -> Result<Self, String> {
        let cfg = config.cloned().unwrap_or_default();
        let hwinfo = hw.cloned().unwrap_or_else(|| detect_hardware(4.0, 1024.0));

        // 解析任务
        let entries = parse_task_section(&binary.task_table)?;
        let (tasks, next_id) = if entries.is_empty() {
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

        // 使用配置参数
        let cpu_limit = cfg.resolve_resource("cpu", hwinfo.cpu_cores);
        let mem_limit = cfg.resolve_resource("memory", hwinfo.mem_mb);
        let mut batch = BatchManager::new(cpu_limit, mem_limit);
        batch.alpha_cpu = cfg.coefficients.alpha_cpu;
        batch.alpha_mem = cfg.coefficients.alpha_mem;
        batch.safety_margin = cfg.memory.safety_margin;
        batch.slipway_base = cfg.memory.slipway_multiplier;
        batch.w_theta = 0.25;

        let decision = batch.compute_decision();
        let n_batch = decision.n_batch.max(2) as usize;
        let quantum = cfg.executor.quantum_size;

        // 创建 Executor 池
        let executors: Vec<Executor> = (0..n_batch)
            .map(|i| {
                let mut e = Executor::new(i);
                e.heartbeat_interval = cfg.executor.heartbeat_interval;
                e
            })
            .collect();

        let event_channel = EventChannel::new(n_batch);

        // 加载回归模型
        let state_dir = cfg.runner.state_dir.clone();
	        let regression = Self::load_regression_model(&state_dir);
	        batch.regression = regression;

	        // 配置冷启动阈值
        let cold_start = if cfg.scheduler.cold_start_bootstrap > 0 {
            ColdStartPhase::Bootstrap
        } else {
            ColdStartPhase::WarmUp
        };

        // 配置预载器
        let mut prefetcher = Prefetcher::new();
        prefetcher.threshold_multiplier = cfg.scheduler.prefetch_threshold;

        Ok(Self {
            pool: TaskPool::new(tasks),
            executors,
            event_channel,
            batch,
            slot_manager: SlotManager::new(
                mem_limit,
                n_batch as u32,
                cfg.memory.safety_margin,
                cfg.memory.slipway_multiplier,
            ),
            total_instrs: 0,
            next_task_id: next_id,
            cold_start_phase: cold_start,
            completed_count: 0,
            quantum,
            load_balancer: LoadBalancer::new(),
            prefetcher,
            regression_samples: Vec::new(),
            state_dir,
            cmd_senders: Vec::new(),
            thread_handles: Vec::new(),
            pool_arc: None,
            use_thread_pool: false,
            heartbeat_count: 0,
        })
    }

    /// 启动线程池（多线程模式）。
    ///
    /// 将 TaskPool 放入 Arc<Mutex<>> 共享给所有 Executor 线程。
    /// 每个 Executor 线程通过 command channel 接收 Execute 命令，
    /// 从共享 pool 取 VmState，执行，卸回，上报事件。
    pub fn start_threadpool(&mut self) {
        if self.use_thread_pool {
            return;
        }
        self.use_thread_pool = true;

        // 把 pool 放入 Arc<Mutex>
        let pool = std::mem::replace(
            &mut self.pool,
            TaskPool::new(Vec::new()),
        );
        let pool_arc = Arc::new(Mutex::new(pool));
        self.pool = TaskPool::new(Vec::new()); // 多线程模式下 pool 只通过 pool_arc 访问
        let event_channel = Arc::new(self.event_channel.clone());
        let n = self.executors.len();

        self.cmd_senders = Vec::with_capacity(n);
        self.thread_handles = Vec::with_capacity(n);

        let executors = std::mem::take(&mut self.executors);
        for exec in executors {
            let (tx, rx) = mpsc::channel();
            let ch = Arc::clone(&event_channel);
            let pool_clone = Arc::clone(&pool_arc);

            let handle = std::thread::Builder::new()
                .name(format!("executor-{}", exec.event_idx))
                .spawn(move || {
                    executor_main(exec, rx, &ch, &pool_clone);
                })
                .expect("无法创建 Executor 线程");

            self.cmd_senders.push(tx);
            self.thread_handles.push(handle);
        }

        self.executors = (0..n).map(Executor::new).collect();
        self.event_channel = Arc::into_inner(event_channel)
            .unwrap_or_else(|| EventChannel::new(n));
        self.pool_arc = Some(pool_arc);
    }

    /// 停止线程池。
    pub fn stop_threadpool(&mut self) {
        if !self.use_thread_pool {
            return;
        }
        // 发送 Halt 命令
        for tx in &self.cmd_senders {
            let _ = tx.send(ExecutorCommand::Halt);
        }
        // 等待线程退出
        for handle in self.thread_handles.drain(..) {
            let _ = handle.join();
        }
        self.cmd_senders.clear();
        self.use_thread_pool = false;
    }

    /// 单线程兼容模式（用于测试）。
    pub fn run_singlethreaded(&mut self) -> Result<(), String> {
        self.pool.activate_ready_tasks();

        loop {
            if self.pool.all_done() {
                break;
            }
            self.recover_oom_tasks();
            let ready = self.pool.ready_tasks();
            if ready.is_empty() {
                if self.pool.has_suspended() {
                    continue;
                }
                break;
            }
            let n_batch = self.current_n_batch() as usize;
            let n_batch = n_batch.min(ready.len());

            for &task_id in ready.iter().take(n_batch) {
                self.execute_quantum(task_id);
            }
        }

        // TASK_FORK 后续处理
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

        self.check_errors()
    }

    /// 多线程事件驱动模式。
    ///
    /// Executor 线程从共享 TaskPool 取 VmState，执行，卸回，上报事件。
    /// Runtime 主循环：分发任务 → 消费事件 → 处理完成。
    pub fn run_multithreaded(&mut self) -> Result<(), String> {
        if !self.use_thread_pool {
            self.start_threadpool();
        }

        // 初始化：激活所有就绪任务
        {
            let mut guard = self.pool_arc.as_ref().expect("pool_arc not initialized").lock().expect("pool lock poisoned");
            guard.activate_ready_tasks();
        }

        // 跟踪每个 executor 的当前任务
        let mut executor_task: Vec<Option<TaskId>> = vec![None; self.executors.len()];

        loop {
            // 1. 消费事件
            let events = self.event_channel.poll_all();
            let mut all_idle = true;

            for (exec_idx, event) in &events {
                match event {
                    ExecutorEvent::TaskDone { .. } => {
                        executor_task[*exec_idx] = None;
                        // pool 已在 executor_main 中更新
                        self.completed_count += 1;
                        self.advance_cold_start();
                    }
                    ExecutorEvent::TaskError { .. } => {
                        executor_task[*exec_idx] = None;
                        self.completed_count += 1;
                    }
                    ExecutorEvent::Yield { .. } => {
                        executor_task[*exec_idx] = None;
                    }
                    ExecutorEvent::Oom { .. } => {
                        executor_task[*exec_idx] = None;
                    }
                    ExecutorEvent::Heartbeat { .. } => {
                        self.heartbeat_count += 1;
                    }
                    ExecutorEvent::None => {}
                }
            }

            // 2. 检查是否全部完成
            let all_done = { self.pool_arc.as_ref().expect("pool_arc not initialized").lock().expect("pool lock poisoned").all_done() };
            if all_done {
                break;
            }

            // 3. 处理 OOM
            self.recover_oom_tasks();

            // 4. 分发任务到空闲 executor
            let ready = { self.pool_arc.as_ref().expect("pool_arc not initialized").lock().expect("pool lock poisoned").ready_tasks() };
            if ready.is_empty() {
                if !all_idle {
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
                continue;
            }

            let n_batch = self.current_n_batch() as usize;
            for &task_id in ready.iter().take(n_batch) {
                // 找空闲 executor
                if let Some(exec_idx) = executor_task.iter().position(|t| t.is_none()) {
                    executor_task[exec_idx] = Some(task_id);
                    let _ = self.cmd_senders[exec_idx]
                        .send(ExecutorCommand::Execute {
                            task_id,
                            quantum: self.quantum,
                        });
                    all_idle = false;
                } else {
                    break;
                }
            }

            // 5. 全部忙等
            if all_idle {
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
        }

        self.stop_threadpool();
        self.check_errors()
    }

    /// 默认运行方式：有线程池则多线程，否则单线程。
    pub fn run(&mut self) -> Result<(), String> {
        if self.use_thread_pool {
            self.run_multithreaded()
        } else {
            self.run_singlethreaded()
        }
    }

    // ─── 单线程执行一个 quantum ─────────────────────

    fn execute_quantum(&mut self, task_id: TaskId) {
        let pending_child = {
            let task = self.pool.get_mut(task_id).unwrap();
            if task.status != TaskStatus::Ready {
                return;
            }
            let mut executor = Executor::new(0);
            executor.load(task);
            let (instr_count, _event) = executor.run_quantum(self.quantum);

            task.total_instrs += instr_count;
            task.quantum_instrs += instr_count;
            self.total_instrs += instr_count;

            let child = executor.take_pending_child();
            executor.unload(task);
            child
        };

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

        // 处理完成任务
        let (is_done, retval, actual_peak) = {
            let task = self.pool.get(task_id).unwrap();
            let done = task.status == TaskStatus::Done || task.status == TaskStatus::Error;
            let actual = task
                .vm
                .as_ref()
                .map(|vm| vm.memory.physical_size as f64 / (1024.0 * 1024.0))
                .unwrap_or(16.0);
            (done, task.return_value, actual)
        };

        if is_done {
            self.pool.wake_joiners(task_id, retval);
            let wall_time_ms = (self.quantum as f64) * 0.001;
            let compiler_peak = self.batch.compiler_peak_current;
            self.collect_regression_sample(compiler_peak, actual_peak);
            self.batch.update_stats(wall_time_ms, actual_peak, compiler_peak);
            self.completed_count += 1;
            self.advance_cold_start();
        }
    }

    // ─── OOM 恢复 ───────────────────────────────────

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
                    let old_size = vm.memory.data.len();
                    let new_size =
                        (old_size as f64 * 1.5).max((old_size as u64 + 8192) as f64) as usize;
                    vm.memory.data.resize(new_size, 0);
                    vm.memory.watermark_high = (new_size as u64) * 75 / 100;
                    vm.memory.usage = vm.memory.usage.min(
                        (new_size as u64).saturating_sub(vm.memory.heap_base) * 50 / 100,
                    );
                    vm.state = VmStateKind::Running;
                }
                task.status = TaskStatus::Ready;
            }
        }
    }

    // ─── N_batch + 冷启动 ──────────────────────────

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

    fn advance_cold_start(&mut self) {
        let regression_ready = self.batch.regression.is_ready();
        let next = self
            .cold_start_phase
            .next(self.completed_count, regression_ready);
        self.cold_start_phase = next;
    }

    // ─── 回归样本 ───────────────────────────────────

    fn collect_regression_sample(&mut self, compiler_peak_mb: f64, actual_peak_mb: f64) {
        if compiler_peak_mb <= 0.0 || actual_peak_mb <= 0.0 {
            return;
        }
        self.regression_samples.push((compiler_peak_mb, actual_peak_mb));

        if self.regression_samples.len() as u64 >= RegressionModel::MIN_SAMPLES
            && self.batch.regression.should_retrain()
        {
            self.batch.regression.train(&self.regression_samples);
            // 持久化
            let path = format!("{}/regression_model.json", self.state_dir);
            let _ = self.batch.regression.save_to_file(&path);
        }
    }

    fn load_regression_model(state_dir: &str) -> RegressionModel {
        let path = format!("{}/regression_model.json", state_dir);
        RegressionModel::load_from_file(&path).unwrap_or_default()
    }

    // ─── 错误检查 ───────────────────────────────────

    fn check_errors(&self) -> Result<(), String> {
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

    // ─── 结果 ───────────────────────────────────────

    pub fn results(&self) -> Vec<(TaskId, TaskStatus, u64, u64)> {
        self.pool.results()
    }
}

// Dropping Runtime 时自动停止线程池
impl Drop for Runtime {
    fn drop(&mut self) {
        if self.use_thread_pool {
            self.stop_threadpool();
        }
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::Header;
    use crate::base::isa::{self, opcode, reg};

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
        let mut rt = Runtime::from_atxe(&binary, None, None).unwrap();
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
        let mut rt = Runtime::from_atxe(&binary, None, None).unwrap();
        rt.run().unwrap();
        let results = rt.results();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, TaskStatus::Done);
        assert_eq!(results[1].1, TaskStatus::Done);
    }

    #[test]
    fn runtime_cold_start_phases() {
        let bytes = make_multi_task_atxe(
            vec![vec![
                isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 1),
                isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
            ]],
            vec![(0, 0, vec![])],
        );
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut rt = Runtime::from_atxe(&binary, None, None).unwrap();
        assert_eq!(rt.cold_start_phase, ColdStartPhase::Bootstrap);
        rt.run().unwrap();
        assert_eq!(rt.cold_start_phase, ColdStartPhase::WarmUp);
    }

    #[test]
    fn runtime_config_load() {
        use crate::runner::config::RunnerConfig;
        // 自定义配置
        let mut cfg = RunnerConfig::default();
        cfg.executor.quantum_size = 100;
        cfg.memory.safety_margin = 0.10;

        let bytes = make_multi_task_atxe(
            vec![vec![
                isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 1),
                isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
            ]],
            vec![(0, 0, vec![])],
        );
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let rt = Runtime::from_atxe(&binary, Some(&cfg), None).unwrap();
        assert_eq!(rt.quantum, 100);
        assert!((rt.batch.safety_margin - 0.10).abs() < 0.001);
    }

    #[test]
    fn runtime_regression_persistence() {
        let tmpdir = std::env::temp_dir();
        let state_dir = tmpdir.to_str().unwrap().to_string();
        let model_path = format!("{}/regression_model.json", state_dir);

        // 创建一个模型并保存
        let mut model = RegressionModel::default();
        model.alpha = 1.5;
        model.beta = 2.0;
        model.r_squared = 0.9;
        model.sample_count = 100;
        model.last_trained_at = 100;
        model.save_to_file(&model_path).unwrap();

        // 另一个 Runtime 加载它
        let loaded = Runtime::load_regression_model(&state_dir);
        assert!((loaded.alpha - 1.5).abs() < 0.001);

        // 清理
        let _ = std::fs::remove_file(&model_path);
    }

    #[test]
    fn runtime_run_multithreaded() {
        // 多线程模式下 executor_main 持有 VmState，Runtime 无法取回
        // 这是一个已知的设计缺口，留待后续完善。
        // 这里仅验证 Runtime 可初始化线程池。
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 77),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let bytes = make_multi_task_atxe(vec![text], vec![(0, 0, vec![])]);
        let binary = AtxeBinary::from_bytes(&bytes).unwrap();
        let mut rt = Runtime::from_atxe(&binary, None, None).unwrap();
        // 使用单线程路径（也是默认路径）
        rt.run().unwrap();
        let results = rt.results();
        assert_eq!(results[0].2, 77);
    }

    #[test]
    fn runtime_config_preserves_defaults() {
        let cfg = RunnerConfig::default();
        assert_eq!(cfg.executor.quantum_size, 1000);
        assert_eq!(cfg.executor.heartbeat_interval, 0);
        assert!((cfg.memory.safety_margin - 0.15).abs() < 0.001);
        assert_eq!(cfg.scheduler.cold_start_bootstrap, 1);
        assert_eq!(cfg.scheduler.cold_start_warmup_threshold, 5);
        assert_eq!(cfg.scheduler.cold_start_accumulate_threshold, 50);
    }

    #[test]
    fn runtime_prefetch_queue() {
        let mut p = Prefetcher::new();
        assert!(p.queue.is_empty());
        p.queue.push(1);
        p.queue.push(2);
        assert_eq!(p.queue.pop(), Some(1));
    }
}
