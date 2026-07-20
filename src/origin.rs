//! 远程连接管理（origin）。
//!
//! 管理远程 runner 的连接别名：添加、列出、删除。
//! 配置持久化到 `atomix.toml` 的 `[origin]` 段。
//!
//! 详见 docs/10-命令行规范.md §4.3。

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
    pub connections: Vec<OriginEntry>,
}
