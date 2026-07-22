//! ATXP 服务器 (v0.5) — 接受远程连接，处理监控/控制消息。
//!
//! 使用 tokio 异步 TCP listener，每个连接独立 task 处理。
//! 协议设计: docs/05-通信协议.md

use crate::base::atxp::{self, MsgType};
use crate::runner::runtime::Runtime;
use prost::Message;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone)]
pub struct ServerConfig {
    pub listen_addr: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:9000".to_string(),
        }
    }
}

/// ATXP 服务器 (v0.5)。
pub struct AtxpServer {
    pub config: ServerConfig,
    pub runtime: Arc<Mutex<Runtime>>,
}

impl AtxpServer {
    pub fn new(config: ServerConfig, runtime: Runtime) -> Self {
        Self {
            config,
            runtime: Arc::new(Mutex::new(runtime)),
        }
    }

    /// 启动服务器（阻塞，直到 ctrl-c）。
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(&self.config.listen_addr).await?;
        println!("ATXP 服务器已启动 (v0.5): {}", self.config.listen_addr);

        loop {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, addr) = result?;
                    let rt = self.runtime.clone();
                    tokio::spawn(async move {
                        println!("新连接: {}", addr);
                        if let Err(e) = handle_connection(stream, rt).await {
                            eprintln!("连接处理错误 ({}): {}", addr, e);
                        }
                        println!("连接断开: {}", addr);
                    });
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("收到 Ctrl-C，关闭服务器");
                    break;
                }
            }
        }
        Ok(())
    }
}

// ─── 连接处理 ───────────────────────────────────────

/// 处理单个客户端连接（帧读取循环）。
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    runtime: Arc<Mutex<Runtime>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = Vec::new();

    loop {
        let mut tmp = [0u8; 1024];
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break; // 连接关闭
        }
        buf.extend_from_slice(&tmp[..n]);

        // 尝试解析帧（可能有多帧）
        loop {
            match atxp::decode_frame(&buf) {
                Some((hdr, payload, rest)) => {
                    buf = rest.to_vec();
                    handle_frame(hdr, &payload, &mut stream, &runtime).await?;
                }
                None => break, // 数据不足，等更多
            }
        }
    }
    Ok(())
}

// ─── 帧分发 ─────────────────────────────────────────

/// 根据 msg_type 分发到对应处理函数。
async fn handle_frame(
    hdr: atxp::FrameHeader,
    payload: &[u8],
    stream: &mut tokio::net::TcpStream,
    runtime: &Arc<Mutex<Runtime>>,
) -> Result<(), Box<dyn std::error::Error>> {
    match hdr.msg_type {
        0x01 => handle_hello(payload, stream, hdr.seq_id).await,
        0x04 => handle_request(payload, stream, runtime, hdr.seq_id).await,
        0x07 => handle_submit(payload, stream, runtime, hdr.seq_id).await,
        0x09 => handle_heartbeat(payload, stream, hdr.seq_id).await,
        0x0A => handle_bye(payload, stream, hdr.seq_id).await,
        _ => {
            // 未知消息类型 → Error
            let err = atxp::proto::Error {
                code: 1,
                message: format!("不支持的消息类型: {:#04x}", hdr.msg_type),
                endpoint: String::new(),
            };
            send_frame(stream, MsgType::Error as u8, hdr.seq_id, &err.encode_to_vec()).await?;
            Ok(())
        }
    }
}

// ─── Hello 握手 ─────────────────────────────────────

async fn handle_hello(
    payload: &[u8],
    stream: &mut tokio::net::TcpStream,
    seq_id: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(client_hello) = atxp::proto::Hello::decode(payload) {
        // 版本协商：主版本必须一致
        let client_major = (client_hello.protocol_version >> 16) as u16;
        let server_version: u32 = 0x0001_0000; // v1.0
        let server_major = (server_version >> 16) as u16;

        if client_major != server_major {
            let err = atxp::proto::Error {
                code: 412, // VersionMismatch
                message: format!(
                    "版本不兼容: client v{}.{}, server v{}.{}",
                    client_major,
                    (client_hello.protocol_version & 0xFFFF) as u16,
                    server_major,
                    (server_version & 0xFFFF) as u16,
                ),
                endpoint: "hello".into(),
            };
            send_frame(stream, MsgType::Error as u8, seq_id, &err.encode_to_vec()).await?;
            return Ok(());
        }

        // 返回 Server Hello
        let server_hello = atxp::proto::Hello {
            protocol_version: server_version,
            features: 0,
            auth_token: String::new(),
            client_id: format!("atomix-runner/{}", server_version),
            mode: 1, // REMOTE
        };
        send_frame(
            stream,
            MsgType::Hello as u8,
            seq_id,
            &server_hello.encode_to_vec(),
        )
        .await?;
    } else {
        let err = atxp::proto::Error {
            code: 400,
            message: "无效的 Hello 消息".into(),
            endpoint: "hello".into(),
        };
        send_frame(stream, MsgType::Error as u8, seq_id, &err.encode_to_vec()).await?;
    }
    Ok(())
}

// ─── Request 分发 ──────────────────────────────────

async fn handle_request(
    payload: &[u8],
    stream: &mut tokio::net::TcpStream,
    runtime: &Arc<Mutex<Runtime>>,
    seq_id: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = match atxp::proto::Request::decode(payload) {
        Ok(r) => r,
        Err(_) => {
            let err = atxp::proto::Error {
                code: 400,
                message: "无效的 Request 消息".into(),
                endpoint: String::new(),
            };
            send_frame(stream, MsgType::Error as u8, seq_id, &err.encode_to_vec()).await?;
            return Ok(());
        }
    };

    let response = match (request.resource.as_str(), request.action.as_str()) {
        ("runner", "status") => handle_runner_status(runtime).await,
        ("runner", "config") => handle_runner_config(runtime).await,
        ("task", "list") => handle_task_list(runtime).await,
        ("task", "status") => {
            let task_id = request.params.get("task_id").and_then(|v| {
                std::str::from_utf8(v).ok()
            }).unwrap_or("");
            handle_task_status(runtime, task_id).await
        }
        ("task", "output") => {
            let task_id = request.params.get("task_id").and_then(|v| {
                std::str::from_utf8(v).ok()
            }).unwrap_or("");
            handle_task_output(runtime, task_id).await
        }
        ("task", "stats") => {
            let task_id = request.params.get("task_id").and_then(|v| {
                std::str::from_utf8(v).ok()
            }).unwrap_or("");
            handle_task_stats(runtime, task_id).await
        }
        ("task", "log") => {
            let task_id = request.params.get("task_id").and_then(|v| {
                std::str::from_utf8(v).ok()
            }).unwrap_or("");
            let _lines = request.params.get("lines")
                .and_then(|v| {
                    if v.len() == 8 {
                        Some(u64::from_le_bytes([v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]]) as usize)
                    } else { None }
                })
                .unwrap_or(50);
            handle_task_log(runtime, task_id).await
        }
        ("controller", "status") => handle_controller_status(runtime).await,
        ("slots", "layout") => handle_slots_layout(runtime).await,
        _ => {
            // 未知端点 → Error
            atxp::proto::Response {
                resource: request.resource.clone(),
                action: request.action.clone(),
                data: Vec::new(),
                error: format!("未知端点: {}/{}", request.resource, request.action),
            }
        }
    };

    send_frame(
        stream,
        MsgType::Response as u8,
        seq_id,
        &response.encode_to_vec(),
    )
    .await?;
    Ok(())
}

// ─── 端点处理函数 ─────────────────────────────────

async fn handle_runner_status(runtime: &Arc<Mutex<Runtime>>) -> atxp::proto::Response {
    let rt = runtime.lock().await;
    let status = atxp::proto::RunnerStatus {
        state: 1, // RUNNING
        mode: 1,  // REMOTE
        uptime_ms: 0,
        version: env!("CARGO_PKG_VERSION").to_string(),
        task_count: rt.pool.len() as u32,
        running_count: rt.pool.ready_tasks().len() as u32,
        mem_used_mb: 0,
        mem_limit_mb: 0,
        cpu_usage_pct: 0.0,
        total_instrs: rt.total_instrs,
        completed_count: rt.completed_count,
    };
    atxp::proto::Response {
        resource: "runner".into(),
        action: "status".into(),
        data: status.encode_to_vec(),
        error: String::new(),
    }
}

async fn handle_runner_config(runtime: &Arc<Mutex<Runtime>>) -> atxp::proto::Response {
    let rt = runtime.lock().await;
    let config = atxp::proto::RunnerConfig {
        listen_addr: String::new(),
        task_dir: String::new(),
        state_dir: rt.state_dir.clone(),
        max_tasks: 0,
        max_concurrent: rt.executors.len() as u32,
        quantum: rt.quantum,
        stream_buf_mb: 0,
        deny_commands: Vec::new(),
        tls_enabled: false,
        heartbeat_ms: 0,
        trace_level: 0,
        trace_sparse_n: 0,
    };
    atxp::proto::Response {
        resource: "runner".into(),
        action: "config".into(),
        data: config.encode_to_vec(),
        error: String::new(),
    }
}

async fn handle_task_list(runtime: &Arc<Mutex<Runtime>>) -> atxp::proto::Response {
    let rt = runtime.lock().await;
    let tasks: Vec<atxp::proto::TaskInfo> = rt
        .pool
        .results()
        .into_iter()
        .map(|(id, status, _retval, cycles)| atxp::proto::TaskInfo {
            task_id: id.to_string(),
            name: String::new(),
            state: status as u32,
            disk_addr: 0,
            mem_addr: 0,
            created_at: 0,
            started_at: 0,
            cycles,
            memory_mb: 0,
        })
        .collect();
    let list = atxp::proto::TaskList { tasks };
    atxp::proto::Response {
        resource: "task".into(),
        action: "list".into(),
        data: list.encode_to_vec(),
        error: String::new(),
    }
}

async fn handle_task_status(runtime: &Arc<Mutex<Runtime>>, task_id: &str) -> atxp::proto::Response {
    let id: u16 = task_id.parse().unwrap_or(0);
    let rt = runtime.lock().await;
    let status = if let Some(task) = rt.pool.get(id) {
        atxp::proto::TaskStatus {
            task_id: id.to_string(),
            state: task.status as u32,
            current_step: String::new(),
            current_pc: task.vm.as_ref().map(|vm| vm.pc as u32).unwrap_or(0),
            total_cycles: task.total_instrs,
            memory_used: task.vm.as_ref().map(|vm| vm.memory.usage).unwrap_or(0),
            started_at: 0,
            error_message: task.vm.as_ref()
                .and_then(|vm| match &vm.state {
                    crate::runner::VmStateKind::Error(msg) => Some(msg.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
            elapsed_ms: 0,
        }
    } else {
        atxp::proto::TaskStatus {
            task_id: task_id.to_string(),
            state: 5, // Error
            ..Default::default()
        }
    };
    atxp::proto::Response {
        resource: "task".into(),
        action: "status".into(),
        data: status.encode_to_vec(),
        error: String::new(),
    }
}

async fn handle_task_output(runtime: &Arc<Mutex<Runtime>>, task_id: &str) -> atxp::proto::Response {
    let id: u16 = task_id.parse().unwrap_or(0);
    let rt = runtime.lock().await;
    let (retval, error_msg) = if let Some(task) = rt.pool.get(id) {
        (task.return_value, task.vm.as_ref()
            .and_then(|vm| match &vm.state {
                crate::runner::VmStateKind::Error(msg) => Some(msg.clone()),
                _ => None,
            })
            .unwrap_or_default())
    } else {
        (0, "task not found".into())
    };
    // 用 serde_json 构建 TaskOutput 风格的响应
    let data = serde_json::to_vec(&serde_json::json!({
        "task_id": task_id,
        "status": if error_msg.is_empty() { "done" } else { "error" },
        "output": retval,
        "error": error_msg,
    })).unwrap_or_default();
    atxp::proto::Response {
        resource: "task".into(),
        action: "output".into(),
        data,
        error: String::new(),
    }
}

async fn handle_task_stats(runtime: &Arc<Mutex<Runtime>>, task_id: &str) -> atxp::proto::Response {
    let id: u16 = task_id.parse().unwrap_or(0);
    let rt = runtime.lock().await;
    let instrs = rt.pool.get(id).map(|t| t.total_instrs).unwrap_or(0);
    let stats = atxp::proto::TaskStats {
        total_instrs: instrs,
        ..Default::default()
    };
    atxp::proto::Response {
        resource: "task".into(),
        action: "stats".into(),
        data: stats.encode_to_vec(),
        error: String::new(),
    }
}

async fn handle_task_log(runtime: &Arc<Mutex<Runtime>>, task_id: &str) -> atxp::proto::Response {
    let id: u16 = task_id.parse().unwrap_or(0);
    let rt = runtime.lock().await;
    let log_text = if let Some(task) = rt.pool.get(id) {
        format!(
            "Task {}: {} instrs, status={:?}, ret={}",
            task_id,
            task.total_instrs,
            task.status,
            task.return_value,
        )
    } else {
        format!("Task {}: not found", task_id)
    };
    atxp::proto::Response {
        resource: "task".into(),
        action: "log".into(),
        data: log_text.into_bytes(),
        error: String::new(),
    }
}

async fn handle_controller_status(runtime: &Arc<Mutex<Runtime>>) -> atxp::proto::Response {
    let rt = runtime.lock().await;
    let state = atxp::proto::ControllerState {
        n_batch: rt.executors.len() as u32,
        hard_ceiling: rt.executors.len() as u32,
        soft_ceiling: rt.executors.len() as u32,
        backlog_depth: rt.pool.ready_tasks().len() as u32,
        high_backlog_mode: false,
        cold_start_phase: rt.cold_start_phase == crate::runner::runtime::ColdStartPhase::Bootstrap,
        beta: rt.batch.factor_beta(),
        lambda_speed: rt.batch.factor_lambda(),
        sigma_volume: rt.batch.factor_sigma(),
        gamma_variance: rt.batch.factor_gamma(),
        merged_factor: rt.batch.merge_factors(),
        alpha_mem_current: rt.batch.alpha_mem_current,
        oom_count: rt.batch.oom_count,
        oom_state: format!("{:?}", rt.batch.oom_state),
        total_slots: rt.slot_manager.slots.len() as u32,
        used_slots: rt.slot_manager.allocated_count() as u32,
        slipway_slots: rt.slot_manager.slipway_slots.len() as u32,
        dead_slots: rt.slot_manager.dead_zones.len() as u32,
        slot_size_mb: rt.slot_manager.slot_size as f64 / (1024.0 * 1024.0),
        slipway_multiplier: rt.slot_manager.slipway_multiplier,
    };
    atxp::proto::Response {
        resource: "controller".into(),
        action: "status".into(),
        data: state.encode_to_vec(),
        error: String::new(),
    }
}

async fn handle_slots_layout(runtime: &Arc<Mutex<Runtime>>) -> atxp::proto::Response {
    let rt = runtime.lock().await;
    let slots: Vec<atxp::proto::SlotInfo> = rt
        .slot_manager
        .slots
        .iter()
        .map(|slot| atxp::proto::SlotInfo {
            slot_id: slot.id as u32,
            task_id: slot.task_id.map(|id: u16| id.to_string()).unwrap_or_default(),
            base_addr: slot.base,
            size: slot.size,
            used: slot.physical_size,
            watermark: 0,
            status: match slot.status {
                crate::runner::slot::SlotStatus::Free => 0,
                crate::runner::slot::SlotStatus::Occupied => 1,
                crate::runner::slot::SlotStatus::Dead => 2,
                crate::runner::slot::SlotStatus::Slipway => 3,
            },
            zone: 0,
        })
        .collect();
    // calc_fragmentation 是私有方法，用近似值
    let fragmentation = rt.slot_manager.dead_zones.len() as f64
        / (rt.slot_manager.slots.len() + rt.slot_manager.slipway_slots.len()).max(1) as f64;
    let layout = atxp::proto::SlotLayout {
        slots,
        fragmentation,
    };
    atxp::proto::Response {
        resource: "slots".into(),
        action: "layout".into(),
        data: layout.encode_to_vec(),
        error: String::new(),
    }
}

// ─── Submit ─────────────────────────────────────────

async fn handle_submit(
    payload: &[u8],
    stream: &mut tokio::net::TcpStream,
    runtime: &Arc<Mutex<Runtime>>,
    seq_id: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let submit = match atxp::proto::Submit::decode(payload) {
        Ok(s) => s,
        Err(_) => {
            let result = atxp::proto::SubmitResult {
                status: 1, // REJECTED
                task_id: String::new(),
                message: "无效的 Submit 消息".into(),
                compile_errors: Vec::new(),
            };
            send_frame(stream, MsgType::SubmitResult as u8, seq_id, &result.encode_to_vec()).await?;
            return Ok(());
        }
    };

    let binary = if submit.mode == 0 {
        // SOURCE mode: 编译 .atx
        let opt = format!("{}", submit.compile_opts.as_ref().map(|c| c.opt_level).unwrap_or(0));
        let (bin, errors) = crate::compiler::compile(&submit.source, &opt);
        if !errors.is_empty() {
            let compile_errs: Vec<atxp::proto::CompileError> = errors
                .into_iter()
                .map(|e| atxp::proto::CompileError {
                    line: 0,
                    col: 0,
                    message: e,
                    source_line: String::new(),
                })
                .collect();
            let result = atxp::proto::SubmitResult {
                status: 2, // COMPILE_ERROR
                task_id: String::new(),
                message: "编译失败".into(),
                compile_errors: compile_errs,
            };
            send_frame(stream, MsgType::SubmitResult as u8, seq_id, &result.encode_to_vec()).await?;
            return Ok(());
        }
        bin
    } else {
        // BINARY mode: 直接使用传入的二进制
        submit.binary.clone()
    };

    // 将任务加入 Runtime 池
    let task_id = {
        let mut rt = runtime.lock().await;
        let id = rt.next_task_id;
        rt.next_task_id = id.wrapping_add(1);
        // 通过 AtxeBinary 加载
        if let Some(atxe) = crate::base::ir::AtxeBinary::from_bytes(&binary) {
            match crate::runner::VmState::from_atxe(&atxe) {
                Ok(vm) => {
                    use crate::runner::task::{Task, TaskStatus};
                    let task = Task {
                        id,
                        entry_offset: vm.pc,
                        status: TaskStatus::Ready,
                        deps: Vec::new(),
                        vm: Some(vm),
                        return_value: 0,
                        total_instrs: 0,
                        quantum_instrs: 0,
                        join_waiting_for: None,
                    };
                    rt.pool.add_task(task);
                    // 触发执行
                    rt.pool.activate_ready_tasks();
                    id.to_string()
                }
                Err(e) => {
                    let result = atxp::proto::SubmitResult {
                        status: 1, // REJECTED
                        task_id: String::new(),
                        message: format!("VM 加载失败: {}", e),
                        compile_errors: Vec::new(),
                    };
                    send_frame(stream, MsgType::SubmitResult as u8, seq_id, &result.encode_to_vec()).await?;
                    return Ok(());
                }
            }
        } else {
            let result = atxp::proto::SubmitResult {
                status: 1,
                task_id: String::new(),
                message: "无效的 .atxe 二进制".into(),
                compile_errors: Vec::new(),
            };
            send_frame(stream, MsgType::SubmitResult as u8, seq_id, &result.encode_to_vec()).await?;
            return Ok(());
        }
    };

    let result = atxp::proto::SubmitResult {
        status: 0, // ACCEPTED
        task_id,
        message: "任务已接收并加入执行队列".into(),
        compile_errors: Vec::new(),
    };
    send_frame(stream, MsgType::SubmitResult as u8, seq_id, &result.encode_to_vec()).await?;
    Ok(())
}

// ─── Heartbeat ──────────────────────────────────────

async fn handle_heartbeat(
    _payload: &[u8],
    stream: &mut tokio::net::TcpStream,
    seq_id: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let ack = atxp::proto::Ack {
        status: 0,
        message: "OK".into(),
    };
    send_frame(stream, MsgType::Ack as u8, seq_id, &ack.encode_to_vec()).await?;
    Ok(())
}

// ─── Bye ────────────────────────────────────────────

async fn handle_bye(
    _payload: &[u8],
    stream: &mut tokio::net::TcpStream,
    seq_id: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    // 回复 Bye 确认
    let bye = atxp::proto::Bye {
        reason: 0, // NORMAL
        message: "bye".into(),
    };
    send_frame(stream, MsgType::Bye as u8, seq_id, &bye.encode_to_vec()).await?;
    Ok(())
}

// ─── 辅助函数 ───────────────────────────────────────

/// 发送一帧。
async fn send_frame(
    stream: &mut tokio::net::TcpStream,
    msg_type: u8,
    seq_id: u32,
    payload: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = atxp::encode_frame(msg_type, seq_id, payload);
    stream.write_all(&frame).await?;
    Ok(())
}
