//! Home 页面 — Step 执行日志（对应设计文档 §3.1）

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

pub struct HomePage {
    title: String,
    pub selected: usize,
    pub scroll: usize,
    pub expanded_phases: Vec<ExecutionPhase>,
}

impl HomePage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Home — Step 执行日志".to_string(),
            selected: 0,
            scroll: 0,
            expanded_phases: vec![ExecutionPhase::Task, ExecutionPhase::System],
        }
    }

    fn display_lines(&self, session: &LocalDebugSession) -> Vec<HomeLine> {
        let mut lines = Vec::new();
        let trace = &session.trace;
        for phase in ExecutionPhase::all() {
            let steps: Vec<&crate::debug::trace::StepRecord> = trace.steps.iter().filter(|s| s.phase == *phase).collect();
            let step_count = steps.len();
            let is_expanded = self.expanded_phases.contains(phase);
            lines.push(HomeLine::PhaseHeader { phase: *phase, count: step_count, expanded: is_expanded });
            if is_expanded {
                for step in steps {
                    lines.push(HomeLine::Step((*step).clone()));
                }
            }
        }
        lines
    }
}

enum HomeLine {
    PhaseHeader { phase: ExecutionPhase, count: usize, expanded: bool },
    Step(crate::debug::trace::StepRecord),
}

impl Page for HomePage {
    fn title(&self) -> &str { &self.title }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        if area.height < 3 { return; }
        let lines = self.display_lines(session);
        let total_lines = lines.len();
        let max_visible = (area.height as usize).saturating_sub(2);
        if total_lines == 0 {
            frame.render_widget(Paragraph::new("（无 Step 数据）"), area);
            return;
        }
        if self.selected >= total_lines { self.selected = total_lines.saturating_sub(1); }
        if self.selected < self.scroll { self.scroll = self.selected; }
        if self.selected >= self.scroll + max_visible {
            self.scroll = self.selected.saturating_sub(max_visible).saturating_add(1);
        }

        let visible_lines: Vec<Line> = lines.iter().enumerate().skip(self.scroll).take(max_visible).map(|(i, line)| {
            let is_selected = i == self.selected;
            match line {
                HomeLine::PhaseHeader { phase, count, expanded } => {
                    let symbol = if *expanded { "▼" } else { "▶" };
                    let style = if is_selected { Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD) }
                                 else { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) };
                    Line::from(Span::styled(format!(" {}  {} 段（{} Step）", symbol, phase.name(), count), style))
                }
                HomeLine::Step(step) => {
                    let status_style = match step.status {
                        StepStatus::Completed => Style::default().fg(Color::Green),
                        StepStatus::Error => Style::default().fg(Color::Red),
                        StepStatus::Skipped => Style::default().fg(Color::Gray),
                        StepStatus::Pending => Style::default().fg(Color::Yellow),
                    };
                    let elapsed = if step.elapsed_us >= 1000 { format!("{:.1}ms", step.elapsed_us as f64 / 1000.0) } else { format!("{}μs", step.elapsed_us) };
                    let error_info = step.error_summary.as_ref().map(|e| format!("  ⚠ {}", e)).unwrap_or_default();
                    let bg = if is_selected { Style::default().bg(Color::Blue).fg(Color::White) } else { Style::default() };
                    Line::from(vec![
                        Span::styled(format!(" {} ", step.status.symbol()), status_style),
                        Span::styled(format!(" {}  {}", step.name, elapsed), bg),
                        Span::styled(error_info, Style::default().fg(Color::Red)),
                    ])
                }
            }
        }).collect();

        let title = format!(" {} — {} Step ", self.title, session.trace.step_count());
        let widget = Paragraph::new(visible_lines)
            .block(Block::default().title(title).borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray)))
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_enter(&mut self, session: &mut LocalDebugSession, status: &mut String) {
        let lines = self.display_lines(session);
        if self.selected >= lines.len() { return; }
        match &lines[self.selected] {
            HomeLine::PhaseHeader { phase, expanded, .. } => {
                if *expanded { self.expanded_phases.retain(|p| p != phase); }
                else { self.expanded_phases.push(*phase); }
            }
            HomeLine::Step(step) => {
                *status = format!("Step: {}", step.name);
            }
        }
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}
