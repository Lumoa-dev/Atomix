//! ATXP 客户端 — 远程 runner 操作封装。
//!
//! 基于 base::atxp_transport::AtxpTransport 的 Runner 专用客户端。
//! 方法返回真实 Protobuf 类型（而非 JSON 字符串）。
//!
//! 用于 `atomix runner run --origin` 和 `atomix task --origin`。

use crate::base::atxp;
use crate::base::atxp_transport::{AtxpTransport, TransportConfig};
use prost::Message;
use std::collections::HashMap;

/// ATXP Runner 客户端。
pub struct AtxpClient {
    transport: AtxpTransport,
}

impl AtxpClient {
    /// 连接到远程 runner。
    pub fn connect(addr: &str, port: u16) -> Result<Self, String> {
        let transport = AtxpTransport::connect(addr, port)?;
        Ok(Self { transport })
    }

    /// 使用自定义配置连接。
    pub fn connect_with_config(addr: &str, port: u16, config: TransportConfig) -> Result<Self, String> {
        let transport = AtxpTransport::connect_with_config(addr, port, config)?;
        Ok(Self { transport })
    }

    /// 从 OriginConfig 查找别名并连接。
    pub fn connect_by_alias(alias: &str) -> Result<Self, String> {
        let config = crate::origin::OriginConfig::load();
        let entry = config
            .find(alias)
            .ok_or_else(|| format!("未找到远程连接: {}", alias))?;
        Self::connect(&entry.address, entry.port)
    }

    // ─── Runner 操作 ─────────────────────────────

    /// 查询远程 runner 状态。
    pub fn query_status(&mut self) -> Result<atxp::proto::RunnerStatus, String> {
        let resp = self.transport.request("runner", "status", &empty_params())?;
        if !resp.error.is_empty() {
            return Err(resp.error);
        }
        atxp::proto::RunnerStatus::decode(resp.data.as_slice())
            .map_err(|e| format!("RunnerStatus 解码失败: {}", e))
    }

    /// 查询远程 Runner 配置。
    pub fn query_config(&mut self) -> Result<atxp::proto::RunnerConfig, String> {
        let resp = self.transport.request("runner", "config", &empty_params())?;
        if !resp.error.is_empty() {
            return Err(resp.error);
        }
        atxp::proto::RunnerConfig::decode(resp.data.as_slice())
            .map_err(|e| format!("RunnerConfig 解码失败: {}", e))
    }

    // ─── 任务操作 ─────────────────────────────

    /// 查询远程任务列表。
    pub fn query_tasks(&mut self) -> Result<atxp::proto::TaskList, String> {
        let resp = self.transport.request("task", "list", &empty_params())?;
        if !resp.error.is_empty() {
            return Err(resp.error);
        }
        atxp::proto::TaskList::decode(resp.data.as_slice())
            .map_err(|e| format!("TaskList 解码失败: {}", e))
    }

    /// 查询指定任务的状态。
    pub fn query_task_status(&mut self, task_id: &str) -> Result<atxp::proto::TaskStatus, String> {
        let mut params = HashMap::new();
        params.insert("task_id".into(), task_id.as_bytes().to_vec());
        let resp = self.transport.request("task", "status", &params)?;
        if !resp.error.is_empty() {
            return Err(resp.error);
        }
        atxp::proto::TaskStatus::decode(resp.data.as_slice())
            .map_err(|e| format!("TaskStatus 解码失败: {}", e))
    }

    /// 查询任务日志。
    pub fn query_task_log(&mut self, task_id: &str, lines: usize) -> Result<String, String> {
        let mut params = HashMap::new();
        params.insert("task_id".into(), task_id.as_bytes().to_vec());
        params.insert("lines".into(), (lines as u64).to_le_bytes().to_vec());
        let resp = self.transport.request("task", "log", &params)?;
        if !resp.error.is_empty() {
            return Err(resp.error);
        }
        String::from_utf8(resp.data).map_err(|e| format!("日志解码失败: {}", e))
    }

    /// 查询任务执行统计。
    pub fn query_task_stats(&mut self, task_id: &str) -> Result<atxp::proto::TaskStats, String> {
        let mut params = HashMap::new();
        params.insert("task_id".into(), task_id.as_bytes().to_vec());
        let resp = self.transport.request("task", "stats", &params)?;
        if !resp.error.is_empty() {
            return Err(resp.error);
        }
        atxp::proto::TaskStats::decode(resp.data.as_slice())
            .map_err(|e| format!("TaskStats 解码失败: {}", e))
    }

    /// 提交任务到远程 runner 执行。
    pub fn submit_task(&mut self, binary: &[u8]) -> Result<String, String> {
        let submit = atxp::proto::Submit {
            mode: 1, // SUBMIT_BINARY
            source: String::new(),
            binary: binary.to_vec(),
            task_name: String::new(),
            output_mode: 0, // OUTPUT_POLL
            ..Default::default()
        };
        let result = self.transport.submit(&submit)?;
        if result.status == 0 { // STATUS_ACCEPTED
            Ok(result.task_id)
        } else {
            Err(format!(
                "任务提交失败: {}",
                result.message
            ))
        }
    }

    // ─── 控制器 & 槽位 ─────────────────────────

    /// 查询控制器状态。
    pub fn query_controller(&mut self) -> Result<atxp::proto::ControllerState, String> {
        let resp = self.transport.request("controller", "status", &empty_params())?;
        if !resp.error.is_empty() {
            return Err(resp.error);
        }
        atxp::proto::ControllerState::decode(resp.data.as_slice())
            .map_err(|e| format!("ControllerState 解码失败: {}", e))
    }

    /// 查询内存槽位布局。
    pub fn query_slots(&mut self) -> Result<atxp::proto::SlotLayout, String> {
        let resp = self.transport.request("slots", "layout", &empty_params())?;
        if !resp.error.is_empty() {
            return Err(resp.error);
        }
        atxp::proto::SlotLayout::decode(resp.data.as_slice())
            .map_err(|e| format!("SlotLayout 解码失败: {}", e))
    }

    // ─── 生命周期 ─────────────────────────────

    /// 发送心跳。
    pub fn heartbeat(&mut self) -> Result<(), String> {
        self.transport.heartbeat()
    }

    /// 断开连接。
    pub fn disconnect(&mut self) -> Result<(), String> {
        self.transport.disconnect()
    }

    /// 获取底层传输层引用。
    pub fn transport(&self) -> &AtxpTransport {
        &self.transport
    }

    /// 检查是否仍连接。
    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    // ─── JSON 兼容方法（供 TUI 等使用） ──────────

    /// 查询状态并以 JSON 返回。
    pub fn query_status_json(&mut self) -> Result<serde_json::Value, String> {
        self.query_status().map(|s| {
            serde_json::json!({
                "state": s.state,
                "version": s.version,
                "task_count": s.task_count,
                "running_count": s.running_count,
                "total_instrs": s.total_instrs,
                "completed_count": s.completed_count,
                "mem_used_mb": s.mem_used_mb,
                "mem_limit_mb": s.mem_limit_mb,
            })
        })
    }

    /// 查询任务列表并以 JSON 返回。
    pub fn query_tasks_json(&mut self) -> Result<Vec<serde_json::Value>, String> {
        self.query_tasks().map(|list| {
            list.tasks
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.task_id,
                        "name": t.name,
                        "state": t.state,
                        "cycles": t.cycles,
                        "memory_mb": t.memory_mb,
                    })
                })
                .collect()
        })
    }

    /// 查询配置并以 JSON 返回。
    pub fn query_config_json(&mut self) -> Result<serde_json::Value, String> {
        self.query_config().map(|c| {
            serde_json::json!({
                "max_concurrent": c.max_concurrent,
                "quantum": c.quantum,
                "heartbeat_ms": c.heartbeat_ms,
                "trace_level": c.trace_level,
                "tls_enabled": c.tls_enabled,
            })
        })
    }

    /// 查询槽位布局并以 JSON 返回。
    pub fn query_slots_json(&mut self) -> Result<serde_json::Value, String> {
        self.query_slots().map(|layout| {
            let slots: Vec<serde_json::Value> = layout
                .slots
                .into_iter()
                .map(|s| {
                    serde_json::json!({
                        "slot_id": s.slot_id,
                        "task_id": s.task_id,
                        "base_addr": s.base_addr,
                        "size": s.size,
                        "used": s.used,
                        "status": s.status,
                    })
                })
                .collect();
            serde_json::json!({
                "slots": slots,
                "fragmentation": layout.fragmentation,
            })
        })
    }

    /// 查询控制器状态并以 JSON 返回。
    pub fn query_controller_json(&mut self) -> Result<serde_json::Value, String> {
        self.query_controller().map(|c| {
            serde_json::json!({
                "n_batch": c.n_batch,
                "hard_ceiling": c.hard_ceiling,
                "backlog_depth": c.backlog_depth,
                "oom_count": c.oom_count,
                "alpha_mem_current": c.alpha_mem_current,
                "beta": c.beta,
                "lambda_speed": c.lambda_speed,
                "sigma_volume": c.sigma_volume,
                "merged_factor": c.merged_factor,
                "total_slots": c.total_slots,
            })
        })
    }
}

/// 空的参数 map。
fn empty_params() -> HashMap<String, Vec<u8>> {
    HashMap::new()
}
