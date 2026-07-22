//! 远程 TUI 调试器 — 12 个远程监控页面。
//!
//! 对应设计文档 §7.1（远程页面）。
//! 远程模式通过 ATXP 协议连接远程 Runner，提供任务监控、Runner 状态、
//! 内存槽位、控制器参数等运行时观察能力。
//!
//! 注意：远程模式不支持断点、单步、watch、时间轴、数据追踪等深度调试功能。

use crate::debug::session::LocalDebugSession;
use crate::debug::tui::pages::{Page, PageId};
use crate::runner::client::AtxpClient;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use std::collections::HashMap;
use std::time::Instant;

// ═══════════════════════════════════════════════════════════
// 远程会话状态
// ═══════════════════════════════════════════════════════════

/// 远程调试会话 — 通过 ATXP 连接远程 Runner。
pub struct RemoteSession {
    /// 当前连接的别名。
    pub alias: String,
    /// ATXP 客户端。
    client: AtxpClient,
    /// 最后一次刷新的数据。
    pub last_refresh: Instant,
    /// 缓存的远程状态。
    pub status: serde_json::Value,
    /// 缓存的任务列表。
    pub tasks: Vec<serde_json::Value>,
    /// 缓存的 Runner 配置。
    pub config: serde_json::Value,
    /// 连接是否正常。
    pub connected: bool,
}

impl RemoteSession {
    /// 通过别名连接到远程 Runner。
    pub fn connect_by_alias(alias: &str) -> Result<Self, String> {
        let mut client = AtxpClient::connect_by_alias(alias)?;
        let status = client
            .query_status()
            .unwrap_or(serde_json::json!({"error": "query failed"}));
        let tasks = client.query_tasks().unwrap_or_default();
        let config = client.query_config().unwrap_or(serde_json::json!({}));

        Ok(Self {
            alias: alias.to_string(),
            client,
            last_refresh: Instant::now(),
            status,
            tasks,
            config,
            connected: true,
        })
    }

    /// 刷新远程数据。
    pub fn refresh(&mut self) {
        if !self.connected {
            return;
        }
        self.status = self.client.query_status().unwrap_or(self.status.clone());
        self.tasks = self.client.query_tasks().unwrap_or(self.tasks.clone());
        self.config = self.client.query_config().unwrap_or(self.config.clone());
        self.last_refresh = Instant::now();
    }

    /// 提交任务到远程。
    pub fn submit_task(&mut self, binary: &[u8]) -> Result<String, String> {
        self.client.submit_task(binary)
    }

    /// 获取任务日志。
    pub fn task_log(&mut self, task_id: &str, lines: usize) -> Result<String, String> {
        self.client.query_task_log(task_id, lines)
    }

    /// 获取远程 perf 数据。
    pub fn perf_data(&mut self) -> Result<serde_json::Value, String> {
        self.client.query_perf()
    }

    /// 获取槽位布局。
    pub fn slot_layout(&mut self) -> Result<serde_json::Value, String> {
        self.client.query_slots()
    }

    /// 获取控制器状态。
    pub fn controller_status(&mut self) -> Result<serde_json::Value, String> {
        self.client.query_controller()
    }
}

// ═══════════════════════════════════════════════════════════
// 远程页面基类
// ═══════════════════════════════════════════════════════════

/// 远程页面都需要实现的刷新接口。
pub trait RemotePage: Page {
    /// 刷新远程数据。
    fn refresh_data(&mut self, session: &mut RemoteSession);
    /// 页面标题。
    fn remote_title(&self) -> &str;
}

// ═══════════════════════════════════════════════════════════
// 12 个远程页面实现
// ═══════════════════════════════════════════════════════════

// ─── 7.1.1 Connection Manager ──────────────────────────────

pub struct ConnectionsPage {
    pub connections: Vec<(String, String, u16, bool)>,
    pub selected: usize,
}

impl ConnectionsPage {
    pub fn new() -> Self {
        let config = crate::origin::OriginConfig::load();
        let connections = config
            .connection
            .iter()
            .map(|e| (e.alias.clone(), e.address.clone(), e.port, false))
            .collect();
        Self {
            connections,
            selected: 0,
        }
    }
}

impl Page for ConnectionsPage {
    fn title(&self) -> &str {
        "Connection Manager — 连接管理"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let mut lines = vec![
            Line::from(Span::styled(
                " 已保存的远程连接",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "  Alias         Address:Port         Status",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  ─────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        for (i, (alias, addr, port, connected)) in self.connections.iter().enumerate() {
            let status = if *connected {
                "● connected"
            } else {
                "○ disconnected"
            };
            let style = if i == self.selected {
                Style::default().fg(Color::Yellow).bg(Color::DarkGray)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "  {:<14} {:<18} {}",
                    alias,
                    format!("{}:{}", addr, port),
                    status
                ),
                style,
            )));
        }
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "  Enter 连接  Del 删除  r 刷新",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_enter(&mut self, _session: &mut LocalDebugSession, status: &mut String) {
        if self.selected < self.connections.len() {
            *status = format!("正在连接到 {}...", self.connections[self.selected].0);
        }
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.2 Runner Dashboard ────────────────────────────────

pub struct DashboardPage {
    pub data: serde_json::Value,
}

impl DashboardPage {
    pub fn new() -> Self {
        Self {
            data: serde_json::json!({}),
        }
    }
}

impl Page for DashboardPage {
    fn title(&self) -> &str {
        "Runner Dashboard — Runner 概览"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let d = &self.data;
        let mut lines = vec![
            Line::from(Span::styled(
                " Runner 状态概览",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!(
                "  Status:     {}",
                d.get("status").and_then(|v| v.as_str()).unwrap_or("—")
            ))),
            Line::from(Span::raw(format!(
                "  Tasks:      {} running, {} completed, {} pending",
                d.get("running").and_then(|v| v.as_u64()).unwrap_or(0),
                d.get("completed").and_then(|v| v.as_u64()).unwrap_or(0),
                d.get("pending").and_then(|v| v.as_u64()).unwrap_or(0)
            ))),
            Line::from(Span::raw(format!(
                "  Memory:     {} MB / {} MB",
                d.get("mem_used").and_then(|v| v.as_u64()).unwrap_or(0),
                d.get("mem_total").and_then(|v| v.as_u64()).unwrap_or(0)
            ))),
            Line::from(Span::raw(format!(
                "  CPU:        {}%",
                d.get("cpu_pct").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ))),
            Line::from(Span::raw(format!(
                "  Uptime:     {}s",
                d.get("uptime").and_then(|v| v.as_u64()).unwrap_or(0)
            ))),
            Line::from(Span::raw(format!(
                "  Instr/sec:  {}",
                d.get("instr_rate").and_then(|v| v.as_u64()).unwrap_or(0)
            ))),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "  按 r 刷新数据",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.3 Task List ───────────────────────────────────────

pub struct TaskListPage {
    pub tasks: Vec<serde_json::Value>,
    pub selected: usize,
}

impl TaskListPage {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            selected: 0,
        }
    }
}

impl Page for TaskListPage {
    fn title(&self) -> &str {
        "Task List — 任务列表"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let mut lines = vec![
            Line::from(Span::styled(
                " 任务池状态",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "  ID  Name           Status     Instrs   Memory   Time",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  ─────────────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        for (i, task) in self.tasks.iter().enumerate() {
            let status_color = match task.get("status").and_then(|v| v.as_str()).unwrap_or("") {
                "running" => Color::Green,
                "pending" => Color::Yellow,
                "error" => Color::Red,
                _ => Color::White,
            };
            let is_sel = i == self.selected;
            let bg_style = if is_sel {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            let status_style = Style::default().fg(status_color);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "  {:<4} ",
                        task.get("id").and_then(|v| v.as_u64()).unwrap_or(0)
                    ),
                    bg_style,
                ),
                Span::styled(
                    format!(
                        "{:<14} ",
                        task.get("name").and_then(|v| v.as_str()).unwrap_or("—")
                    ),
                    bg_style,
                ),
                Span::styled(
                    format!(
                        "{:<10} ",
                        task.get("status").and_then(|v| v.as_str()).unwrap_or("—")
                    ),
                    status_style,
                ),
                Span::styled(
                    format!(
                        "{:<8} ",
                        task.get("instrs").and_then(|v| v.as_u64()).unwrap_or(0)
                    ),
                    bg_style,
                ),
                Span::styled(
                    format!(
                        "{:<8} ",
                        task.get("memory").and_then(|v| v.as_u64()).unwrap_or(0)
                    ),
                    bg_style,
                ),
                Span::styled(
                    format!(
                        "{}",
                        task.get("elapsed").and_then(|v| v.as_f64()).unwrap_or(0.0)
                    ),
                    bg_style,
                ),
            ]));
        }
        if self.tasks.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无任务数据）",
                Style::default().fg(Color::Gray),
            )));
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_enter(&mut self, _session: &mut LocalDebugSession, _status: &mut String) {}
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.4 Task Snapshot ──────────────────────────────────

pub struct TaskSnapshotPage {
    pub task_id: String,
    pub data: serde_json::Value,
}

impl TaskSnapshotPage {
    pub fn new() -> Self {
        Self {
            task_id: String::new(),
            data: serde_json::json!({}),
        }
    }
}

impl Page for TaskSnapshotPage {
    fn title(&self) -> &str {
        "Task Snapshot — 任务快照"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let d = &self.data;
        let mut lines = vec![
            Line::from(Span::styled(
                format!(" 任务 {} 快照", self.task_id),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!("  Status: {:?}", d.get("status")))),
            Line::from(Span::raw(format!(
                "  PC: {:#x}",
                d.get("pc").and_then(|v| v.as_u64()).unwrap_or(0)
            ))),
            Line::from(Span::raw("")),
            Line::from(Span::styled(" Registers", Style::default().fg(Color::Cyan))),
        ];
        if let Some(regs) = d.get("regs").and_then(|v| v.as_array()) {
            for (i, val) in regs.iter().enumerate() {
                lines.push(Line::from(Span::raw(format!(
                    "  R{}: {:#018x}",
                    i,
                    val.as_u64().unwrap_or(0)
                ))));
            }
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.5 Controller Panel ────────────────────────────────

pub struct ControllerPage {
    pub data: serde_json::Value,
}

impl ControllerPage {
    pub fn new() -> Self {
        Self {
            data: serde_json::json!({}),
        }
    }
}

impl Page for ControllerPage {
    fn title(&self) -> &str {
        "Controller Panel — 控制器面板"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let d = &self.data;
        let mut lines = vec![
            Line::from(Span::styled(
                " 自适应控制器状态",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!(
                "  Batches:    {}",
                d.get("batches").and_then(|v| v.as_u64()).unwrap_or(0)
            ))),
            Line::from(Span::raw(format!(
                "  Backlog:    {}",
                d.get("backlog").and_then(|v| v.as_u64()).unwrap_or(0)
            ))),
            Line::from(Span::raw(format!(
                "  OOM Feedback: {}",
                d.get("oom_feedback")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            ))),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                " Sigmoid Factors",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::raw(format!(
                "  α (alpha):  {:.4}",
                d.get("alpha").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ))),
            Line::from(Span::raw(format!(
                "  β (beta):   {:.4}",
                d.get("beta").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ))),
            Line::from(Span::raw(format!(
                "  γ (gamma):  {:.4}",
                d.get("gamma").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ))),
            Line::from(Span::raw(format!(
                "  δ (delta):  {:.4}",
                d.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ))),
        ];
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.6 Memory Slots ───────────────────────────────────

pub struct SlotsPage {
    pub data: serde_json::Value,
}

impl SlotsPage {
    pub fn new() -> Self {
        Self {
            data: serde_json::json!({}),
        }
    }
}

impl Page for SlotsPage {
    fn title(&self) -> &str {
        "Memory Slots — 内存槽位"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let d = &self.data;
        let normal = d.get("normal_slots").and_then(|v| v.as_u64()).unwrap_or(0);
        let slipway = d.get("slipway_slots").and_then(|v| v.as_u64()).unwrap_or(0);
        let dead = d.get("dead_slots").and_then(|v| v.as_u64()).unwrap_or(0);
        let usage_pct = d.get("usage_pct").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let frag = d
            .get("fragmentation")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let mut lines = vec![
            Line::from(Span::styled(
                " 内存槽位布局",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!("  NORMAL:  {} 槽位", normal))),
            Line::from(Span::raw(format!("  SLIPWAY: {} 槽位", slipway))),
            Line::from(Span::raw(format!("  DEAD:    {} 槽位", dead))),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!("  使用率:   {:.1}%", usage_pct))),
            Line::from(Span::raw(format!("  碎片率:  {:.1}%", frag))),
        ];
        // 文本可视化槽位
        let total = normal + slipway + dead;
        if total > 0 {
            let bar_width = (area.width as usize).saturating_sub(4).min(60);
            let n_count = (normal as f64 / total as f64 * bar_width as f64).round() as usize;
            let s_count = (slipway as f64 / total as f64 * bar_width as f64).round() as usize;
            let d_count = bar_width.saturating_sub(n_count + s_count);
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::raw(format!(
                "  {}{}{}",
                "█".repeat(n_count),
                "▓".repeat(s_count),
                "░".repeat(d_count)
            ))));
            lines.push(Line::from(Span::raw("  NORMAL SLIPWAY DEAD")));
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.7 Submit Task ─────────────────────────────────────

pub struct SubmitPage {
    pub binary_path: String,
    pub task_name: String,
    pub mode: String,
    pub opt_level: String,
}

impl SubmitPage {
    pub fn new() -> Self {
        Self {
            binary_path: String::new(),
            task_name: String::new(),
            mode: "release".to_string(),
            opt_level: "O0".to_string(),
        }
    }
}

impl Page for SubmitPage {
    fn title(&self) -> &str {
        "Submit Task — 提交任务"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let mut lines = vec![
            Line::from(Span::styled(
                " 提交任务到远程 Runner",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!("  源文件:   {}", self.binary_path))),
            Line::from(Span::raw(format!("  任务名:   {}", self.task_name))),
            Line::from(Span::raw(format!("  模式:     {}", self.mode))),
            Line::from(Span::raw(format!("  优化级别: {}", self.opt_level))),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "  Enter 提交  Tab 切换字段",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.8 Runner Config ──────────────────────────────────

pub struct ConfigPage {
    pub data: serde_json::Value,
}

impl ConfigPage {
    pub fn new() -> Self {
        Self {
            data: serde_json::json!({}),
        }
    }
}

impl Page for ConfigPage {
    fn title(&self) -> &str {
        "Runner Config — Runner 配置"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let d = &self.data;
        let mut lines = vec![
            Line::from(Span::styled(
                " Runner 配置",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
        ];
        if let Some(obj) = d.as_object() {
            for (key, val) in obj.iter() {
                let writable = ["max_concurrent", "quantum", "trace_level", "deny_commands"]
                    .contains(&key.as_str());
                let marker = if writable { "✎" } else { " " };
                lines.push(Line::from(Span::raw(format!(
                    "  {} {} = {}",
                    marker, key, val
                ))));
            }
        }
        if lines.len() <= 2 {
            lines.push(Line::from(Span::styled(
                "  （无配置数据）",
                Style::default().fg(Color::Gray),
            )));
        }
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "  ✎ = 可写字段  s 保存修改",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.9 Task Pool ──────────────────────────────────────

pub struct PoolPage {
    pub data: serde_json::Value,
}

impl PoolPage {
    pub fn new() -> Self {
        Self {
            data: serde_json::json!({}),
        }
    }
}

impl Page for PoolPage {
    fn title(&self) -> &str {
        "Task Pool — 任务池"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let d = &self.data;
        let pending = d.get("pending").and_then(|v| v.as_u64()).unwrap_or(0);
        let ready = d.get("ready").and_then(|v| v.as_u64()).unwrap_or(0);
        let running = d.get("running").and_then(|v| v.as_u64()).unwrap_or(0);
        let error = d.get("error").and_then(|v| v.as_u64()).unwrap_or(0);
        let depth = d.get("depth").and_then(|v| v.as_u64()).unwrap_or(0);

        let mut lines = vec![
            Line::from(Span::styled(
                " 任务池分布",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!(
                "  Pending: {}  Ready: {}  Running: {}  Error: {}",
                pending, ready, running, error
            ))),
            Line::from(Span::raw(format!("  依赖深度: {}", depth))),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                " 依赖 DAG 可视化（简化）",
                Style::default().fg(Color::Cyan),
            )),
        ];
        if let Some(deps) = d.get("dependencies").and_then(|v| v.as_array()) {
            for dep in deps {
                lines.push(Line::from(Span::raw(format!(
                    "  {} → {}",
                    dep.get("from").and_then(|v| v.as_str()).unwrap_or("?"),
                    dep.get("to").and_then(|v| v.as_str()).unwrap_or("?")
                ))));
            }
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.10 Runner Logs ──────────────────────────────────

pub struct LogsPage {
    pub logs: Vec<String>,
    pub filter: String,
    pub paused: bool,
}

impl LogsPage {
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
            filter: String::new(),
            paused: false,
        }
    }
}

impl Page for LogsPage {
    fn title(&self) -> &str {
        "Runner Logs — Runner 日志"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let status = if self.paused {
            "⏸ PAUSED"
        } else {
            "▶ LIVE"
        };
        let max_visible = (area.height as usize).saturating_sub(3);
        let mut lines = vec![
            Line::from(Span::styled(
                format!(" Runner 日志 [{}]", status),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
        ];
        let display_logs: Vec<&str> = self
            .logs
            .iter()
            .rev()
            .take(max_visible)
            .rev()
            .map(|s| s.as_str())
            .collect();
        for line in &display_logs {
            let color = if line.contains("ERROR") {
                Color::Red
            } else if line.contains("WARN") {
                Color::Yellow
            } else if line.contains("INFO") {
                Color::Green
            } else {
                Color::White
            };
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(color),
            )));
        }
        if self.logs.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无日志）",
                Style::default().fg(Color::Gray),
            )));
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 7.1.11 Memory Slot Animation ─────────────────────────

pub struct SlotsAnimPage {
    pub data: serde_json::Value,
    pub frame: usize,
}

impl SlotsAnimPage {
    pub fn new() -> Self {
        Self {
            data: serde_json::json!({}),
            frame: 0,
        }
    }
}

impl Page for SlotsAnimPage {
    fn title(&self) -> &str {
        "Slots Animation — 内存槽动画"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let d = &self.data;
        let normal = d.get("normal_slots").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let slipway = d.get("slipway_slots").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let dead = d.get("dead_slots").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        let mut lines = vec![
            Line::from(Span::styled(
                " 内存槽位分配/回收动画",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  Space 暂停/继续  ←→ 调速",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::raw("")),
        ];

        // 方块可视化
        let total = (normal + slipway + dead).max(1);
        let cols = 8usize;
        let rows = (total + cols - 1) / cols;
        for r in 0..rows.min(area.height as usize - 4) {
            let mut row = "  ".to_string();
            for c in 0..cols {
                let idx = r * cols + c;
                let ch = if idx < normal {
                    "█"
                } else if idx < normal + slipway {
                    "▓"
                } else if idx < total {
                    "░"
                } else {
                    " "
                };
                row.push_str(ch);
            }
            lines.push(Line::from(Span::styled(row, Style::default())));
        }
        lines.push(Line::from(Span::raw(format!(
            "  Frame: {} | NORMAL {} | SLIPWAY {} | DEAD {}",
            self.frame, normal, slipway, dead
        ))));
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {
        self.frame += 1;
    }
}

// ─── 7.1.12 Performance Analysis (Remote) ─────────────────

pub struct RemotePerfPage {
    pub data: serde_json::Value,
}

impl RemotePerfPage {
    pub fn new() -> Self {
        Self {
            data: serde_json::json!({}),
        }
    }
}

impl Page for RemotePerfPage {
    fn title(&self) -> &str {
        "Performance (Remote) — 远程性能分析"
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _session: &mut LocalDebugSession) {
        let d = &self.data;
        let mut lines = vec![
            Line::from(Span::styled(
                " 远程 Runner 性能指标",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!(
                "  CPU 使用率:       {:.1}%",
                d.get("cpu").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ))),
            Line::from(Span::raw(format!(
                "  内存使用率:       {:.1}%",
                d.get("memory").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ))),
            Line::from(Span::raw(format!(
                "  任务吞吐量:       {:.1}/s",
                d.get("throughput").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ))),
            Line::from(Span::raw(format!(
                "  队列深度:         {}",
                d.get("queue_depth").and_then(|v| v.as_u64()).unwrap_or(0)
            ))),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                " 控制器参数趋势 (最近 10 个样本)",
                Style::default().fg(Color::Cyan),
            )),
        ];
        if let Some(history) = d.get("controller_history").and_then(|v| v.as_array()) {
            for entry in history.iter().rev().take(10) {
                let batch = entry.get("batch").and_then(|v| v.as_u64()).unwrap_or(0);
                let alpha = entry.get("alpha").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let beta = entry.get("beta").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let gamma = entry.get("gamma").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let delta = entry.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0);
                lines.push(Line::from(Span::raw(format!(
                    "  batch={:<4} alpha={:.3} beta={:.3} gamma={:.3} delta={:.3}",
                    batch, alpha, beta, gamma, delta
                ))));
            }
        }
        if lines.len() <= 4 {
            lines.push(Line::from(Span::styled(
                "  （无性能数据）",
                Style::default().fg(Color::Gray),
            )));
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
            area,
        );
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}
