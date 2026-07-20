//! atomix-runner — 独立运行时执行器。
//!
//! 子命令:
//!   run <file.atxe>    加载并执行 .atxe（默认行为）
//!   daemon             启动 ATXP 服务器，常驻等待远程任务
//!
//! 支持从 runner.toml 加载配置。

use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser)]
#[command(
    name = "atomix-runner",
    about = "Atomix 运行时执行器 — 执行任务 / 常驻服务"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 加载并执行 .atxe（单次运行）
    Run {
        /// .atxe 文件路径
        file: PathBuf,
        /// 配置文件路径
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// 启动 ATXP 服务器，常驻等待远程任务
    Daemon {
        /// 监听地址（默认 0.0.0.0:9000）
        #[arg(short, long, default_value = "0.0.0.0:9000")]
        listen: String,
        /// 配置文件路径
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Run { file, config } => {
            let config_str = config.as_ref().and_then(|p| p.to_str());
            cmd_run(&file, config_str);
        }
        Command::Daemon { listen, config } => {
            let config_str = config.as_ref().and_then(|p| p.to_str());
            cmd_daemon(&listen, config_str);
        }
    }
}

/// 单次运行模式：加载 .atxe 并执行。
fn cmd_run(file: &PathBuf, config_path: Option<&str>) {
    let bytes = match fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("错误: 无法读取文件 {}: {}", file.display(), e);
            std::process::exit(1);
        }
    };

    let binary = match atomix::base::ir::AtxeBinary::from_bytes(&bytes) {
        Some(b) => b,
        None => {
            eprintln!("错误: 无效的 .atxe 文件");
            std::process::exit(1);
        }
    };

    println!("已加载: {} 条指令", binary.header.total_instrs);

    let config = config_path.and_then(|p| {
        match atomix::runner::config::RunnerConfig::load(Some(p)) {
            Ok(cfg) => { println!("已加载配置: {}", p); Some(cfg) }
            Err(e) => { eprintln!("警告: 配置加载失败 ({}), 使用默认配置", e); None }
        }
    });

    let mut runtime = match atomix::runner::runtime::Runtime::from_atxe(&binary, config.as_ref(), None) {
        Ok(rt) => rt,
        Err(e) => { eprintln!("错误: 创建 Runtime 失败: {}", e); std::process::exit(1); }
    };

    match runtime.run() {
        Ok(()) => {
            println!("\n执行完成: 总计 {} 条指令", runtime.total_instrs);
            for (id, status, retval, instrs) in runtime.results() {
                let s = match status {
                    atomix::runner::task::TaskStatus::Done => "完成",
                    atomix::runner::task::TaskStatus::Error => "出错",
                    _ => "其他",
                };
                println!("  Task {}: {} ({} 条指令, 返回值: {})", id, s, instrs, retval);
            }
        }
        Err(e) => { eprintln!("\n执行错误: {}", e); std::process::exit(1); }
    }
}

/// 常驻服务模式：启动 Runtime + ATXP 服务器。
#[tokio::main]
async fn cmd_daemon(listen: &str, config_path: Option<&str>) {
    println!("Atomix Runner 守护进程启动...");
    println!("监听地址: {}", listen);

    // 加载配置
    let config = config_path.and_then(|p| {
        match atomix::runner::config::RunnerConfig::load(Some(p)) {
            Ok(cfg) => { println!("已加载配置: {}", p); Some(cfg) }
            Err(e) => { eprintln!("警告: 配置加载失败 ({}), 使用默认配置", e); None }
        }
    });

    // 创建空的 Runtime（没有初始任务）
    // Runtime::from_atxe 需要一个二进制，但我们还没有任务
    // 创建一个最小化的 .atxe 作为占位
    let header = atomix::base::ir::Header::new(0, 1);
    let empty_binary = atomix::base::ir::AtxeBinary {
        header,
        sections: Vec::new(),
        text: vec![0; 1],
        rodata: vec![],
        task_table: vec![],
        debug_info: vec![],
        exn_table: vec![],
        zones: vec![],
    };

    let runtime = match atomix::runner::runtime::Runtime::from_atxe(&empty_binary, config.as_ref(), None) {
        Ok(rt) => {
            println!("Runtime 已初始化 ({} 个执行器)", rt.executors.len());
            rt
        }
        Err(e) => {
            eprintln!("错误: 创建 Runtime 失败: {}", e);
            std::process::exit(1);
        }
    };

    // 创建 ATXP 服务器
    let server = atomix::runner::server::AtxpServer {
        config: atomix::runner::server::ServerConfig {
            listen_addr: listen.to_string(),
        },
        runtime: Arc::new(Mutex::new(runtime)),
    };

    // 运行服务器（阻塞）
    if let Err(e) = server.run().await {
        eprintln!("服务器错误: {}", e);
        std::process::exit(1);
    }

    println!("Atomix Runner 守护进程已停止。");
}
