//! ATXP 服务器 — 接受远程连接，处理监控/控制消息。
//!
//! 使用 tokio 异步 TCP listener，每个连接独立 task 处理。

use crate::base::atxp;
use crate::runner::runtime::Runtime;
use prost::Message;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

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

/// ATXP 服务器。
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
        println!("ATXP 服务器已启动: {}", self.config.listen_addr);

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

/// 处理单个客户端连接。
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    runtime: Arc<Mutex<Runtime>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncReadExt;

    let mut buf = Vec::new();

    loop {
        // 读取数据到缓冲区
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
                    handle_message(hdr, &payload, &mut stream, &runtime).await?;
                }
                None => {
                    // 数据不足，等待更多
                    break;
                }
            }
        }
    }
    Ok(())
}

/// 处理一条 ATXP 消息。
async fn handle_message(
    hdr: atxp::FrameHeader,
    payload: &[u8],
    stream: &mut tokio::net::TcpStream,
    runtime: &Arc<Mutex<Runtime>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncWriteExt;

    match hdr.msg_type {
        0x05 => {
            // Query
            // 解析 Query protobuf
            if let Ok(query) = atxp::proto::Query::decode(payload) {
                let result = handle_query(&query, runtime).await;
                // 发送响应
                let resp_payload = result.encode_to_vec();
                let frame = atxp::encode_frame(0x06, hdr.seq_id, &resp_payload);
                stream.write_all(&frame).await?;
            }
        }
        0x04 => {
            // Command
            if let Ok(cmd) = atxp::proto::Command::decode(payload) {
                handle_command(&cmd, stream, runtime).await?;
            }
        }
        0x0D => {
            // Submit
            if let Ok(submit) = atxp::proto::Submit::decode(payload) {
                handle_submit(&submit, stream, runtime).await?;
            }
        }
        0x0B => {
            // Heartbeat
            // 回复 Ack
            let ack = atxp::proto::Ack {
                status: 0,
                message: "OK".into(),
            };
            let resp_payload = ack.encode_to_vec();
            stream
                .write_all(&atxp::encode_frame(0x02, hdr.seq_id, &resp_payload))
                .await?;
        }
        _ => {
            // 未知消息类型，回复 Error
            let err = atxp::proto::Error {
                code: 1,
                message: format!("不支持的消息类型: {:#x}", hdr.msg_type),
                endpoint: String::new(),
            };
            let resp_payload = err.encode_to_vec();
            let frame = atxp::encode_frame(0x03, hdr.seq_id, &resp_payload);
            stream.write_all(&frame).await?;
        }
    }
    Ok(())
}

/// 处理 Query 消息。
async fn handle_query(
    query: &atxp::proto::Query,
    runtime: &Arc<Mutex<Runtime>>,
) -> atxp::proto::QueryResult {
    let endpoint = &query.endpoint;
    let data = match endpoint.as_str() {
        "runner/status" => {
            let rt = runtime.lock().await;
            let status = serde_json::json!({
                "state": "running",
                "tasks_total": rt.pool.len(),
                "tasks_completed": rt.completed_count,
                "total_instrs": rt.total_instrs,
            });
            serde_json::to_vec(&status).unwrap_or_default()
        }
        "runner/tasks" => {
            let rt = runtime.lock().await;
            let tasks: Vec<serde_json::Value> = rt
                .pool
                .results()
                .into_iter()
                .map(|(id, status, retval, instrs)| {
                    serde_json::json!({
                        "id": id,
                        "status": format!("{:?}", status),
                        "return_value": retval,
                        "total_instrs": instrs,
                    })
                })
                .collect();
            serde_json::to_vec(&tasks).unwrap_or_default()
        }
        _ => serde_json::to_vec(&serde_json::json!({
            "error": format!("未知端点: {}", endpoint)
        }))
        .unwrap_or_default(),
    };
    atxp::proto::QueryResult {
        endpoint: endpoint.clone(),
        data,
    }
}

/// 处理 Command 消息。
async fn handle_command(
    cmd: &atxp::proto::Command,
    stream: &mut tokio::net::TcpStream,
    _runtime: &Arc<Mutex<Runtime>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncWriteExt;

    let ack = match cmd.endpoint.as_str() {
        "runner/stop" => {
            // TODO: 实现停止
            atxp::proto::Ack {
                status: 0,
                message: "停止命令已接收".into(),
            }
        }
        _ => atxp::proto::Ack {
            status: 1,
            message: format!("未知命令端點: {}", cmd.endpoint),
        },
    };
    let resp_payload = ack.encode_to_vec();
    stream
        .write_all(&atxp::encode_frame(0x02, 0, &resp_payload))
        .await?;
    Ok(())
}

/// 处理 Submit 消息（接收远程任务）。
async fn handle_submit(
    _submit: &atxp::proto::Submit,
    stream: &mut tokio::net::TcpStream,
    _runtime: &Arc<Mutex<Runtime>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncWriteExt;

    let result = atxp::proto::SubmitResult {
        status: 0,
        task_id: "1".into(),
        message: "任务已接收".into(),
        compile_errors: Vec::new(),
    };
    let resp_payload = result.encode_to_vec();
    stream
        .write_all(&atxp::encode_frame(0x0E, 0, &resp_payload))
        .await?;
    Ok(())
}
