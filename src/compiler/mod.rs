//! Atomix 编译器。
//!
//! 完成 .atx 源码到 .atxe 二进制产物的完整编译管线：
//!
//! 词法分析 → 语法分析 → 语义分析 → IR 生成 → 优化 → 链接
//!
//! 详见 docs/04-编译管线.md。

pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod symbol;
pub mod type_checker;
pub mod semantic;
