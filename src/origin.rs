//! 远程连接管理（origin）。
//!
//! 管理远程 runner 的连接别名：添加、列出、删除。
//! 配置持久化到 `atomix.toml` 的 `[origin]` 段。
//!
//! 详见 docs/10-命令行规范.md §4.3。

use std::path::Path;

/// 远程连接配置。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OriginEntry {
    pub alias: String,
    pub address: String,
    pub port: u16,
}

/// Origin 配置（`atomix.toml` 的 `[origin]` 段）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct OriginConfig {
    #[serde(default)]
    pub connection: Vec<OriginEntry>,
}

impl OriginConfig {
    /// 从 `atomix.toml` 加载 origin 配置。
    pub fn load() -> Self {
        let path = Path::new("atomix.toml");
        if !path.exists() {
            return Self::default();
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        // 尝试从完整 config 中只提取 [origin] 段
        // serde 直接解析整个文件可能失败，我们用 toml::Value 提取
        if let Ok(value) = content.parse::<toml::Value>() {
            if let Some(origin) = value.get("origin") {
                if let Ok(config) = origin.clone().try_into::<OriginConfig>() {
                    return config;
                }
            }
        }
        Self::default()
    }

    /// 保存 origin 配置到 `atomix.toml`。
    pub fn save(&self) -> Result<(), String> {
        let path = Path::new("atomix.toml");
        let content = if path.exists() {
            std::fs::read_to_string(path).unwrap_or_default()
        } else {
            String::new()
        };

        // 用 toml::Value 操作现有的 atomix.toml
        let mut value: toml::Value = content
            .parse()
            .unwrap_or(toml::Value::Table(toml::value::Table::new()));

        // 序列化 origin 配置
        let origin_value =
            toml::Value::try_from(self).map_err(|e| format!("序列化 origin 配置失败: {}", e))?;

        if let toml::Value::Table(ref mut table) = value {
            table.insert("origin".to_string(), origin_value);
        }

        let new_content =
            toml::to_string_pretty(&value).map_err(|e| format!("序列化配置失败: {}", e))?;

        std::fs::write(path, new_content).map_err(|e| format!("写入配置失败: {}", e))?;

        Ok(())
    }

    /// 添加或更新一个 remote 连接。
    pub fn upsert(&mut self, entry: OriginEntry) {
        if let Some(existing) = self.connection.iter_mut().find(|c| c.alias == entry.alias) {
            existing.address = entry.address;
            existing.port = entry.port;
        } else {
            self.connection.push(entry);
        }
    }

    /// 删除一个 remote 连接。
    pub fn remove(&mut self, alias: &str) -> bool {
        let len = self.connection.len();
        self.connection.retain(|c| c.alias != alias);
        self.connection.len() < len
    }

    /// 查找一个 remote 连接。
    pub fn find(&self, alias: &str) -> Option<&OriginEntry> {
        self.connection.iter().find(|c| c.alias == alias)
    }
}

/// 检查远程连接是否可达，返回 JSON 状态。
pub fn check_status(entry: &OriginEntry) -> Result<serde_json::Value, String> {
    use crate::base::atxp_transport::AtxpTransport;

    let mut transport = AtxpTransport::connect(&entry.address, entry.port)?;

    // 发送 Request runner/status
    use prost::Message;
    let req = crate::base::atxp::proto::Request {
        resource: "runner".into(),
        action: "status".into(),
        params: std::collections::HashMap::new(),
    };
    let payload = req.encode_to_vec();
    let (hdr, resp_payload) = transport.exchange(
        crate::base::atxp::MsgType::Request as u8,
        &payload,
    )?;

    if hdr.msg_type == crate::base::atxp::MsgType::Error as u8 {
        if let Ok(err) = crate::base::atxp::proto::Error::decode(resp_payload.as_slice()) {
            return Err(format!("查询被拒绝: {}", err.message));
        }
        return Err("查询被拒绝".to_string());
    }

    if hdr.msg_type != crate::base::atxp::MsgType::Response as u8 {
        return Err("响应类型异常".to_string());
    }

    if let Ok(response) = crate::base::atxp::proto::Response::decode(resp_payload.as_slice()) {
        if !response.error.is_empty() {
            return Err(response.error);
        }
        if let Ok(status) = crate::base::atxp::proto::RunnerStatus::decode(response.data.as_slice()) {
            return Ok(serde_json::json!({
                "state": status.state,
                "version": status.version,
                "task_count": status.task_count,
                "running_count": status.running_count,
                "total_instrs": status.total_instrs,
                "mem_used_mb": status.mem_used_mb,
                "mem_limit_mb": status.mem_limit_mb,
            }));
        }
    }

    Err("无法解析远程响应".to_string())
}
