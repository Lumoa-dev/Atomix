//! Atomix 调试器 — 原生派生调试工具。
//!
//! 复用 runner 内部的 VM/decode/execute 代码，提供交互式 REPL 调试体验。
//!
//! # 架构
//!
//! ```text
//! atomix-debug <file.atxe>
//!     └── DebugSession (repl.rs)
//!           ├── 反汇编器 (disassemble.rs)   ← 复用 runner::decode
//!           ├── 执行控制                    ← 复用 runner::execute
//!           ├── 内存查看                    ← 复用 runner::memory
//!           └── 断点管理                    ← 指令替换 + TRAP
//! ```

pub mod debug_segment;
pub mod disassemble;
pub mod repl;
