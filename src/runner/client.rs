//! ATXP 客户端 — 连接远程 runner，发送查询/提交/控制命令。
//!
//! 用于 `atomix runner run --origin` 和 `atomix task --origin`。

use crate::base::atxp;
use prost::Message;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// ATXP 客户端连接。
pub struct AtxpClient {
    stream: TcpStream,
    seq_id: u32,
}

impl AtxpClient {
    /// 连接到远程 runner。
    pub fn connect(addr: &str, port: u16) -> Result<Self, String> {
        let address = format!("{}:{}", addr, port);
        let stream = TcpStream::connect_timeout(
            &address.parse().map_err(|e| format!("地址解析失败: {}", e))?,
            Duration::from_secs(10),
        ).map_err(|e| format!("连接失败: {}", e))?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("设置超时失败: {}", e))?;
        Ok(Self { stream, seq_id: 0 })
    }

    /// 从 OriginConfig 查找别名并连接。
    pub fn connect_by_alias(alias: &str) -> Result<Self, String> {
        let config = crate::origin::OriginConfig::load();
        let entry = config.find(alias)
            .ok_or_else(|| format!("未找到远程连接: {}", alias))?;
        Self::connect(&entry.address, entry.port)
    }

    /// 发送并接收一帧消息。
    fn exchange(&mut self, msg_type: u8, payload: &[u8]) -> Result<(u8, Vec<u8>), String> {
        self.seq_id += 1;
        let frame = atxp::encode_frame(msg_type, self.seq_id, payload);
        self.stream.write_all(&frame)
            .map_err(|e| format!("发送失败: {}", e))?;

        // 读取响应
        let mut buf = vec![0u8; 65536];
        let n = self.stream.read(&mut buf)
            .map_err(|e| format!("读取失败: {}", e))?;
        buf.truncate(n);

        match atxp::decode_frame(&buf) {
            Some((hdr, resp_payload, _)) => Ok((hdr.msg_type, resp_payload)),
            None => Err("无效的响应帧".to_string()),
        }
    }

    /// 查询远程 runner 状态。
    pub fn query_status(&mut self) -> Result<serde_json::Value, String> {
        let query = atxp::proto::Query {
            endpoint: "runner/status".into(),
            operation: 0,
            params: Vec::new(),
        };
        let (_, resp) = self.exchange(0x05, &query.encode_to_vec())?;
        if let Ok(result) = atxp::proto::QueryResult::decode(resp.as_slice()) {
            serde_json::from_slice(&result.data)
                .map_err(|e| format!("JSON 解析失败: {}", e))
        } else {
            Err("QueryResult 解码失败".to_string())
        }
    }

    /// 查询远程任务列表。
    pub fn query_tasks(&mut self) -> Result<Vec<serde_json::Value>, String> {
        let query = atxp::proto::Query {
            endpoint: "runner/tasks".into(),
            operation: 0,
            params: Vec::new(),
        };
        let (_, resp) = self.exchange(0x05, &query.encode_to_vec())?;
        if let Ok(result) = atxp::proto::QueryResult::decode(resp.as_slice()) {
            serde_json::from_slice(&result.data)
                .map_err(|e| format!("JSON 解析失败: {}", e))
        } else {
            Err("QueryResult 解码失败".to_string())
        }
    }

    /// 提交任务到远程 runner 执行。
    pub fn submit_task(&mut self, binary: &[u8]) -> Result<String, String> {
        let submit = atxp::proto::Submit {
            mode: 0,
            source: String::new(),
            binary: binary.to_vec(),
            task_name: String::new(),
            output_mode: 0,
            ..Default::default()
        };
        let (_, resp) = self.exchange(0x0D, &submit.encode_to_vec())?;
        if let Ok(result) = atxp::proto::SubmitResult::decode(resp.as_slice()) {
            if result.status == 0 {
                Ok(result.task_id)
            } else {
                Err(format!("任务提交失败: {}", result.message))
            }
        } else {
            Err("SubmitResult 解码失败".to_string())
        }
    }

    /// 发送心跳。
    pub fn heartbeat(&mut self) -> Result<(), String> {
        let hb = atxp::proto::Ack { status: 0, message: "ping".into() };
        let (msg_type, _) = self.exchange(0x0B, &hb.encode_to_vec())?;
        if msg_type == 0x02 { Ok(()) }
        else { Err("心跳响应异常".to_string()) }
    }

    /// 查询远程 Runner 配置。
    pub fn query_config(&mut self) -> Result<serde_json::Value, String> {
        let query = atxp::proto::Query {
            endpoint: "runner/config".into(),
            operation: 0,
            params: Vec::new(),
        };
        let (_, resp) = self.exchange(0x05, &query.encode_to_vec())?;
        if let Ok(result) = atxp::proto::QueryResult::decode(resp.as_slice()) {
            serde_json::from_slice(&result.data)
                .map_err(|e| format!("JSON 解析失败: {}", e))
        } else {
            Err("QueryResult 解码失败".to_string())
        }
    }

    /// 查询指定任务的日志。
    pub fn query_task_log(&mut self, task_id: &str, _lines: usize) -> Result<String, String> {
        let query = atxp::proto::Query {
            endpoint: format!("task/{}/log", task_id),
            operation: 0,
            params: Vec::new(),
        };
        let (_, resp) = self.exchange(0x05, &query.encode_to_vec())?;
        if let Ok(result) = atxp::proto::QueryResult::decode(resp.as_slice()) {
            String::from_utf8(result.data).map_err(|e| format!("UTF-8 解码失败: {}", e))
        } else {
            Err("QueryResult 解码失败".to_string())
        }
    }

    /// 查询远程性能指标。
    pub fn query_perf(&mut self) -> Result<serde_json::Value, String> {
        let query = atxp::proto::Query {
            endpoint: "runner/perf".into(),
            operation: 0,
            params: Vec::new(),
        };
        let (_, resp) = self.exchange(0x05, &query.encode_to_vec())?;
        if let Ok(result) = atxp::proto::QueryResult::decode(resp.as_slice()) {
            serde_json::from_slice(&result.data)
                .map_err(|e| format!("JSON 解析失败: {}", e))
        } else {
            Err("QueryResult 解码失败".to_string())
        }
    }

    /// 查询内存槽位布局。
    pub fn query_slots(&mut self) -> Result<serde_json::Value, String> {
        let query = atxp::proto::Query {
            endpoint: "runner/slots".into(),
            operation: 0,
            params: Vec::new(),
        };
        let (_, resp) = self.exchange(0x05, &query.encode_to_vec())?;
        if let Ok(result) = atxp::proto::QueryResult::decode(resp.as_slice()) {
            serde_json::from_slice(&result.data)
                .map_err(|e| format!("JSON 解析失败: {}", e))
        } else {
            Err("QueryResult 解码失败".to_string())
        }
    }

    /// 查询控制器状态。
    pub fn query_controller(&mut self) -> Result<serde_json::Value, String> {
        let query = atxp::proto::Query {
            endpoint: "runner/controller".into(),
            operation: 0,
            params: Vec::new(),
        };
        let (_, resp) = self.exchange(0x05, &query.encode_to_vec())?;
        if let Ok(result) = atxp::proto::QueryResult::decode(resp.as_slice()) {
            serde_json::from_slice(&result.data)
                .map_err(|e| format!("JSON 解析失败: {}", e))
        } else {
            Err("QueryResult 解码失败".to_string())
        }
    }
}
