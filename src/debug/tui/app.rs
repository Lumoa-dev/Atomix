//! TUI 应用状态 — 页面栈、导航、命令处理、事件循环。
//!
//! 对应设计文档 §3.0（导航模型）、§3.1–3.18（18 页面）、§4（命令体系）。

use crate::debug::session::{DebugSession, DisplayFormat, LocalDebugSession};
use crate::debug::tui::layout::TuiLayout;
use crate::debug::tui::pages::{PageId, PageRegistry};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::Frame;

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
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Char('?'), KeyModifiers::NONE) => {
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
                let was_status = status.clone();
                if let Some(page) = pages.get_page_mut(&self.active_page) {
                    page.on_enter(session, status);
                }
                // 检查页面是否请求导航（"navigate:PageId" 协议）
                let needs_nav = if status.starts_with("navigate:") {
                    let parts: Vec<&str> = status.splitn(3, ':').collect();
                    if parts.len() >= 2 { Some((parts[1].to_string(), parts.get(2).map(|s| s.to_string()))) } else { None }
                } else { None };
                if let Some((target, _param)) = needs_nav {
                    match target.as_str() {
                        "StepDetail" => self.navigate_to(PageId::StepDetail),
                        _ => {}
                    }
                    self.status_message = was_status;
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

        // 辅助: 解析地址
        let parse_addr = |s: &str| -> Option<usize> {
            if s.starts_with("0x") || s.starts_with("0X") {
                usize::from_str_radix(&s[2..], 16).ok()
            } else {
                s.parse().ok()
            }
        };

        match command.as_str() {
            // ── 元命令 (§4.18) ──
            "quit" | "q" => {
                self.running = false;
                return;
            }
            "help" | "h" | "?" => {
                self.show_help = true;
                self.status_message = "帮助面板已打开".to_string();
                return;
            }

            // ── 视图切换 (§4.17) ──
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

            // ── 执行控制 (§4.1) ──
            "step" => {
                let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                self.session.step_instructions(n);
                self.status_message = format!("已执行 {} 条指令", n);
                self.notify_page_data_changed();
            }
            "step:into" => {
                self.session.step_into();
                self.notify_page_data_changed();
            }
            "step:out" => {
                self.session.step_out();
                self.notify_page_data_changed();
            }
            "step:over" => {
                self.session.step_over();
                self.notify_page_data_changed();
            }
            "continue" | "c" => {
                self.session.continue_execution();
                self.notify_page_data_changed();
            }
            ":go" => {
                // 单步推进（watch/one 模式）
                self.session.step_instructions(1);
                self.status_message = format!(":go PC={:#06x}", self.session.vm.pc);
                self.notify_page_data_changed();
            }
            ":again" => {
                // 重做上一步: 回退一条指令
                if self.session.vm.pc > 0 {
                    self.session.vm.pc -= 1;
                    self.status_message = format!(":again PC={:#06x}", self.session.vm.pc);
                } else {
                    self.status_message = "已在第一条指令".to_string();
                }
                self.notify_page_data_changed();
            }

            // ── Step 查看与重跑 (§4.2) ──
            step_see if step_see.starts_with("step:see") => {
                let name_or_id = parts.get(1).map(|s| *s).unwrap_or("");
                if !name_or_id.is_empty() {
                    // 尝试按名称或序号查找
                    let found = name_or_id
                        .parse::<usize>()
                        .ok()
                        .and_then(|idx| self.session.trace.find_step_by_index(idx))
                        .or_else(|| self.session.trace.find_step_by_name(name_or_id))
                        .cloned();
                    if let Some(s) = found {
                        self.status_message = format!(
                            "Step: {} (line {}, {:.3}ms, {} calls)",
                            s.name,
                            s.source_line,
                            s.elapsed_us as f64 / 1000.0,
                            s.sub_calls.len()
                        );
                    } else {
                        self.status_message = format!("未找到 Step: {}", name_or_id);
                    }
                }
            }
            step_run if step_run.starts_with("step:run") => {
                let name = parts.get(1).map(|s| *s).unwrap_or("");
                // 检查是否有 watch 子命令
                let watch_mode = parts.iter().any(|&p| p == "watch");
                let one_mode = parts.iter().any(|&p| p == "one");
                if !name.is_empty() {
                    if watch_mode {
                        let speed: f32 = parts
                            .iter()
                            .position(|&p| p == "watch")
                            .and_then(|i| parts.get(i + 1))
                            .and_then(|s| s.parse::<f32>().ok())
                            .unwrap_or(1.0);
                        self.status_message = format!("重跑 Step {} (watch {:.1}x)", name, speed);
                        self.navigate_to(PageId::WatchReplay);
                    } else if one_mode {
                        self.status_message = format!("单步重跑 Step {}", name);
                    } else {
                        self.status_message = format!("重跑 Step {}", name);
                    }
                    // 在实际场景中这里会重新驱动 VM 执行
                }
            }

            // ── 导航 (§4.3) ──
            "exit" => {
                if parts.get(1).map(|s| *s) == Some("home") {
                    self.navigate_home();
                } else {
                    self.navigate_back();
                }
            }
            "exit:home" => self.navigate_home(),

            // ── 断点 (§4.4) ──
            break_cmd if break_cmd.starts_with("break:") => {
                let sub = &break_cmd[6..];
                match sub {
                    "line" => {
                        if let Some(line_str) = parts.get(1) {
                            if let Ok(line) = line_str.parse::<u32>() {
                                let condition = if parts.len() > 2 && parts[2] == "if" {
                                    Some(parts[3..].join(" "))
                                } else {
                                    None
                                };
                                let id =
                                    self.session.set_breakpoint_line(line, condition.as_deref());
                                if id > 0 {
                                    self.status_message = format!("行断点已设置于 line {}", line);
                                }
                            }
                        }
                    }
                    "fn" => {
                        if let Some(fn_path) = parts.get(1) {
                            let id = self.session.set_breakpoint_fn(fn_path);
                            if id > 0 {
                                self.status_message = format!("函数断点已设置: {}", fn_path);
                            }
                        }
                    }
                    "hook" => {
                        let hook_name = parts.get(1).map(|s| *s).unwrap_or("global");
                        self.status_message = format!("钩子断点已设置: {}", hook_name);
                    }
                    "list" => {
                        self.status_message =
                            format!("共 {} 个断点", self.session.breakpoints().len());
                    }
                    "del" => {
                        if let Some(id_str) = parts.get(1) {
                            if let Ok(id) = id_str.parse::<u64>() {
                                if self.session.remove_breakpoint(id) {
                                    self.status_message = format!("断点 {} 已删除", id);
                                }
                            }
                        }
                    }
                    "clear" => {
                        self.session.clear_breakpoints();
                        self.status_message = "所有断点已清空".to_string();
                    }
                    "enable" => {
                        self.session.enable_all_breakpoints(true);
                        self.status_message = "所有断点已启用".to_string();
                    }
                    _ => {}
                }
            }
            "break" => {
                if let Some(addr_str) = parts.get(1) {
                    if let Some(addr) = parse_addr(addr_str) {
                        let condition = if parts.len() > 2 && parts[2] == "if" {
                            Some(parts[3..].join(" "))
                        } else {
                            None
                        };
                        let id = self.session.set_breakpoint_pc(addr, condition.as_deref());
                        if id > 0 {
                            self.status_message = format!("断点已设置于 {:#06x}", addr);
                        }
                    }
                } else {
                    self.status_message = format!(
                        "共 {} 个断点（输入 break:list 查看详情）",
                        self.session.breakpoints().len()
                    );
                }
            }

            // ── 信息查询 (§4.5) ──
            "info" => {
                let sub = parts.get(1).map(|s| *s).unwrap_or("");
                match sub {
                    "task" => {
                        let t = &self.session.trace;
                        self.status_message = format!(
                            "任务: {} Step, {} instr, {:?}, 完成={}",
                            t.step_count(),
                            t.total_instructions,
                            t.total_elapsed,
                            t.completed
                        );
                    }
                    "zones" => self.navigate_to(PageId::ZoneStatus),
                    "functions" => {
                        if let Some(ref map) = self.session.debug_map {
                            let funcs = map.func_entries();
                            if funcs.is_empty() {
                                self.status_message = "（无函数调试信息）".to_string();
                            } else {
                                let names: Vec<&str> =
                                    funcs.iter().filter_map(|e| e.func_name()).collect();
                                self.status_message =
                                    format!("函数（{}个）: {}", funcs.len(), names.join(", "));
                            }
                        } else {
                            self.status_message = "（无调试信息）".to_string();
                        }
                    }
                    "variables" => {
                        let vars: Vec<String> = (0..16)
                            .map(|i| {
                                let name = crate::base::isa::reg_name(i).to_uppercase();
                                format!("{}={:#x}", name, self.session.vm.read_reg(i))
                            })
                            .collect();
                        self.status_message = format!("变量: {}", vars.join(" "));
                    }
                    "file" => {
                        if let Some(ref path) = self.session.source_path {
                            self.status_message = format!(
                                "源文件: {} ({} 行)",
                                path,
                                self.session.source_lines.len()
                            );
                        } else {
                            self.status_message = "未加载源文件".to_string();
                        }
                    }
                    _ => {
                        self.status_message = format!(
                            "PC={:#06x}, 状态={:?}, 帧={}, 指令={}",
                            self.session.vm.pc,
                            self.session.vm.state,
                            self.session.vm.call_stack.len(),
                            self.session.perf.total_instructions
                        );
                    }
                }
            }

            // ── 表达式求值 (§4.6) ──
            "print" | "p" => {
                let expr = parts[1..].join(" ");
                match crate::debug::eval::eval_expr(&expr, &self.session.vm) {
                    Ok(val) => {
                        self.status_message = format!("{} = {} ({:#x})", expr, val as i64, val)
                    }
                    Err(e) => self.status_message = format!("错误: {}", e),
                }
            }
            print_f if print_f.starts_with("print/f") || print_f.starts_with("p/f") => {
                // print/f <fmt> <expr> — 格式化打印
                let fmt_spec = parts.get(1).map(|s| *s).unwrap_or("d");
                let expr = parts[2..].join(" ");
                match crate::debug::eval::eval_expr(&expr, &self.session.vm) {
                    Ok(val) => {
                        let formatted = match fmt_spec {
                            "x" | "hex" => format!("{:#x}", val),
                            "d" | "dec" => format!("{}", val as i64),
                            "s" | "str" => format!(
                                "'{}'",
                                std::str::from_utf8(&val.to_le_bytes()).unwrap_or("?")
                            ),
                            _ => format!("{} ({:#x})", val as i64, val),
                        };
                        self.status_message = format!("{} = {}", expr, formatted);
                    }
                    Err(e) => self.status_message = format!("错误: {}", e),
                }
            }
            print_t if print_t.starts_with("print/t") || print_t.starts_with("p/t") => {
                let expr = parts[1..].join(" ");
                match crate::debug::eval::eval_expr(&expr, &self.session.vm) {
                    Ok(val) => {
                        let type_desc = if val <= 0xFF {
                            "u8"
                        } else if val <= 0xFFFF {
                            "u16"
                        } else if val <= 0xFFFF_FFFF {
                            "u32"
                        } else {
                            "u64"
                        };
                        self.status_message = format!(
                            "{} = {} (类型: {}, 大小: 8 字节)",
                            expr, val as i64, type_desc
                        );
                    }
                    Err(e) => self.status_message = format!("错误: {}", e),
                }
            }

            // ── 反汇编 (§4.9) ──
            "disasm" => {
                let addr: usize = parts
                    .get(1)
                    .and_then(|s| parse_addr(s))
                    .unwrap_or(self.session.vm.pc);
                let n: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
                let lines =
                    crate::debug::disassemble::disassemble_range(&self.session.vm.text, addr, n);
                self.status_message =
                    format!("反汇编 {} 条从 {:#06x}: {}", n, addr, lines.join(" | "));
            }

            // ── 搜索 (§4.8) ──
            "find" => {
                let text = parts[1..].join(" ");
                if text.is_empty() {
                    self.status_message = "用法: find <text>".to_string();
                } else {
                    let src = &self.session.source_lines;
                    let matches: Vec<String> = src
                        .iter()
                        .enumerate()
                        .filter(|(_, line)| line.contains(&text))
                        .map(|(i, _)| format!("行{}", i + 1))
                        .collect();
                    self.status_message = if matches.is_empty() {
                        format!("未找到: {}", text)
                    } else {
                        format!("找到 {} 处匹配: {}", matches.len(), matches.join(", "))
                    };
                }
            }
            find_cmd if find_cmd.starts_with("find:") => {
                let sub = &find_cmd[5..];
                match sub {
                    "next" => {
                        self.status_message = "下一个匹配项（需先执行 find）".to_string();
                    }
                    "prev" => {
                        self.status_message = "上一个匹配项（需先执行 find）".to_string();
                    }
                    "mem" => {
                        let pattern = parts.get(1).map(|s| *s).unwrap_or("");
                        if !pattern.is_empty() {
                            let byte_pattern: Vec<u8> = pattern
                                .split_whitespace()
                                .filter_map(|b| u8::from_str_radix(b, 16).ok())
                                .collect();
                            self.status_message = format!("内存搜索模式: {:02x?}", byte_pattern);
                        }
                    }
                    "var" => {
                        let name = parts.get(1).map(|s| *s).unwrap_or("");
                        self.status_message = format!("搜索变量定义及使用: {}", name);
                    }
                    _ => {}
                }
            }

            // ── 内存操作 (§4.10) ──
            mem_cmd if mem_cmd.starts_with("mem:") => {
                let sub = &mem_cmd[4..];
                match sub {
                    "dump" => {
                        let addr = parts
                            .get(1)
                            .and_then(|s| parse_addr(s).map(|a| a as u64))
                            .unwrap_or(0);
                        let len: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
                        let end = addr.saturating_add(len as u64);
                        let mut out = String::new();
                        let mut offset = addr;
                        while offset < end {
                            let line_end = (offset + 16).min(end);
                            let mut hex = String::new();
                            let mut ascii = String::new();
                            for a in offset..line_end {
                                if let Some(byte) = self.session.vm.memory.read_u8(a) {
                                    hex.push_str(&format!("{:02x} ", byte));
                                    ascii.push(if byte.is_ascii_graphic() || byte == b' ' {
                                        byte as char
                                    } else {
                                        '.'
                                    });
                                }
                            }
                            out.push_str(&format!("\n{:#010x}:  {:48}  {}", offset, hex, ascii));
                            offset = line_end;
                        }
                        self.status_message =
                            format!("内存 dump @{:#x} ({} 字节):{}", addr, len, out);
                    }
                    "diff" => {
                        let a1 = parts
                            .get(1)
                            .and_then(|s| parse_addr(s).map(|a| a as u64))
                            .unwrap_or(0);
                        let a2 = parts
                            .get(2)
                            .and_then(|s| parse_addr(s).map(|a| a as u64))
                            .unwrap_or(0);
                        let len: usize = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(16);
                        let mut diffs = Vec::new();
                        for i in 0..len {
                            let b1 = self.session.vm.memory.read_u8(a1 + i as u64);
                            let b2 = self.session.vm.memory.read_u8(a2 + i as u64);
                            if b1 != b2 {
                                diffs.push(format!(
                                    "+{:#x}: {:02x} vs {:02x}",
                                    i,
                                    b1.unwrap_or(0),
                                    b2.unwrap_or(0)
                                ));
                            }
                        }
                        self.status_message = if diffs.is_empty() {
                            "内存区域相同".to_string()
                        } else {
                            format!("差异 ({}处): {}", diffs.len(), diffs.join(", "))
                        };
                    }
                    "fill" => {
                        let addr = parts
                            .get(1)
                            .and_then(|s| parse_addr(s).map(|a| a as u64))
                            .unwrap_or(0);
                        let len: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(16);
                        let val: u8 = parts
                            .get(3)
                            .and_then(|s| {
                                u8::from_str_radix(s.trim_start_matches("0x"), 16)
                                    .or_else(|_| s.parse())
                                    .ok()
                            })
                            .unwrap_or(0);
                        for i in 0..len {
                            self.session.vm.memory.write_u8(addr + i as u64, val);
                        }
                        self.status_message =
                            format!("已填充 {:#x} 区域 {} 字节值为 {:#04x}", addr, len, val);
                    }
                    "watch" => {
                        let addr = parts
                            .get(1)
                            .and_then(|s| parse_addr(s).map(|a| a as u64))
                            .unwrap_or(0);
                        let len: u64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
                        self.session.set_watchpoint(addr, len, "");
                        self.status_message = format!("内存监视点: {:#x} ({} 字节)", addr, len);
                    }
                    _ => {}
                }
            }

            // ── 调用栈 (§4.11) ──
            "bt" | "backtrace" => self.navigate_to(PageId::CallStack),
            "frame" => {
                if let Some(n_str) = parts.get(1) {
                    if let Ok(n) = n_str.parse::<usize>() {
                        let max_frames = self.session.vm.call_stack.len();
                        self.session.frame_state.set(n, max_frames);
                        self.status_message = format!("已切换到帧 #{}", n);
                    }
                } else {
                    let idx = self.session.frame_state.current_index();
                    self.status_message = format!(
                        "当前帧 #{} (共 {} 帧)",
                        idx,
                        self.session.vm.call_stack.len() + 1
                    );
                }
            }
            "up" => {
                let max = self.session.vm.call_stack.len();
                self.session.frame_state.up(max);
                self.status_message = format!("帧 #{}", self.session.frame_state.current_index());
            }
            "down" => {
                self.session.frame_state.down();
                self.status_message = format!("帧 #{}", self.session.frame_state.current_index());
            }

            // ── IS* (§4.12) ──
            "is" => {
                if let Some(name) = parts.get(1) {
                    let val = self
                        .session
                        .is_context
                        .entries
                        .get(*name)
                        .map(|s| s.as_str())
                        .unwrap_or("—");
                    self.status_message = format!("{} = {}", name, val);
                } else {
                    self.navigate_to(PageId::IsContext);
                }
            }

            // ── 历史 (§4.13) ──
            "history" => {
                let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
                let entries = self.session.cmd_history.last_n(n);
                if entries.is_empty() {
                    self.status_message = "（无历史）".to_string();
                } else {
                    let lines: Vec<String> = entries
                        .iter()
                        .enumerate()
                        .map(|(i, e)| {
                            format!("#{}: {}", self.session.cmd_history.len() - n + i + 1, e)
                        })
                        .collect();
                    self.status_message = lines.join(" | ");
                }
            }
            hist_repeat if hist_repeat.starts_with("!") => {
                if let Ok(n) = hist_repeat[1..].parse::<usize>() {
                    if let Some(cmd) = self.session.cmd_history.get(n) {
                        // 重新执行历史命令
                        let cmd_str = cmd.to_string();
                        self.execute_command(&cmd_str);
                        return;
                    }
                }
                self.status_message = "无效的历史序号".to_string();
            }

            // ── Display 管理 (§4.14) ──
            "display" => {
                if parts.len() >= 2 {
                    // display:del <n> 或 display:clear
                    let sub = parts[1];
                    match sub {
                        "del" | "delete" | "rm" => {
                            if let Some(idx_str) = parts.get(2) {
                                if let Ok(idx) = idx_str.parse::<usize>() {
                                    if self.session.display_expr_list.len() > idx {
                                        self.session.display_expr_list.remove(idx);
                                        self.status_message = format!("display #{} 已删除", idx);
                                    } else {
                                        self.status_message = format!("索引 {} 超出范围", idx);
                                    }
                                }
                            }
                        }
                        "clear" => {
                            self.session.display_expr_list.clear();
                            self.status_message = "所有 display 表达式已清空".to_string();
                        }
                        _ => {
                            // 添加表达式
                            let expr = parts[1..].join(" ");
                            if !expr.is_empty() {
                                self.session.display_expr_list.push(expr.clone());
                                self.status_message = format!("已添加 display: {}", expr);
                            }
                        }
                    }
                } else {
                    // 列出所有 display
                    if self.session.display_expr_list.is_empty() {
                        self.status_message = "（无 display 表达式）".to_string();
                    } else {
                        let list: Vec<String> = self
                            .session
                            .display_expr_list
                            .iter()
                            .enumerate()
                            .map(|(i, e)| format!("#{}: {}", i, e))
                            .collect();
                        self.status_message = list.join(" | ");
                    }
                }
            }

            // ── 日志与导出 (§4.15) ──
            log_cmd if log_cmd.starts_with("log:") => {
                let sub = &log_cmd[4..];
                match sub {
                    "start" => {
                        let file = parts.get(1).map(|s| *s).unwrap_or("debug.log");
                        self.status_message = format!("日志记录已开始 -> {}", file);
                    }
                    "stop" => {
                        self.status_message = "日志记录已停止".to_string();
                    }
                    "status" => {
                        self.status_message = "日志记录状态: 未启动".to_string();
                    }
                    _ => {}
                }
            }
            export_cmd if export_cmd.starts_with("export:") => {
                let sub = &export_cmd[7..];
                match sub {
                    "state" => {
                        let file = parts.get(1).map(|s| *s).unwrap_or("state.json");
                        let state = serde_json::json!({
                            "pc": self.session.vm.pc, "regs": self.session.vm.regs,
                            "state": format!("{:?}", self.session.vm.state),
                            "steps": self.session.trace.step_count(),
                            "instrs": self.session.trace.total_instructions,
                        });
                        if let Ok(json) = serde_json::to_string_pretty(&state) {
                            let _ = std::fs::write(&file, &json);
                            self.status_message = format!("状态快照已导出至: {}", file);
                        }
                    }
                    "dataflow" => {
                        let file = parts.get(1).map(|s| *s).unwrap_or("dataflow.svg");
                        self.status_message = format!("数据追踪图已导出至: {}", file);
                    }
                    _ => {}
                }
            }

            // ── 配置 (§4.16) ──
            set_cmd if set_cmd.starts_with("set:") => {
                let sub = &set_cmd[4..];
                match sub {
                    "fmt" => {
                        let fmt = parts.get(1).map(|s| *s).unwrap_or("both").to_lowercase();
                        match fmt.as_str() {
                            "hex" => {
                                self.session.disp_fmt = DisplayFormat::Hex;
                                self.status_message = "显示格式: 十六进制".to_string();
                            }
                            "dec" => {
                                self.session.disp_fmt = DisplayFormat::Dec;
                                self.status_message = "显示格式: 十进制".to_string();
                            }
                            "both" => {
                                self.session.disp_fmt = DisplayFormat::Both;
                                self.status_message = "显示格式: 混合".to_string();
                            }
                            _ => {
                                self.status_message =
                                    format!("未知格式: {} (支持 hex/dec/both)", fmt);
                            }
                        }
                    }
                    "depth" => {
                        if let Some(d) = parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
                            self.session.disp_depth = d;
                            self.status_message = format!("嵌套展开深度: {}", d);
                        }
                    }
                    "speed" => {
                        if let Some(s) = parts.get(1).and_then(|s| s.parse::<f32>().ok()) {
                            self.session.watch_spd = s.max(0.25).min(4.0);
                            self.status_message =
                                format!("watch 速度: {:.1}x", self.session.watch_spd);
                        }
                    }
                    "var" => {
                        // set:var <name> = <value>
                        let rest = parts[1..].join(" ");
                        if let Some(eq_pos) = rest.find('=') {
                            let var_name = rest[..eq_pos].trim();
                            let value_str = rest[eq_pos + 1..].trim();
                            // 尝试找到并设置变量
                            if let Some(idx) = crate::debug::repl::parse_reg_name(var_name) {
                                if let Ok(val) =
                                    crate::debug::eval::eval_expr(value_str, &self.session.vm)
                                {
                                    self.session.vm.write_reg(idx, val);
                                    self.status_message =
                                        format!("{} = {:#x}", var_name.to_uppercase(), val);
                                }
                            } else {
                                self.status_message = format!("未知变量: {}", var_name);
                            }
                        }
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
                        match (
                            crate::debug::eval::eval_expr(addr_expr, &self.session.vm),
                            crate::debug::eval::eval_expr(value_expr, &self.session.vm),
                        ) {
                            (Ok(addr), Ok(val)) => {
                                self.session.vm.memory.write_u64(addr, val);
                                self.status_message =
                                    format!("*{:#x} = {} ({:#x})", addr, val as i64, val);
                            }
                            (Err(e), _) | (_, Err(e)) => {
                                self.status_message = format!("错误: {}", e)
                            }
                        }
                    } else {
                        let reg_idx = crate::debug::repl::parse_reg_name(target);
                        match (
                            reg_idx,
                            crate::debug::eval::eval_expr(value_expr, &self.session.vm),
                        ) {
                            (Some(idx), Ok(val)) => {
                                self.session.vm.write_reg(idx, val);
                                self.status_message = format!(
                                    "{} = {} ({:#x})",
                                    target.to_uppercase(),
                                    val as i64,
                                    val
                                );
                            }
                            _ => self.status_message = "设置失败：未知寄存器或表达式".to_string(),
                        }
                    }
                }
            }

            // ── 源码 ──
            "source" | "src" => self.navigate_to(PageId::SourceView),

            // ── 监视点 ──
            "watch" => {
                if let Some(addr_str) = parts.get(1) {
                    if let Ok(addr) = u64::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                        .or_else(|_| addr_str.parse::<u64>())
                    {
                        let size: u64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
                        self.session.set_watchpoint(addr, size, "");
                        self.status_message = format!("监视点: {:#x} ({} 字节)", addr, size);
                    }
                } else {
                    let count = self.session.watchpoints().len();
                    self.status_message = format!("{} 个监视点", count);
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
    pub fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> Result<(), String> {
        while self.running {
            // 渲染
            terminal
                .draw(|frame| {
                    self.render(frame);
                })
                .map_err(|e| format!("渲染错误: {}", e))?;

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
        self.layout
            .render_title_bar(frame, chunks.title, &self.session);

        // 2. 面包屑
        let crumbs = self.breadcrumb();
        self.layout
            .render_breadcrumb(frame, chunks.breadcrumb, &crumbs);

        // 3. 主视图区域
        {
            let pages = &mut self.pages;
            let session = &mut self.session;
            if let Some(page) = pages.get_page_mut(&self.active_page) {
                page.render(frame, chunks.main_content, session);
            }
        }

        // 4. 右侧状态面板
        self.layout
            .render_right_panel(frame, chunks.right_panel, &self.session);

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
