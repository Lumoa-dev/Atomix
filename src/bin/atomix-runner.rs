//! atomix-runner — 独立运行时执行器。
//!
//! 加载 .atxe 并执行任务。支持开发模式单次执行和生产模式常驻。
//! Phase 2 实现完整 VM 执行逻辑。

use clap::Parser;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "atomix-runner", about = "Atomix 运行时执行器 — 加载并执行 .atxe")]
struct Args {
    /// .atxe 文件路径
    file: PathBuf,

    /// 最大执行指令数（0 = 无限制）
    #[arg(long = "max-instr", default_value = "100000")]
    max_instr: u64,
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

    // 加载到 VM
    let mut vm = match atomix::vm::VmState::load_atxe(&bytes) {
        Ok(vm) => {
            println!("VM 已加载: {} 条指令, entry={}", vm.text.len(), vm.pc);
            vm
        }
        Err(e) => {
            eprintln!("错误: 加载 .atxe 失败: {}", e);
            std::process::exit(1);
        }
    };

    // 执行
    let mut instr_count = 0u64;
    let max_instr = if args.max_instr == 0 { u64::MAX } else { args.max_instr };

    while vm.is_running() && instr_count < max_instr {
        atomix::vm::execute::execute_instruction(&mut vm);
        instr_count += 1;
    }

    // 输出结果
    match &vm.state {
        atomix::vm::VmStateKind::Halted => {
            println!("执行完成: {} 条指令", instr_count);
            println!("R4 (返回值): {}", vm.read_reg(4));
        }
        atomix::vm::VmStateKind::Error(e) => {
            eprintln!("执行错误: {} ({} 条指令后)", e, instr_count);
            std::process::exit(1);
        }
        _ => {
            println!("执行停止: {} 条指令 (状态: {:?})", instr_count, vm.state);
        }
    }
}
