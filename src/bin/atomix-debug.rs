//! Atomix 调试器 — 独立入口。
//!
//! 用法:
//!   atomix-debug <file.atxe>
//!
//! 加载编译好的 .atxe 文件，进入交互式 REPL 调试会话。
//!
//! 所有核心逻辑复用自：
//! - `runner::VmState::load_atxe()` — 加载二进制
//! - `debug::repl::DebugSession` — 调试会话
//! - `debug::disassemble` — 反汇编器

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("用法: atomix-debug <file.atxe>");
        eprintln!("加载 .atxe 文件并进入交互式调试会话。");
        process::exit(1);
    }

    let path = &args[1];

    // 读取 .atxe 文件
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("无法读取文件 '{}': {}", path, e);
            process::exit(1);
        }
    };

    // 加载 VM 状态
    let vm = match atomix::runner::VmState::load_atxe(&bytes) {
        Ok(vm) => vm,
        Err(e) => {
            eprintln!("无法加载 .atxe: {}", e);
            process::exit(1);
        }
    };

    println!("Loaded: {} ({} instructions, {} bytes debug)", 
        path,
        vm.text.len(),
        vm.debug_info.len(),
    );

    // 进入调试 REPL
    let debug_bytes = vm.debug_info.clone();
    let mut session = atomix::debug::repl::DebugSession::new(vm);
    session.set_debug_map_from_bytes(&debug_bytes);
    // 尝试加载同名的 .atx 源码文件
    let atx_path = std::path::Path::new(path).with_extension("atx");
    if atx_path.exists() {
        session.set_source(atx_path.to_str().unwrap());
    }
    atomix::debug::repl::run_repl(&mut session);
}
