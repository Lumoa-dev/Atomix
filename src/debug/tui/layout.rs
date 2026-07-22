//! TUI 布局 — 设计文档 §2

use crate::debug::session::LocalDebugSession;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

pub struct LayoutChunks {
    pub title: Rect,
    pub breadcrumb: Rect,
    pub main_content: Rect,
    pub right_panel: Rect,
    pub command_bar: Rect,
}

pub struct TuiLayout {
    pub right_panel_mode: String,
}

impl TuiLayout {
    pub fn new() -> Self {
        Self {
            right_panel_mode: "regs".to_string(),
        }
    }

    pub fn set_right_panel_mode(&mut self, mode: &str) {
        self.right_panel_mode = mode.to_string();
    }

    pub fn calculate_layout(&self, area: Rect) -> LayoutChunks {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(1),
            ])
            .split(area);
        let content_area = vertical[2];
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(content_area);
        LayoutChunks {
            title: vertical[0],
            breadcrumb: vertical[1],
            main_content: horizontal[0],
            right_panel: horizontal[1],
            command_bar: vertical[3],
        }
    }

    pub fn render_title_bar(&self, frame: &mut Frame, area: Rect, session: &LocalDebugSession) {
        let file_name = session
            .source_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("untitled");
        let spans = vec![
            Span::styled(
                "atomix debug",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" — {} ", file_name)),
            Span::styled(
                format!(
                    "PC={:#06x} Step={}",
                    session.vm.pc,
                    session.trace.step_count()
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ];
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Black)),
            area,
        );
    }

    pub fn render_breadcrumb(&self, frame: &mut Frame, area: Rect, crumbs: &[String]) {
        let mut spans = Vec::new();
        for (i, crumb) in crumbs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" ▸ "));
            }
            spans.push(Span::styled(
                crumb.clone(),
                if i == crumbs.len() - 1 {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Blue)
                },
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
            area,
        );
    }

    pub fn render_right_panel(&self, frame: &mut Frame, area: Rect, session: &LocalDebugSession) {
        if area.width < 20 || area.height < 5 {
            return;
        }

        match self.right_panel_mode.as_str() {
            "mem" => self.render_right_panel_mem(frame, area, session),
            "is" => self.render_right_panel_is(frame, area, session),
            _ => self.render_right_panel_regs(frame, area, session),
        }
    }

    fn render_right_panel_regs(&self, frame: &mut Frame, area: Rect, session: &LocalDebugSession) {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
            ])
            .split(area);

        // Title
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Registers ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))),
            vertical[0],
        );

        // Register values
        let var_lines: Vec<Line> = (0..16.min(vertical[1].height as usize - 1))
            .map(|i| {
                let name = crate::base::isa::reg_name(i).to_uppercase();
                let val = session.vm.read_reg(i);
                Line::from(Span::raw(format!("  {:>8}: {:#018x}  ({})", name, val, val as i64)))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(var_lines)
                .style(Style::default().fg(Color::White))
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: false }),
            vertical[1],
        );
    }

    fn render_right_panel_mem(&self, frame: &mut Frame, area: Rect, session: &LocalDebugSession) {
        // Memory hex dump starting at address 0
        let bytes_per_line = 8;
        let max_lines = (area.height as usize).saturating_sub(2);

        let mut mem_lines = Vec::new();
        mem_lines.push(Line::from(Span::styled(
            " Memory Hex Dump ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        for line_idx in 0..max_lines {
            let addr = (line_idx * bytes_per_line) as u64;
            let mut hex = String::new();
            let mut ascii = String::new();
            for byte_idx in 0..bytes_per_line {
                let a = addr.wrapping_add(byte_idx as u64);
                if let Some(byte) = session.vm.memory.read_u8(a) {
                    hex.push_str(&format!("{:02x} ", byte));
                    ascii.push(if byte.is_ascii_graphic() || byte == b' ' {
                        byte as char
                    } else {
                        '.'
                    });
                } else {
                    hex.push_str("   ");
                    ascii.push('.');
                }
            }
            mem_lines.push(Line::from(Span::styled(
                format!("  {:#010x}:  {:24}  {}", addr, hex, ascii),
                Style::default().fg(Color::White),
            )));
        }

        frame.render_widget(
            Paragraph::new(mem_lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn render_right_panel_is(&self, frame: &mut Frame, area: Rect, session: &LocalDebugSession) {
        let ctx = &session.is_context;

        // Collect IS* entries into grouped lines.
        let groups = [
            ("Exception", &["IS_EXCEPTION", "IS_EXCEPTION_MSG", "IS_EXCEPTION_PC"] as &[&str]),
            ("Count", &["IS_COUNT_INSTR", "IS_COUNT_STEP", "IS_COUNT_CALL", "IS_COUNT_ALLOC"]),
            ("CallCtx", &["IS_CALL_DEPTH", "IS_CALL_STACK_SIZE", "IS_FRAME_SP", "IS_FRAME_FP", "IS_FRAME_RA"]),
            ("System", &["IS_SYS_PC", "IS_SYS_STATE", "IS_SYS_MEM_USED", "IS_SYS_MEM_TOTAL", "IS_SYS_QUANTUM", "IS_SYS_PROFILE"]),
            ("Task", &["IS_TASK_ID", "IS_TASK_STATUS", "IS_TASK_DEPTH", "IS_TASK_JOIN_TARGET"]),
            ("Data", &["IS_DATA_RODATA_SIZE", "IS_DATA_STACK_USED", "IS_DATA_HEAP_USED"]),
        ];

        let mut is_lines = Vec::new();
        is_lines.push(Line::from(Span::styled(
            " IS* Context ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        for (_group_name, keys) in &groups {
            for key in *keys {
                let val = ctx.entries.get(*key).map(|s| s.as_str()).unwrap_or("—");
                is_lines.push(Line::from(Span::raw(format!("  {}: {}", key, val))));
            }
        }

        frame.render_widget(
            Paragraph::new(is_lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    pub fn render_command_bar(
        &self,
        frame: &mut Frame,
        area: Rect,
        command_mode: bool,
        command_buffer: &str,
        status_message: &str,
    ) {
        let prompt = if command_mode {
            format!("> {}", command_buffer)
        } else {
            format!(
                " :  {}  |  ↑↓ nav  Enter select  Esc back  h help  q quit",
                status_message
            )
        };
        let style = if command_mode {
            Style::default().fg(Color::Green).bg(Color::Black)
        } else {
            Style::default().fg(Color::White).bg(Color::Black)
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::raw(prompt)))
                .style(style)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                ),
            area,
        );
    }

    pub fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let help_text = vec![
            Line::from(Span::styled(
                " Atomix Debugger — 键盘快捷键 & 命令参考",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled("  导航", Style::default().fg(Color::Cyan))),
            Line::from(Span::raw(
                "    ↑↓          上下导航    Enter       选中/进入",
            )),
            Line::from(Span::raw(
                "    Esc         返回上一层  q           退出调试器",
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled("  视图切换", Style::default().fg(Color::Cyan))),
            Line::from(Span::raw(
                "    :src        源码视图    :df         数据时间轴",
            )),
            Line::from(Span::raw(
                "    :hooks      钩子时间轴  :deps       任务依赖树",
            )),
            Line::from(Span::raw(
                "    :disasm     反汇编      :regs       寄存器与内存",
            )),
            Line::from(Span::raw(
                "    :bt         调用栈      :is         IS* 全览",
            )),
            Line::from(Span::raw("    :breaks     断点管理    :segments   段信息")),
            Line::from(Span::raw(
                "    :perf       性能分析    :zones      Zone 状态",
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled("  执行控制", Style::default().fg(Color::Cyan))),
            Line::from(Span::raw("    step [n]    执行 n 条指令")),
            Line::from(Span::raw("    continue    运行到断点或结束")),
            Line::from(Span::raw("    step:into   进入当前 Step")),
            Line::from(Span::raw("    step:out    跳出当前 Step")),
            Line::from(Span::raw("")),
            Line::from(Span::styled("  断点", Style::default().fg(Color::Cyan))),
            Line::from(Span::raw("    break <addr>         设置 PC 断点")),
            Line::from(Span::raw("    break:line <n>       设置行号断点")),
            Line::from(Span::raw("    break:list           列出所有断点")),
            Line::from(Span::raw(
                "    break:del <id>       删除断点    break:clear  清空断点",
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled("  信息查询", Style::default().fg(Color::Cyan))),
            Line::from(Span::raw("    print <expr>   打印表达式值")),
            Line::from(Span::raw("    info           当前上下文概览")),
            Line::from(Span::raw("    history        命令历史")),
            Line::from(Span::raw("")),
            Line::from(Span::styled("  设置", Style::default().fg(Color::Cyan))),
            Line::from(Span::raw("    set <reg> = <value>   设置寄存器")),
            Line::from(Span::raw("    set:fmt hex|dec       设置显示格式")),
            Line::from(Span::raw("    set:depth <n>         设置嵌套深度")),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "  按 Esc 关闭帮助",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let help_widget = Paragraph::new(help_text)
            .style(Style::default().bg(Color::Black).fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false });
        let help_area = Rect {
            x: area.width / 8,
            y: 1,
            width: area.width * 3 / 4,
            height: area.height.saturating_sub(2).min(40),
        };
        frame.render_widget(help_widget, help_area);
    }
}
