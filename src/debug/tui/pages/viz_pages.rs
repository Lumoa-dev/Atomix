//! 可视化页面 — Data Timeline、Hook Timeline、Task Dependency、Watch Replay
//!
//! 对应设计文档 §3.3（Data Timeline）、§3.4（Hook Timeline）、§3.5（Task Dependency）、§3.7（Watch Replay）
//!
//! 使用 Unicode 框线字符绘制树形/时间轴可视化。

use crate::debug::session::LocalDebugSession;
use crate::debug::trace::{ExecutionPhase, StepStatus};
use crate::debug::tui::pages::Page;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

// ═══════════════════════════════════════════════════════════
// 3.3 Data Timeline — 数据时间轴（横向时间轴树）
// ═══════════════════════════════════════════════════════════

pub struct DataTimelinePage {
    title: String,
    pub selected_var: usize,
    pub zoom_level: f32,
    pub show_all_events: bool,
}

impl DataTimelinePage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Data Timeline — 数据时间轴".to_string(),
            selected_var: 0,
            zoom_level: 1.0,
            show_all_events: false,
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
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " 数据流: INPUT ──▶ Step ──▶ OUT     ←→ 平移  ↑↓ 切换变量  +/- 缩放",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        let task_steps: Vec<_> = trace
            .steps
            .iter()
            .filter(|s| s.phase == ExecutionPhase::Task)
            .collect();

        if task_steps.is_empty() && trace.variable_events.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无数据）",
                Style::default().fg(Color::Gray),
            )));
            frame.render_widget(Paragraph::new(lines), area);
            return;
        }

        // 时间轴头部 — Step 名称列
        let zoom = self.zoom_level.max(0.25).min(4.0);
        let step_width = (4.0 * zoom).round() as usize;
        let max_steps = (max_visible / 3).min(12);
        let mut header = "  ".to_string();
        for step in task_steps.iter().take(max_steps) {
            let label = if step.name.len() > step_width {
                format!("{:.width$}", step.name, width = step_width)
            } else {
                format!("{:^width$}", step.name, width = step_width)
            };
            header.push_str(&label);
            header.push(' ');
        }
        lines.push(Line::from(Span::styled(
            &header,
            Style::default().fg(Color::Cyan),
        )));

        // 时间轴横线
        let mut axis = "  ".to_string();
        for i in 0..task_steps.len().min(max_steps) {
            let width = step_width + 1;
            axis.push_str(&"─".repeat(width));
            if i + 1 < task_steps.len().min(max_steps) {
                axis.push('┬');
            }
        }
        lines.push(Line::from(Span::styled(
            &axis,
            Style::default().fg(Color::DarkGray),
        )));

        // 变量生命周期线 — 每行一个变量，显示流经哪些 Step
        let max_vars = max_visible.saturating_sub(4).min(8);
        let mut shown = 0;
        for var_idx in self.selected_var..trace.variable_events.len() {
            if shown >= max_vars {
                break;
            }
            let event = &trace.variable_events[var_idx];

            // ● 变量节点
            let mut line = format!("  ● {} ", event.name);

            // 绘制变量穿过各 Step 的路径
            for step in task_steps.iter().take(max_steps) {
                let is_consumer = step.name == event.step_name;
                let marker = if is_consumer {
                    "──■──"
                } else {
                    "──·──"
                };
                line.push_str(marker);
            }
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(Color::White),
            )));
            shown += 1;
        }

        // 选中变量的详细数据流
        if let Some(event) = trace.variable_events.get(self.selected_var) {
            lines.push(Line::from(Span::raw("")));
            let old_val = event
                .old_value
                .map(|v| format!("{:#x}", v))
                .unwrap_or_else(|| "—".to_string());
            lines.push(Line::from(Span::styled(
                format!(
                    "  ▼ {}: {} → {}  @PC={:#06x}  Step:{}",
                    event.name, old_val, event.new_value, event.pc, event.step_name
                ),
                Style::default().fg(Color::Yellow),
            )));
            // 完整流向路径
            let mut flow = "  INPUT ".to_string();
            for s in &task_steps {
                if s.name == event.step_name
                    || s.input_vars.iter().any(|v| v == &event.name)
                    || s.output_vars.iter().any(|v| v == &event.name)
                {
                    flow.push_str(&format!("─▶ {} ─▶", s.name));
                }
            }
            flow.push_str(" OUT");
            lines.push(Line::from(Span::styled(
                flow,
                Style::default().fg(Color::Cyan),
            )));
        }

        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn on_zoom_in(&mut self, _session: &mut LocalDebugSession) {
        self.zoom_level = (self.zoom_level * 1.5).min(8.0);
    }
    fn on_zoom_out(&mut self, _session: &mut LocalDebugSession) {
        self.zoom_level = (self.zoom_level / 1.5).max(0.25);
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// 3.4 Hook Timeline — 钩子时间轴（横向时间轴树）
// ═══════════════════════════════════════════════════════════

pub struct HookTimelinePage {
    title: String,
    pub selected_branch: usize,
    pub zoom_level: f32,
}

impl HookTimelinePage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Hook Timeline — 钩子时间轴".to_string(),
            selected_branch: 0,
            zoom_level: 1.0,
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
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " WORKS 钩子执行链     ←→ 平移  ↑↓ 切换分支  +/- 缩放",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        if trace.hook_timeline.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无钩子事件 — WORKS 生命周期钩子需运行时支持）",
                Style::default().fg(Color::Gray),
            )));
        } else {
            // 按 WORKS 实例分组
            let mut works_groups: std::collections::HashMap<
                &str,
                Vec<&crate::debug::trace::HookEvent>,
            > = std::collections::HashMap::new();
            for event in &trace.hook_timeline {
                works_groups
                    .entry(event.works_name.as_str())
                    .or_default()
                    .push(event);
            }

            for (works_name, events) in works_groups.iter() {
                let elapsed_total: u64 = events.iter().map(|e| e.elapsed_us).sum();
                lines.push(Line::from(Span::styled(
                    format!(
                        "\n  ┌─ {} [{}μs total] ─────────────────",
                        works_name, elapsed_total
                    ),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                )));

                for (i, event) in events.iter().enumerate() {
                    if i >= max_visible / 2 {
                        break;
                    }
                    let elapsed = format!("{}μs", event.elapsed_us);
                    let is_last = i == events.len() - 1;
                    let branch = if is_last { "└─" } else { "├─" };
                    let cond = event
                        .condition
                        .as_ref()
                        .map(|c| format!(" [if {}]", c))
                        .unwrap_or_default();
                    let fanout = if event.is_fanout { " ⚡FANOUT" } else { "" };

                    lines.push(Line::from(Span::styled(
                        format!(
                            "  {}── {:>12} ──▶ {}{}  [{}]{}",
                            branch, event.hook_name, event.action, cond, elapsed, fanout
                        ),
                        if event.is_fanout {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default().fg(Color::White)
                        },
                    )));
                }
            }
        }

        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn on_zoom_in(&mut self, _session: &mut LocalDebugSession) {
        self.zoom_level = (self.zoom_level * 1.5).min(8.0);
    }
    fn on_zoom_out(&mut self, _session: &mut LocalDebugSession) {
        self.zoom_level = (self.zoom_level / 1.5).max(0.25);
    }
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// 3.5 Task Dependency — 任务依赖树（横向层次树）
// ═══════════════════════════════════════════════════════════

pub struct TaskDependencyPage {
    title: String,
    pub selected_task: usize,
    pub show_detail: bool,
}

impl TaskDependencyPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Task Dependency — 任务依赖树".to_string(),
            selected_task: 0,
            show_detail: false,
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
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " 任务依赖 DAG    FORK ──▶ (橙色)    JOIN ◀── (青色)",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        let task_steps: Vec<_> = trace
            .steps
            .iter()
            .filter(|s| s.phase == ExecutionPhase::Task)
            .collect();

        if task_steps.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无任务 Step）",
                Style::default().fg(Color::Gray),
            )));
        } else {
            // 按层次分组建树
            let depth = 3;
            let mut levels: Vec<Vec<usize>> = vec![Vec::new(); depth];
            for (i, _) in task_steps.iter().enumerate() {
                levels[i % depth].push(i);
            }

            lines.push(Line::from(Span::styled(
                "  [ROOT] Task Pool",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));

            for level in 0..depth {
                if level < levels.len() && !levels[level].is_empty() {
                    let is_last_level = level == depth - 1;
                    let branch = if is_last_level {
                        "  └─"
                    } else {
                        "  ├─"
                    };

                    lines.push(Line::from(Span::styled(
                        format!("{}── FORK ──▶ Batch {}", branch, level),
                        Style::default().fg(Color::Rgb(255, 165, 0)),
                    )));

                    for (j, &idx) in levels[level].iter().enumerate() {
                        if j >= max_visible / 3 {
                            break;
                        }
                        let step = &task_steps[idx];
                        let is_last_in_level = j == levels[level].len() - 1;
                        let leaf_branch = if is_last_in_level {
                            "      └─"
                        } else {
                            "      ├─"
                        };
                        let sel_mark = if idx == self.selected_task {
                            " ▶"
                        } else {
                            "  "
                        };

                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{}── ", leaf_branch),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled(
                                format!("{}", step.status.symbol()),
                                Style::default().fg(Color::Green),
                            ),
                            Span::styled(
                                format!(" {}{}", step.name, sel_mark),
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                format!("  [line {}]", step.source_line),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));

                        if j > 0 {
                            lines.push(Line::from(Span::styled(
                                format!("      │  ◀── JOIN ── result from {}", step.name),
                                Style::default().fg(Color::Cyan),
                            )));
                        }
                    }
                }
            }

            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                format!(
                    "  共 {} 任务 | 橙色=FORK 边 | 青色=JOIN 边",
                    task_steps.len()
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }

        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// 3.7 Watch Replay — 回放
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
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Step 回放",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Space 暂停/继续  ←→ 调速 0.25x–4x",
            Style::default().fg(Color::DarkGray),
        )));

        // 进度条
        let bar_width = 40usize;
        let progress_filled = (self.progress * bar_width as f32).round() as usize;
        let bar: String = (0..bar_width)
            .map(|i| if i < progress_filled { '█' } else { '░' })
            .collect();
        let play_symbol = if self.is_playing { "⏸" } else { "▶" };

        lines.push(Line::from(Span::styled(
            format!(
                "  {}  |{}|  {:.0}%  Speed: {:.1}x",
                play_symbol,
                bar,
                self.progress * 100.0,
                self.speed
            ),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(Span::raw("")));

        // 子操作清单
        let task_steps: Vec<_> = trace
            .steps
            .iter()
            .filter(|s| s.phase == ExecutionPhase::Task)
            .collect();
        let completed_count = (task_steps.len() as f32 * self.progress).round() as usize;

        lines.push(Line::from(Span::styled(
            "  Sub-operations:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for (i, step) in task_steps.iter().enumerate() {
            if i >= (area.height as usize).saturating_sub(8) {
                break;
            }
            let (marker, color) = if i < completed_count {
                ("✓", Color::Green)
            } else if i == completed_count {
                ("▶", Color::Yellow)
            } else {
                ("·", Color::Gray)
            };
            lines.push(Line::from(Span::styled(
                format!("    {}  {}  [line {}]", marker, step.name, step.source_line),
                Style::default().fg(color),
            )));
        }

        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            format!(
                "  PC={:#06x}  Instr={}  State={:?}",
                session.vm.pc, session.perf.total_instructions, session.vm.state
            ),
            Style::default().fg(Color::DarkGray),
        )));

        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn on_key_shortcut(
        &mut self,
        _session: &mut LocalDebugSession,
        key: char,
        status: &mut String,
    ) {
        if key == ' ' {
            self.is_playing = !self.is_playing;
            *status = if self.is_playing {
                "回放已暂停".to_string()
            } else {
                "回放中".to_string()
            };
        }
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}
