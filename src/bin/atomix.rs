//! Atomix CLI — 统一用户入口。
//!
//! 按子命令派发到对应的后端逻辑（编译、运行、包管理等）。
//! 所有后端逻辑共享同一套库代码（src/lib.rs）。

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "atomix",
    version,
    about = "Atomix — 任务执行 DSL 编译器与运行时"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 编译任务（产出 .atxe）
    Build {
        /// 源文件路径
        source: String,
        /// 优化级别: 0, 1, 2, s (默认 0)
        #[arg(long = "opt", default_value = "0")]
        opt_level: String,
    },
    /// 语法与类型检查（不产出产物）
    Check {
        /// 源文件路径
        source: String,
    },
    /// 清理构建产物
    Clean,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Build { source, opt_level } => {
            println!("编译: {source} (优化级别: {opt_level})");
            // TODO: Phase 1 完成后调用完整编译管线
        }
        Command::Check { source } => {
            println!("检查: {source}");
            // TODO: Phase 1 完成后调用词法/语法/语义检查
        }
        Command::Clean => {
            println!("清理构建产物");
            // TODO: 清理 .atomix/build/ 和 output/
        }
    }
}
