//! 详情页面 — Step Detail、Input Detail、Output Detail、Exception Detail
//!
//! 对应设计文档 §3.2（Step Detail）、§3.8（INPUT Detail）、§3.9（OUT Detail）、§3.13（Exception Detail）

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
// Step Detail 页面（§3.2）
// ═══════════════════════════════════════════════════════════

pub struct StepDetailPage {
    title: String,
    pub step_index: usize,
    pub scroll: usize,
}

impl StepDetailPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Step Detail — Step 详情".to_string(),
            step_index: 0,
            scroll: 0,
        }
    }
}

impl Page for StepDetailPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let trace = &session.trace;
        let max_visible = (area.height as usize).saturating_sub(2);

        let step = if self.step_index < trace.steps.len() {
            Some(&trace.steps[self.step_index])
        } else {
            None
        };

        let mut lines = Vec::new();

        if let Some(step) = step {
            // 顶部：CALL 语句和行号
            let status_symbol = step.status.symbol();
            let status_color = match step.status {
                StepStatus::Completed => Color::Green,
                StepStatus::Error => Color::Red,
                StepStatus::Skipped => Color::Gray,
                StepStatus::Pending => Color::Yellow,
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", status_symbol),
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}", step.name),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  line {}", step.source_line),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::from(Span::raw("")));

            // 执行信息
            let elapsed = if step.elapsed_us >= 1000 {
                format!("{:.2}ms", step.elapsed_us as f64 / 1000.0)
            } else {
                format!("{}μs", step.elapsed_us)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "  执行耗时: {}  |  PC范围: {:#06x}–{:#06x}",
                    elapsed, step.pc_range.0, step.pc_range.1
                ),
                Style::default().fg(Color::White),
            )));

            // 输入参数列表
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                "  输入参数",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            if step.input_vars.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    （无）",
                    Style::default().fg(Color::Gray),
                )));
            } else {
                for var in &step.input_vars {
                    lines.push(Line::from(Span::raw(format!("    {}", var))));
                }
            }

            // 输出变量列表
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                "  输出变量",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            if step.output_vars.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    （无）",
                    Style::default().fg(Color::Gray),
                )));
            } else {
                for var in &step.output_vars {
                    lines.push(Line::from(Span::raw(format!("    {}", var))));
                }
            }

            // 子调用列表
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                "  子调用",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            if step.sub_calls.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    （无）",
                    Style::default().fg(Color::Gray),
                )));
            } else {
                for call in &step.sub_calls {
                    match call {
                        crate::debug::trace::SubCall::FnCall {
                            name,
                            args,
                            result,
                            elapsed_us,
                        } => {
                            let elapsed = if *elapsed_us >= 1000 {
                                format!("{:.1}ms", *elapsed_us as f64 / 1000.0)
                            } else {
                                format!("{}μs", elapsed_us)
                            };
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "    fn: {}({}) → {:?} [{}]",
                                    name,
                                    args.join(", "),
                                    result,
                                    elapsed
                                ),
                                Style::default().fg(Color::Blue),
                            )));
                        }
                        crate::debug::trace::SubCall::WorksCall {
                            name,
                            lifecycle,
                            elapsed_us,
                            result,
                        } => {
                            let lifecycle_str: Vec<&str> =
                                lifecycle.iter().map(|p| p.symbol()).collect();
                            let elapsed = if *elapsed_us >= 1000 {
                                format!("{:.1}ms", *elapsed_us as f64 / 1000.0)
                            } else {
                                format!("{}μs", elapsed_us)
                            };
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "    works: {} [{}] → {:?} [{}]",
                                    name,
                                    lifecycle_str.join(" → "),
                                    result,
                                    elapsed
                                ),
                                Style::default().fg(Color::Magenta),
                            )));
                        }
                    }
                }
            }

            // 错误信息
            if let Some(ref err) = step.error_summary {
                lines.push(Line::from(Span::raw("")));
                lines.push(Line::from(Span::styled(
                    format!("  ⚠ 错误: {}", err),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )));
            }

            // 底部源码片段（当前行附带）
            if let Some(ref map) = session.debug_map {
                if let Some(line) = map.line_for_pc(step.pc_range.0) {
                    let src_lines = &session.source_lines;
                    if (line as usize) <= src_lines.len() {
                        lines.push(Line::from(Span::raw("")));
                        lines.push(Line::from(Span::styled(
                            format!("  {} │ {}", line, src_lines[line as usize - 1].trim()),
                            Style::default().fg(Color::Yellow),
                        )));
                    }
                }
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  （无 Step 数据）",
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
// INPUT Detail 页面（§3.8）
// ═══════════════════════════════════════════════════════════

pub struct InputDetailPage {
    title: String,
}

impl InputDetailPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "INPUT Detail — 输入数据源".to_string(),
        }
    }
}

impl Page for InputDetailPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let trace = &session.trace;
        let input_steps = trace.steps_by_phase(ExecutionPhase::Input);

        let mut lines = vec![
            Line::from(Span::styled(
                " 输入常量",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                format!(
                    "  {:<20} {:<10} {:<10} {}",
                    "名称", "类型", "状态", "消费者"
                ),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  ─────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        if input_steps.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无输入 Step）",
                Style::default().fg(Color::Gray),
            )));
        } else {
            for step in input_steps {
                let status_str = step.status.symbol();
                let consumers = trace
                    .steps
                    .iter()
                    .filter(|s| s.phase == ExecutionPhase::Task)
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(Line::from(Span::raw(format!(
                    "  {:<20} {:<10} {:<10} {}",
                    step.name, "—", status_str, consumers,
                ))));
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
// OUT Detail 页面（§3.9）
// ═══════════════════════════════════════════════════════════

pub struct OutputDetailPage {
    title: String,
}

impl OutputDetailPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "OUT Detail — 产出交付".to_string(),
        }
    }
}

impl Page for OutputDetailPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let trace = &session.trace;
        let out_steps = trace.steps_by_phase(ExecutionPhase::Out);

        let mut lines = vec![
            Line::from(Span::styled(
                " 产出变量",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                format!(
                    "  {:<20} {:<10} {:<12} {}",
                    "名称", "类型", "交付状态", "目标"
                ),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  ─────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        if out_steps.is_empty() {
            lines.push(Line::from(Span::styled(
                "  （无产出 Step）",
                Style::default().fg(Color::Gray),
            )));
        } else {
            for step in out_steps {
                let deliver_status = match step.status {
                    StepStatus::Completed => "✓ 已交付",
                    StepStatus::Error => "✗ 失败",
                    StepStatus::Skipped => "— 未执行",
                    StepStatus::Pending => "· 待定",
                };
                lines.push(Line::from(Span::raw(format!(
                    "  {:<20} {:<10} {:<12} {}",
                    step.name, "—", deliver_status, "output",
                ))));
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
// Exception Detail 页面（§3.13）
// ═══════════════════════════════════════════════════════════

pub struct ExceptionDetailPage {
    title: String,
}

impl ExceptionDetailPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Exception Detail — 异常详情".to_string(),
        }
    }
}

impl Page for ExceptionDetailPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let detail = session.exception_detail();

        let mut lines = vec![
            Line::from(Span::styled(
                " 异常上下文",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
        ];

        if let Some(ref exc) = detail {
            lines.push(Line::from(Span::styled(
                format!("  异常类型: {}", exc.error_type),
                Style::default().fg(Color::Red),
            )));
            lines.push(Line::from(Span::styled(
                format!("  错误码: {}", exc.error_code),
                Style::default().fg(Color::Red),
            )));
            lines.push(Line::from(Span::styled(
                format!("  错误消息: {}", exc.error_message),
                Style::default().fg(Color::Yellow),
            )));

            if let Some(line) = exc.source_line {
                lines.push(Line::from(Span::raw(format!("  源位置: 行 {}", line))));
            }
            lines.push(Line::from(Span::raw(format!(
                "  PC: {:#06x}",
                exc.source_pc
            ))));
            lines.push(Line::from(Span::raw(format!(
                "  调用栈深度: {}",
                exc.call_stack_depth
            ))));

            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                "  传播信息",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            let propagated = if exc.is_propagated { "是" } else { "否" };
            let caught = if exc.is_caught { "是" } else { "否" };
            lines.push(Line::from(Span::raw(format!(
                "  是否向上传播: {}",
                propagated
            ))));
            lines.push(Line::from(Span::raw(format!(
                "  是否被 TRY 块捕获: {}",
                caught
            ))));

            // 异常时刻的寄存器快照
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                "  异常时刻的寄存器快照",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for i in 0..16 {
                let name = crate::base::isa::reg_name(i).to_uppercase();
                let val = session.vm.read_reg(i);
                lines.push(Line::from(Span::raw(format!(
                    "    {:>8}: {:#018x}",
                    name, val,
                ))));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  （当前无异常）",
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
