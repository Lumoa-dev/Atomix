//! 可视化页面 — Data Timeline、Hook Timeline、Task Dependency、Watch Replay
//!
//! 对应设计文档 §3.3（Data Timeline）、§3.4（Hook Timeline）、§3.5（Task Dependency）、§3.7（Watch Replay）
//!
//! 使用 Unicode 框线字符绘制树形/时间轴可视化。

use crate::debug::session::LocalDebugSession;
use crate::debug::trace::{ExecutionPhase, VariableEvent};
use crate::debug::tui::pages::Page;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
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
    pub _show_all_events: bool,
    pub scroll_offset: usize,
}

impl DataTimelinePage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Data Timeline — 数据时间轴".to_string(),
            selected_var: 0,
            zoom_level: 1.0,
            _show_all_events: false,
            scroll_offset: 0,
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
            " 数据流: ● var ──▶ ■ step ──▶ OUT     ←→ 平移  ↑↓ 切换变量  +/- 缩放",
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

        // ── Build variable-to-events map ──────────────────────────────
        let mut var_events_map: std::collections::BTreeMap<&str, Vec<&VariableEvent>> =
            std::collections::BTreeMap::new();
        for event in &trace.variable_events {
            var_events_map.entry(event.name.as_str()).or_default().push(event);
        }

        // ── Determine consumed variables (appear in any step's I/O) ──
        let consumed: std::collections::HashSet<&str> = {
            let mut set = std::collections::HashSet::new();
            for step in &task_steps {
                for v in &step.input_vars {
                    set.insert(v.as_str());
                }
                for v in &step.output_vars {
                    set.insert(v.as_str());
                }
            }
            set
        };

        // ── Zoom and layout geometry ──────────────────────────────────
        let zoom = self.zoom_level.max(0.25).min(4.0);
        let step_width = (6usize.max(4) as f64 * zoom as f64).round() as usize;
        let step_gap = step_width.saturating_add(2);
        let max_steps = (max_visible / 3).min(12);
        let max_vars = max_visible.saturating_sub(5).min(10);

        // ── Header: step name row ─────────────────────────────────────
        let mut header = "       ".to_string();
        for step in task_steps.iter().take(max_steps) {
            let label = if step.name.len() > step_width {
                format!("{:.width$}", step.name, width = step_width)
            } else {
                format!("{:^width$}", step.name, width = step_width)
            };
            header.push(' ');
            header.push_str(&label);
        }
        header.push_str("   STATUS");
        lines.push(Line::from(Span::styled(&header, Style::default().fg(Color::Cyan))));

        // ── Axis: ──■── chain ─────────────────────────────────────────
        let mut axis = "  ──────".to_string();
        for i in 0..task_steps.len().min(max_steps) {
            axis.push_str(&"─".repeat(step_gap));
            if i + 1 < task_steps.len().min(max_steps) {
                axis.push('┬');
            }
        }
        axis.push_str("───");
        lines.push(Line::from(Span::styled(&axis, Style::default().fg(Color::DarkGray))));

        // ── Variable rows ─────────────────────────────────────────────
        let var_names: Vec<&str> = var_events_map.keys().copied().collect();
        let mut shown = 0;

        for var_idx in self.selected_var..var_names.len() {
            if shown >= max_vars {
                break;
            }
            let var_name = var_names[var_idx];
            let events = &var_events_map[var_name];

            // ● variable node + name label
            let mut row = format!("  ● {:<8}", var_name);

            // Step flow markers: ──■── if variable passes through, ──·── otherwise
            for step in task_steps.iter().take(max_steps) {
                let flows_through = events.iter().any(|e| e.step_name == step.name);
                let marker = if flows_through { "──■──" } else { "──·──" };
                row.push_str(marker);
            }

            // Status suffix: OUT / ✗ broken
            let is_consumed = consumed.contains(var_name);
            let status = if is_consumed { "──▶ OUT" } else { "  ✗ broken" };
            row.push_str(status);

            lines.push(Line::from(Span::styled(
                row,
                if is_consumed {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Red)
                },
            )));
            shown += 1;
        }

        // ── Merge points ═══  where multiple variables converge ──────
        let mut merge_printed = 0;
        for step in task_steps.iter().take(max_steps) {
            let vars_in_step: Vec<&str> = var_names
                .iter()
                .copied()
                .filter(|vn| {
                    var_events_map
                        .get(vn)
                        .map_or(false, |evs| evs.iter().any(|e| e.step_name == step.name))
                })
                .collect();
            if vars_in_step.len() > 1 && merge_printed < max_vars.saturating_sub(shown) {
                let vnames: Vec<&str> = vars_in_step.iter().copied().collect();
                lines.push(Line::from(Span::styled(
                    format!("  ═══ merge at {}: {}", step.name, vnames.join(", ")),
                    Style::default().fg(Color::Yellow),
                )));
                merge_printed += 1;
            }
        }

        // ── Selected variable detail ──────────────────────────────────
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
            // Flow path
            let mut flow = "  ● ".to_string();
            flow.push_str(&event.name);
            flow.push_str(" ──▶");
            for s in &task_steps {
                if s.name == event.step_name
                    || s.input_vars.iter().any(|v| v == &event.name)
                    || s.output_vars.iter().any(|v| v == &event.name)
                {
                    flow.push_str(&format!(" ■ {} ──▶", s.name));
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
    pub _selected_branch: usize,
    pub zoom_level: f32,
}

impl HookTimelinePage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Hook Timeline — 钩子时间轴".to_string(),
            _selected_branch: 0,
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

                let mut fanout_active = false;
                let max_events = max_visible / 2;

                for (i, event) in events.iter().enumerate() {
                    if i >= max_events {
                        break;
                    }
                    let elapsed = format!("{}μs", event.elapsed_us);
                    let is_last = i == events.len() - 1;
                    let cond = event
                        .condition
                        .as_ref()
                        .map(|c| format!(" [if {}]", c))
                        .unwrap_or_default();

                    if event.is_fanout {
                        // ── Fanout parent node ────────────────────────
                        let branch = if is_last { "  └──" } else { "  ├──" };
                        let line = format!(
                            "{} {:>12} ──▶ {}{}  [{}] ⚡",
                            branch, event.hook_name, event.action, cond, elapsed,
                        );
                        lines.push(Line::from(Span::styled(
                            line,
                            Style::default().fg(Color::Yellow),
                        )));
                        fanout_active = true;

                    } else if fanout_active {
                        // ── Child of preceding fanout node ────────────
                        // Look ahead: does the next event end the fanout?
                        let next_ends = is_last
                            || events
                                .get(i + 1)
                                .map(|e| e.is_fanout)
                                .unwrap_or(true);
                        let child_branch = if next_ends { "  │   └──" } else { "  │   ├──" };
                        let line = format!(
                            "{} {:>12} ──▶ {}{}  [{}]",
                            child_branch, event.hook_name, event.action, cond, elapsed,
                        );
                        lines.push(Line::from(Span::styled(
                            line,
                            Style::default().fg(Color::White),
                        )));
                        if next_ends {
                            fanout_active = false;
                        }

                    } else {
                        // ── Normal (non-fanout) node ──────────────────
                        let branch = if is_last { "  └──" } else { "  ├──" };
                        let line = format!(
                            "{} {:>12} ──▶ {}{}  [{}]",
                            branch, event.hook_name, event.action, cond, elapsed,
                        );
                        lines.push(Line::from(Span::styled(
                            line,
                            Style::default().fg(Color::White),
                        )));
                    }
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
    pub _show_detail: bool,
}

impl TaskDependencyPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Task Dependency — 任务依赖树".to_string(),
            selected_task: 0,
            _show_detail: false,
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
            frame.render_widget(Paragraph::new(lines), area);
            return;
        }

        // ── Parse fork / join steps and build groups ──────────────────
        struct ForkGroup<'a> {
            fork: &'a crate::debug::trace::StepRecord,
            children: Vec<&'a crate::debug::trace::StepRecord>,
            join: Option<&'a crate::debug::trace::StepRecord>,
        }

        let mut fork_groups: Vec<ForkGroup> = Vec::new();
        let mut ungrouped: Vec<&crate::debug::trace::StepRecord> = Vec::new();
        let mut current_fork: Option<ForkGroup> = None;

        for step in &task_steps {
            let lower = step.name.to_lowercase();
            if lower.contains("fork") {
                // Finalise any previous open fork group
                if let Some(fg) = current_fork.take() {
                    fork_groups.push(fg);
                }
                current_fork = Some(ForkGroup {
                    fork: step,
                    children: Vec::new(),
                    join: None,
                });
            } else if lower.contains("join") {
                if let Some(mut fg) = current_fork.take() {
                    fg.join = Some(step);
                    fork_groups.push(fg);
                } else {
                    ungrouped.push(step);
                }
            } else {
                match current_fork.as_mut() {
                    Some(fg) => fg.children.push(step),
                    None => ungrouped.push(step),
                }
            }
        }
        // Flush any dangling fork group
        if let Some(fg) = current_fork.take() {
            fork_groups.push(fg);
        }

        // ── Render ────────────────────────────────────────────────────
        lines.push(Line::from(Span::styled(
            "  [ROOT] Task Pool",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));

        let max_rows = max_visible.saturating_sub(3);
        let mut rows = 0usize;

        // Render fork groups
        for (gi, group) in fork_groups.iter().enumerate() {
            if rows >= max_rows {
                break;
            }
            let is_last_group = gi == fork_groups.len().saturating_sub(1) && ungrouped.is_empty();
            let fork_branch = if is_last_group { "  └──" } else { "  ├──" };

            // FORK edge (orange)
            lines.push(Line::from(Span::styled(
                format!("{} ── FORK ──▶ {} [line {}]",
                    fork_branch, group.fork.name, group.fork.source_line),
                Style::default().fg(Color::Rgb(255, 165, 0)),
            )));
            rows += 1;

            // Children of this fork
            for (ci, child) in group.children.iter().enumerate() {
                if rows >= max_rows {
                    break;
                }
                let is_last_child = ci == group.children.len().saturating_sub(1);
                let child_branch = if is_last_child { "  │   └──" } else { "  │   ├──" };
                let sel_mark = "  "; // selection not shown per-child for simplicity

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}── ", child_branch),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{}", child.status.symbol()),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(
                        format!(" {}{}", child.name, sel_mark),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("  [line {}]", child.source_line),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
                rows += 1;
            }

            // JOIN edge (cyan)
            if let Some(join) = group.join {
                if rows < max_rows {
                    let join_branch = if is_last_group { "  └──" } else { "  ├──" };
                    lines.push(Line::from(Span::styled(
                        format!("{} ◀── JOIN ── {} [line {}]",
                            join_branch, join.name, join.source_line),
                        Style::default().fg(Color::Cyan),
                    )));
                    rows += 1;
                }
            }
        }

        // Render ungrouped steps (no fork/join association)
        for (ui, step) in ungrouped.iter().enumerate() {
            if rows >= max_rows {
                break;
            }
            let is_last = ui == ungrouped.len().saturating_sub(1);
            let leaf_branch = if is_last { "  └──" } else { "  ├──" };
            let sel_mark = "  ";

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
            rows += 1;
        }

        // Summary footer
        lines.push(Line::from(Span::raw("")));
        let fork_count = fork_groups.len();
        let join_count = fork_groups.iter().filter(|g| g.join.is_some()).count();
        lines.push(Line::from(Span::styled(
            format!(
                "  共 {} 任务 | {} FORK | {} JOIN | 橙色=FORK 边  青色=JOIN 边",
                task_steps.len(),
                fork_count,
                join_count,
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
