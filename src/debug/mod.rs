//! Atomix 调试器 — 原生调试工具。
//!
//! 复用 runner 内部的 VM/decode/execute 代码，提供交互式调试体验。
//!
//! # 架构
//!
//! ```text
//! atomix-debug / atomix task <file>
//!     └── session::LocalDebugSession (session.rs)
//!           ├── trace::ExecutionTrace     执行轨迹收集
//!           ├── trace::TraceCollector     执行时数据采集
//!           ├── disassemble (disassemble.rs)   反汇编器 ← 复用 runner::decode
//!           ├── eval (eval.rs)                 表达式求值器
//!           ├── debug_segment (debug_segment.rs) ADBG .debug 段解析
//!           ├── repl (repl.rs)                  CLI REPL 前端（兼容保留）
//!           └── tui (tui/)                      TUI 前端（待实现）
//! ```
//!
//! # 设计文档对照
//!
//! | 模块 | 对应文档章节 | 状态 |
//! |------|-------------|------|
//! | trace.rs | §6.3 关键数据结构、§3.17 IS* 变量 | ✅ 已实现 |
//! | session.rs | §6.1 模块复用、§6.2 数据流 | ✅ 已实现 |
//! | repl.rs | §4 命令体系（CLI 模式） | ⚠️ 增强中 |
//! | disassemble.rs | §3.11 IR/Disassembly | ✅ 已实现 |
//! | eval.rs | §4.6 表达式求值 | ✅ 已实现 |
//! | debug_segment.rs | §3.18 Segment Info .debug 段 | ✅ 已实现 |
//! | tui/ | §2 TUI 布局、§3 页面体系 | 🚧 实现中 |

pub mod trace;
pub mod session;
pub mod debug_segment;
pub mod disassemble;
pub mod eval;
pub mod repl;

// 重新导出常用类型
pub use trace::{
    ExecutionTrace, StepRecord, StepStatus, ExecutionPhase,
    TraceCollector, VariableEvent, IsEvent, HookEvent,
    SubCall, WorksPhase, IsVariable, IsGroup,
    IS_VARIABLES, is_variables_by_group,
};
pub use session::{
    DebugSession, LocalDebugSession, Breakpoint, BreakpointType,
    Watchpoint, CommandHistory, FrameState, DisplayFormat,
    PerfCounters, IsContextSnapshot, ExceptionDetail,
};
pub use disassemble::format_instruction;
pub use debug_segment::DebugMap;
