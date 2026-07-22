//! 远程 TUI 应用 — 事件循环、布局、命令处理。
//!
//! 对应设计文档 §7（远程模式）、§5.4（远程 TUI 命令）。

use crate::debug::tui::layout::TuiLayout;
use crate::debug::tui::pages::{Page, PageId};
use crate::debug::tui::remote::{
    RemoteSession, ConnectionsPage, DashboardPage, TaskListPage,
    TaskSnapshotPage, ControllerPage, SlotsPage, SubmitPage,
    ConfigPage, PoolPage, LogsPage, SlotsAnimPage, RemotePerfPage,
};
use crate::debug::session::LocalDebugSession;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 远程 TUI 应用状态。
pub struct RemoteTuiApp {
    pub remote: RemoteSession,
    pub layout: TuiLayout,
    pub running: bool,
    pub status_message: String,
    pub page_registry: HashMap<PageId, Box<dyn Page>>,
    pub active_page: PageId,
    pub page_stack: Vec<PageId>,
    pub command_buffer: String,
    pub command_mode: bool,
    pub show_help: bool,
    local_session: LocalDebugSession,
    last_refresh: Instant,
}

impl RemoteTuiApp {
    /// 通过别名连接到远程 Runner 并创建 TUI 应用。
    pub fn connect(alias: &str) -> Result<Self, String> {
        let remote = RemoteSession::connect_by_alias(alias)?;
        let mut page_registry: HashMap<PageId, Box<dyn Page>> = HashMap::new();
        page_registry.insert(PageId::RemoteConnections, Box::new(ConnectionsPage::new()));
        page_registry.insert(PageId::RemoteDashboard, Box::new(DashboardPage::new()));
        page_registry.insert(PageId::RemoteTaskList, Box::new(TaskListPage::new()));
        page_registry.insert(PageId::RemoteTaskSnapshot, Box::new(TaskSnapshotPage::new()));
        page_registry.insert(PageId::RemoteController, Box::new(ControllerPage::new()));
        page_registry.insert(PageId::RemoteSlots, Box::new(SlotsPage::new()));
        page_registry.insert(PageId::RemoteSubmit, Box::new(SubmitPage::new()));
        page_registry.insert(PageId::RemoteConfig, Box::new(ConfigPage::new()));
        page_registry.insert(PageId::RemoteTaskPool, Box::new(PoolPage::new()));
        page_registry.insert(PageId::RemoteLogs, Box::new(LogsPage::new()));
        page_registry.insert(PageId::RemoteSlotsAnim, Box::new(SlotsAnimPage::new()));
        page_registry.insert(PageId::RemotePerf, Box::new(RemotePerfPage::new()));

        // 占位 LocalDebugSession（远程 TUI 不使用本地调试）
        let dummy_text = vec![crate::base::isa::encode_ji(crate::base::isa::opcode::TRAP, 0)];
        let dummy_binary = crate::base::ir::AtxeBinary {
            header: crate::base::ir::Header::new(0, 1),
            sections: vec![], text: dummy_text, rodata: vec![],
            task_table: vec![], debug_info: vec![], exn_table: vec![], zones: vec![],
        };
        let dummy_vm = crate::runner::VmState::from_atxe(&dummy_binary)
            .map_err(|e| format!("占位 VM 创建失败: {}", e))?;
        let mut ls = LocalDebugSession::new(dummy_vm);
        ls.collected = true;

        Ok(Self {
            remote,
            layout: TuiLayout::new(),
            running: true,
            status_message: format!("已连接到 {}", alias),
            page_registry,
            active_page: PageId::RemoteDashboard,
            page_stack: vec![PageId::RemoteDashboard],
            command_buffer: String::new(),
            command_mode: false,
            show_help: false,
            local_session: ls,
            last_refresh: Instant::now(),
        })
    }

    fn current_page_mut(&mut self) -> Option<&mut Box<dyn Page>> {
        self.page_registry.get_mut(&self.active_page)
    }

    pub fn navigate_to(&mut self, page_id: PageId) {
        self.page_stack.push(page_id.clone());
        self.active_page = page_id;
    }

    pub fn navigate_back(&mut self) {
        if self.page_stack.len() > 1 {
            self.page_stack.pop();
            if let Some(prev) = self.page_stack.last() {
                self.active_page = prev.clone();
            }
        }
    }

    /// 定期刷新远程数据。
    fn refresh_data(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.remote.refresh();
        // 通过 refresh_remote trait 方法通知所有页面
        for (_id, page) in self.page_registry.iter_mut() {
            page.refresh_remote(&self.remote);
        }
        self.last_refresh = Instant::now();
    }

    /// 执行远程命令。
    fn execute_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }
        let command = parts[0].to_lowercase();
        match command.as_str() {
            "quit" | "q" => self.running = false,
            "help" | "h" | "?" => self.show_help = !self.show_help,
            "exit" => self.navigate_back(),
            "r" | "refresh" => {
                self.last_refresh = Instant::now() - Duration::from_secs(2);
                self.refresh_data();
                self.status_message = "数据已刷新".to_string();
            }
            ":connections" => self.navigate_to(PageId::RemoteConnections),
            ":dashboard" => self.navigate_to(PageId::RemoteDashboard),
            ":tasks" => self.navigate_to(PageId::RemoteTaskList),
            ":pool" => self.navigate_to(PageId::RemoteTaskPool),
            ":controller" => self.navigate_to(PageId::RemoteController),
            ":slots" => self.navigate_to(PageId::RemoteSlots),
            ":slots-anim" => self.navigate_to(PageId::RemoteSlotsAnim),
            ":submit" => self.navigate_to(PageId::RemoteSubmit),
            ":config" => self.navigate_to(PageId::RemoteConfig),
            ":logs" => self.navigate_to(PageId::RemoteLogs),
            ":perf" => self.navigate_to(PageId::RemotePerf),
            _ if command.starts_with(":task ") => {
                self.navigate_to(PageId::RemoteTaskSnapshot);
                let id = parts.get(1).unwrap_or(&"");
                self.status_message = format!("查看任务 {}", id);
            }
            _ => {
                self.status_message = format!("未知远程命令: {}", command);
            }
        }
    }

    /// 远程 TUI 主事件循环。
    pub fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> Result<(), String> {
        while self.running {
            self.refresh_data();
            terminal
                .draw(|frame| {
                    let area = frame.size();
                    let chunks = self.layout.calculate_layout(area);
                    self.layout
                        .render_title_bar(frame, chunks.title, &self.local_session);
                    let crumbs: Vec<String> = self
                        .page_stack
                        .iter()
                        .filter_map(|id| {
                            self.page_registry.get(id).map(|p| p.title().to_string())
                        })
                        .collect();
                    self.layout
                        .render_breadcrumb(frame, chunks.breadcrumb, &crumbs);
                    // 使用 split borrowing: 分别访问 page_registry 和 local_session
                    let registry = &mut self.page_registry;
                    let session = &mut self.local_session;
                    if let Some(page) = registry.get_mut(&self.active_page) {
                        page.render(frame, chunks.main_content, session);
                    }
                    self.layout
                        .render_right_panel(frame, chunks.right_panel, &self.local_session);
                    self.layout.render_command_bar(
                        frame,
                        chunks.command_bar,
                        self.command_mode,
                        &self.command_buffer,
                        &self.status_message,
                    );
                    if self.show_help {
                        self.layout.render_help_overlay(frame, area);
                    }
                })
                .map_err(|e| format!("渲染错误: {}", e))?;

            if event::poll(Duration::from_millis(100))
                .map_err(|e| format!("事件错误: {}", e))?
            {
                if let Event::Key(key) =
                    event::read().map_err(|e| format!("读事件错误: {}", e))?
                {
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat {
                        match key.code {
                            KeyCode::Esc => {
                                if self.show_help {
                                    self.show_help = false;
                                } else {
                                    self.navigate_back();
                                }
                            }
                            KeyCode::Char('q') => self.running = false,
                            KeyCode::Char(':') => {
                                self.command_mode = true;
                                self.command_buffer.clear();
                            }
                            KeyCode::Enter => {
                                if self.command_mode {
                                    let cmd = self.command_buffer.trim().to_string();
                                    if !cmd.is_empty() {
                                        self.execute_command(&cmd);
                                    }
                                    self.command_mode = false;
                                    self.command_buffer.clear();
                                } else {
                                    let registry = &mut self.page_registry;
                                    let session = &mut self.local_session;
                                    let status = &mut self.status_message;
                                    if let Some(page) = registry.get_mut(&self.active_page) {
                                        page.on_enter(session, status);
                                    }
                                }
                            }
                            KeyCode::Char(c) if self.command_mode => {
                                self.command_buffer.push(c);
                            }
                            KeyCode::Backspace if self.command_mode => {
                                self.command_buffer.pop();
                            }
                            KeyCode::Char(c) => {
                                self.execute_command(&c.to_string());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// 启动远程 TUI 的外部入口点。
pub fn run_remote_tui(alias: &str) -> Result<(), String> {
    crossterm::terminal::enable_raw_mode()
        .map_err(|e| format!("raw mode 启用失败: {}", e))?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)
        .map_err(|e| format!("alternate screen 进入失败: {}", e))?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal =
        ratatui::Terminal::new(backend).map_err(|e| format!("终端创建失败: {}", e))?;

    let result = RemoteTuiApp::connect(alias)?.run(&mut terminal);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)
        .map_err(|e| format!("alternate screen 离开失败: {}", e))?;
    crossterm::terminal::disable_raw_mode()
        .map_err(|e| format!("raw mode 禁用失败: {}", e))?;
    result
}
