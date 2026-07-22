//! 可视化页面 — Data Timeline、Hook Timeline、Task Dependency、Watch Replay
//!
//! 对应设计文档 §3.3（Data Timeline）、§3.4（Hook Timeline）、§3.5（Task Dependency）、§3.7（Watch Replay）

use crate::debug::session::LocalDebugSession;
use crate::debug::trace::ExecutionPhase;
use crate::debug::tui::pages::Page;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

// ═══════════════════════════════════════════════════════════
// Data Timeline 页面（§3.3）
// ═══════════════════════════════════════════════════════════

pub struct DataTimelinePage {
    title: String,
    pub scroll: usize,
    pub selected_var: usize,
    pub zoom_level: f32,
}

impl DataTimelinePage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Data Timeline — 数据时间轴".to_string(),
            scroll: 0,
            selected_var: 0,
            zoom_level: 1.0,
        }
    }
}

impl Page for DataTimelinePage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let trace = &session.trace;
        let max_visible = (area.height as usize).saturating_sub(3);

        let mut lines = vec![
            Line::from(Span::styled(
                " 数据流动方向: INPUT → Step → OUT",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  ←→ 平移  ↑↓ 切换变量  +/- 缩放",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::raw("")),
        ];

        // 展示变量生命周期
        let steps = trace.steps_by_phase(ExecutionPhase::Task);
        if steps.is_empty() {
            lines.push(Line::from(Span::styled("  （无数据）", Style::default().fg(Color::Gray))));
        } else {
            // 时间轴头部
            let mut header = "  ".to_string();
            for step in steps.iter().take(max_visible.saturating_sub(4)) {
                let name = if step.name.len() > 8 {
                    format!("{:>8}", &step.name[..8.min(step.name.len())])
                } else {
                    format!("{:>8}", step.name)
                };
                header.push_str(&name);
            }
            lines.push(Line::from(Span::styled(header, Style::default().fg(Color::Cyan))));

            // 用简单的文本图表示数据流
            // 这里简化为展示变量事件
            for event in trace.variable_events.iter().skip(self.selected_var).take(5) {
                let old = event.old_value.map(|v| format!("{:#x}", v)).unwrap_or_else(|| "—".to_string());
                lines.push(Line::from(Span::styled(
                    format!("  {}: {} → {}  @{:#06x} [{}]",
                        event.name, old, event.new_value, event.pc, event.step_name),
                    Style::default().fg(Color::White),
                )));
            }

            if trace.variable_events.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  （无变量事件）",
                    Style::default().fg(Color::Gray),
                )));
            }
        }

        let block = Block::default()
            .title(format!(" {} ({} 变量事件) ", self.title, trace.variable_events.len()))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let widget = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_zoom_in(&mut self, _session: &mut LocalDebugSession) {
        self.zoom_level = (self.zoom_level * 2.0).min(8.0);
    }

    fn on_zoom_out(&mut self, _session: &mut LocalDebugSession) {
        self.zoom_level = (self.zoom_level / 2.0).max(0.25);
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// Hook Timeline 页面（§3.4）
// ═══════════════════════════════════════════════════════════

pub struct HookTimelinePage {
    title: String,
    pub scroll: usize,
}

impl HookTimelinePage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Hook Timeline — 钩子时间轴".to_string(),
            scroll: 0,
        }
    }
}

impl Page for HookTimelinePage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let trace = &session.trace;
        let max_visible = (area.height as usize).saturating_sub(2);

        let mut lines = vec![
            Line::from(Span::styled(
                " WORKS 钩子执行序列",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
        ];

        if trace.hook_timeline.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无钩子事件 — WORKS 生命周期钩子尚未在运行时中实现）",
                Style::default().fg(Color::Gray),
            )));
        } else {
            for event in trace.hook_timeline.iter().take(max_visible.saturating_sub(3)) {
                let elapsed = if event.elapsed_us >= 1000 {
                    format!("{:.1}ms", event.elapsed_us as f64 / 1000.0)
                } else {
                    format!("{}μs", event.elapsed_us)
                };

                let condition_info = event.condition.as_ref()
                    .map(|c| format!(" [if {}]", c))
                    .unwrap_or_default();

                let fanout_mark = if event.is_fanout { " ⚡扇出" } else { "" };

                lines.push(Line::from(Span::styled(
                    format!("  {}.{} → {}{}  [{}]{}",
                        event.works_name, event.hook_name, event.action,
                        condition_info, elapsed, fanout_mark),
                    Style::default().fg(if event.is_fanout { Color::Yellow } else { Color::White }),
                )));
            }
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
// Task Dependency 页面（§3.5）
// ═══════════════════════════════════════════════════════════

pub struct TaskDependencyPage {
    title: String,
    pub scroll: usize,
}

impl TaskDependencyPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Task Dependency — 任务依赖树".to_string(),
            scroll: 0,
        }
    }
}

impl Page for TaskDependencyPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let trace = &session.trace;
        let max_visible = (area.height as usize).saturating_sub(2);

        let mut lines = vec![
            Line::from(Span::styled(
                " 任务依赖关系 (FORK/JOIN)",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  FORK → (橙色)  JOIN ← (青色)",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::raw("")),
        ];

        // 展示 Step 执行顺序作为简单的依赖树
        let task_steps = trace.steps_by_phase(ExecutionPhase::Task);
        if task_steps.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无任务 Step）",
                Style::default().fg(Color::Gray),
            )));
        } else {
            for step in task_steps.iter().take(max_visible.saturating_sub(4)) {
                let status = step.status.symbol();
                let indent = "  ";
                lines.push(Line::from(Span::styled(
                    format!("{} {} {}  [line {}]",
                        indent, status, step.name, step.source_line),
                    Style::default().fg(Color::White),
                )));
            }

            // 依赖信息
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                format!("  共 {} 个任务 Step", task_steps.len()),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "  FORK/JOIN 可视化需要运行时 TASK_FORK/TASK_JOIN 支持",
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
// Watch Replay 页面（§3.7）
// ═══════════════════════════════════════════════════════════

pub struct WatchReplayPage {
    title: String,
    pub is_playing: bool,
    pub speed: f32,
    pub progress: f32,
}

impl WatchReplayPage {
    pub fn new(session: &LocalDebugSession) -> Self {
        Self {
            title: "Watch Replay — 回放".to_string(),
            is_playing: false,
            speed: session.watch_spd,
            progress: 0.0,
        }
    }
}

impl Page for WatchReplayPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let trace = &session.trace;
        self.speed = session.watch_spd;

        let mut lines = vec![
            Line::from(Span::styled(
                " Step 回放",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
        ];

        // 进度条
        let bar_width = 40usize;
        let progress_filled = (self.progress * bar_width as f32).round() as usize;
        let bar: String = (0..bar_width).map(|i| {
            if i < progress_filled { '█' } else { '░' }
        }).collect();

        let play_symbol = if self.is_playing { "⏸" } else { "▶" };
        lines.push(Line::from(Span::styled(
            format!("  {}  |{}|  {:.0}%  Speed: {:.1}x",
                play_symbol, bar, self.progress * 100.0, self.speed),
            Style::default().fg(Color::Yellow),
        )));

        lines.push(Line::from(Span::styled(
            "  Space 暂停/继续  ←→ 调速",
            Style::default().fg(Color::DarkGray),
        )));

        lines.push(Line::from(Span::raw("")));

        // 子操作清单
        let task_steps = trace.steps_by_phase(ExecutionPhase::Task);
        let completed_count = (task_steps.len() as f32 * self.progress) as usize;

        lines.push(Line::from(Span::styled(
            "  Sub-operations:",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));

        for (i, step) in task_steps.iter().enumerate() {
            if i >= (area.height as usize).saturating_sub(8) {
                break;
            }
            let marker = if i < completed_count {
                "✓".to_string()
            } else if i == completed_count {
                "▶".to_string()
            } else {
                "·".to_string()
            };
            let color = if i < completed_count {
                Color::Green
            } else if i == completed_count {
                Color::Yellow
            } else {
                Color::Gray
            };
            lines.push(Line::from(Span::styled(
                format!("    {}  {}", marker, step.name),
                Style::default().fg(color),
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

    fn on_key_shortcut(&mut self, _session: &mut LocalDebugSession, key: char, status: &mut String) {
        match key {
            ' ' => {
                self.is_playing = !self.is_playing;
                *status = if self.is_playing { "回放已暂停".to_string() } else { "回放中".to_string() };
            }
            _ => {}
        }
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}
