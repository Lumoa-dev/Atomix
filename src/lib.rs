//! Atomix — 任务执行 DSL 的编译器与运行时。
//!
//! 核心架构：
//! - `base` — 基础设施：ISA 定义、IR 二进制格式、错误类型
//! - `compiler` — 编译管线：词法/语法/语义分析、IR 生成、优化、链接

pub mod base;
pub mod compiler;
