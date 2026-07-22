//! 信息页面 — Call Stack、Breakpoints、Zone Status、IS* Context、Segment Info、Perf Analysis
//!
//! 对应设计文档 §3.15（Call Stack）、§3.16（Breakpoints）、§3.14（Zone Status）、
//! §3.17（IS* Context）、§3.18（Segment Info）、§3.19（Performance Analysis）

use crate::debug::session::{DebugSession, LocalDebugSession};
use crate::debug::trace::{IsGroup, IS_VARIABLES};
use crate::debug::tui::pages::Page;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

// ═══════════════════════════════════════════════════════════
// Call Stack 页面（§3.15）
// ═══════════════════════════════════════════════════════════

pub struct CallStackPage {
    title: String,
    pub selected: usize,
    pub scroll: usize,
}

impl CallStackPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Call Stack — 调用栈".to_string(),
            selected: 0,
            scroll: 0,
        }
    }
}

impl Page for CallStackPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let cs = &session.vm.call_stack;
        let selected = session.frame_state.current_index();
        let max_visible = (area.height as usize).saturating_sub(2);

        let mut lines = vec![
            Line::from(Span::styled(
                format!(" 调用栈（共 {} 帧）", cs.len() + 1),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
        ];

        // 当前帧
        let marker = if selected == 0 { "→" } else { " " };
        let style = if selected == 0 {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let source_info = session.debug_map.as_ref()
            .and_then(|m| m.line_for_pc(session.vm.pc))
            .map(|l| format!(" line {}", l))
            .unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!("  {} #0  pc={:#06x} (current){}", marker, session.vm.pc, source_info),
            style,
        )));

        // 历史帧
        for (i, frame) in cs.iter().rev().enumerate() {
            let depth = i + 1;
            let marker = if selected == depth { "→" } else { " " };
            let style = if selected == depth {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("  {} #{}  return_pc={:#06x} sp={:#x}", marker, depth, frame.return_pc, frame.sp),
                style,
            )));
        }

        if cs.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无历史帧）",
                Style::default().fg(Color::Gray),
            )));
        }

        let block = Block::default()
            .title(self.title.clone())
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let widget = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// Breakpoints 页面（§3.16）
// ═══════════════════════════════════════════════════════════

pub struct BreakpointsPage {
    title: String,
    pub selected: usize,
}

impl BreakpointsPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Breakpoints — 断点管理".to_string(),
            selected: 0,
        }
    }
}

impl Page for BreakpointsPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let breakpoints = session.breakpoints();
        let max_visible = (area.height as usize).saturating_sub(2);

        let mut lines = Vec::new();

        if breakpoints.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无断点）",
                Style::default().fg(Color::Gray),
            )));
        } else {
            // 表头
            lines.push(Line::from(Span::styled(
                "  ID  Type     Location       Hits  Status  Condition",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "  ─────────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )));

            for (i, bp) in breakpoints.iter().enumerate() {
                if i >= max_visible { break; }
                let (bp_type_str, location) = match &bp.bp_type {
                    crate::debug::session::BreakpointType::Pc(addr) => ("PC", format!("{:#06x}", addr)),
                    crate::debug::session::BreakpointType::Line(line) => ("Line", format!("{}", line)),
                    crate::debug::session::BreakpointType::Function(f) => ("Fn", f.clone()),
                    crate::debug::session::BreakpointType::Hook(h) => ("Hook", h.clone()),
                };
                let status_str = if bp.enabled { "enabled" } else { "disabled" };
                let cond_str = bp.condition.as_deref().unwrap_or("—");
                let is_selected = i == self.selected;
                let style = if is_selected {
                    Style::default().bg(Color::Blue).fg(Color::White)
                } else if !bp.enabled {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default().fg(Color::White)
                };

                lines.push(Line::from(Span::styled(
                    format!("  {:>3}  {:<8}  {:<14}  {:<4}  {:<8}  {}",
                        bp.id, bp_type_str, location, bp.hit_count, status_str, cond_str),
                    style,
                )));
            }
        }

        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "  d 删除  e 启用/禁用  c 编辑条件  break:clear 清空全部",
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .title(format!(" {} ({}) ", self.title, breakpoints.len()))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let widget = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_key_shortcut(&mut self, session: &mut LocalDebugSession, key: char, status: &mut String) {
        let breakpoints = session.breakpoints().to_vec();
        if self.selected >= breakpoints.len() { return; }
        match key {
            'd' => {
                let id = breakpoints[self.selected].id;
                session.remove_breakpoint(id);
                *status = format!("断点 {} 已删除", id);
            }
            'e' => {
                let id = breakpoints[self.selected].id;
                session.toggle_breakpoint(id);
                *status = format!("断点 {} 状态已切换", id);
            }
            _ => {}
        }
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// Zone Status 页面（§3.14）
// ═══════════════════════════════════════════════════════════

pub struct ZoneStatusPage {
    title: String,
}

impl ZoneStatusPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Zone Status — Zone 状态".to_string(),
        }
    }
}

impl Page for ZoneStatusPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let zones = session.zone_info();
        let (total, used, free, peak) = session.memory_stats();

        let mut lines = vec![
            Line::from(Span::styled(" Zone 状态一览", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                format!("  {:<20} {:<16} {:<10} {}", "名称", "生命周期", "状态", "PC 范围"),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  ─────────────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        for (name, lifecycle, status, pc_range) in &zones {
            lines.push(Line::from(Span::raw(format!(
                "  {:<20} {:<16} {:<10} {}",
                name, lifecycle, status, pc_range
            ))));
        }

        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(" 内存统计", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(Span::raw(format!("  总内存:     {} 字节", total))));
        lines.push(Line::from(Span::raw(format!("  已用:       {} 字节 ({:.1}%)", used, if total > 0 { used as f64 / total as f64 * 100.0 } else { 0.0 }))));
        lines.push(Line::from(Span::raw(format!("  空闲:       {} 字节", free))));
        lines.push(Line::from(Span::raw(format!("  峰值:       {} 字节", peak))));

        let block = Block::default()
            .title(self.title.clone())
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let widget = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// IS* Context 页面（§3.17）
// ═══════════════════════════════════════════════════════════

pub struct IsContextPage {
    title: String,
    pub selected_group: usize,
    pub search_query: String,
    groups: [IsGroup; 7],
}

impl IsContextPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "IS* Context — IS* 全览".to_string(),
            selected_group: 0,
            search_query: String::new(),
            groups: [IsGroup::Exception, IsGroup::Count, IsGroup::CallContext,
                     IsGroup::System, IsGroup::Time, IsGroup::Task, IsGroup::Data],
        }
    }
}

impl Page for IsContextPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let ctx = &session.is_context;
        let max_visible = (area.height as usize).saturating_sub(4);

        let mut lines = vec![
            Line::from(Span::styled(
                " 分组: 异常 | 计数 | 调用上下文 | 系统/环境 | 时间 | 任务 | 数据",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  Tab 切换分组  / 搜索",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::raw("")),
        ];

        // 显示所有 IS* 变量及其值
        let mut count = 0;
        for v in IS_VARIABLES {
            if count >= max_visible { break; }
            let val = ctx.entries.get(v.name).map(|s| s.as_str()).unwrap_or("—");
            let is_current_group = self.groups.get(self.selected_group) == Some(&v.group);

            if is_current_group {
                let style = Style::default().fg(Color::White);
                lines.push(Line::from(Span::styled(
                    format!("  {:<30} = {}", v.name, val),
                    style,
                )));
                count += 1;
            }
        }

        if count == 0 {
            lines.push(Line::from(Span::styled(
                format!("  （分组: {} — 当前无数据）", self.groups[self.selected_group].name()),
                Style::default().fg(Color::Gray),
            )));
        }

        let block = Block::default()
            .title(format!(" {} [{}] ", self.title, self.groups[self.selected_group].name()))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let widget = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_key_shortcut(&mut self, _session: &mut LocalDebugSession, _key: char, _status: &mut String) {
        match _key {
            '\t' => {
                self.selected_group = (self.selected_group + 1) % self.groups.len();
            }
            '/' => {
                // 搜索模式由 app 处理
            }
            _ => {}
        }
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// Segment Info 页面（§3.18）
// ═══════════════════════════════════════════════════════════

pub struct SegmentInfoPage {
    title: String,
}

impl SegmentInfoPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Segment Info — 段信息".to_string(),
        }
    }
}

impl Page for SegmentInfoPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let segments = session.segment_info();

        let mut lines = vec![
            Line::from(Span::styled(" 段表", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                format!("  {:<12} {:>10}  {}", "段名", "大小", "说明"),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  ─────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        for (name, size, desc) in &segments {
            let size_str = if *size >= 1024 {
                format!("{} KB", size / 1024)
            } else {
                format!("{} B", size)
            };
            lines.push(Line::from(Span::raw(format!(
                "  {:<12} {:>10}  {}",
                name, size_str, desc
            ))));
        }

        // .debug 段详情
        if let Some(ref map) = session.debug_map {
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                " .debug 段 (ADBG)", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::raw(format!(
                "  条目数: {}  版本: 1  格式: ADBG",
                map.len(),
            ))));
            let line_entries = map.line_entries().len();
            let func_entries = map.func_entries().len();
            let var_entries = map.var_entries().len();
            let call_entries = map.call_entries().len();
            lines.push(Line::from(Span::raw(format!(
                "  LINE: {}  FUNC: {}  VAR: {}  CALL: {}",
                line_entries, func_entries, var_entries, call_entries,
            ))));
        }

        let block = Block::default()
            .title(self.title.clone())
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let widget = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// Performance Analysis 页面（§3.19）
// ═══════════════════════════════════════════════════════════

pub struct PerfAnalysisPage {
    title: String,
    pub show_hot_path: bool,
}

impl PerfAnalysisPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Performance Analysis — 性能分析".to_string(),
            show_hot_path: false,
        }
    }
}

impl Page for PerfAnalysisPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let perf = &session.perf;
        let max_visible = (area.height as usize).saturating_sub(3);

        let mut lines = vec![
            Line::from(Span::styled(" 指令执行分布", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
            Line::from(Span::raw("")),
        ];

        // Opcode 分布
        let dist = session.opcode_distribution();
        for (name, count, category) in dist.iter().take(10) {
            let pct = if perf.total_instructions > 0 {
                *count as f64 / perf.total_instructions as f64 * 100.0
            } else {
                0.0
            };
            let cat_color = match *category {
                "ARITH" => Color::Blue,
                "MEM" => Color::Magenta,
                "CTRL" => Color::Yellow,
                "SYSTEM" => Color::Red,
                "ECALL" => Color::Red,
                "TASK" => Color::Yellow,
                "CMP" => Color::Blue,
                _ => Color::White,
            };
            lines.push(Line::from(Span::styled(
                format!("  {:>8} × {:<8}  {:>6.1}%  [{}]",
                    count, name, pct, category),
                Style::default().fg(cat_color),
            )));
        }

        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            format!("  合计: {} 条指令", perf.total_instructions),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )));

        // 分类汇总
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(" 分类统计", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(Span::raw(format!("  ARITH:  {}  MEM: {}  CTRL: {}  SYSTEM: {}",
            perf.arith_count, perf.mem_count, perf.ctrl_count, perf.system_count))));

        // 内存概况
        lines.push(Line::from(Span::raw("")));
        let (total, used, _free, peak) = session.memory_stats();
        lines.push(Line::from(Span::styled(" 内存概况", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(Span::raw(format!("  当前用量: {} 字节 / {} 字节 (峰值: {} 字节)", used, total, peak))));
        lines.push(Line::from(Span::raw(format!("  分配/释放: {} / {}", perf.alloc_count, perf.free_count))));

        // Hot Path
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(" Hot Path (Top 5)", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
        let hot = session.hot_path(5);
        for (pc, count, desc) in &hot {
            lines.push(Line::from(Span::raw(format!("  {:#06x}: {}×  {}", pc, count, desc))));
        }

        let block = Block::default()
            .title(self.title.clone())
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let widget = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}
