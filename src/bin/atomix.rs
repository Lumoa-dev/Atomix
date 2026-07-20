//! Atomix CLI — 统一用户入口。
//!
//! 按子命令派发到对应的后端逻辑（编译、运行、包管理等）。
//! 所有后端逻辑共享同一套库代码（src/lib.rs）。

use clap::{Parser, Subcommand};
use std::fs;
use std::path::Path;

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
        /// 输出路径（默认 source 替换 .atx 为 .atxe）
        #[arg(short = 'o')]
        output: Option<String>,
    },
    /// 语法与类型检查（不产出产物）
    Check {
        /// 源文件路径
        source: String,
    },
    /// 清理构建产物
    Clean,
    /// 调试任务（进入交互式 REPL）
    Debug {
        /// .atxe 或 .atx 文件路径
        file: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Build {
            source,
            opt_level,
            output,
        } => {
            let source_content = match fs::read_to_string(&source) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("错误: 无法读取源文件 `{source}`: {e}");
                    std::process::exit(1);
                }
            };

            let (binary, errors) = atomix::compiler::compile(&source_content, &opt_level);

            if !errors.is_empty() {
                for err in &errors {
                    eprintln!("{}", err);
                }
                if binary.is_empty() {
                    std::process::exit(1);
                }
            }

            let output_path = output.unwrap_or_else(|| {
                let p = Path::new(&source);
                if p.extension().is_some_and(|e| e == "atx") {
                    p.with_extension("atxe").to_string_lossy().to_string()
                } else {
                    format!("{}.atxe", source)
                }
            });

            if let Err(e) = fs::write(&output_path, &binary) {
                eprintln!("错误: 无法写入输出文件 `{output_path}`: {e}");
                std::process::exit(1);
            }

            println!("编译成功: {} → {} ({} 字节)", source, output_path, binary.len());
        }
        Command::Check { source } => {
            let source_content = match fs::read_to_string(&source) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("错误: 无法读取源文件 `{source}`: {e}");
                    std::process::exit(1);
                }
            };

            let (_, errors) = atomix::compiler::compile(&source_content, "0");

            if errors.is_empty() {
                println!("检查通过: {source}");
            } else {
                for err in &errors {
                    eprintln!("{}", err);
                }
                std::process::exit(1);
            }
        }
        Command::Clean => {
            let build_dir = Path::new(".atomix");
            if build_dir.exists() {
                let _ = fs::remove_dir_all(build_dir);
            }
            let output_dir = Path::new("output");
            if output_dir.exists() {
                let _ = fs::remove_dir_all(output_dir);
            }
            println!("清理完成");
        }
        Command::Debug { file } => {
            let path = Path::new(&file);

            // 判断是 .atx（需编译）还是 .atxe（直接加载）
            let vm = if path.extension().is_some_and(|e| e == "atx") {
                // 编译源文件
                let source = match fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("错误: 无法读取源文件 `{file}`: {e}");
                        std::process::exit(1);
                    }
                };
                let (binary, errors) = atomix::compiler::compile(&source, "0");
                if !errors.is_empty() {
                    for err in &errors {
                        eprintln!("{}", err);
                    }
                    if binary.is_empty() {
                        std::process::exit(1);
                    }
                }
                match atomix::runner::VmState::load_atxe(&binary) {
                    Ok(vm) => vm,
                    Err(e) => {
                        eprintln!("无法加载编译产物: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                // 直接加载 .atxe
                let bytes = match fs::read(path) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("错误: 无法读取文件 `{file}`: {e}");
                        std::process::exit(1);
                    }
                };
                match atomix::runner::VmState::load_atxe(&bytes) {
                    Ok(vm) => vm,
                    Err(e) => {
                        eprintln!("无法加载 .atxe: {e}");
                        std::process::exit(1);
                    }
                }
            };

            println!("Loaded: {} ({} instructions, {} bytes debug info)", file, vm.text.len(), vm.debug_info.len());

            // 解析 .debug 段
            let debug_bytes = vm.debug_info.clone();
            let mut session = atomix::debug::repl::DebugSession::new(vm);
            // 加载 .debug 映射
            session.set_debug_map_from_bytes(&debug_bytes);
            // 如果是 .atx 源文件，加载源码
            if path.extension().is_some_and(|e| e == "atx") {
                session.set_source(&file);
            } else {
                // 尝试找同名的 .atx 文件
                let atx_path = path.with_extension("atx");
                if atx_path.exists() {
                    session.set_source(atx_path.to_str().unwrap_or(""));
                }
            }

            atomix::debug::repl::run_repl(&mut session);
        }
    }
}
