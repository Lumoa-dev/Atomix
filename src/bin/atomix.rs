//! Atomix CLI — 统一用户入口。
//!
//! 按子命令派发到对应的后端逻辑（编译、执行、深度检查等）。
//! 所有后端逻辑共享同一套库代码（src/lib.rs）。

use atomix::debug::DebugSession;
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
    ///
    /// 设计文档 §5.1（本地调试 CLI）
    Task {
        /// 任务名称或文件路径
        name: String,
        /// 远程 runner 别名（指定后走远程监控模式）
        #[arg(long)]
        origin: Option<String>,
        /// 进入本地 TUI，跳过默认运行
        #[arg(long)]
        no_run: bool,
        /// 运行并直接查看指定 Step
        #[arg(long)]
        step: Option<String>,
        /// 运行后打印表达式值
        #[arg(long)]
        print: Option<String>,
        /// 检查断点命中情况
        #[arg(long)]
        check: bool,
        /// 运行前设置断点（行号）
        #[arg(long)]
        break_line: Option<u32>,
        /// 导出 VM 快照到文件
        #[arg(long)]
        export_state: Option<String>,
        /// 导出数据追踪图 SVG
        #[arg(long)]
        export_dataflow: bool,
        /// 运行并记录日志到文件
        #[arg(long)]
        log: Option<String>,
        /// 列出所有 Step
        #[arg(long)]
        list_steps: bool,
        /// 列出变量及最终值
        #[arg(long)]
        list_vars: bool,
        /// 列出 IS* 最终状态
        #[arg(long)]
        list_is: bool,
        /// 反汇编指定地址
        #[arg(long)]
        disasm: Option<String>,
        /// 内存 dump（格式: addr,len）
        #[arg(long)]
        mem_dump: Option<String>,
    },
    /// 远程连接管理（设计文档 §5.2）
    Origin {
        #[command(subcommand)]
        action: OriginAction,
    },
}

#[derive(Subcommand)]
enum OriginAction {
    /// 连接到远程 runner 并进入远程 TUI
    Connect {
        /// 连接别名
        alias: String,
    },
    /// 断开远程连接
    Disconnect {
        /// 要断开的别名（默认所有）
        alias: Option<String>,
    },
    /// 列出所有远程连接
    List,
    /// 查看远程 runner 状态
    Status {
        /// 连接别名
        alias: Option<String>,
    },
    /// 远程任务管理
    Task {
        #[command(subcommand)]
        action: OriginTaskAction,
    },
    /// 远程 runner 管理
    Runner {
        #[command(subcommand)]
        action: OriginRunnerAction,
    },
    /// 查看任务池分布
    Pool {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 按状态过滤
        #[arg(long)]
        status: Option<String>,
    },
    /// 查看控制器面板
    Controller {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 显示历史趋势
        #[arg(long)]
        history: bool,
    },
    /// 查看内存槽位布局
    Slots {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 紧凑显示
        #[arg(long)]
        compact: bool,
    },
    /// 远程日志管理
    Log {
        #[command(subcommand)]
        action: OriginLogAction,
    },
    /// 远程性能分析
    Perf {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        #[command(subcommand)]
        action: OriginPerfAction,
    },
    /// 远程数据导出
    Export {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        #[command(subcommand)]
        action: OriginExportAction,
    },
}

#[derive(Subcommand)]
enum OriginTaskAction {
    /// 列出任务
    List {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 按状态过滤
        #[arg(long)]
        status: Option<String>,
        /// 限制数量
        #[arg(long)]
        limit: Option<usize>,
        /// 排序字段
        #[arg(long)]
        sort: Option<String>,
    },
    /// 查看任务详情
    Show {
        /// 任务 ID
        id: String,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 显示寄存器
        #[arg(long)]
        regs: bool,
        /// 显示内存
        #[arg(long)]
        mem: bool,
        /// 显示调用栈
        #[arg(long)]
        bt: bool,
        /// 显示 IS*
        #[arg(long)]
        is: bool,
    },
    /// 提交任务
    Submit {
        /// 源文件或 .atxe 路径
        file: String,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 任务名称
        #[arg(long)]
        name: Option<String>,
        /// 运行模式
        #[arg(long)]
        mode: Option<String>,
        /// 优化级别
        #[arg(long)]
        opt: Option<String>,
        /// 超时（秒）
        #[arg(long)]
        timeout: Option<u64>,
        /// 等待完成并下载产出
        #[arg(long)]
        wait: bool,
        /// 输出路径
        #[arg(long)]
        output: Option<String>,
    },
    /// 取消任务
    Cancel {
        /// 任务 ID
        id: String,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
    },
    /// 获取任务产出
    Output {
        /// 任务 ID
        id: String,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 保存到文件
        #[arg(long)]
        file: Option<String>,
    },
    /// 获取任务日志
    Log {
        /// 任务 ID
        id: String,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 行数
        #[arg(long, default_value = "50")]
        lines: usize,
    },
}

#[derive(Subcommand)]
enum OriginRunnerAction {
    /// 查看/修改 Runner 配置
    Config {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 获取配置项
        #[arg(long)]
        get: Option<String>,
        /// 设置配置项
        #[arg(long)]
        set: Option<String>,
    },
    /// 查看 Runner 统计
    Stats {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
        /// 实时刷新
        #[arg(long)]
        live: bool,
    },
}

#[derive(Subcommand)]
enum OriginLogAction {
    /// 实时查看日志尾部
    Tail {
        /// 按任务过滤
        #[arg(long)]
        task: Option<String>,
        /// 日志级别过滤
        #[arg(long)]
        level: Option<String>,
        /// 行数
        #[arg(long, default_value = "50")]
        lines: usize,
        /// 持续跟随
        #[arg(long)]
        follow: bool,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
    },
    /// 设置日志级别
    Level {
        /// 日志级别
        level: String,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
    },
    /// 清空日志缓冲
    Clear {
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
    },
}

#[derive(Subcommand)]
enum OriginPerfAction {
    /// 全部指标
    All,
    /// CPU 使用率
    Cpu,
    /// 内存使用率
    Memory,
    /// 任务吞吐量
    Throughput,
    /// 控制器参数趋势
    Controller,
}

#[derive(Subcommand)]
enum OriginExportAction {
    /// 导出任务状态快照
    State {
        /// 任务 ID
        id: String,
        /// 输出文件路径
        file: String,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
    },
    /// 导出 Runner 全貌快照
    Snapshot {
        /// 输出文件路径
        file: String,
        /// 连接别名
        #[arg(long)]
        alias: Option<String>,
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
            RunnerAction::Run { name, origin } => {
                cmd_runner_run(name.as_deref(), origin.as_deref())
            }
            RunnerAction::Status => cmd_runner_status(),
            RunnerAction::Stop => cmd_runner_stop(),
        },
        Command::Task {
            name,
            origin,
            no_run,
            step,
            print,
            check,
            break_line,
            export_state,
            export_dataflow,
            log,
            list_steps,
            list_vars,
            list_is,
            disasm,
            mem_dump,
        } => cmd_task(
            &name,
            origin.as_deref(),
            no_run,
            step.as_deref(),
            print.as_deref(),
            check,
            break_line,
            export_state.as_deref(),
            export_dataflow,
            log.as_deref(),
            list_steps,
            list_vars,
            list_is,
            disasm.as_deref(),
            mem_dump.as_deref(),
        ),
        Command::Origin { action } => match action {
            OriginAction::Connect { alias } => cmd_origin_connect(&alias),
            OriginAction::Disconnect { alias } => cmd_origin_disconnect(alias.as_deref()),
            OriginAction::List => cmd_origin_list(),
            OriginAction::Status { alias } => cmd_origin_status(alias.as_deref()),
            OriginAction::Task { action } => match action {
                OriginTaskAction::List { alias, status, limit, sort } => cmd_origin_task_list(alias.as_deref(), status.as_deref(), limit, sort.as_deref()),
                OriginTaskAction::Show { id, alias, regs, mem, bt, is } => cmd_origin_task_show(&id, alias.as_deref(), regs, mem, bt, is),
                OriginTaskAction::Submit { file, alias, name, mode, opt, timeout, wait, output } => cmd_origin_task_submit(&file, alias.as_deref(), name.as_deref(), mode.as_deref(), opt.as_deref(), timeout, wait, output.as_deref()),
                OriginTaskAction::Cancel { id, alias } => cmd_origin_task_cancel(&id, alias.as_deref()),
                OriginTaskAction::Output { id, alias, file } => cmd_origin_task_output(&id, alias.as_deref(), file.as_deref()),
                OriginTaskAction::Log { id, alias, lines } => cmd_origin_task_log(&id, alias.as_deref(), lines),
            },
            OriginAction::Runner { action } => match action {
                OriginRunnerAction::Config { alias, get, set } => cmd_origin_runner_config(alias.as_deref(), get.as_deref(), set.as_deref()),
                OriginRunnerAction::Stats { alias, live } => cmd_origin_runner_stats(alias.as_deref(), live),
            },
            OriginAction::Pool { alias, status } => cmd_origin_pool(alias.as_deref(), status.as_deref()),
            OriginAction::Controller { alias, history } => cmd_origin_controller(alias.as_deref(), history),
            OriginAction::Slots { alias, compact } => cmd_origin_slots(alias.as_deref(), compact),
            OriginAction::Log { action } => match action {
                OriginLogAction::Tail { task, level, lines, follow, alias } => cmd_origin_log_tail(alias.as_deref(), task.as_deref(), level.as_deref(), lines, follow),
                OriginLogAction::Level { level, alias } => cmd_origin_log_level(alias.as_deref(), &level),
                OriginLogAction::Clear { alias } => cmd_origin_log_clear(alias.as_deref()),
            },
            OriginAction::Perf { alias, action } => match action {
                OriginPerfAction::All => cmd_origin_perf(alias.as_deref(), "all"),
                OriginPerfAction::Cpu => cmd_origin_perf(alias.as_deref(), "cpu"),
                OriginPerfAction::Memory => cmd_origin_perf(alias.as_deref(), "memory"),
                OriginPerfAction::Throughput => cmd_origin_perf(alias.as_deref(), "throughput"),
                OriginPerfAction::Controller => cmd_origin_perf(alias.as_deref(), "controller"),
            },
            OriginAction::Export { alias, action } => match action {
                OriginExportAction::State { id, file, alias: a } => cmd_origin_export_state(&id, &file, a.as_deref().or(alias.as_deref())),
                OriginExportAction::Snapshot { file, alias: a } => cmd_origin_export_snapshot(&file, a.as_deref().or(alias.as_deref())),
            },
        },
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

    println!(
        "编译成功: {} → {} ({} 字节)",
        source,
        output_path,
        binary.len()
    );
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

fn cmd_runner_run(name: Option<&str>, origin: Option<&str>) {
    if let Some(origin_alias) = origin {
        // 远程模式
        let task_name = name.unwrap_or("");
        if task_name.is_empty() {
            eprintln!("用法: atomix runner run <name> --origin <别名>");
            std::process::exit(1);
        }
        cmd_runner_run_remote(task_name, origin_alias);
        return;
    }

    // 本地模式
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
    use atomix::runner::VmStateKind;
    use atomix::runner::execute::execute_instruction;

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
            println!(
                "任务完成: {} 条指令, {:.2?}, 返回值 = {}",
                instr_count, elapsed, retval as i64
            );
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

// ─── Origin（远程连接管理，设计文档 §5.2）────────────────────

/// 解析别名，优先使用显式别名，否则读环境变量 ATOMIX_ORIGIN。
fn resolve_alias(alias: Option<&str>) -> Result<String, String> {
    if let Some(a) = alias { return Ok(a.to_string()); }
    std::env::var("ATOMIX_ORIGIN")
        .map_err(|_| "未指定连接别名".to_string())
}

fn with_client<F>(alias: Option<&str>, op: F)
where F: FnOnce(&mut atomix::runner::client::AtxpClient) -> Result<(), String>,
{
    let alias = match resolve_alias(alias) {
        Ok(a) => a,
        Err(e) => { eprintln!("错误: {}", e); std::process::exit(1); }
    };
    let mut client = match atomix::runner::client::AtxpClient::connect_by_alias(&alias) {
        Ok(c) => c,
        Err(e) => { eprintln!("连接失败: {}", e); std::process::exit(1); }
    };
    if let Err(e) = op(&mut client) { eprintln!("操作失败: {}", e); std::process::exit(1); }
}

fn cmd_origin_connect(alias: &str) {
    let config = atomix::origin::OriginConfig::load();
    match config.find(alias) {
        Some(entry) => {
            println!("连接到 {}:{} ...", entry.address, entry.port);
            match atomix::origin::check_status(entry) {
                Ok(status) => { println!("状态: {}", serde_json::to_string_pretty(&status).unwrap_or_default()); }
                Err(e) => { eprintln!("失败: {}", e); std::process::exit(1); }
            }
        }
        None => { eprintln!("未找到: {}", alias); std::process::exit(1); }
    }
}

fn cmd_origin_disconnect(alias: Option<&str>) {
    match alias {
        Some(a) => println!("断开 {}", a),
        None => println!("断开所有"),
    }
}

fn cmd_origin_list() {
    let config = atomix::origin::OriginConfig::load();
    if config.connection.is_empty() { println!("（无）"); return; }
    for e in &config.connection { println!("  {} = {}:{}", e.alias, e.address, e.port); }
}

fn cmd_origin_status(alias: Option<&str>) {
    with_client(alias, |c| {
        println!("状态: {}", serde_json::to_string_pretty(&c.query_status()?).unwrap_or_default());
        Ok(())
    });
}

fn cmd_origin_task_list(alias: Option<&str>, status: Option<&str>, _lim: Option<usize>, _sort: Option<&str>) {
    with_client(alias, |c| {
        for t in c.query_tasks()? {
            let s = t.get("status").and_then(|v| v.as_str()).unwrap_or("?");
            if let Some(ref f) = status { if s != *f { continue; } }
            println!("  #{} {} [{}]", t.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                t.get("name").and_then(|v| v.as_str()).unwrap_or("?"), s);
        }
        Ok(())
    });
}

fn cmd_origin_task_show(id: &str, alias: Option<&str>, _r: bool, _m: bool, _b: bool, _i: bool) {
    with_client(alias, |c| { println!("{}", c.query_task_log(id, 20)?); Ok(()) });
}

fn cmd_origin_task_submit(file: &str, alias: Option<&str>, _name: Option<&str>, _mode: Option<&str>, _opt: Option<&str>, _to: Option<u64>, _wait: bool, _out: Option<&str>) {
    let p = std::path::Path::new(file);
    let bin = if p.extension().is_some_and(|e| e == "atxe") { fs::read(p).unwrap() }
        else { let s = fs::read_to_string(p).unwrap(); let (b, _) = atomix::compiler::compile(&s, "0"); if b.is_empty() { std::process::exit(1); } println!("编译: {}B", b.len()); b };
    with_client(alias, |c| { println!("提交成功, ID: {}", c.submit_task(&bin)?); Ok(()) });
}

fn cmd_origin_task_cancel(id: &str, alias: Option<&str>) { println!("取消 {} (需 ATXP 支持)", id); with_client(alias, |_| Ok(())); }
fn cmd_origin_task_output(id: &str, alias: Option<&str>, _f: Option<&str>) { println!("获取 {} 产出", id); with_client(alias, |_| Ok(())); }
fn cmd_origin_task_log(id: &str, alias: Option<&str>, l: usize) { with_client(alias, |c| { println!("{}", c.query_task_log(id, l)?); Ok(()) }); }
fn cmd_origin_runner_config(alias: Option<&str>, get: Option<&str>, _set: Option<&str>) { with_client(alias, |c| { let cfg = c.query_config()?; if let Some(k) = get { println!("{}", cfg.get(k).map(|v| v.to_string()).unwrap_or("?".into())); } else { println!("{}", serde_json::to_string_pretty(&cfg).unwrap_or_default()); } Ok(()) }); }
fn cmd_origin_runner_stats(alias: Option<&str>, _live: bool) { with_client(alias, |c| { println!("{}", serde_json::to_string_pretty(&c.query_status()?).unwrap_or_default()); Ok(()) }); }
fn cmd_origin_pool(alias: Option<&str>, _sf: Option<&str>) { println!("Pool:"); with_client(alias, |_| Ok(())); }
fn cmd_origin_controller(alias: Option<&str>, _h: bool) { with_client(alias, |c| { println!("{}", serde_json::to_string_pretty(&c.query_controller()?).unwrap_or_default()); Ok(()) }); }
fn cmd_origin_slots(alias: Option<&str>, _c: bool) { with_client(alias, |c| { println!("{}", serde_json::to_string_pretty(&c.query_slots()?).unwrap_or_default()); Ok(()) }); }
fn cmd_origin_log_tail(alias: Option<&str>, _t: Option<&str>, _l: Option<&str>, _n: usize, _f: bool) { with_client(alias, |_| { println!("日志流 (Ctrl+C)"); Ok(()) }); }
fn cmd_origin_log_level(alias: Option<&str>, level: &str) { println!("级别: {}", level); with_client(alias, |_| Ok(())); }
fn cmd_origin_log_clear(alias: Option<&str>) { println!("日志清空"); with_client(alias, |_| Ok(())); }
fn cmd_origin_perf(alias: Option<&str>, metric: &str) { with_client(alias, |c| { println!("{}: {}", metric, serde_json::to_string_pretty(&c.query_perf()?).unwrap_or_default()); Ok(()) }); }
fn cmd_origin_export_state(id: &str, file: &str, alias: Option<&str>) { with_client(alias, |c| { let s = serde_json::json!({"task_id": id, "status": c.query_status()?}); let _ = fs::write(file, &serde_json::to_string_pretty(&s).unwrap()); println!("导出 {}", file); Ok(()) }); }
fn cmd_origin_export_snapshot(file: &str, alias: Option<&str>) { with_client(alias, |c| { let s = serde_json::json!({"snapshot": c.query_status()?}); let _ = fs::write(file, &serde_json::to_string_pretty(&s).unwrap()); println!("导出 {}", file); Ok(()) }); }
// ─── Task（深度检查 / 本地 debug）────────────────────

/// 远程执行任务：编译 → ATXP Submit → 远程执行 → 显示状态。
fn cmd_runner_run_remote(task_name: &str, alias: &str) {
    // 编译 .atx → .atxe
    let path = Path::new(task_name);
    let binary = if path.extension().is_some_and(|e| e == "atxe") {
        match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("错误: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        let source = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("错误: {}", e);
                std::process::exit(1);
            }
        };
        let (bin, errors) = atomix::compiler::compile(&source, "0");
        if !errors.is_empty() {
            for e in &errors {
                eprintln!("{}", e);
            }
            if bin.is_empty() {
                std::process::exit(1);
            }
        }
        println!("编译成功: {} 字节", bin.len());
        bin
    };

    // 连接远程 runner
    println!("正在连接远程: {} ...", alias);
    let mut client = match atomix::runner::client::AtxpClient::connect_by_alias(alias) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("连接失败: {}", e);
            std::process::exit(1);
        }
    };

    // 查询远程状态
    match client.query_status() {
        Ok(status) => println!(
            "远程状态: {}",
            serde_json::to_string_pretty(&status).unwrap_or_default()
        ),
        Err(e) => eprintln!("查询状态失败: {}", e),
    }

    // 提交任务
    println!("正在提交任务 ...");
    match client.submit_task(&binary) {
        Ok(task_id) => println!("任务已提交, ID: {}", task_id),
        Err(e) => {
            eprintln!("提交失败: {}", e);
            std::process::exit(1);
        }
    }

    // 查询任务列表
    match client.query_tasks() {
        Ok(tasks) => {
            println!("\n任务列表:");
            for t in &tasks {
                println!("  {}", serde_json::to_string_pretty(t).unwrap_or_default());
            }
        }
        Err(e) => eprintln!("查询任务失败: {}", e),
    }

    println!("\n远程执行完成。");
}

/// 远程任务监控：连接远程 runner，查询任务状态。
fn _cmd_task_remote(task_name: &str, alias: &str) {
    let path = Path::new(task_name);

    // 编译 .atx → .atxe（为了获取任务信息）
    let binary = if path.extension().is_some_and(|e| e == "atxe") {
        match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("错误: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        let source = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("错误: {}", e);
                std::process::exit(1);
            }
        };
        let (bin, errors) = atomix::compiler::compile(&source, "0");
        if !errors.is_empty() {
            for e in &errors {
                eprintln!("{}", e);
            }
            if bin.is_empty() {
                std::process::exit(1);
            }
        }
        bin
    };

    // 解码获取任务信息
    let atxe = atomix::base::ir::AtxeBinary::from_bytes(&binary).unwrap_or_else(|| {
        eprintln!("编译产物无效");
        std::process::exit(1);
    });

    println!("任务: {} ({} 条指令)", task_name, atxe.header.total_instrs);

    // 连接远程 runner
    println!("正在连接远程: {} ...", alias);
    let mut client = match atomix::runner::client::AtxpClient::connect_by_alias(alias) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("连接失败: {}", e);
            std::process::exit(1);
        }
    };

    // 查询远程状态
    match client.query_status() {
        Ok(status) => {
            println!("\n远程 runner 状态:");
            println!(
                "  {}",
                serde_json::to_string_pretty(&status).unwrap_or_default()
            );
        }
        Err(e) => eprintln!("查询状态失败: {}", e),
    }

    // 提交任务（远程执行）
    match client.submit_task(&binary) {
        Ok(task_id) => println!("\n任务已提交, 远程 ID: {}", task_id),
        Err(e) => eprintln!("提交任务失败: {}", e),
    }

    // 查询任务列表
    match client.query_tasks() {
        Ok(tasks) => {
            println!("\n远程任务:");
            for t in &tasks {
                println!("  {}", serde_json::to_string_pretty(t).unwrap_or_default());
            }
        }
        Err(e) => eprintln!("查询任务失败: {}", e),
    }
}

fn cmd_task(
    name: &str,
    origin: Option<&str>,
    no_run: bool,
    step: Option<&str>,
    print_expr: Option<&str>,
    check: bool,
    break_line: Option<u32>,
    export_state: Option<&str>,
    export_dataflow: bool,
    log_file: Option<&str>,
    list_steps: bool,
    list_vars: bool,
    list_is: bool,
    disasm_addr: Option<&str>,
    mem_dump: Option<&str>,
) {
    let path = Path::new(name);

    if let Some(origin_alias) = origin {
        // 启动远程 TUI（设计文档 §5.4）
        match atomix::debug::tui::remote_app::run_remote_tui(origin_alias) {
            Ok(()) => {},
            Err(e) => {
                eprintln!("远程 TUI 错误: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // 编译或加载 .atxe
    let vm = if path.extension().is_some_and(|e| e == "atx") {
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
                println!("编译成功: {} ({} 指令)", name, vm.text.len());
                vm
            }
            Err(e) => {
                eprintln!("无法加载编译产物: {e}");
                std::process::exit(1);
            }
        }
    } else {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("错误: 无法读取文件 `{name}`: {e}");
                std::process::exit(1);
            }
        };
        match atomix::runner::VmState::load_atxe(&bytes) {
            Ok(vm) => {
                println!("加载成功: {} ({} 指令)", name, vm.text.len());
                vm
            }
            Err(e) => {
                eprintln!("无法加载 .atxe: {e}");
                std::process::exit(1);
            }
        }
    };

    // 创建 LocalDebugSession
    let debug_bytes = vm.debug_info.clone();
    let mut session = atomix::debug::session::LocalDebugSession::new(vm);
    session.set_debug_map_from_bytes(&debug_bytes);
    if path.extension().is_some_and(|e| e == "atx") {
        session.set_source(name);
    } else {
        let atx_path = path.with_extension("atx");
        if atx_path.exists() {
            session.set_source(atx_path.to_str().unwrap_or(""));
        }
    }

    // 处理 CLI 标志（非交互模式）
    let is_cli_mode = no_run
        || step.is_some()
        || print_expr.is_some()
        || check
        || break_line.is_some()
        || export_state.is_some()
        || export_dataflow
        || log_file.is_some()
        || list_steps
        || list_vars
        || list_is
        || disasm_addr.is_some()
        || mem_dump.is_some();

    if is_cli_mode {
        // CLI 非交互模式
        if !no_run && !check {
            session.collect_trace();
        }

        if let Some(line) = break_line {
            session.set_breakpoint_line(line, None);
        }

        if let Some(step_name) = step {
            if let Some(s) = session.trace.find_step_by_name(step_name) {
                println!(
                    "Step: {} (line {}, {})",
                    s.name,
                    s.source_line,
                    s.status.name()
                );
            } else {
                println!("未找到 Step: {}", step_name);
            }
        }

        if let Some(expr) = print_expr {
            match atomix::debug::eval::eval_expr(expr, &session.vm) {
                Ok(val) => println!("{} = {}", expr, atomix::debug::eval::format_result(val)),
                Err(e) => println!("错误: {}", e),
            }
        }

        if list_steps {
            println!("Step 列表:");
            for (i, s) in session.trace.steps.iter().enumerate() {
                println!(
                    "  {}: {} [{}] line {}",
                    i,
                    s.name,
                    s.status.symbol(),
                    s.source_line
                );
            }
        }

        if list_vars {
            println!("变量及最终值:");
            for i in 0..16 {
                let name = atomix::base::isa::reg_name(i).to_uppercase();
                println!("  {} = {:#x}", name, session.vm.read_reg(i));
            }
        }

        if list_is {
            println!("IS* 最终状态:");
            for (name, val) in &session.is_context.entries {
                if val != "—" {
                    println!("  {} = {}", name, val);
                }
            }
        }

        if let Some(addr_str) = disasm_addr {
            let addr = usize::from_str_radix(addr_str.trim_start_matches("0x"), 16).unwrap_or(0);
            let lines = atomix::debug::disassemble::disassemble_range(&session.vm.text, addr, 8);
            for l in lines {
                println!("{}", l);
            }
        }

        if let Some(dump) = mem_dump {
            let parts: Vec<&str> = dump.split(',').collect();
            let addr = parts
                .get(0)
                .and_then(|s| u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
                .unwrap_or(0);
            let len: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
            let end = addr.saturating_add(len as u64);
            let mut offset = addr;
            while offset < end {
                let line_end = (offset + 16).min(end);
                let mut hex = String::new();
                let mut ascii = String::new();
                for a in offset..line_end {
                    if let Some(byte) = session.vm.memory.read_u8(a) {
                        hex.push_str(&format!("{:02x} ", byte));
                        ascii.push(if byte.is_ascii_graphic() || byte == b' ' {
                            byte as char
                        } else {
                            '.'
                        });
                    }
                }
                println!("{:#010x}:  {:48}  {}", offset, hex, ascii);
                offset = line_end;
            }
        }

        if let Some(path) = export_state {
            let state = serde_json::json!({
                "pc": session.vm.pc,
                "regs": session.vm.regs,
                "state": format!("{:?}", session.vm.state),
                "steps": session.trace.step_count(),
                "total_instrs": session.trace.total_instructions,
            });
            if let Ok(json) = serde_json::to_string_pretty(&state) {
                if fs::write(&path, &json).is_ok() {
                    println!("状态已导出至: {}", path);
                }
            }
        }

        return;
    }

    // 默认：启动 TUI
    if let Err(e) = atomix::debug::tui::run_tui(session) {
        eprintln!("TUI 错误: {}", e);
        std::process::exit(1);
    }
}
