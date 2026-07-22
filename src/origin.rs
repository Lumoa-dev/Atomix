//! 远程连接管理（origin）。
//!
//! 管理远程 runner 的连接别名：添加、列出、删除。
//! 配置持久化到 `atomix.toml` 的 `[origin]` 段。
//!
//! 详见 docs/10-命令行规范.md §4.3。

use std::collections::HashMap;
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
        let mut content = if path.exists() {
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
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let addr = format!("{}:{}", entry.address, entry.port);
    let mut stream = TcpStream::connect_timeout(
        &addr.parse().map_err(|e| format!("地址解析失败: {}", e))?,
        Duration::from_secs(5),
    )
    .map_err(|e| format!("连接失败: {}", e))?;

    // 发送 Query runner/status
    use prost::Message;
    let query = crate::base::atxp::proto::Query {
        endpoint: "runner/status".into(),
        operation: 0, // GET
        params: Vec::new(),
    };
    let payload = query.encode_to_vec();
    let frame = crate::base::atxp::encode_frame(0x05, 1, &payload);

    stream
        .write_all(&frame)
        .map_err(|e| format!("发送失败: {}", e))?;

    // 读取响应
    let mut buf = vec![0u8; 4096];
    let n = stream
        .read(&mut buf)
        .map_err(|e| format!("读取失败: {}", e))?;
    buf.truncate(n);

    // 解码帧
    if let Some((_hdr, resp_payload, _rest)) = crate::base::atxp::decode_frame(&buf) {
        if let Ok(result) = crate::base::atxp::proto::QueryResult::decode(resp_payload.as_slice()) {
            let status: serde_json::Value = serde_json::from_slice(&result.data)
                .unwrap_or(serde_json::json!({"raw": "unknown"}));
            return Ok(status);
        }
    }

    Err("无法解析远程响应".to_string())
}
