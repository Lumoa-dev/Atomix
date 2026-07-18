//! Atomix IR 代码生成器。
//!
//! 将语义分析后的 AST 编译为 .atxe 二进制格式的指令序列。
//! 详见 docs/04-编译管线.md §5、docs/02-指令集规范.md §4。

pub mod instr;
pub mod expr;
pub mod stmt;
