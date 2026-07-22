//! 调试会话抽象 — DebugSession trait 与 LocalDebugSession 实现。
//!
//! 对应设计文档 §6.1「模块复用」和 §6.2「数据流」。
//!
//! DebugSession trait 定义了调试会话的统一接口：
//! - `LocalDebugSession`：本地调试（VM 进程内）
//! - `RemoteDebugSession`：远程调试（通过 ATXP 协议，待实现）

use crate::base::ir::SectionEntry;
use crate::base::isa::{self, opcode, reg};
use crate::debug::debug_segment::DebugMap;
use crate::debug::disassemble;
use crate::debug::eval;
use crate::debug::trace::{ExecutionPhase, ExecutionTrace, IS_VARIABLES, TraceCollector};
use crate::runner::VmState;
use crate::runner::VmStateKind;
use crate::runner::decode;
use crate::runner::execute::execute_instruction;

use std::collections::HashMap;
use std::time::Instant;

// ─── 数据监视点 ──────────────────────────────────────────

/// 内存数据监视点。
#[derive(Debug, Clone)]
pub struct Watchpoint {
    pub addr: u64,
    pub size: u64,
    pub label: String,
    pub hit_count: u64,
}

// ─── 断点类型 ──────────────────────────────────────────────

/// 断点类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakpointType {
    /// PC 地址断点。
    Pc(usize),
    /// 源码行号断点。
    Line(u32),
    /// 函数路径断点。
    Function(String),
    /// 钩子断点。
    Hook(String),
}

/// 断点定义。
#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub id: u64,
    pub bp_type: BreakpointType,
    pub condition: Option<String>,
    pub hit_count: u64,
    pub enabled: bool,
    pub original_instr: Option<u32>,
}

// ─── 命令历史 ──────────────────────────────────────────────

/// 命令历史记录。
#[derive(Debug, Clone)]
pub struct CommandHistory {
    entries: Vec<String>,
    max_entries: usize,
}

impl CommandHistory {
    pub fn new(max: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max),
            max_entries: max,
        }
    }

    pub fn push(&mut self, cmd: String) {
        if self.entries.last().map_or(false, |last| *last == cmd) {
            return; // 去重连续相同命令
        }
        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(cmd);
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        if index == 0 || index > self.entries.len() {
            return None;
        }
        self.entries
            .get(self.entries.len() - index)
            .map(|s| s.as_str())
    }

    pub fn all(&self) -> &[String] {
        &self.entries
    }

    pub fn last_n(&self, n: usize) -> Vec<&str> {
        let len = self.entries.len();
        let start = len.saturating_sub(n);
        self.entries[start..].iter().map(|s| s.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─── 帧选择状态 ──────────────────────────────────────────

/// 调用栈帧选择状态。
#[derive(Debug, Clone)]
pub struct FrameState {
    /// 当前选中的帧索引（0 = 最内层/当前执行）。
    pub selected: usize,
}

impl FrameState {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    /// 获取当前帧在 call_stack 中的索引（0 表示当前执行帧）。
    pub fn current_index(&self) -> usize {
        self.selected
    }

    /// 上移一帧（向内层）。
    pub fn up(&mut self, max_frames: usize) {
        if self.selected < max_frames {
            self.selected += 1;
        }
    }

    /// 下移一帧（向外层）。
    pub fn down(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn set(&mut self, n: usize, max_frames: usize) {
        self.selected = n.min(max_frames);
    }
}

// ─── 显示格式 ──────────────────────────────────────────────

/// 数值显示格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayFormat {
    Hex,
    Dec,
    Both,
}

// ─── 性能计数器 ──────────────────────────────────────────

/// 性能统计计数器。
#[derive(Debug, Clone)]
pub struct PerfCounters {
    /// 各 opcode 的执行次数。
    pub opcode_counts: [u64; 256],
    /// 各 opcode 分类的执行次数。
    pub arith_count: u64,
    pub mem_count: u64,
    pub ctrl_count: u64,
    pub system_count: u64,
    /// 各 PC 地址的执行次数（hot path）。
    pub pc_hits: HashMap<usize, u64>,
    /// 各 Step 的耗时。
    pub step_times: Vec<(String, u64)>,
    /// 分配/释放次数。
    pub alloc_count: u64,
    pub free_count: u64,
    /// 峰值内存用量。
    pub peak_memory: u64,
    /// 指令总执行数。
    pub total_instructions: u64,
}

impl Default for PerfCounters {
    fn default() -> Self {
        Self {
            opcode_counts: [0u64; 256],
            arith_count: 0,
            mem_count: 0,
            ctrl_count: 0,
            system_count: 0,
            pc_hits: HashMap::new(),
            step_times: Vec::new(),
            alloc_count: 0,
            free_count: 0,
            peak_memory: 0,
            total_instructions: 0,
        }
    }
}

// ─── 调试会话 trait ──────────────────────────────────────

/// 调试会话的统一接口。
///
/// 提供本地和远程调试器的共同行为。
pub trait DebugSession {
    /// 获取 VM 状态引用。
    fn vm(&self) -> &VmState;
    /// 获取 VM 状态可变引用。
    fn vm_mut(&mut self) -> &mut VmState;
    /// 获取执行轨迹。
    fn trace(&self) -> &ExecutionTrace;
    /// 获取执行轨迹可变引用。
    fn trace_mut(&mut self) -> &mut ExecutionTrace;
    /// 获取调试映射。
    fn debug_map(&self) -> Option<&DebugMap>;
    /// 获取源码行。
    fn source_lines(&self) -> &[String];
    /// 获取源码路径。
    fn source_path(&self) -> Option<&str>;

    // ─── 执行控制 ──────────────────────────────────────

    /// 单步执行 n 条指令。
    fn step_instructions(&mut self, n: usize);
    /// 运行到下一个断点或任务结束。
    fn continue_execution(&mut self);
    /// 运行到下一个 Step（高级 Step 边界）。
    fn step_over(&mut self);
    /// 进入当前 Step 内部。
    fn step_into(&mut self);
    /// 跳出当前 Step。
    fn step_out(&mut self);

    // ─── 断点管理 ──────────────────────────────────────

    /// 设置 PC 断点。
    fn set_breakpoint_pc(&mut self, addr: usize, condition: Option<&str>) -> u64;
    /// 设置行号断点。
    fn set_breakpoint_line(&mut self, line: u32, condition: Option<&str>) -> u64;
    /// 设置函数断点。
    fn set_breakpoint_fn(&mut self, fn_path: &str) -> u64;
    /// 删除断点。
    fn remove_breakpoint(&mut self, id: u64) -> bool;
    /// 切换断点启用/禁用。
    fn toggle_breakpoint(&mut self, id: u64) -> bool;
    /// 清空所有断点。
    fn clear_breakpoints(&mut self);
    /// 启用/禁用所有断点。
    fn enable_all_breakpoints(&mut self, enabled: bool);
    /// 获取所有断点。
    fn breakpoints(&self) -> &[Breakpoint];
    /// 获取断点可变引用。
    fn breakpoints_mut(&mut self) -> &mut Vec<Breakpoint>;

    // ─── 监视点管理 ──────────────────────────────────────

    /// 设置内存监视点。
    fn set_watchpoint(&mut self, addr: u64, size: u64, label: &str);
    /// 获取所有监视点。
    fn watchpoints(&self) -> &[Watchpoint];
    /// 检查是否命中监视点。
    fn check_watchpoint_hit(&self, accessed_addr: u64, access_size: u64) -> Option<usize>;

    // ─── 帧管理 ──────────────────────────────────────────

    /// 获取帧状态。
    fn frame_state(&self) -> &FrameState;
    /// 获取帧状态可变引用。
    fn frame_state_mut(&mut self) -> &mut FrameState;

    // ─── 显示管理 ──────────────────────────────────────

    /// 添加自动显示表达式。
    fn add_display_expr(&mut self, expr: &str);
    /// 删除自动显示表达式。
    fn remove_display_expr(&mut self, index: usize) -> bool;
    /// 清空自动显示表达式。
    fn clear_display_exprs(&mut self);
    /// 获取自动显示表达式列表。
    fn display_exprs(&self) -> &[String];

    // ─── 历史 ──────────────────────────────────────────

    /// 记录命令到历史。
    fn record_history(&mut self, cmd: &str);
    /// 获取历史。
    fn history(&self) -> &CommandHistory;
    /// 获取历史可变引用。
    fn history_mut(&mut self) -> &mut CommandHistory;

    // ─── 性能计数器 ──────────────────────────────────────

    /// 获取性能计数器。
    fn perf_counters(&self) -> &PerfCounters;
    /// 获取性能计数器可变引用。
    fn perf_counters_mut(&mut self) -> &mut PerfCounters;

    // ─── 面板/视图 ──────────────────────────────────────

    /// 获取当前数值显示格式。
    fn display_format(&self) -> DisplayFormat;
    /// 设置数值显示格式。
    fn set_display_format(&mut self, fmt: DisplayFormat);
    /// 获取嵌套展开深度。
    fn display_depth(&self) -> usize;
    /// 设置嵌套展开深度。
    fn set_display_depth(&mut self, depth: usize);
    /// 获取 watch 默认速度。
    fn watch_speed(&self) -> f32;
    /// 设置 watch 默认速度。
    fn set_watch_speed(&mut self, speed: f32);

    // ─── 信息查询 ──────────────────────────────────────

    /// 获取当前 PC 对应的源码行号。
    fn current_source_line(&self) -> Option<u32>;
    /// 获取当前指令描述。
    fn current_instruction(&self) -> String;
    /// 获取当前作用域变量（模拟）。
    fn scope_variables(&self) -> Vec<(&str, u64)>;
    /// 获取当前 PC 附近的源码行。
    fn source_context(&self, n: usize) -> Vec<(u32, String, bool)>;
}

// ─── IS* 上下文快照 ──────────────────────────────────────

/// IS* 上下文快照，用于右侧面板持久展示。
#[derive(Debug, Clone)]
pub struct IsContextSnapshot {
    pub entries: HashMap<String, String>,
}

impl IsContextSnapshot {
    pub fn new() -> Self {
        let mut entries = HashMap::new();
        // 用默认值初始化
        for v in IS_VARIABLES {
            entries.insert(v.name.to_string(), "—".to_string());
        }
        Self { entries }
    }

    pub fn update_from_vm(&mut self, vm: &VmState) {
        self.entries
            .insert("IS_SYS_PC".to_string(), format!("{:#06x}", vm.pc));
        self.entries
            .insert("IS_SYS_STATE".to_string(), format!("{:?}", vm.state));
        self.entries.insert(
            "IS_SYS_MEM_USED".to_string(),
            format!("{}", vm.memory.usage),
        );
        self.entries
            .insert("IS_SYS_MEM_TOTAL".to_string(), format!("{}", vm.mem_size));
        self.entries
            .insert("IS_COUNT_INSTR".to_string(), format!("{}", vm.quantum));
        self.entries.insert(
            "IS_CALL_STACK_SIZE".to_string(),
            format!("{}", vm.call_stack.len()),
        );
        self.entries
            .insert("IS_TASK_ID".to_string(), format!("{}", vm.task_id));
        self.entries.insert(
            "IS_CALL_DEPTH".to_string(),
            format!("{}", vm.call_stack.len()),
        );
        self.entries
            .insert("IS_SYS_QUANTUM".to_string(), format!("{}", vm.quantum));

        // 寄存器值
        self.entries.insert(
            "IS_FRAME_SP".to_string(),
            format!("{:#x}", vm.read_reg(reg::SP)),
        );
        self.entries.insert(
            "IS_FRAME_FP".to_string(),
            format!("{:#x}", vm.read_reg(reg::FP)),
        );
        self.entries.insert(
            "IS_FRAME_RA".to_string(),
            format!("{:#x}", vm.read_reg(reg::RA)),
        );

        // 调用栈顶
        if let Some(top) = vm.call_stack.last() {
            self.entries.insert(
                "IS_CALL_STACK_TOP".to_string(),
                format!("return_pc={:#06x}", top.return_pc),
            );
        }
    }
}

impl Default for IsContextSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

// ─── LocalDebugSession ──────────────────────────────────────

/// 本地调试会话。
///
/// 编译器和 VM 模块直接链接进同一进程，通过 `pub mod` 按需导入。
pub struct LocalDebugSession {
    pub vm: VmState,
    pub trace: ExecutionTrace,
    pub trace_collector: Option<TraceCollector>,
    pub debug_map: Option<DebugMap>,
    pub source_path: Option<String>,
    pub source_lines: Vec<String>,

    // 断点
    bp_list: Vec<Breakpoint>,
    bp_next_id: u64,
    /// 断点保存的原始指令（地址 → 原始指令码）。
    bp_originals: HashMap<usize, u32>,

    // 监视点
    wp_list: Vec<Watchpoint>,

    // 帧状态
    pub frame_state: FrameState,

    // 显示
    pub display_expr_list: Vec<String>,

    // 历史
    pub cmd_history: CommandHistory,

    // 性能计数器
    pub perf: PerfCounters,

    // 配置
    pub disp_fmt: DisplayFormat,
    pub disp_depth: usize,
    pub watch_spd: f32,

    // 收集时间
    _exec_start: Option<Instant>,

    // IS* 上下文
    pub is_context: IsContextSnapshot,

    // 执行控制
    pub collected: bool,
    /// Home 页面选中的 Step 索引（供 StepDetail 页面使用）。
    pub selected_step_index: Option<usize>,
}

impl LocalDebugSession {
    /// 创建新的本地调试会话。
    pub fn new(vm: VmState) -> Self {
        let debug_bytes = vm.debug_info.clone();
        let debug_map = if !debug_bytes.is_empty() {
            DebugMap::from_bytes(&debug_bytes)
        } else {
            None
        };

        Self {
            vm,
            trace: ExecutionTrace::new(),
            trace_collector: None,
            debug_map,
            source_path: None,
            source_lines: Vec::new(),
            bp_list: Vec::new(),
            bp_next_id: 1,
            bp_originals: HashMap::new(),
            wp_list: Vec::new(),
            frame_state: FrameState::new(),
            display_expr_list: Vec::new(),
            cmd_history: CommandHistory::new(500),
            perf: PerfCounters::default(),
            disp_fmt: DisplayFormat::Both,
            disp_depth: 3,
            watch_spd: 1.0,
            _exec_start: None,
            is_context: IsContextSnapshot::new(),
            collected: false,
            selected_step_index: None,
        }
    }

    /// 加载源码文件。
    pub fn set_source(&mut self, path: &str) {
        self.source_path = Some(path.to_string());
        if let Ok(content) = std::fs::read_to_string(path) {
            self.source_lines = content.lines().map(|l| l.to_string()).collect();
        }
    }

    /// 从字节加载 debug map。
    pub fn set_debug_map_from_bytes(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.debug_map = DebugMap::from_bytes(bytes);
        }
    }

    /// 默认运行收集：完整执行 VM 并收集轨迹。
    ///
    /// 对应设计文档 §6.2「数据流」第 3 步。
    pub fn collect_trace(&mut self) {
        if self.collected {
            return;
        }

        let mut collector = TraceCollector::new();
        collector.start();

        // 默认完整执行
        let start = Instant::now();
        let _instr_count = 0u64;

        // 逐指令执行并收集
        while self.vm.is_running() {
            let pc_before = self.vm.pc;

            // 检查断点（如果有断点且用户已设置）
            if !self.bp_list.is_empty() {
                if let Some(bp) = self.bp_list.iter().find(|b| {
                    b.enabled
                        && match b.bp_type {
                            BreakpointType::Pc(addr) => pc_before == addr,
                            _ => false,
                        }
                }) {
                    // 检查条件
                    if let Some(ref cond) = bp.condition {
                        match eval::eval_expr(cond, &self.vm) {
                            Ok(val) if val == 0 => {
                                // 条件不满足，继续
                            }
                            Ok(_) => {
                                // 命中断点，停止收集并保留当前状态
                                self.vm.state = VmStateKind::Suspended;
                                break;
                            }
                            Err(_) => {}
                        }
                    } else {
                        self.vm.state = VmStateKind::Suspended;
                        break;
                    }
                }
            }

            // 执行一条指令
            execute_instruction(&mut self.vm);
            collector.record_instruction();

            // 记录 opcode 统计
            if pc_before < self.vm.text.len() {
                let instr = self.vm.text[pc_before];
                let op = (instr >> 24) as u8;
                self.perf.opcode_counts[op as usize] += 1;
                *self.perf.pc_hits.entry(pc_before).or_insert(0) += 1;
                self.perf.total_instructions += 1;

                // 分类统计
                match op {
                    0x00..=0x0F => self.perf.system_count += 1,
                    0x10..=0x1F => self.perf.mem_count += 1,
                    0x20..=0x38 | 0x40..=0x45 => self.perf.arith_count += 1,
                    0x50..=0x55 | 0x60..=0x63 => self.perf.ctrl_count += 1,
                    0x70 => self.perf.system_count += 1,
                    _ => {}
                }

                // ── 数据层：Step 边界检测、变量追踪、子调用记录 ──

                // 检测 CALL 指令作为 Step 边界 —— 从 debug_map 获取函数名
                if op == opcode::CALL as u8 {
                    let entry = &decode::dispatch_table()[op as usize];
                    let ops = decode::decode(instr, entry.enc);
                    let offset = ops.imm as i32;
                    let target_pc = (pc_before as i32).wrapping_add(offset) as usize;
                    let source_line = self.debug_map.as_ref()
                        .and_then(|m| m.line_for_pc(pc_before))
                        .unwrap_or(0);

                    // 从 debug_map 中查找该 PC 对应的函数名
                    let call_name = if let Some(ref map) = self.debug_map {
                        map.func_entries().iter()
                            .find(|e| e.pc_start as usize == target_pc)
                            .and_then(|e| e.func_name())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("call_{:#06x}", target_pc))
                    } else {
                        format!("call_{:#06x}", target_pc)
                    };

                    collector.begin_step(&call_name, ExecutionPhase::Task, source_line, pc_before);
                    collector.record_sub_call(
                        crate::debug::trace::SubCall::FnCall {
                            name: call_name.clone(),
                            args: Vec::new(),    // 参数名在指令级不可得
                            result: None,
                            elapsed_us: 0,       // 结束时填充
                        }
                    );
                }

                // 检测 ECALL 作为系统调用 Step
                if op == opcode::ECALL as u8 {
                    let entry = &decode::dispatch_table()[op as usize];
                    let ops = decode::decode(instr, entry.enc);
                    let syscall_name = disassemble::syscall_name(ops.imm);
                    let source_line = self.debug_map.as_ref()
                        .and_then(|m| m.line_for_pc(pc_before))
                        .unwrap_or(0);

                    collector.begin_step(syscall_name, ExecutionPhase::System, source_line, pc_before);
                }

                // 检测 JMPR (0x54) — 函数返回指令 —— 结束当前 Step
                if op == 0x54 {
                    let _current_depth = self.vm.call_stack.len();
                    // 从 CALL 进入时 call_stack 加深，JMPR 返回时 call_stack 变浅
                    // 当发现 JMPR 且 call_stack 变浅，说明函数返回，结束当前 Step
                    collector.end_step(pc_before + 1);
                }

                // 检测 TRAP 指令 — 程序结束
                if op == opcode::TRAP as u8 {
                    // 给一个 Step 边界使最后一个指令被包含
                    collector.end_step(pc_before + 1);
                }

                // 检测 FORK/JOIN 用于任务依赖图
                if op == opcode::TASK_FORK as u8 {
                    let source_line = self.debug_map.as_ref()
                        .and_then(|m| m.line_for_pc(pc_before)).unwrap_or(0);
                    collector.begin_step("fork", ExecutionPhase::Task, source_line, pc_before);
                }
                if op == opcode::TASK_JOIN as u8 {
                    let source_line = self.debug_map.as_ref()
                        .and_then(|m| m.line_for_pc(pc_before)).unwrap_or(0);
                    collector.begin_step("join", ExecutionPhase::Task, source_line, pc_before);
                }

                // 检测 MOVI/STORE 作为变量事件 —— 使用当前 Step 名称
                if op == opcode::MOVI as u8 {
                    let entry = &decode::dispatch_table()[op as usize];
                    let ops = decode::decode(instr, entry.enc);
                    let rd = ops.rd as usize;
                    let reg_name = isa::reg_name(rd).to_uppercase();
                    let old_val = self.vm.read_reg(rd);
                    collector.record_variable(
                        &reg_name, "int", Some(old_val), ops.imm as u64,
                        pc_before,
                        self.debug_map.as_ref().and_then(|m| m.line_for_pc(pc_before)).unwrap_or(0),
                        "exec",  // TraceCollector 会自动解析为当前 Step 名
                        false,
                    );
                    // 如果当前有活跃 Step，也将变量名加入 output_vars
                    if let Some(ref step) = collector.current_step {
                        if step.name.contains("call_") || !step.name.is_empty() {
                            // 暂时不做 output_vars 自动填充（需要源码级变量名信息）
                        }
                    }
                }

                // 检测 LOAD 作为输入变量引用
                if op == opcode::LOAD as u8 {
                    let entry = &decode::dispatch_table()[op as usize];
                    let ops = decode::decode(instr, entry.enc);
                    let rd = ops.rd as usize;
                    let reg_name = isa::reg_name(rd).to_uppercase();
                    let old_val = self.vm.read_reg(rd);
                    collector.record_variable(
                        &reg_name, "int", Some(old_val), self.vm.read_reg(rd),
                        pc_before,
                        self.debug_map.as_ref().and_then(|m| m.line_for_pc(pc_before)).unwrap_or(0),
                        "exec", false,
                    );
                }
            }

            // 更新 IS* 上下文
            self.is_context.update_from_vm(&self.vm);
        }

        // 完成收集
        let elapsed = start.elapsed();
        self.perf
            .step_times
            .push(("total".to_string(), elapsed.as_micros() as u64));

        // 更新内存峰值
        if self.vm.memory.usage > self.perf.peak_memory {
            self.perf.peak_memory = self.vm.memory.usage;
        }

        self.trace = collector.finalize();
        self.trace.total_elapsed = elapsed;
        self.trace.completed = matches!(self.vm.state, VmStateKind::Halted);
        if let VmStateKind::Error(ref msg) = self.vm.state {
            self.trace.error_message = Some(msg.clone());
        }

        self.collected = true;
        self._exec_start = None;
    }

    /// 获取段信息。
    pub fn segment_info(&self) -> Vec<(&'static str, usize, String)> {
        let mut segments = Vec::new();
        segments.push((
            ".text",
            self.vm.text.len() * 4,
            format!("{} 条指令", self.vm.text.len()),
        ));
        segments.push((
            ".rodata",
            self.vm.rodata.len(),
            format!("{} 字节", self.vm.rodata.len()),
        ));
        segments.push((
            ".debug",
            self.vm.debug_info.len(),
            format!("{} 字节 ADBG", self.vm.debug_info.len()),
        ));
        segments.push((
            ".exn",
            self.vm.exn_table.len(),
            format!("{} 字节", self.vm.exn_table.len()),
        ));
        let total = (self.vm.text.len() * 4)
            + self.vm.rodata.len()
            + self.vm.debug_info.len()
            + self.vm.exn_table.len();
        segments.push(("总计", total, format!("{} 字节", total)));
        segments
    }

    /// 获取 Zone 状态 — 从实际执行数据分析。
    ///
    /// Zone 是一个编译期概念（代码区域），运行时从 .atxe 的 zones 段、
    /// debug 信息中的 FUNC 条目、以及实际执行的 PC 范围综合推断。
    pub fn zone_info(&self) -> Vec<(String, String, String, String)> {
        let mut zones = Vec::new();

        // 1. 尝试从 debug_map 中的 FUNC 条目推断 Zone
        if let Some(ref map) = self.debug_map {
            let func_entries = map.func_entries();
            if !func_entries.is_empty() {
                for entry in &func_entries {
                    let name = entry.func_name().unwrap_or("anonymous").to_string();
                    let pc_range = format!("{:#06x}–{:#06x}", entry.pc_start, entry.pc_end);
                    let status = if self.vm.pc >= entry.pc_start as usize
                        && self.vm.pc <= entry.pc_end as usize
                    {
                        "active".to_string()
                    } else {
                        "loaded".to_string()
                    };
                    zones.push((name, "PERSISTENT".to_string(), status, pc_range));
                }
                return zones;
            }
        }

        // 2. 如果没有 FUNC 条目，从 PC 命中统计推断热点区域
        let text_len = self.vm.text.len();
        if text_len > 0 {
            let total_pc_hits: u64 = self.perf.pc_hits.values().sum();
            if total_pc_hits > 0 {
                // 划分区域：每 25% 的 text 为一个 Zone
                let zone_size = text_len / 4 + 1;
                for i in 0..4 {
                    let start = i * zone_size;
                    let end = ((i + 1) * zone_size - 1).min(text_len.saturating_sub(1));
                    if start >= text_len {
                        break;
                    }
                    let zone_hits: u64 = (start..=end)
                        .filter_map(|pc| self.perf.pc_hits.get(&pc))
                        .sum();
                    let pct = if total_pc_hits > 0 {
                        zone_hits as f64 / total_pc_hits as f64 * 100.0
                    } else {
                        0.0
                    };
                    let lifecycle = if i == 0 || i == 3 {
                        "PERSISTENT"
                    } else {
                        "EXEC_UNLOAD"
                    };
                    let is_active = self.vm.pc >= start && self.vm.pc <= end;
                    let status = if is_active {
                        format!("active ({:.1}%)", pct)
                    } else if zone_hits > 0 {
                        format!("loaded ({:.1}%)", pct)
                    } else {
                        "lazy".to_string()
                    };
                    let zone_name = format!("zone_{}", i);
                    zones.push((
                        zone_name,
                        lifecycle.to_string(),
                        status,
                        format!("{:#06x}–{:#06x}", start, end),
                    ));
                }
            } else {
                // 3. 纯静态：按 .text 段大小等分
                let zone_size = text_len / 4 + 1;
                for i in 0..4.min(text_len) {
                    let start = i * zone_size;
                    let end = ((i + 1) * zone_size - 1).min(text_len.saturating_sub(1));
                    if start >= text_len {
                        break;
                    }
                    zones.push((
                        format!("zone_{}", i),
                        "PERSISTENT".to_string(),
                        if self.vm.pc >= start && self.vm.pc <= end {
                            "active".to_string()
                        } else {
                            "loaded".to_string()
                        },
                        format!("{:#06x}–{:#06x}", start, end),
                    ));
                }
            }
        }

        // 4. 如果完全没有数据，至少返回 main
        if zones.is_empty() {
            zones.push((
                "main".to_string(),
                "PERSISTENT".to_string(),
                if self.vm.is_running() {
                    "active"
                } else {
                    "halted"
                }
                .to_string(),
                format!("{:#06x}–{:#06x}", 0, text_len.saturating_sub(1)),
            ));
        }

        zones
    }

    /// 生成 opcode 分布。
    pub fn opcode_distribution(&self) -> Vec<(&'static str, u64, &'static str)> {
        let table = decode::dispatch_table();
        let mut dist = Vec::new();
        for (op, &count) in self.perf.opcode_counts.iter().enumerate() {
            if count > 0 {
                let entry = &table[op];
                let category = match op {
                    0x00..=0x0F => "SYSTEM",
                    0x10..=0x1F => "MEM",
                    0x20..=0x38 => "ARITH",
                    0x40..=0x45 => "CMP",
                    0x50..=0x55 => "CTRL",
                    0x60..=0x63 => "TASK",
                    0x70 => "ECALL",
                    0x80..=0x81 => "MEM",
                    0xF0..=0xF1 => "SYSTEM",
                    _ => "OTHER",
                };
                dist.push((entry.name, count, category));
            }
        }
        dist.sort_by(|a, b| b.1.cmp(&a.1));
        dist
    }

    /// 获取 hot path（Top-N PC）。
    pub fn hot_path(&self, n: usize) -> Vec<(usize, u64, String)> {
        let mut hits: Vec<_> = self
            .perf
            .pc_hits
            .iter()
            .map(|(pc, count)| {
                let desc = if *pc < self.vm.text.len() {
                    disassemble::format_instruction(*pc, self.vm.text[*pc])
                } else {
                    "—".to_string()
                };
                (*pc, *count, desc)
            })
            .collect();
        hits.sort_by(|a, b| b.1.cmp(&a.1));
        hits.truncate(n);
        hits
    }

    /// 获取内存统计。
    pub fn memory_stats(&self) -> (u64, u64, u64, u64) {
        let total = self.vm.mem_size;
        let used = self.vm.memory.usage;
        let free = total.saturating_sub(used);
        let peak = self.perf.peak_memory;
        (total, used, free, peak)
    }

    /// 获取已解析的 .atxe 段表。
    pub fn section_table(&self) -> Vec<SectionEntry> {
        // 从当前加载的数据重建段信息
        // 实际上我们应该从原始二进制中解析，但这里我们从内存数据推断
        Vec::new()
    }

    /// 获取异常详情（如果 VM 处于错误状态）。
    pub fn exception_detail(&self) -> Option<ExceptionDetail> {
        match &self.vm.state {
            VmStateKind::Error(msg) => Some(ExceptionDetail {
                error_type: "RuntimeError".to_string(),
                error_code: 1,
                error_message: msg.clone(),
                source_line: self
                    .debug_map
                    .as_ref()
                    .and_then(|m| m.line_for_pc(self.vm.pc)),
                source_pc: self.vm.pc,
                call_stack_depth: self.vm.call_stack.len(),
                is_propagated: false,
                is_caught: false,
            }),
            _ => None,
        }
    }
}

/// 异常详情。
#[derive(Debug, Clone)]
pub struct ExceptionDetail {
    pub error_type: String,
    pub error_code: u32,
    pub error_message: String,
    pub source_line: Option<u32>,
    pub source_pc: usize,
    pub call_stack_depth: usize,
    pub is_propagated: bool,
    pub is_caught: bool,
}

// ─── DebugSession trait 实现 ──────────────────────────────

impl DebugSession for LocalDebugSession {
    fn vm(&self) -> &VmState {
        &self.vm
    }
    fn vm_mut(&mut self) -> &mut VmState {
        &mut self.vm
    }
    fn trace(&self) -> &ExecutionTrace {
        &self.trace
    }
    fn trace_mut(&mut self) -> &mut ExecutionTrace {
        &mut self.trace
    }
    fn debug_map(&self) -> Option<&DebugMap> {
        self.debug_map.as_ref()
    }
    fn source_lines(&self) -> &[String] {
        &self.source_lines
    }
    fn source_path(&self) -> Option<&str> {
        self.source_path.as_deref()
    }

    // ─── 执行控制 ──────────────────────────────────────

    fn step_instructions(&mut self, n: usize) {
        for _ in 0..n {
            if !self.vm.is_running() {
                break;
            }
            let pc_before = self.vm.pc;
            execute_instruction(&mut self.vm);
            self.perf.total_instructions += 1;
            if pc_before < self.vm.text.len() {
                let op = (self.vm.text[pc_before] >> 24) as u8;
                self.perf.opcode_counts[op as usize] += 1;
            }
        }
        self.is_context.update_from_vm(&self.vm);
    }

    fn continue_execution(&mut self) {
        let max_steps = 1_000_000;
        let mut steps = 0;
        while self.vm.is_running() && steps < max_steps {
            let pc_before = self.vm.pc;
            execute_instruction(&mut self.vm);
            steps += 1;
            self.perf.total_instructions += 1;

            // 检查断点
            if let Some(bp) = self.bp_list.iter().find(|b| {
                b.enabled
                    && match b.bp_type {
                        BreakpointType::Pc(addr) => pc_before == addr,
                        _ => false,
                    }
            }) {
                if let Some(ref cond) = bp.condition {
                    match eval::eval_expr(cond, &self.vm) {
                        Ok(val) if val == 0 => continue,
                        Err(e) => {
                            println!("⚠ 条件求值错误: {}", e);
                            continue;
                        }
                        _ => {}
                    }
                }
                println!("⏸ 命中断点于 {:#06x}", pc_before);
                return;
            }

            if !self.vm.is_running() {
                break;
            }
        }
        self.is_context.update_from_vm(&self.vm);
        match self.vm.state {
            VmStateKind::Halted => println!("⏹ VM 已停止（{} 条指令）", steps),
            VmStateKind::Error(ref e) => println!("⛔ VM 错误: {}", e),
            VmStateKind::Suspended => println!("⏸ VM 已挂起"),
            _ => {
                if steps >= max_steps {
                    println!("⚠ 达到最大步数限制")
                }
            }
        }
    }

    fn step_over(&mut self) {
        // Step-over: 在当前 Step 边界上步进
        // 简化实现：运行到下一个 CALL/ECALL/TRAP 指令
        let max_steps = 10_000;
        for _ in 0..max_steps {
            if !self.vm.is_running() {
                break;
            }
            let pc_before = self.vm.pc;
            execute_instruction(&mut self.vm);
            self.perf.total_instructions += 1;
            if pc_before < self.vm.text.len() {
                let op = (self.vm.text[pc_before] >> 24) as u8;
                if op == opcode::CALL as u8 || op == opcode::ECALL as u8 || op == opcode::TRAP as u8
                {
                    println!("→ Step 边界于 {:#06x}", self.vm.pc);
                    return;
                }
            }
        }
        self.is_context.update_from_vm(&self.vm);
    }

    fn step_into(&mut self) {
        // 执行一条指令（进入）
        self.step_instructions(1);
    }

    fn step_out(&mut self) {
        // 运行到从当前 CALL 返回
        let current_depth = self.vm.call_stack.len();
        let max_steps = 10_000;
        for _ in 0..max_steps {
            if !self.vm.is_running() {
                break;
            }
            execute_instruction(&mut self.vm);
            self.perf.total_instructions += 1;
            if self.vm.call_stack.len() < current_depth {
                return;
            }
        }
        self.is_context.update_from_vm(&self.vm);
    }

    // ─── 断点管理 ──────────────────────────────────────

    fn set_breakpoint_pc(&mut self, addr: usize, condition: Option<&str>) -> u64 {
        if addr >= self.vm.text.len() {
            println!("地址 {:#06x} 超出 .text 段", addr);
            return 0;
        }
        let id = self.bp_next_id;
        self.bp_next_id += 1;

        let original = self.vm.text[addr];
        self.bp_originals.insert(addr, original);
        self.vm.text[addr] = isa::encode_ji(opcode::TRAP, 0);

        self.bp_list.push(Breakpoint {
            id,
            bp_type: BreakpointType::Pc(addr),
            condition: condition.map(|s| s.to_string()),
            hit_count: 0,
            enabled: true,
            original_instr: Some(original),
        });
        id
    }

    fn set_breakpoint_line(&mut self, line: u32, condition: Option<&str>) -> u64 {
        // 通过 debug_map 查找行号对应的 PC
        if let Some(ref map) = self.debug_map {
            // 查找最近的大于等于该行号的 PC
            for entry in &map.entries {
                if entry.source_line == line && entry.kind == 4 {
                    return self.set_breakpoint_pc(entry.pc_start as usize, condition);
                }
            }
            // 降级：找最接近的行号
            if let Some(best) = map
                .line_entries()
                .iter()
                .min_by_key(|e| (e.source_line as i32 - line as i32).abs())
            {
                return self.set_breakpoint_pc(best.pc_start as usize, condition);
            }
        }
        println!("无法将行号 {} 映射到 PC（缺少 debug 信息）", line);
        0
    }

    fn set_breakpoint_fn(&mut self, _fn_path: &str) -> u64 {
        // 函数断点需要更复杂的符号解析
        // 当前简化：尝试在 debug_map 中查找
        println!("函数断点暂不支持完整符号解析");
        // 返回 0 表示未设置
        0
    }

    fn remove_breakpoint(&mut self, id: u64) -> bool {
        if let Some(pos) = self.bp_list.iter().position(|b| b.id == id) {
            let bp = &self.bp_list[pos];
            // 恢复原始指令
            if let BreakpointType::Pc(addr) = bp.bp_type {
                if let Some(&orig) = self.bp_originals.get(&addr) {
                    if addr < self.vm.text.len() {
                        self.vm.text[addr] = orig;
                    }
                }
                self.bp_originals.remove(&addr);
            }
            self.bp_list.remove(pos);
            true
        } else {
            false
        }
    }

    fn toggle_breakpoint(&mut self, id: u64) -> bool {
        if let Some(bp) = self.bp_list.iter_mut().find(|b| b.id == id) {
            bp.enabled = !bp.enabled;
            // 切换时恢复/设置 TRAP
            if let BreakpointType::Pc(addr) = bp.bp_type {
                if addr < self.vm.text.len() {
                    if bp.enabled {
                        self.vm.text[addr] = isa::encode_ji(opcode::TRAP, 0);
                    } else if let Some(&orig) = self.bp_originals.get(&addr) {
                        self.vm.text[addr] = orig;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    fn clear_breakpoints(&mut self) {
        // 恢复所有原始指令
        for (addr, &orig) in &self.bp_originals {
            if *addr < self.vm.text.len() {
                self.vm.text[*addr] = orig;
            }
        }
        self.bp_originals.clear();
        self.bp_list.clear();
    }

    fn enable_all_breakpoints(&mut self, enabled: bool) {
        for bp in &mut self.bp_list {
            bp.enabled = enabled;
            if let BreakpointType::Pc(addr) = bp.bp_type {
                if addr < self.vm.text.len() {
                    if enabled {
                        self.vm.text[addr] = isa::encode_ji(opcode::TRAP, 0);
                    } else if let Some(&orig) = self.bp_originals.get(&addr) {
                        self.vm.text[addr] = orig;
                    }
                }
            }
        }
    }

    fn breakpoints(&self) -> &[Breakpoint] {
        &self.bp_list
    }
    fn breakpoints_mut(&mut self) -> &mut Vec<Breakpoint> {
        &mut self.bp_list
    }

    // ─── 监视点 ──────────────────────────────────────

    fn set_watchpoint(&mut self, addr: u64, size: u64, label: &str) {
        if self.wp_list.iter().any(|w| w.addr == addr) {
            println!("监视点已存在于 {:#x}", addr);
            return;
        }
        self.wp_list.push(Watchpoint {
            addr,
            size,
            label: label.to_string(),
            hit_count: 0,
        });
        println!("监视点已设置: {:#x} ({} 字节, {})", addr, size, label);
    }

    fn watchpoints(&self) -> &[Watchpoint] {
        &self.wp_list
    }

    fn check_watchpoint_hit(&self, accessed_addr: u64, access_size: u64) -> Option<usize> {
        for (i, wp) in self.wp_list.iter().enumerate() {
            let wp_end = wp.addr.wrapping_add(wp.size);
            let acc_end = accessed_addr.wrapping_add(access_size);
            if accessed_addr < wp_end && acc_end > wp.addr {
                return Some(i);
            }
        }
        None
    }

    // ─── 帧状态 ──────────────────────────────────────

    fn frame_state(&self) -> &FrameState {
        &self.frame_state
    }
    fn frame_state_mut(&mut self) -> &mut FrameState {
        &mut self.frame_state
    }

    // ─── 显示 ──────────────────────────────────────

    fn add_display_expr(&mut self, expr: &str) {
        if !expr.is_empty() {
            self.display_expr_list.push(expr.to_string());
            println!("{} 已添加到 display 列表", expr);
        }
    }

    fn remove_display_expr(&mut self, index: usize) -> bool {
        if index < self.display_expr_list.len() {
            self.display_expr_list.remove(index);
            true
        } else {
            false
        }
    }

    fn clear_display_exprs(&mut self) {
        self.display_expr_list.clear();
        println!("display 列表已清空");
    }

    fn display_exprs(&self) -> &[String] {
        &self.display_expr_list
    }

    // ─── 历史 ──────────────────────────────────────

    fn record_history(&mut self, cmd: &str) {
        self.cmd_history.push(cmd.to_string());
    }
    fn history(&self) -> &CommandHistory {
        &self.cmd_history
    }
    fn history_mut(&mut self) -> &mut CommandHistory {
        &mut self.cmd_history
    }

    // ─── 性能计数器 ──────────────────────────────────────

    fn perf_counters(&self) -> &PerfCounters {
        &self.perf
    }
    fn perf_counters_mut(&mut self) -> &mut PerfCounters {
        &mut self.perf
    }

    // ─── 面板/视图 ──────────────────────────────────────

    fn display_format(&self) -> DisplayFormat {
        self.disp_fmt
    }
    fn set_display_format(&mut self, fmt: DisplayFormat) {
        self.disp_fmt = fmt;
    }
    fn display_depth(&self) -> usize {
        self.disp_depth
    }
    fn set_display_depth(&mut self, depth: usize) {
        self.disp_depth = depth;
    }
    fn watch_speed(&self) -> f32 {
        self.watch_spd
    }
    fn set_watch_speed(&mut self, speed: f32) {
        self.watch_spd = speed.max(0.25).min(4.0);
    }

    // ─── 信息查询 ──────────────────────────────────────

    fn current_source_line(&self) -> Option<u32> {
        self.debug_map
            .as_ref()
            .and_then(|m| m.line_for_pc(self.vm.pc))
    }

    fn current_instruction(&self) -> String {
        if self.vm.pc < self.vm.text.len() {
            disassemble::format_instruction(self.vm.pc, self.vm.text[self.vm.pc])
        } else {
            format!("pc={:#06x} (越界)", self.vm.pc)
        }
    }

    fn scope_variables(&self) -> Vec<(&str, u64)> {
        // 返回当前作用域可见的寄存器变量
        let mut vars: Vec<(&str, u64)> = Vec::new();
        for i in 0..isa::REG_COUNT {
            let name = isa::reg_name(i);
            vars.push((name, self.vm.read_reg(i)));
        }
        vars
    }

    fn source_context(&self, n: usize) -> Vec<(u32, String, bool)> {
        let line = self.current_source_line().unwrap_or(1) as usize;
        if self.source_lines.is_empty() {
            return Vec::new();
        }
        let start = line.saturating_sub(n / 2).max(1);
        let end = (start + n).min(self.source_lines.len() + 1);
        let mut ctx = Vec::new();
        for lnum in start..end {
            let is_current = Some(lnum as u32) == self.current_source_line();
            let text = if lnum <= self.source_lines.len() {
                self.source_lines[lnum - 1].clone()
            } else {
                String::new()
            };
            ctx.push((lnum as u32, text, is_current));
        }
        ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::{AtxeBinary, Header};
    use crate::base::isa;

    fn make_test_session(text: Vec<u32>) -> LocalDebugSession {
        let header = Header::new(0, text.len() as u16);
        let binary = AtxeBinary {
            header,
            sections: vec![],
            text,
            rodata: vec![],
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let vm = VmState::from_atxe(&binary).unwrap();
        LocalDebugSession::new(vm)
    }

    #[test]
    fn session_creation() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let s = make_test_session(text);
        assert!(!s.collected);
        assert_eq!(s.breakpoints().len(), 0);
        assert_eq!(s.watchpoints().len(), 0);
    }

    #[test]
    fn collect_trace_halt() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut s = make_test_session(text);
        s.collect_trace();
        assert!(s.collected);
        assert!(s.trace.completed);
        assert!(s.trace.total_instructions > 0);
    }

    #[test]
    fn breakpoint_set_and_remove() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 1),
            isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 2),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut s = make_test_session(text);
        let id = s.set_breakpoint_pc(1, None);
        assert!(id > 0);
        assert_eq!(s.breakpoints().len(), 1);
        assert!(s.remove_breakpoint(id));
        assert_eq!(s.breakpoints().len(), 0);
    }

    #[test]
    fn toggle_breakpoint() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = make_test_session(text);
        let id = s.set_breakpoint_pc(0, None);
        assert!(s.breakpoints()[0].enabled);
        assert!(s.toggle_breakpoint(id));
        assert!(!s.breakpoints()[0].enabled);
    }

    #[test]
    fn clear_breakpoints() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = make_test_session(text);
        s.set_breakpoint_pc(0, None);
        s.set_breakpoint_pc(0, None);
        s.clear_breakpoints();
        assert_eq!(s.breakpoints().len(), 0);
    }

    #[test]
    fn frame_state_navigation() {
        let mut f = FrameState::new();
        assert_eq!(f.current_index(), 0);
        f.up(5);
        assert_eq!(f.current_index(), 1);
        f.down();
        assert_eq!(f.current_index(), 0);
        f.set(3, 10);
        assert_eq!(f.current_index(), 3);
    }

    #[test]
    fn command_history() {
        let mut h = CommandHistory::new(10);
        assert!(h.is_empty());
        h.push("step".into());
        h.push("regs".into());
        assert_eq!(h.len(), 2);
        assert_eq!(h.get(1), Some("regs"));
        assert_eq!(h.get(2), Some("step"));
        assert!(h.get(3).is_none());
    }

    #[test]
    fn command_history_dedup() {
        let mut h = CommandHistory::new(10);
        h.push("step".into());
        h.push("step".into()); // 重复，应被去重
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn display_format_toggle() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = make_test_session(text);
        assert_eq!(s.display_format(), DisplayFormat::Both);
        s.set_display_format(DisplayFormat::Hex);
        assert_eq!(s.display_format(), DisplayFormat::Hex);
    }

    #[test]
    fn display_expr_management() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = make_test_session(text);
        s.add_display_expr("a0");
        s.add_display_expr("t0");
        assert_eq!(s.display_exprs().len(), 2);
        assert!(s.remove_display_expr(0));
        assert_eq!(s.display_exprs().len(), 1);
        s.clear_display_exprs();
        assert_eq!(s.display_exprs().len(), 0);
    }

    #[test]
    fn is_context_initialized() {
        let ctx = IsContextSnapshot::new();
        assert_eq!(ctx.entries.len(), 72);
    }

    #[test]
    fn source_context_empty() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let s = make_test_session(text);
        assert!(s.source_context(5).is_empty());
    }

    #[test]
    fn exception_detail() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = make_test_session(text);
        assert!(s.exception_detail().is_none());
        s.vm.state = VmStateKind::Error("test error".into());
        let detail = s.exception_detail();
        assert!(detail.is_some());
        assert_eq!(detail.unwrap().error_message, "test error");
    }

    #[test]
    fn watch_speed_clamping() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = make_test_session(text);
        s.set_watch_speed(0.1);
        assert!((s.watch_speed() - 0.25).abs() < 0.01);
        s.set_watch_speed(5.0);
        assert!((s.watch_speed() - 4.0).abs() < 0.01);
    }
}
