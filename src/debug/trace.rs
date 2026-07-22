//! 执行轨迹 — 默认运行收集的数据结构。
//!
//! 对应设计文档 §6.3「关键数据结构」。
//!
//! 启动时自动完整执行，收集以下数据以供后续查看/展开/追踪，
//! 所有查看操作基于已收集的数据，无需再次运行。
//!
//! # 数据流
//! ```text
//! 默认运行 → collect_trace() → ExecutionTrace
//!                                   ├── steps: Vec<StepRecord>
//!                                   ├── variable_events: Vec<VariableEvent>
//!                                   ├── is_timeline: Vec<IsEvent>
//!                                   └── hook_timeline: Vec<HookEvent>
//! ```

use std::time::Duration;

// ─── 执行阶段 ──────────────────────────────────────────────

/// 执行日志的四段结构（Home 页面的段落划分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionPhase {
    /// SYSTEM — 系统准备阶段。
    System,
    /// INPUT — 输入数据加载阶段。
    Input,
    /// TASK — 任务执行阶段（每个 CALL 为一个 Step）。
    Task,
    /// OUT — 产出交付阶段。
    Out,
}

impl ExecutionPhase {
    pub fn name(&self) -> &'static str {
        match self {
            Self::System => "SYSTEM",
            Self::Input => "INPUT",
            Self::Task => "TASK",
            Self::Out => "OUT",
        }
    }

    pub fn all() -> &'static [ExecutionPhase] {
        &[Self::System, Self::Input, Self::Task, Self::Out]
    }
}

// ─── Step 状态 ─────────────────────────────────────────────

/// Step 的执行状态（Home 页面的标记符号来源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepStatus {
    /// ✓ 已执行。
    Completed,
    /// ✗ 执行错误。
    Error,
    /// — 被跳过（因条件不满足或依赖失败）。
    Skipped,
    /// 待执行/正在执行。
    Pending,
}

impl StepStatus {
    /// 返回 Home 页面使用的标记符号。
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Completed => "✓",
            Self::Error => "✗",
            Self::Skipped => "—",
            Self::Pending => "·",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Error => "error",
            Self::Skipped => "skipped",
            Self::Pending => "pending",
        }
    }
}

// ─── 子调用类型 ────────────────────────────────────────────

/// Step 内部的子调用（Step Detail 页面展示）。
#[derive(Debug, Clone)]
pub enum SubCall {
    /// 函数调用：TOOLS::fn_name(args)。
    FnCall {
        name: String,
        args: Vec<String>,
        result: Option<String>,
        elapsed_us: u64,
    },
    /// WORKS 调用及其生命周期。
    WorksCall {
        name: String,
        lifecycle: Vec<WorksPhase>,
        elapsed_us: u64,
        result: Option<String>,
    },
}

/// WORKS 生命周期阶段（Step Detail 页面展示）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorksPhase {
    /// INIT — 初始化。
    Init,
    /// START — 开始执行。
    Start,
    /// HOOK — 某个钩子触发。
    Hook { name: &'static str, elapsed_us: u64 },
    /// DONE — 完成。
    Done,
    /// ERROR — 错误终止。
    Error(String),
}

impl WorksPhase {
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Init => "INIT",
            Self::Start => "START",
            Self::Hook { name, .. } => name,
            Self::Done => "DONE",
            Self::Error(_) => "ERROR",
        }
    }
}

// ─── Step 记录 ─────────────────────────────────────────────

/// 一条 Step 的完整执行记录。
#[derive(Debug, Clone)]
pub struct StepRecord {
    /// Step 名称（CALL 语句中的函数名）。
    pub name: String,
    /// 所属阶段。
    pub phase: ExecutionPhase,
    /// 执行状态。
    pub status: StepStatus,
    /// 执行耗时（微秒）。
    pub elapsed_us: u64,
    /// 子调用列表。
    pub sub_calls: Vec<SubCall>,
    /// 输入变量名列表。
    pub input_vars: Vec<String>,
    /// 输出变量名列表。
    pub output_vars: Vec<String>,
    /// PC 范围（起始，结束）。
    pub pc_range: (usize, usize),
    /// 对应的源码行号。
    pub source_line: u32,
    /// 错误摘要（仅 status == Error 时）。
    pub error_summary: Option<String>,
}

impl StepRecord {
    /// 创建一个新的 Step 记录（初始为 Pending）。
    pub fn new(name: &str, phase: ExecutionPhase, source_line: u32) -> Self {
        Self {
            name: name.to_string(),
            phase,
            status: StepStatus::Pending,
            elapsed_us: 0,
            sub_calls: Vec::new(),
            input_vars: Vec::new(),
            output_vars: Vec::new(),
            pc_range: (0, 0),
            source_line,
            error_summary: None,
        }
    }
}

// ─── 变量事件 ──────────────────────────────────────────────

/// 变量值变化事件（用于数据时间轴和变量追踪）。
#[derive(Debug, Clone)]
pub struct VariableEvent {
    /// 变量名。
    pub name: String,
    /// 变量类型描述。
    pub type_desc: String,
    /// 变化前的值（十六进制）。
    pub old_value: Option<u64>,
    /// 变化后的值（十六进制）。
    pub new_value: u64,
    /// 发生变化的 PC。
    pub pc: usize,
    /// 对应的源码行号。
    pub source_line: u32,
    /// 所属 Step 名称。
    pub step_name: String,
    /// 是否为 INPUT 来源。
    pub from_input: bool,
    /// 发生时间戳（相对执行开始，微秒）。
    pub timestamp_us: u64,
}

// ─── IS* 事件 ──────────────────────────────────────────────

/// IS* 变量变化事件（用于 IS* 时间线）。
#[derive(Debug, Clone)]
pub struct IsEvent {
    /// IS* 变量名（如 IS_EXCEPTION、IS_COUNT 等）。
    pub name: String,
    /// 旧值。
    pub old_value: String,
    /// 新值。
    pub new_value: String,
    /// 发生变化的 PC。
    pub pc: usize,
    /// 时间戳（微秒）。
    pub timestamp_us: u64,
}

// ─── 钩子事件 ──────────────────────────────────────────────

/// 钩子执行事件（用于 Hook Timeline 页面）。
#[derive(Debug, Clone)]
pub struct HookEvent {
    /// WORKS 实例名称。
    pub works_name: String,
    /// 钩子名称。
    pub hook_name: String,
    /// 触发条件（如果有）。
    pub condition: Option<String>,
    /// 执行的动作名称。
    pub action: String,
    /// 执行结果。
    pub status: StepStatus,
    /// 耗时（微秒）。
    pub elapsed_us: u64,
    /// 时间戳（微秒）。
    pub timestamp_us: u64,
    /// 是否为扇出分支。
    pub is_fanout: bool,
}

// ─── 执行轨迹 ──────────────────────────────────────────────

/// 一次完整执行的记录。
///
/// 启动时自动收集，所有查看/导航/分析操作均基于此数据。
#[derive(Debug, Clone)]
pub struct ExecutionTrace {
    /// 所有 Step 记录，按执行顺序排列。
    pub steps: Vec<StepRecord>,
    /// 变量变化事件序列。
    pub variable_events: Vec<VariableEvent>,
    /// IS* 时间线事件。
    pub is_timeline: Vec<IsEvent>,
    /// 钩子时间线事件。
    pub hook_timeline: Vec<HookEvent>,
    /// 总执行指令数。
    pub total_instructions: u64,
    /// 总执行耗时。
    pub total_elapsed: Duration,
    /// 执行是否成功完成。
    pub completed: bool,
    /// 错误信息（如有）。
    pub error_message: Option<String>,
}

impl ExecutionTrace {
    /// 创建一个空轨迹。
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            variable_events: Vec::new(),
            is_timeline: Vec::new(),
            hook_timeline: Vec::new(),
            total_instructions: 0,
            total_elapsed: Duration::default(),
            completed: false,
            error_message: None,
        }
    }

    /// 根据名称查找 Step 记录。
    pub fn find_step_by_name(&self, name: &str) -> Option<&StepRecord> {
        self.steps.iter().find(|s| s.name == name)
    }

    /// 根据序号查找 Step 记录。
    pub fn find_step_by_index(&self, index: usize) -> Option<&StepRecord> {
        self.steps.get(index)
    }

    /// 返回指定阶段的 Step 列表。
    pub fn steps_by_phase(&self, phase: ExecutionPhase) -> Vec<&StepRecord> {
        self.steps.iter().filter(|s| s.phase == phase).collect()
    }

    /// 返回所有错误 Step。
    pub fn error_steps(&self) -> Vec<&StepRecord> {
        self.steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Error))
            .collect()
    }

    /// 返回所有被跳过的 Step。
    pub fn skipped_steps(&self) -> Vec<&StepRecord> {
        self.steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Skipped))
            .collect()
    }

    /// 获取某个 PC 地址相关的 Step。
    pub fn step_for_pc(&self, pc: usize) -> Option<&StepRecord> {
        self.steps
            .iter()
            .find(|s| pc >= s.pc_range.0 && pc < s.pc_range.1)
    }

    /// 执行是否是空的（没有收集到任何 Step）。
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Step 总数。
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 已完成 Step 数。
    pub fn completed_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Completed))
            .count()
    }

    /// 错误 Step 数。
    pub fn error_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Error))
            .count()
    }
}

impl Default for ExecutionTrace {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Step 收集器（用于在 VM 执行期间收集数据）─────────────

/// 在 VM 执行期间收集 Step 轨迹的收集器。
///
/// 通过 `execute_instruction` 调用前后插入钩子来收集数据。
#[derive(Debug, Clone)]
pub struct TraceCollector {
    /// 正在构建的轨迹。
    pub trace: ExecutionTrace,
    /// 当前正在执行的 Step（如果有）。
    pub current_step: Option<StepRecord>,
    /// 当前 Step 开始时的 PC。
    pub current_step_start_pc: usize,
    /// 当前 Step 开始时间。
    pub current_step_start_time: std::time::Instant,
    /// 全局执行开始时间。
    pub global_start: Option<std::time::Instant>,
    /// 指令计数。
    pub instruction_count: u64,
    /// 是否需要收集变量事件。
    pub collect_variables: bool,
    /// 是否需要收集 IS* 事件。
    pub collect_is: bool,
    /// 是否需要收集钩子事件。
    pub collect_hooks: bool,
}

impl TraceCollector {
    /// 创建一个新的轨迹收集器。
    pub fn new() -> Self {
        Self {
            trace: ExecutionTrace::new(),
            current_step: None,
            current_step_start_pc: 0,
            current_step_start_time: std::time::Instant::now(),
            global_start: None,
            instruction_count: 0,
            collect_variables: true,
            collect_is: true,
            collect_hooks: true,
        }
    }

    /// 开始全局执行计时。
    pub fn start(&mut self) {
        self.global_start = Some(std::time::Instant::now());
        self.current_step_start_time = std::time::Instant::now();
    }

    /// 开始一个新的 Step。
    pub fn begin_step(
        &mut self,
        name: &str,
        phase: ExecutionPhase,
        source_line: u32,
        pc: usize,
    ) {
        // 如果有未完成的上一个 Step，先结束它
        if self.current_step.is_some() {
            self.end_step(pc);
        }

        let mut step = StepRecord::new(name, phase, source_line);
        step.pc_range.0 = pc;
        self.current_step = Some(step);
        self.current_step_start_pc = pc;
        self.current_step_start_time = std::time::Instant::now();
    }

    /// 结束当前 Step。
    pub fn end_step(&mut self, end_pc: usize) {
        if let Some(mut step) = self.current_step.take() {
            step.elapsed_us = self.current_step_start_time.elapsed().as_micros() as u64;
            step.pc_range.1 = end_pc;
            if step.status == StepStatus::Pending {
                step.status = StepStatus::Completed;
            }
            self.trace.steps.push(step);
        }
    }

    /// 标记当前 Step 为错误状态。
    pub fn mark_step_error(&mut self, error: &str) {
        if let Some(ref mut step) = self.current_step {
            step.status = StepStatus::Error;
            step.error_summary = Some(error.to_string());
        }
    }

    /// 标记当前 Step 为跳过状态。
    pub fn mark_step_skipped(&mut self) {
        if let Some(ref mut step) = self.current_step {
            step.status = StepStatus::Skipped;
        }
    }

    /// 记录一条变量事件。
    pub fn record_variable(
        &mut self,
        name: &str,
        type_desc: &str,
        old_value: Option<u64>,
        new_value: u64,
        pc: usize,
        source_line: u32,
        step_name: &str,
        from_input: bool,
    ) {
        if !self.collect_variables {
            return;
        }
        let timestamp = self.global_start
            .map(|start| start.elapsed().as_micros() as u64)
            .unwrap_or(0);
        self.trace.variable_events.push(VariableEvent {
            name: name.to_string(),
            type_desc: type_desc.to_string(),
            old_value,
            new_value,
            pc,
            source_line,
            step_name: step_name.to_string(),
            from_input,
            timestamp_us: timestamp,
        });
    }

    /// 记录一条 IS* 事件。
    pub fn record_is(&mut self, name: &str, old_value: &str, new_value: &str, pc: usize) {
        if !self.collect_is {
            return;
        }
        let timestamp = self.global_start
            .map(|start| start.elapsed().as_micros() as u64)
            .unwrap_or(0);
        self.trace.is_timeline.push(IsEvent {
            name: name.to_string(),
            old_value: old_value.to_string(),
            new_value: new_value.to_string(),
            pc,
            timestamp_us: timestamp,
        });
    }

    /// 记录一条钩子事件。
    pub fn record_hook(
        &mut self,
        works_name: &str,
        hook_name: &str,
        condition: Option<&str>,
        action: &str,
        status: StepStatus,
        elapsed_us: u64,
        is_fanout: bool,
    ) {
        if !self.collect_hooks {
            return;
        }
        let timestamp = self.global_start
            .map(|start| start.elapsed().as_micros() as u64)
            .unwrap_or(0);
        self.trace.hook_timeline.push(HookEvent {
            works_name: works_name.to_string(),
            hook_name: hook_name.to_string(),
            condition: condition.map(|s| s.to_string()),
            action: action.to_string(),
            status,
            elapsed_us,
            timestamp_us: timestamp,
            is_fanout,
        });
    }

    /// 记录一条指令执行（递增计数）。
    pub fn record_instruction(&mut self) {
        self.instruction_count += 1;
    }

    /// 完成收集，返回最终轨迹。
    pub fn finalize(mut self) -> ExecutionTrace {
        self.end_step(usize::MAX);
        self.trace.total_instructions = self.instruction_count;
        if let Some(start) = self.global_start {
            self.trace.total_elapsed = start.elapsed();
        }
        self.trace.completed = true;
        self.trace
    }
}

impl Default for TraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ─── IS* 变量定义 ──────────────────────────────────────────

/// IS* 变量分组（对应设计文档 §3.17 IS* Context 页面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IsGroup {
    /// 异常相关。
    Exception,
    /// 计数相关。
    Count,
    /// 调用上下文。
    CallContext,
    /// 系统/环境。
    System,
    /// 时间相关。
    Time,
    /// 任务相关。
    Task,
    /// 数据相关。
    Data,
}

impl IsGroup {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Exception => "异常",
            Self::Count => "计数",
            Self::CallContext => "调用上下文",
            Self::System => "系统/环境",
            Self::Time => "时间",
            Self::Task => "任务",
            Self::Data => "数据",
        }
    }
}

/// 一条 IS* 变量定义（名称、分组、说明）。
#[derive(Debug, Clone)]
pub struct IsVariable {
    pub name: &'static str,
    pub group: IsGroup,
    pub description: &'static str,
}

/// 全部 72 个 IS* 变量（设计文档 §3.17）。
pub const IS_VARIABLES: &[IsVariable] = &[
    // 异常 (8)
    IsVariable { name: "IS_EXCEPTION", group: IsGroup::Exception, description: "当前异常码" },
    IsVariable { name: "IS_EXCEPTION_MSG", group: IsGroup::Exception, description: "异常消息" },
    IsVariable { name: "IS_EXCEPTION_PC", group: IsGroup::Exception, description: "异常发生 PC" },
    IsVariable { name: "IS_EXCEPTION_HANDLED", group: IsGroup::Exception, description: "是否已被处理" },
    IsVariable { name: "IS_EXCEPTION_PROPAGATES", group: IsGroup::Exception, description: "是否向上传播" },
    IsVariable { name: "IS_EXCEPTION_CAUGHT_BY", group: IsGroup::Exception, description: "捕获者（TRY 块）" },
    IsVariable { name: "IS_EXCEPTION_ZONE", group: IsGroup::Exception, description: "异常发生 Zone" },
    IsVariable { name: "IS_EXCEPTION_STEP", group: IsGroup::Exception, description: "异常发生 Step" },
    // 计数 (8)
    IsVariable { name: "IS_COUNT_INSTR", group: IsGroup::Count, description: "已执行指令数" },
    IsVariable { name: "IS_COUNT_STEP", group: IsGroup::Count, description: "已执行 Step 数" },
    IsVariable { name: "IS_COUNT_CALL", group: IsGroup::Count, description: "函数调用次数" },
    IsVariable { name: "IS_COUNT_FORK", group: IsGroup::Count, description: "FORK 次数" },
    IsVariable { name: "IS_COUNT_JOIN", group: IsGroup::Count, description: "JOIN 次数" },
    IsVariable { name: "IS_COUNT_ECALL", group: IsGroup::Count, description: "ECALL 次数" },
    IsVariable { name: "IS_COUNT_ALLOC", group: IsGroup::Count, description: "内存分配次数" },
    IsVariable { name: "IS_COUNT_ERROR", group: IsGroup::Count, description: "错误次数" },
    // 调用上下文 (12)
    IsVariable { name: "IS_CALLER", group: IsGroup::CallContext, description: "调用者函数名" },
    IsVariable { name: "IS_CALLEE", group: IsGroup::CallContext, description: "被调用者函数名" },
    IsVariable { name: "IS_CALL_PC", group: IsGroup::CallContext, description: "CALL 指令 PC" },
    IsVariable { name: "IS_CALL_RETURN_PC", group: IsGroup::CallContext, description: "返回 PC" },
    IsVariable { name: "IS_CALL_DEPTH", group: IsGroup::CallContext, description: "调用深度" },
    IsVariable { name: "IS_CALL_STACK_SIZE", group: IsGroup::CallContext, description: "调用栈大小" },
    IsVariable { name: "IS_CALL_STACK_TOP", group: IsGroup::CallContext, description: "栈顶函数" },
    IsVariable { name: "IS_FRAME_SP", group: IsGroup::CallContext, description: "当前帧 SP" },
    IsVariable { name: "IS_FRAME_FP", group: IsGroup::CallContext, description: "当前帧 FP" },
    IsVariable { name: "IS_FRAME_RA", group: IsGroup::CallContext, description: "当前帧 RA" },
    IsVariable { name: "IS_ARG_COUNT", group: IsGroup::CallContext, description: "参数个数" },
    IsVariable { name: "IS_RETURN_VALUE", group: IsGroup::CallContext, description: "返回值" },
    // 系统/环境 (12)
    IsVariable { name: "IS_SYS_PROFILE", group: IsGroup::System, description: "内存 Profile" },
    IsVariable { name: "IS_SYS_MEM_TOTAL", group: IsGroup::System, description: "总内存" },
    IsVariable { name: "IS_SYS_MEM_USED", group: IsGroup::System, description: "已用内存" },
    IsVariable { name: "IS_SYS_MEM_FREE", group: IsGroup::System, description: "空闲内存" },
    IsVariable { name: "IS_SYS_MEM_PEAK", group: IsGroup::System, description: "峰值内存" },
    IsVariable { name: "IS_SYS_QUANTUM", group: IsGroup::System, description: "Quantum 配额" },
    IsVariable { name: "IS_SYS_QUANTUM_USED", group: IsGroup::System, description: "已用 Quantum" },
    IsVariable { name: "IS_SYS_PC", group: IsGroup::System, description: "当前 PC" },
    IsVariable { name: "IS_SYS_STATE", group: IsGroup::System, description: "VM 状态" },
    IsVariable { name: "IS_SYS_PROFILE_NAME", group: IsGroup::System, description: "Profile 名称" },
    IsVariable { name: "IS_SYS_VERSION", group: IsGroup::System, description: "VM 版本" },
    IsVariable { name: "IS_SYS_FLAGS", group: IsGroup::System, description: "系统标志" },
    // 时间 (8)
    IsVariable { name: "IS_TIME_ELAPSED", group: IsGroup::Time, description: "已用时间" },
    IsVariable { name: "IS_TIME_STEP_START", group: IsGroup::Time, description: "当前 Step 开始时间" },
    IsVariable { name: "IS_TIME_STEP_ELAPSED", group: IsGroup::Time, description: "当前 Step 已用时间" },
    IsVariable { name: "IS_TIME_LAST_INSTR", group: IsGroup::Time, description: "上条指令耗时" },
    IsVariable { name: "IS_TIME_BREAK_HIT", group: IsGroup::Time, description: "断点命中时间" },
    IsVariable { name: "IS_TIME_SUSPENDED", group: IsGroup::Time, description: "挂起总时间" },
    IsVariable { name: "IS_TIME_IDLE", group: IsGroup::Time, description: "空闲时间" },
    IsVariable { name: "IS_TIME_TOTAL", group: IsGroup::Time, description: "总运行时间" },
    // 任务 (12)
    IsVariable { name: "IS_TASK_ID", group: IsGroup::Task, description: "任务 ID" },
    IsVariable { name: "IS_TASK_PARENT", group: IsGroup::Task, description: "父任务 ID" },
    IsVariable { name: "IS_TASK_CHILDREN", group: IsGroup::Task, description: "子任务 ID 列表" },
    IsVariable { name: "IS_TASK_PRIORITY", group: IsGroup::Task, description: "任务优先级" },
    IsVariable { name: "IS_TASK_STATUS", group: IsGroup::Task, description: "任务状态" },
    IsVariable { name: "IS_TASK_DEPTH", group: IsGroup::Task, description: "任务深度" },
    IsVariable { name: "IS_TASK_BATCH", group: IsGroup::Task, description: "所属调度批次" },
    IsVariable { name: "IS_TASK_FORK_PC", group: IsGroup::Task, description: "FORK 时的 PC" },
    IsVariable { name: "IS_TASK_JOIN_TARGET", group: IsGroup::Task, description: "JOIN 等待的目标 ID" },
    IsVariable { name: "IS_TASK_SLOT", group: IsGroup::Task, description: "所在内存槽位" },
    IsVariable { name: "IS_TASK_ORIGIN", group: IsGroup::Task, description: "来源 Runner" },
    IsVariable { name: "IS_TASK_TIMEOUT", group: IsGroup::Task, description: "超时设置" },
    // 数据 (12)
    IsVariable { name: "IS_DATA_INPUT_COUNT", group: IsGroup::Data, description: "输入常量数" },
    IsVariable { name: "IS_DATA_OUTPUT_COUNT", group: IsGroup::Data, description: "产出变量数" },
    IsVariable { name: "IS_DATA_INPUT_SIZE", group: IsGroup::Data, description: "输入总大小" },
    IsVariable { name: "IS_DATA_OUTPUT_SIZE", group: IsGroup::Data, description: "产出总大小" },
    IsVariable { name: "IS_DATA_STACK_USED", group: IsGroup::Data, description: "栈使用量" },
    IsVariable { name: "IS_DATA_HEAP_USED", group: IsGroup::Data, description: "堆使用量" },
    IsVariable { name: "IS_DATA_RODATA_SIZE", group: IsGroup::Data, description: "只读数据大小" },
    IsVariable { name: "IS_DATA_ALLOC_COUNT", group: IsGroup::Data, description: "分配次数" },
    IsVariable { name: "IS_DATA_FREE_COUNT", group: IsGroup::Data, description: "释放次数" },
    IsVariable { name: "IS_DATA_FRAG_RATIO", group: IsGroup::Data, description: "碎片率" },
    IsVariable { name: "IS_DATA_SLOT_USAGE", group: IsGroup::Data, description: "槽位使用率" },
    IsVariable { name: "IS_DATA_CACHE_HITS", group: IsGroup::Data, description: "缓存命中数" },
];

/// 按分组获取 IS* 变量列表。
pub fn is_variables_by_group(group: IsGroup) -> Vec<&'static IsVariable> {
    IS_VARIABLES.iter().filter(|v| v.group == group).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn execution_phase_names() {
        assert_eq!(ExecutionPhase::System.name(), "SYSTEM");
        assert_eq!(ExecutionPhase::Task.name(), "TASK");
        assert_eq!(ExecutionPhase::all().len(), 4);
    }

    #[test]
    fn step_status_symbols() {
        assert_eq!(StepStatus::Completed.symbol(), "✓");
        assert_eq!(StepStatus::Error.symbol(), "✗");
        assert_eq!(StepStatus::Skipped.symbol(), "—");
    }

    #[test]
    fn step_record_creation() {
        let step = StepRecord::new("test_step", ExecutionPhase::Task, 42);
        assert_eq!(step.name, "test_step");
        assert_eq!(step.status, StepStatus::Pending);
        assert_eq!(step.source_line, 42);
    }

    #[test]
    fn execution_trace_empty() {
        let trace = ExecutionTrace::new();
        assert!(trace.is_empty());
        assert_eq!(trace.step_count(), 0);
    }

    #[test]
    fn trace_collector_basic() {
        let mut collector = TraceCollector::new();
        collector.start();
        collector.begin_step("step1", ExecutionPhase::Task, 10, 0);
        collector.record_instruction();
        thread::sleep(Duration::from_micros(1));
        collector.end_step(5);

        collector.begin_step("step2", ExecutionPhase::Task, 20, 5);
        collector.record_instruction();
        collector.end_step(10);

        let trace = collector.finalize();
        assert_eq!(trace.step_count(), 2);
        assert_eq!(trace.steps[0].name, "step1");
        assert_eq!(trace.steps[1].name, "step2");
        assert!(trace.total_instructions > 0);
    }

    #[test]
    fn trace_collector_error() {
        let mut collector = TraceCollector::new();
        collector.start();
        collector.begin_step("failing_step", ExecutionPhase::Task, 15, 0);
        collector.mark_step_error("division by zero");
        collector.end_step(3);

        let trace = collector.finalize();
        assert_eq!(trace.step_count(), 1);
        assert_eq!(trace.steps[0].status, StepStatus::Error);
        assert_eq!(trace.steps[0].error_summary.as_deref(), Some("division by zero"));
    }

    #[test]
    fn trace_collector_skipped() {
        let mut collector = TraceCollector::new();
        collector.start();
        collector.begin_step("skipped_step", ExecutionPhase::Task, 20, 0);
        collector.mark_step_skipped();
        collector.end_step(0);

        let trace = collector.finalize();
        assert_eq!(trace.steps[0].status, StepStatus::Skipped);
    }

    #[test]
    fn trace_collector_variable_events() {
        let mut collector = TraceCollector::new();
        collector.start();
        collector.begin_step("var_step", ExecutionPhase::Task, 5, 0);
        collector.record_variable("x", "int", Some(0), 42, 1, 5, "var_step", false);
        collector.end_step(2);

        let trace = collector.finalize();
        assert_eq!(trace.variable_events.len(), 1);
        assert_eq!(trace.variable_events[0].name, "x");
        assert_eq!(trace.variable_events[0].new_value, 42);
    }

    #[test]
    fn trace_find_step_by_name() {
        let mut trace = ExecutionTrace::new();
        trace.steps.push(StepRecord::new("alpha", ExecutionPhase::Task, 1));
        trace.steps.push(StepRecord::new("beta", ExecutionPhase::Input, 2));

        assert!(trace.find_step_by_name("alpha").is_some());
        assert!(trace.find_step_by_name("gamma").is_none());
        assert_eq!(trace.steps_by_phase(ExecutionPhase::Input).len(), 1);
    }

    #[test]
    fn is_variables_count() {
        assert_eq!(IS_VARIABLES.len(), 72);
    }

    #[test]
    fn is_variables_by_group_count() {
        assert_eq!(is_variables_by_group(IsGroup::Exception).len(), 8);
        assert_eq!(is_variables_by_group(IsGroup::Count).len(), 8);
        assert_eq!(is_variables_by_group(IsGroup::CallContext).len(), 12);
        assert_eq!(is_variables_by_group(IsGroup::System).len(), 12);
        assert_eq!(is_variables_by_group(IsGroup::Time).len(), 8);
        assert_eq!(is_variables_by_group(IsGroup::Task).len(), 12);
        assert_eq!(is_variables_by_group(IsGroup::Data).len(), 12);
    }

    #[test]
    fn works_phase_symbol() {
        assert_eq!(WorksPhase::Init.symbol(), "INIT");
        assert_eq!(WorksPhase::Done.symbol(), "DONE");
        assert_eq!(WorksPhase::Error("msg".into()).symbol(), "ERROR");
    }

    #[test]
    fn sub_call_fn() {
        let call = SubCall::FnCall {
            name: "foo".into(),
            args: vec!["42".into()],
            result: Some("84".into()),
            elapsed_us: 100,
        };
        match call {
            SubCall::FnCall { name, .. } => assert_eq!(name, "foo"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn collector_skips_events_when_disabled() {
        let mut collector = TraceCollector::new();
        collector.collect_variables = false;
        collector.collect_hooks = false;
        collector.start();
        collector.begin_step("s", ExecutionPhase::Task, 1, 0);
        collector.record_variable("x", "int", None, 1, 0, 1, "s", false);
        collector.record_hook("w", "h", None, "a", StepStatus::Completed, 0, false);
        collector.end_step(1);
        let trace = collector.finalize();
        assert!(trace.variable_events.is_empty());
        assert!(trace.hook_timeline.is_empty());
    }

    #[test]
    fn step_for_pc() {
        let mut trace = ExecutionTrace::new();
        let mut s = StepRecord::new("s1", ExecutionPhase::Task, 1);
        s.pc_range = (0, 10);
        trace.steps.push(s);
        assert!(trace.step_for_pc(5).is_some());
        assert!(trace.step_for_pc(15).is_none());
    }
}
