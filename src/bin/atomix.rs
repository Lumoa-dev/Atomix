//! Atomix CLI — 统一用户入口。
//!
//! 按子命令派发到对应的后端逻辑（编译、执行、深度检查等）。
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
    /// 执行引擎（本地跑任务）
    Runner {
        #[command(subcommand)]
        action: RunnerAction,
    },
    /// 深度检查 + 脚手架（本地 debug / 远程监控）
    Task {
        /// 任务名称或文件路径
        name: String,
        /// 远程 runner 别名（指定后走远程监控模式）
        #[arg(long)]
        origin: Option<String>,
    },
    /// 远程连接管理
    Origin {
        #[arg(long)]
        add: Option<String>,
        #[arg(long = "ip")]
        ip: Option<String>,
        #[arg(long = "as")]
        as_name: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        list: bool,
        #[arg(long)]
        remove: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
}

#[derive(Subcommand)]
enum RunnerAction {
    /// 运行任务
    Run {
        /// 任务名称或文件路径（空 = 全部）
        name: Option<String>,
        /// 远程 runner 别名
        #[arg(long)]
        origin: Option<String>,
    },
    /// 查看引擎运行状态
    Status,
    /// 停止引擎
    Stop,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Build {
            source,
            opt_level,
            output,
        } => cmd_build(&source, &opt_level, output.as_deref()),
        Command::Check { source } => cmd_check(&source),
        Command::Clean => cmd_clean(),
        Command::Runner { action } => match action {
            RunnerAction::Run { name, origin } => cmd_runner_run(name.as_deref(), origin.as_deref()),
            RunnerAction::Status => cmd_runner_status(),
            RunnerAction::Stop => cmd_runner_stop(),
        },
        Command::Task { name, origin } => cmd_task(&name, origin.as_deref()),
        Command::Origin { add, ip, as_name, port, list, remove, status } => {
            if list { cmd_origin_list(); }
            else if let Some(alias) = remove { cmd_origin_remove(&alias); }
            else if let Some(alias) = status { cmd_origin_status(&alias); }
            else if let Some(alias) = add {
                let addr = ip.unwrap_or_else(|| {
                    eprintln!("错误: --add 需要 --ip <地址>");
                    std::process::exit(1);
                });
                let port = port.unwrap_or(9000);
                let name = as_name.unwrap_or_else(|| alias.clone());
                cmd_origin_add(&name, &addr, port);
            } else {
                eprintln!("用法: atomix origin --add -ip <地址> -as <别名> [--port <端口>]");
                eprintln!("       atomix origin --list");
                eprintln!("       atomix origin --remove <别名>");
                eprintln!("       atomix origin --status <别名>");
                std::process::exit(1);
            }
        }
    }
}

// ─── Build ─────────────────────────────────────────────

fn cmd_build(source: &str, opt_level: &str, output: Option<&str>) {
    let source_content = match fs::read_to_string(source) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("错误: 无法读取源文件 `{source}`: {e}");
            std::process::exit(1);
        }
    };

    let (binary, errors) = atomix::compiler::compile(&source_content, opt_level);

    if !errors.is_empty() {
        for err in &errors {
            eprintln!("{}", err);
        }
        if binary.is_empty() {
            std::process::exit(1);
        }
    }

    let output_path: String = match output {
        Some(p) => p.to_string(),
        None => {
            let p = Path::new(source);
            if p.extension().is_some_and(|e| e == "atx") {
                p.with_extension("atxe").to_string_lossy().to_string()
            } else {
                format!("{}.atxe", source)
            }
        }
    };

    if let Err(e) = fs::write(&output_path, &binary) {
        eprintln!("错误: 无法写入输出文件 `{output_path}`: {e}");
        std::process::exit(1);
    }

    println!("编译成功: {} → {} ({} 字节)", source, output_path, binary.len());
}

fn cmd_check(source: &str) {
    let source_content = match fs::read_to_string(source) {
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

fn cmd_clean() {
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

// ─── Runner ────────────────────────────────────────────

fn cmd_runner_run(name: Option<&str>, _origin: Option<&str>) {
    // 暂时只支持本地直接跑（无 --origin）
    if let Some(task_name) = name {
        // 检查是指定的 .atxe 还是 .atx
        let path = Path::new(task_name);
        if path.extension().is_some_and(|e| e == "atxe") {
            // 直接加载运行
            let bytes = match fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("错误: 无法读取文件 `{task_name}`: {e}");
                    std::process::exit(1);
                }
            };
            let vm = match atomix::runner::VmState::load_atxe(&bytes) {
                Ok(vm) => vm,
                Err(e) => {
                    eprintln!("无法加载 .atxe: {e}");
                    std::process::exit(1);
                }
            };
            run_vm_and_report(vm);
        } else {
            // 编译 .atx 再运行
            let source = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("错误: 无法读取源文件 `{task_name}`: {e}");
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
            let vm = match atomix::runner::VmState::load_atxe(&binary) {
                Ok(vm) => vm,
                Err(e) => {
                    eprintln!("无法加载编译产物: {e}");
                    std::process::exit(1);
                }
            };
            run_vm_and_report(vm);
        }
    } else {
        eprintln!("用法: atomix runner run <name>");
        std::process::exit(1);
    }
}

fn run_vm_and_report(mut vm: atomix::runner::VmState) {
    use atomix::runner::execute::execute_instruction;
    use atomix::runner::VmStateKind;

    let start = std::time::Instant::now();
    let mut instr_count = 0u64;
    while vm.is_running() {
        execute_instruction(&mut vm);
        instr_count += 1;
    }
    let elapsed = start.elapsed();

    match vm.state {
        VmStateKind::Halted => {
            let retval = vm.read_reg(4); // A0
            println!("任务完成: {} 条指令, {:.2?}, 返回值 = {}", instr_count, elapsed, retval as i64);
        }
        VmStateKind::Error(ref msg) => {
            eprintln!("任务错误 ({} 条指令后): {}", instr_count, msg);
        }
        _ => {
            println!("任务结束 ({} 条指令, {:.2?})", instr_count, elapsed);
        }
    }
}

fn cmd_runner_status() {
    println!("Runner 状态: (本地模式)");
    println!("  TODO: 实现 status 端点查询");
}

fn cmd_runner_stop() {
    println!("Runner 停止: (本地模式)");
    println!("  TODO: 实现 stop 端点");
}

// ─── Origin（远程连接管理）────────────────────────────

fn cmd_origin_add(alias: &str, address: &str, port: u16) {
    let mut config = atomix::origin::OriginConfig::load();
    config.upsert(atomix::origin::OriginEntry {
        alias: alias.to_string(),
        address: address.to_string(),
        port,
    });
    if let Err(e) = config.save() {
        eprintln!("保存配置失败: {}", e);
        std::process::exit(1);
    }
    println!("远程连接已添加: {} = {}:{}", alias, address, port);
}

fn cmd_origin_list() {
    let config = atomix::origin::OriginConfig::load();
    if config.connection.is_empty() {
        println!("（无远程连接）");
        return;
    }
    println!("远程连接列表:");
    for entry in &config.connection {
        println!("  {} = {}:{}", entry.alias, entry.address, entry.port);
    }
}

fn cmd_origin_remove(alias: &str) {
    let mut config = atomix::origin::OriginConfig::load();
    if config.remove(alias) {
        if let Err(e) = config.save() {
            eprintln!("保存配置失败: {}", e);
            std::process::exit(1);
        }
        println!("远程连接已删除: {}", alias);
    } else {
        println!("未找到远程连接: {}", alias);
    }
}

fn cmd_origin_status(alias: &str) {
    let config = atomix::origin::OriginConfig::load();
    match config.find(alias) {
        Some(entry) => {
            println!("正在连接 {}:{} ...", entry.address, entry.port);
            match atomix::origin::check_status(entry) {
                Ok(status) => {
                    println!("远程状态:");
                    println!("  {}", serde_json::to_string_pretty(&status).unwrap_or_default());
                }
                Err(e) => {
                    eprintln!("连接失败: {}", e);
                    std::process::exit(1);
                }
            }
        }
        None => {
            eprintln!("未找到远程连接: {}", alias);
            std::process::exit(1);
        }
    }
}

// ─── Task（深度检查 / 本地 debug）────────────────────

fn cmd_task(name: &str, origin: Option<&str>) {
    let path = Path::new(name);

    if let Some(_remote) = origin {
        // 远程监控模式：通过 ATXP 协议连接远程 runner
        // 当前未实现，占位
        eprintln!("远程检查模式尚未实现（需要通过 ATXP 协议连接远程 runner）");
        std::process::exit(1);
    }

    // 本地深度检查（debug）模式
    let vm = if path.extension().is_some_and(|e| e == "atx") {
        // 编译源文件
        let source = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("错误: 无法读取源文件 `{name}`: {e}");
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
            Ok(vm) => {
                println!("编译成功: {} ({} 指令, {} bytes debug)",
                    name, vm.text.len(), vm.debug_info.len());
                vm
            }
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
                eprintln!("错误: 无法读取文件 `{name}`: {e}");
                std::process::exit(1);
            }
        };
        match atomix::runner::VmState::load_atxe(&bytes) {
            Ok(vm) => {
                println!("加载成功: {} ({} 指令, {} bytes debug)",
                    name, vm.text.len(), vm.debug_info.len());
                vm
            }
            Err(e) => {
                eprintln!("无法加载 .atxe: {e}");
                std::process::exit(1);
            }
        }
    };

    // 进入调试 REPL
    let debug_bytes = vm.debug_info.clone();
    let mut session = atomix::debug::repl::DebugSession::new(vm);

    // 加载 .debug 映射
    session.set_debug_map_from_bytes(&debug_bytes);

    // 尝试加载源码
    if path.extension().is_some_and(|e| e == "atx") {
        session.set_source(name);
    } else {
        let atx_path = path.with_extension("atx");
        if atx_path.exists() {
            session.set_source(atx_path.to_str().unwrap_or(""));
        }
    }

    atomix::debug::repl::run_repl(&mut session);
}
