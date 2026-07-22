//! TUI 应用状态 — 页面栈、导航、命令处理、事件循环。
//!
//! 对应设计文档 §3.0（导航模型）、§3.1–3.18（18 页面）、§4（命令体系）。

use crate::debug::session::{LocalDebugSession, DebugSession, DisplayFormat};
use crate::debug::trace::ExecutionPhase;
use crate::debug::tui::layout::TuiLayout;
use crate::debug::tui::pages::{Page, PageId, PageRegistry};

use ratatui::Frame;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use std::time::Duration;

/// TUI 应用主状态。
pub struct TuiApp {
    /// 调试会话。
    pub session: LocalDebugSession,
    /// 页面注册表。
    pages: PageRegistry,
    /// 页面导航栈（当前页面在栈顶）。
    page_stack: Vec<PageId>,
    /// 当前活动的页面 ID。
    active_page: PageId,
    /// 布局渲染器。
    layout: TuiLayout,
    /// 命令输入缓冲区。
    pub command_buffer: String,
    /// 是否正在输入命令。
    pub command_mode: bool,
    /// 是否显示帮助面板。
    pub show_help: bool,
    /// 消息栏（显示命令结果）。
    pub status_message: String,
    /// 是否运行中。
    running: bool,
    /// 鼠标/键盘事件。
    pub scroll_offset: usize,
    pub selected_index: usize,
}

impl TuiApp {
    /// 创建新的 TUI 应用。
    pub fn new(session: LocalDebugSession) -> Self {
        let mut pages = PageRegistry::new();
        pages.register_all(&session);

        Self {
            session,
            pages,
            page_stack: vec![PageId::Home],
            active_page: PageId::Home,
            layout: TuiLayout::new(),
            command_buffer: String::new(),
            command_mode: false,
            show_help: false,
            status_message: String::new(),
            running: true,
            scroll_offset: 0,
            selected_index: 0,
        }
    }

    /// 获取当前页面的可变引用。
    fn current_page_mut(&mut self) -> Option<&mut Box<dyn Page>> {
        self.pages.get_page_mut(&self.active_page)
    }

    /// 导航到指定页面。
    pub fn navigate_to(&mut self, page_id: PageId) {
        self.page_stack.push(page_id.clone());
        self.active_page = page_id;
        self.scroll_offset = 0;
        self.selected_index = 0;
    }

    /// 返回上一页。
    pub fn navigate_back(&mut self) {
        if self.page_stack.len() > 1 {
            self.page_stack.pop();
            if let Some(prev) = self.page_stack.last() {
                self.active_page = prev.clone();
            }
        }
        self.scroll_offset = 0;
        self.selected_index = 0;
    }

    /// 返回首页。
    pub fn navigate_home(&mut self) {
        self.page_stack.clear();
        self.page_stack.push(PageId::Home);
        self.active_page = PageId::Home;
        self.scroll_offset = 0;
        self.selected_index = 0;
    }

    /// 获取面包屑路径。
    pub fn breadcrumb(&self) -> Vec<String> {
        let mut crumbs = Vec::new();
        for page_id in &self.page_stack {
            if let Some(page) = self.pages.get_page(page_id) {
                crumbs.push(page.title().to_string());
            }
        }
        crumbs
    }

    /// 处理键盘事件。
    fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        if self.command_mode {
            self.handle_command_input(key);
            return;
        }

        match (key, modifiers) {
            (KeyCode::Esc, _) => {
                if self.show_help {
                    self.show_help = false;
                } else {
                    self.navigate_back();
                }
            }
            (KeyCode::Char('h'), KeyModifiers::NONE)
            | (KeyCode::Char('?'), KeyModifiers::NONE) => {
                self.show_help = !self.show_help;
            }
            (KeyCode::Char('q'), KeyModifiers::NONE) => {
                self.running = false;
            }
            (KeyCode::Up, _) => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
            }
            (KeyCode::Down, _) => {
                self.selected_index += 1;
            }
            (KeyCode::Enter, _) => {
                let pages = &mut self.pages;
                let session = &mut self.session;
                let status = &mut self.status_message;
                if let Some(page) = pages.get_page_mut(&self.active_page) {
                    page.on_enter(session, status);
                }
            }
            (KeyCode::Char(':'), _) => {
                self.command_mode = true;
                self.command_buffer.clear();
            }
            (KeyCode::Tab, _) => {}
            (KeyCode::Char('+'), _) | (KeyCode::Char('='), _) => {
                let pages = &mut self.pages;
                let session = &mut self.session;
                if let Some(page) = pages.get_page_mut(&self.active_page) {
                    page.on_zoom_in(session);
                }
            }
            (KeyCode::Char('-'), _) => {
                let pages = &mut self.pages;
                let session = &mut self.session;
                if let Some(page) = pages.get_page_mut(&self.active_page) {
                    page.on_zoom_out(session);
                }
            }
            (KeyCode::Char('b'), _) => {
                let pages = &mut self.pages;
                let session = &mut self.session;
                let status = &mut self.status_message;
                if let Some(page) = pages.get_page_mut(&self.active_page) {
                    page.on_key_shortcut(session, 'b', status);
                }
            }
            (KeyCode::Char('f'), _) => {
                let pages = &mut self.pages;
                let session = &mut self.session;
                let status = &mut self.status_message;
                if let Some(page) = pages.get_page_mut(&self.active_page) {
                    page.on_key_shortcut(session, 'f', status);
                }
            }
            (KeyCode::Char('t'), _) => {
                let pages = &mut self.pages;
                let session = &mut self.session;
                let status = &mut self.status_message;
                if let Some(page) = pages.get_page_mut(&self.active_page) {
                    page.on_key_shortcut(session, 't', status);
                }
            }
            _ => {}
        }
    }

    /// 处理命令输入。
    fn handle_command_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Enter => {
                let cmd = self.command_buffer.trim().to_string();
                if !cmd.is_empty() {
                    let cmd_clone = cmd.clone();
                    self.execute_command(&cmd_clone);
                }
                self.command_mode = false;
                self.command_buffer.clear();
            }
            KeyCode::Esc => {
                self.command_mode = false;
                self.command_buffer.clear();
            }
            KeyCode::Backspace => {
                self.command_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.command_buffer.push(c);
            }
            _ => {}
        }
    }

    /// 执行命令（对应设计文档 §4 命令体系）。
    pub fn execute_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }
        let command = parts[0].to_lowercase();

        // 记录历史
        self.session.cmd_history.push(cmd.to_string());

        match command.as_str() {
            "quit" | "q" => { self.running = false; return; }
            "help" | "h" | "?" => {
                self.show_help = true;
                self.status_message = "帮助面板已打开".to_string();
                return;
            }

            // ── 视图切换 ──
            ":src" => self.navigate_to(PageId::SourceView),
            ":df" | ":dataflow" => self.navigate_to(PageId::DataTimeline),
            ":hooks" | ":lifecycle" => self.navigate_to(PageId::HookTimeline),
            ":deps" | ":tasks" => self.navigate_to(PageId::TaskDependency),
            ":binary" => self.navigate_to(PageId::BinaryView),
            ":disasm" | ":ir" => self.navigate_to(PageId::DisasmView),
            ":regs" => self.navigate_to(PageId::RegsMemory),
            ":mem" => self.navigate_to(PageId::RegsMemory),
            ":zones" => self.navigate_to(PageId::ZoneStatus),
            ":bt" | ":callstack" => self.navigate_to(PageId::CallStack),
            ":breaks" => self.navigate_to(PageId::Breakpoints),
            ":is" => self.navigate_to(PageId::IsContext),
            ":segments" => self.navigate_to(PageId::SegmentInfo),
            ":perf" | ":profile" => self.navigate_to(PageId::PerfAnalysis),

            // ── 执行控制 ──
            "step" => {
                let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                self.session.step_instructions(n);
                self.status_message = format!("已执行 {} 条指令", n);
                self.notify_page_data_changed();
            }
            "step:into" => { self.session.step_into(); self.notify_page_data_changed(); }
            "step:out" => { self.session.step_out(); self.notify_page_data_changed(); }
            "step:over" => { self.session.step_over(); self.notify_page_data_changed(); }
            "continue" | "c" => { self.session.continue_execution(); self.notify_page_data_changed(); }

            // ── 导航 ──
            "exit" => {
                if parts.get(1).map(|s| *s) == Some("home") {
                    self.navigate_home();
                } else {
                    self.navigate_back();
                }
            }

            // ── 断点 ──
            break_cmd if break_cmd.starts_with("break:") => {
                let sub = &break_cmd[6..];
                match sub {
                    "line" => {
                        if let Some(line_str) = parts.get(1) {
                            if let Ok(line) = line_str.parse::<u32>() {
                                let condition = if parts.len() > 2 && parts[2] == "if" {
                                    Some(parts[3..].join(" "))
                                } else { None };
                                let id = self.session.set_breakpoint_line(line, condition.as_deref());
                                if id > 0 { self.status_message = format!("行断点已设置于 line {}", line); }
                            }
                        }
                    }
                    "fn" => {
                        if let Some(fn_path) = parts.get(1) {
                            let id = self.session.set_breakpoint_fn(fn_path);
                            if id > 0 { self.status_message = format!("函数断点已设置: {}", fn_path); }
                        }
                    }
                    "list" => { self.status_message = format!("共 {} 个断点", self.session.breakpoints().len()); }
                    "del" => {
                        if let Some(id_str) = parts.get(1) {
                            if let Ok(id) = id_str.parse::<u64>() {
                                if self.session.remove_breakpoint(id) {
                                    self.status_message = format!("断点 {} 已删除", id);
                                }
                            }
                        }
                    }
                    "clear" => { self.session.clear_breakpoints(); self.status_message = "所有断点已清空".to_string(); }
                    "enable" => { self.session.enable_all_breakpoints(true); self.status_message = "所有断点已启用".to_string(); }
                    _ => {}
                }
            }
            "break" => {
                if let Some(addr_str) = parts.get(1) {
                    if let Ok(addr) = usize::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                        .or_else(|_| addr_str.parse::<usize>())
                    {
                        let condition = if parts.len() > 2 && parts[2] == "if" {
                            Some(parts[3..].join(" "))
                        } else { None };
                        let id = self.session.set_breakpoint_pc(addr, condition.as_deref());
                        if id > 0 { self.status_message = format!("断点已设置于 {:#06x}", addr); }
                    }
                } else {
                    self.status_message = format!("共 {} 个断点", self.session.breakpoints().len());
                }
            }

            // ── 打印/信息 ──
            "print" | "p" => {
                let expr = parts[1..].join(" ");
                match crate::debug::eval::eval_expr(&expr, &self.session.vm) {
                    Ok(val) => self.status_message = format!("{} = {}", expr, val),
                    Err(e) => self.status_message = format!("错误: {}", e),
                }
            }
            "info" => {
                match parts.get(1).map(|s| *s).unwrap_or("") {
                    "task" => {
                        let trace = &self.session.trace;
                        self.status_message = format!("任务: {} Step, {} instr, {:?}",
                            trace.step_count(), trace.total_instructions, trace.total_elapsed);
                    }
                    "zones" => self.navigate_to(PageId::ZoneStatus),
                    "file" => {
                        if let Some(ref path) = self.session.source_path {
                            self.status_message = format!("源文件: {}", path);
                        } else { self.status_message = "未加载源文件".to_string(); }
                    }
                    _ => {
                        self.status_message = format!("PC={:#06x}, 状态={:?}",
                            self.session.vm.pc, self.session.vm.state);
                    }
                }
            }
            "disasm" => {
                let addr: usize = parts.get(1).and_then(|s| {
                    s.trim_start_matches("0x").parse::<usize>().ok()
                }).unwrap_or(self.session.vm.pc);
                let n: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
                self.status_message = format!("反汇编 {} 条指令从 {:#06x}", n, addr);
                self.notify_page_data_changed();
            }

            // ── 设置 ──
            set_cmd if set_cmd.starts_with("set:") => {
                let sub = &set_cmd[4..];
                match sub {
                    "fmt" => {
                        let fmt = parts.get(1).map(|s| *s).unwrap_or("hex");
                        match fmt { "hex" => self.session.disp_fmt = DisplayFormat::Hex, "dec" => self.session.disp_fmt = DisplayFormat::Dec, _ => {} }
                        self.status_message = format!("显示格式已设为 {}", fmt);
                    }
                    "depth" => {
                        if let Some(d) = parts.get(1).and_then(|s| s.parse().ok()) { self.session.disp_depth = d; }
                    }
                    "speed" => {
                        if let Some(s) = parts.get(1).and_then(|s| s.parse::<f32>().ok()) { self.session.watch_spd = s.max(0.25).min(4.0); }
                    }
                    _ => {}
                }
            }
            "set" => {
                let rest = parts[1..].join(" ");
                if let Some(eq_pos) = rest.find('=') {
                    let target = rest[..eq_pos].trim();
                    let value_expr = rest[eq_pos + 1..].trim();
                    if target.starts_with('*') {
                        let addr_expr = target[1..].trim();
                        match (crate::debug::eval::eval_expr(addr_expr, &self.session.vm),
                               crate::debug::eval::eval_expr(value_expr, &self.session.vm)) {
                            (Ok(addr), Ok(val)) => { self.session.vm.memory.write_u64(addr, val); self.status_message = format!("*{:#x} = {}", addr, val); }
                            (Err(e), _) | (_, Err(e)) => self.status_message = format!("错误: {}", e),
                        }
                    } else {
                        let reg_idx = crate::debug::repl::parse_reg_name(target);
                        match (reg_idx, crate::debug::eval::eval_expr(value_expr, &self.session.vm)) {
                            (Some(idx), Ok(val)) => { self.session.vm.write_reg(idx, val); self.status_message = format!("{} = {}", target, val); }
                            _ => self.status_message = "设置失败".to_string(),
                        }
                    }
                }
            }

            // ── 其他命令 ──
            "is" => { self.navigate_to(PageId::IsContext); }
            "history" => {
                let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
                let entries = self.session.cmd_history.last_n(n);
                self.status_message = if entries.is_empty() {
                    "（无历史）".to_string()
                } else {
                    format!("最近 {} 条: {:?}", n, entries)
                };
            }
            "display" => {
                let expr = parts[1..].join(" ");
                if expr.is_empty() {
                    self.status_message = format!("{} 个 display 表达式", self.session.display_expr_list.len());
                } else {
                    self.session.display_expr_list.push(expr.clone());
                    self.status_message = format!("已添加 display: {}", expr);
                }
            }
            "bt" | "backtrace" => self.navigate_to(PageId::CallStack),
            "frame" => {
                if let Some(n_str) = parts.get(1) {
                    if let Ok(n) = n_str.parse::<usize>() {
                        self.session.frame_state.set(n, self.session.vm.call_stack.len());
                        self.status_message = format!("已切换到帧 #{}", n);
                    }
                }
            }
            "source" | "src" => self.navigate_to(PageId::SourceView),
            "watch" => {
                if let Some(addr_str) = parts.get(1) {
                    if let Ok(addr) = u64::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                        .or_else(|_| addr_str.parse::<u64>())
                    {
                        let size: u64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
                        self.session.set_watchpoint(addr, size, "");
                        self.status_message = format!("监视点: {:#x}", addr);
                    }
                }
            }
            _ => {
                self.status_message = format!("未知命令: {}（输入 help 查看帮助）", command);
            }
        }
    }

    fn notify_page_data_changed(&mut self) {
        let pages = &mut self.pages;
        let session = &mut self.session;
        if let Some(page) = pages.get_page_mut(&self.active_page) {
            page.on_data_changed(session);
        }
    }

    /// TUI 主事件循环。
    pub fn run(&mut self, terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) -> Result<(), String> {
        while self.running {
            // 渲染
            terminal.draw(|frame| {
                self.render(frame);
            }).map_err(|e| format!("渲染错误: {}", e))?;

            // 事件处理
            if event::poll(Duration::from_millis(50)).map_err(|e| format!("事件错误: {}", e))? {
                match event::read().map_err(|e| format!("读事件错误: {}", e))? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat {
                            self.handle_key(key.code, key.modifiers);
                        }
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// 渲染当前帧。
    fn render(&mut self, frame: &mut Frame) {
        let area = frame.size();
        let chunks = self.layout.calculate_layout(area);

        // 1. 标题栏
        self.layout.render_title_bar(frame, chunks.title, &self.session);

        // 2. 面包屑
        let crumbs = self.breadcrumb();
        self.layout.render_breadcrumb(frame, chunks.breadcrumb, &crumbs);

        // 3. 主视图区域
        {
            let pages = &mut self.pages;
            let session = &mut self.session;
            if let Some(page) = pages.get_page_mut(&self.active_page) {
                page.render(frame, chunks.main_content, session);
            }
        }

        // 4. 右侧状态面板
        self.layout.render_right_panel(frame, chunks.right_panel, &self.session);

        // 5. 命令栏
        self.layout.render_command_bar(
            frame,
            chunks.command_bar,
            self.command_mode,
            &self.command_buffer,
            &self.status_message,
        );

        // 6. 帮助面板
        if self.show_help {
            self.layout.render_help_overlay(frame, area);
        }
    }
}
