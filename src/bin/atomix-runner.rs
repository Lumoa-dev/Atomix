//! atomix-runner — 独立运行时执行器。
//!
//! 加载 .atxe 并使用 Runtime 执行所有任务。
//! 支持开发模式单次执行和生产模式常驻。

use clap::Parser;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "atomix-runner",
    about = "Atomix 运行时执行器 — 加载并执行 .atxe"
)]
struct Args {
    /// .atxe 文件路径
    file: PathBuf,
}

fn main() {
    let args = Args::parse();

    // 读取 .atxe 文件
    let bytes = match fs::read(&args.file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("错误: 无法读取文件 {}: {}", args.file.display(), e);
            std::process::exit(1);
        }
    };

    // 解码 .atxe
    let binary = match atomix::base::ir::AtxeBinary::from_bytes(&bytes) {
        Some(b) => {
            println!(
                "已加载: {} 条指令, {} 个 section, .task 段 {} 字节",
                b.header.total_instrs,
                b.sections.len(),
                b.task_table.len(),
            );
            b
        }
        None => {
            eprintln!("错误: 无效的 .atxe 文件");
            std::process::exit(1);
        }
    };

    // 创建 Runtime
    let mut runtime = match atomix::runner::runtime::Runtime::from_atxe(&binary) {
        Ok(rt) => {
            println!(
                "Runtime 已初始化: {} 个任务, N_batch={}",
                rt.pool.len(),
                rt.executors.len(),
            );
            rt
        }
        Err(e) => {
            eprintln!("错误: 创建 Runtime 失败: {}", e);
            std::process::exit(1);
        }
    };

    // 运行所有任务
    match runtime.run() {
        Ok(()) => {
            println!("\n执行完成: 总计 {} 条指令", runtime.total_instrs);
            println!("任务结果:");
            for (id, status, retval, instrs) in runtime.results() {
                let status_str = match status {
                    atomix::runner::task::TaskStatus::Done => "完成",
                    atomix::runner::task::TaskStatus::Error => "出错",
                    _ => "其他",
                };
                println!(
                    "  Task {}: {} ({} 条指令, 返回值: {})",
                    id, status_str, instrs, retval
                );
            }
        }
        Err(e) => {
            eprintln!("\n执行错误: {}", e);
            std::process::exit(1);
        }
    }
}
