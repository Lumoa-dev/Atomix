//! ATXP 同步传输层 — 连接管理、帧交换、握手、心跳。
//!
//! 位于 `base` 层，供 CLI / 包管理 / 调试器等所有组件共用。
//! 服务端 (runner/server.rs) 使用 tokio 异步实现，不依赖此模块。
//!
//! 设计文档: docs/05-通信协议.md §2-§7

use crate::base::atxp::{self, FrameHeader, MsgType};
use prost::Message;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

// ─── 传输配置 ──────────────────────────────────────────

/// ATXP 传输配置。
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// 连接超时。
    pub connect_timeout: Duration,
    /// 等待响应的超时。
    pub response_timeout: Duration,
    /// 心跳间隔（0 = 不自动心跳）。
    pub heartbeat_interval: Duration,
    /// 认证令牌（可选）。
    pub auth_token: Option<String>,
    /// 客户端标识。
    pub client_id: String,
    /// 协议版本（主版本高16位, 次版本低16位）。
    pub protocol_version: u32,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            response_timeout: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(30),
            auth_token: None,
            client_id: String::new(),
            protocol_version: 0x0001_0000, // v1.0
        }
    }
}

// ─── 传输层 ────────────────────────────────────────────

/// ATXP 同步传输连接。
///
/// 提供帧级 send/recv 和协议级 request/submit/heartbeat。
pub struct AtxpTransport {
    stream: TcpStream,
    seq_id: u32,
    config: TransportConfig,
    /// 对端返回的 deny_commands 列表（Hello 后填充）。
    pub deny_commands: Vec<String>,
    /// 对端 version（Hello 后填充）。
    pub peer_version: u32,
    /// 对端 features（Hello 后填充）。
    pub peer_features: u64,
}

impl AtxpTransport {
    /// 连接到远程端点并完成 Hello 握手。
    pub fn connect(addr: &str, port: u16) -> Result<Self, String> {
        Self::connect_with_config(addr, port, TransportConfig::default())
    }

    /// 使用自定义配置连接。
    pub fn connect_with_config(
        addr: &str,
        port: u16,
        config: TransportConfig,
    ) -> Result<Self, String> {
        let address = format!("{}:{}", addr, port);
        let stream = TcpStream::connect_timeout(
            &address
                .parse()
                .map_err(|e| format!("地址解析失败: {}", e))?,
            config.connect_timeout,
        )
        .map_err(|e| format!("连接失败 ({}): {}", address, e))?;

        stream
            .set_read_timeout(Some(config.response_timeout))
            .map_err(|e| format!("设置读取超时失败: {}", e))?;

        let mut transport = Self {
            stream,
            seq_id: 0,
            config,
            deny_commands: Vec::new(),
            peer_version: 0,
            peer_features: 0,
        };

        // Hello 握手
        transport.handshake()?;

        Ok(transport)
    }

    // ─── 握手 ───────────────────────────────────────

    /// Hello 握手：发送 Hello 并接收对端 Hello。
    fn handshake(&mut self) -> Result<(), String> {
        let hello = atxp::proto::Hello {
            protocol_version: self.config.protocol_version,
            features: 0,
            auth_token: self.config.auth_token.clone().unwrap_or_default(),
            client_id: self.config.client_id.clone(),
            mode: 0, // LOCAL
        };
        let payload = hello.encode_to_vec();
        let (hdr, resp_payload) = self.exchange(MsgType::Hello as u8, &payload)?;

        if hdr.msg_type == MsgType::Error as u8 {
            if let Ok(err) = atxp::proto::Error::decode(resp_payload.as_slice()) {
                return Err(format!("握手被拒绝: {} ({})", err.message, err.code));
            }
            return Err("握手被拒绝".to_string());
        }

        if hdr.msg_type != MsgType::Hello as u8 {
            return Err(format!(
                "握手响应类型异常: 期望 Hello(0x01), 得到 {:#04x}",
                hdr.msg_type
            ));
        }

        if let Ok(peer_hello) = atxp::proto::Hello::decode(resp_payload.as_slice()) {
            self.peer_version = peer_hello.protocol_version;
            self.peer_features = peer_hello.features;
            // 版本协商：主版本必须一致
            let client_major = (self.config.protocol_version >> 16) as u16;
            let peer_major = (peer_hello.protocol_version >> 16) as u16;
            if client_major != peer_major {
                return Err(format!(
                    "版本不兼容: client v{}.{}, peer v{}.{}",
                    client_major,
                    (self.config.protocol_version & 0xFFFF) as u16,
                    peer_major,
                    (peer_hello.protocol_version & 0xFFFF) as u16,
                ));
            }
        }

        Ok(())
    }

    // ─── Request/Response ───────────────────────────

    /// 发送统一 Request 并接收 Response。
    ///
    /// `params` 为可选的操作参数 map (key → Protobuf 编码 bytes)。
    pub fn request(
        &mut self,
        resource: &str,
        action: &str,
        params: &HashMap<String, Vec<u8>>,
    ) -> Result<atxp::proto::Response, String> {
        let req = atxp::proto::Request {
            resource: resource.to_string(),
            action: action.to_string(),
            params: params.clone(),
        };
        let payload = req.encode_to_vec();
        let (hdr, resp_payload) = self.exchange(MsgType::Request as u8, &payload)?;

        if hdr.msg_type == MsgType::Error as u8 {
            if let Ok(err) = atxp::proto::Error::decode(resp_payload.as_slice()) {
                return Err(format!("请求错误 ({}): {}", err.code, err.message));
            }
            return Err("请求被拒绝".to_string());
        }

        if hdr.msg_type != MsgType::Response as u8 {
            return Err(format!(
                "响应类型异常: 期望 Response(0x05), 得到 {:#04x}",
                hdr.msg_type
            ));
        }

        atxp::proto::Response::decode(resp_payload.as_slice())
            .map_err(|e| format!("Response 解码失败: {}", e))
    }

    // ─── 提交任务 ───────────────────────────────────

    /// 提交任务并等待 SubmitResult。
    pub fn submit(&mut self, submit: &atxp::proto::Submit) -> Result<atxp::proto::SubmitResult, String> {
        let payload = submit.encode_to_vec();
        let (hdr, resp_payload) = self.exchange(MsgType::Submit as u8, &payload)?;

        if hdr.msg_type == MsgType::Error as u8 {
            if let Ok(err) = atxp::proto::Error::decode(resp_payload.as_slice()) {
                return Err(format!("提交错误: {}", err.message));
            }
            return Err("提交被拒绝".to_string());
        }

        if hdr.msg_type != MsgType::SubmitResult as u8 {
            return Err(format!(
                "响应类型异常: 期望 SubmitResult(0x08), 得到 {:#04x}",
                hdr.msg_type
            ));
        }

        atxp::proto::SubmitResult::decode(resp_payload.as_slice())
            .map_err(|e| format!("SubmitResult 解码失败: {}", e))
    }

    // ─── 心跳 ───────────────────────────────────────

    /// 发送心跳并等待 Ack。
    pub fn heartbeat(&mut self) -> Result<(), String> {
        let hb = atxp::proto::Heartbeat {
            client_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        let payload = hb.encode_to_vec();
        let (hdr, _) = self.exchange(MsgType::Heartbeat as u8, &payload)?;

        if hdr.msg_type == MsgType::Ack as u8 {
            Ok(())
        } else if hdr.msg_type == MsgType::Error as u8 {
            Err("心跳被拒绝".to_string())
        } else {
            Err(format!(
                "心跳响应异常: 期望 Ack(0x02), 得到 {:#04x}",
                hdr.msg_type
            ))
        }
    }

    // ─── 断开连接 ───────────────────────────────────

    /// 发送 Bye 并关闭连接。
    pub fn disconnect(&mut self) -> Result<(), String> {
        let bye = atxp::proto::Bye {
            reason: 0, // NORMAL
            message: "bye".into(),
        };
        let payload = bye.encode_to_vec();
        // 发送 Bye，不等待响应
        let _ = self.send_frame(MsgType::Bye as u8, &payload);
        // 关闭 TCP 连接
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        Ok(())
    }

    // ─── 帧级操作 ───────────────────────────────────

    /// 发送一帧。
    pub fn send_frame(&mut self, msg_type: u8, payload: &[u8]) -> Result<(), String> {
        self.seq_id += 1;
        let frame = atxp::encode_frame(msg_type, self.seq_id, payload);
        self.stream
            .write_all(&frame)
            .map_err(|e| format!("发送失败: {}", e))
    }

    /// 接收一帧。如果无数据可用（超时），返回 None 含义的错误。
    pub fn recv_frame(&mut self) -> Result<(FrameHeader, Vec<u8>), String> {
        let mut buf = vec![0u8; 65536];
        let n = self
            .stream
            .read(&mut buf)
            .map_err(|e| format!("读取失败: {}", e))?;
        if n == 0 {
            return Err("连接已关闭".to_string());
        }
        buf.truncate(n);

        match atxp::decode_frame(&buf) {
            Some((hdr, payload, _)) => Ok((hdr, payload)),
            None => Err("无效的帧（CRC 校验失败或数据损坏）".to_string()),
        }
    }

    /// 发送一帧并等待响应。
    pub fn exchange(&mut self, msg_type: u8, payload: &[u8]) -> Result<(FrameHeader, Vec<u8>), String> {
        self.seq_id += 1;
        let frame = atxp::encode_frame(msg_type, self.seq_id, payload);
        self.stream
            .write_all(&frame)
            .map_err(|e| format!("发送失败: {}", e))?;

        self.recv_frame()
    }

    // ─── 状态查询 ───────────────────────────────────

    /// 是否已连接。
    pub fn is_connected(&self) -> bool {
        self.stream.peer_addr().is_ok()
    }

    /// 当前序列号。
    pub fn seq_id(&self) -> u32 {
        self.seq_id
    }
}

impl Drop for AtxpTransport {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_config_defaults() {
        let cfg = TransportConfig::default();
        assert_eq!(cfg.connect_timeout, Duration::from_secs(10));
        assert_eq!(cfg.response_timeout, Duration::from_secs(30));
        assert_eq!(cfg.protocol_version, 0x0001_0000);
        assert!(cfg.auth_token.is_none());
    }

    #[test]
    fn connect_refused() {
        // 连接到一个没有服务的端口，应失败
        let result = AtxpTransport::connect("127.0.0.1", 1);
        assert!(result.is_err());
    }

    #[test]
    fn msg_type_constants() {
        assert_eq!(MsgType::Hello as u8, 0x01);
        assert_eq!(MsgType::Request as u8, 0x04);
        assert_eq!(MsgType::Response as u8, 0x05);
        assert_eq!(MsgType::Submit as u8, 0x07);
        assert_eq!(MsgType::Heartbeat as u8, 0x09);
        assert_eq!(MsgType::Bye as u8, 0x0A);
    }
}
