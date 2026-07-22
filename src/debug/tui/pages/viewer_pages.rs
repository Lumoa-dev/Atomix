//! 查看器页面 — Source View、Binary View、Disasm View、Regs/Mem
//!
//! 对应设计文档 §3.6（Source View）、§3.10（Binary View）、§3.11（IR/Disasm）、§3.12（Regs/Mem）

use crate::debug::session::{DebugSession, LocalDebugSession};
use crate::debug::tui::pages::Page;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

// ═══════════════════════════════════════════════════════════
// Source View 页面（§3.6）
// ═══════════════════════════════════════════════════════════

pub struct SourceViewPage {
    title: String,
    pub _scroll: usize,
    pub _selected_line: usize,
}

impl SourceViewPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Source View — 源码视图".to_string(),
            _scroll: 0,
            _selected_line: 0,
        }
    }
}

impl Page for SourceViewPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        if area.height < 3 {
            return;
        }

        let current_line = session
            .debug_map
            .as_ref()
            .and_then(|m| m.line_for_pc(session.vm.pc))
            .unwrap_or(1) as usize;
        let source_lines = &session.source_lines;
        let max_visible = (area.height as usize).saturating_sub(2);

        if source_lines.is_empty() {
            let widget = Paragraph::new(Line::from(Span::styled(
                "（未加载源文件）",
                Style::default().fg(Color::Gray),
            )));
            frame.render_widget(widget, area);
            return;
        }

        // 计算显示范围
        let start_line = current_line.saturating_sub(max_visible / 2).max(1);
        let end_line = (start_line + max_visible).min(source_lines.len() + 1);

        let mut display_lines = Vec::new();
        for lnum in start_line..end_line {
            let is_exec_line = Some(lnum as u32)
                == session
                    .debug_map
                    .as_ref()
                    .and_then(|m| m.line_for_pc(session.vm.pc));
            let has_breakpoint = session.breakpoints().iter().any(|bp| {
                if let crate::debug::session::BreakpointType::Line(line) = bp.bp_type {
                    line == lnum as u32
                } else {
                    false
                }
            });

            let gutter = if is_exec_line {
                "→".to_string()
            } else if has_breakpoint {
                "●".to_string()
            } else {
                " ".to_string()
            };

            let line_style = if is_exec_line {
                Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let text = if lnum <= source_lines.len() {
                source_lines[lnum - 1].clone()
            } else {
                String::new()
            };

            display_lines.push(Line::from(Span::styled(
                format!("{} {:>4} │ {}", gutter, lnum, text),
                line_style,
            )));
        }

        let file_name = session.source_path.as_deref().unwrap_or("untitled");
        let block = Block::default()
            .title(format!(" {} — {} ", self.title, file_name))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let widget = Paragraph::new(display_lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn on_key_shortcut(&mut self, session: &mut LocalDebugSession, key: char, status: &mut String) {
        match key {
            'b' => {
                // 在当前行设置断点
                if let Some(line) = session
                    .debug_map
                    .as_ref()
                    .and_then(|m| m.line_for_pc(session.vm.pc))
                {
                    session.set_breakpoint_line(line, None);
                    *status = format!("断点已设置于 line {}", line);
                }
            }
            _ => {}
        }
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ═══════════════════════════════════════════════════════════
// Binary View 页面（§3.10）
// ═══════════════════════════════════════════════════════════

pub struct BinaryViewPage {
    title: String,
    pub scroll: usize,
}

impl BinaryViewPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Binary View — 二进制视图".to_string(),
            scroll: 0,
        }
    }
}

impl Page for BinaryViewPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let text = &session.vm.text;
        let pc = session.vm.pc;
        let max_visible = (area.height as usize).saturating_sub(3);
        let max_instrs = text.len();
        let start = self.scroll.min(max_instrs.saturating_sub(max_visible));
        let end = (start + max_visible).min(max_instrs);

        let mut lines = vec![
            Line::from(Span::styled(
                " Offset   Hex(LE)        Binary                     ASCII",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
        ];

        for i in start..end {
            let instr = text[i];
            let is_pc = i == pc;
            let has_bp = session.breakpoints().iter().any(|bp| {
                if let crate::debug::session::BreakpointType::Pc(addr) = bp.bp_type {
                    addr == i
                } else {
                    false
                }
            });

            let marker = if is_pc {
                "→"
            } else if has_bp {
                "●"
            } else {
                " "
            };
            let style = if is_pc {
                Style::default()
                    .fg(Color::Green)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else if has_bp {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::White)
            };

            let hex = format!("{:08x}", instr);
            let bytes = instr.to_le_bytes();
            let binary: String = bytes
                .iter()
                .map(|b| format!("{:08b}", b))
                .collect::<Vec<_>>()
                .join(" ");
            let ascii: String = bytes
                .iter()
                .map(|b| {
                    if b.is_ascii_graphic() || *b == b' ' {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();

            lines.push(Line::from(Span::styled(
                format!(
                    "{} {:06x}:  {}  {:35}  {}",
                    marker,
                    i * 4,
                    hex,
                    binary,
                    ascii
                ),
                style,
            )));
        }

        // 底部显示段大小统计
        let text_size = text.len() * 4;
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            format!(".text: {} 条指令 ({} 字节)", text.len(), text_size),
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .title(format!(" {} — PC={:#06x} ", self.title, pc))
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
// Disasm View 页面（§3.11）
// ═══════════════════════════════════════════════════════════

pub struct DisasmViewPage {
    title: String,
    pub _scroll: usize,
}

impl DisasmViewPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Disassembly — 反汇编视图".to_string(),
            _scroll: 0,
        }
    }

    fn opcode_color(op: u8) -> Color {
        match op {
            0x00..=0x0F => Color::Red,     // SYSTEM
            0x10..=0x1F => Color::Magenta, // MEM
            0x20..=0x38 => Color::Blue,    // ARITH
            0x40..=0x45 => Color::Blue,    // CMP
            0x50..=0x55 => Color::Yellow,  // CTRL
            0x60..=0x63 => Color::Yellow,  // TASK
            0x70 => Color::Red,            // ECALL
            _ => Color::White,
        }
    }
}

impl Page for DisasmViewPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        let text = &session.vm.text;
        let pc = session.vm.pc;
        let max_visible = (area.height as usize).saturating_sub(2);
        let max_instrs = text.len();

        let start = pc.saturating_sub(max_visible / 2);
        let start = start.min(max_instrs.saturating_sub(max_visible));
        let end = (start + max_visible).min(max_instrs);

        let mut lines = Vec::new();
        for i in start..end {
            let instr = text[i];
            let op = (instr >> 24) as u8;
            let is_pc = i == pc;

            let formatted = crate::debug::disassemble::format_instruction(i, instr);
            let marker = if is_pc { "→" } else { " " };
            let op_style = Style::default()
                .fg(Self::opcode_color(op))
                .add_modifier(if is_pc {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                });
            let bg_style = if is_pc {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            lines.push(Line::from(Span::styled(
                format!("{} {}", marker, formatted),
                op_style.patch(bg_style),
            )));
        }

        let table = crate::runner::decode::dispatch_table();
        let op_info = if pc < text.len() {
            let op = (text[pc] >> 24) as u8;
            let entry = &table[op as usize];
            format!("opcode={:#04x} ({})", op, entry.name)
        } else {
            "pc 越界".to_string()
        };

        let block = Block::default()
            .title(format!(" {} — PC={:#06x} {} ", self.title, pc, op_info))
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
// Regs/Mem 页面（§3.12）
// ═══════════════════════════════════════════════════════════

pub struct RegsMemPage {
    title: String,
    pub focus_regs: bool,
    pub mem_start: u64,
}

impl RegsMemPage {
    pub fn new(_session: &LocalDebugSession) -> Self {
        Self {
            title: "Registers & Memory — 寄存器与内存".to_string(),
            focus_regs: true,
            mem_start: 0,
        }
    }

}

impl Page for RegsMemPage {
    fn title(&self) -> &str {
        &self.title
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession) {
        if area.height < 5 {
            return;
        }

        let half = area.height / 2;

        // 上半部分：寄存器
        let reg_lines: Vec<Line> = (0..16)
            .map(|i| {
                let name = crate::base::isa::reg_name(i).to_uppercase();
                let val = session.vm.read_reg(i);
                let is_focused = self.focus_regs;
                let style = if is_focused {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::White)
                };
                Line::from(Span::styled(
                    format!("  {:>8}(R{:>2}): {:#018x}  ({})", name, i, val, val as i64),
                    style,
                ))
            })
            .collect();

        let reg_block = Block::default()
            .title(" Registers ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        let reg_widget = Paragraph::new(reg_lines).block(reg_block);
        frame.render_widget(
            reg_widget,
            Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: half,
            },
        );

        // 下半部分：内存 hex dump
        let start_addr = self.mem_start;
        let bytes_per_line = 16;
        let max_lines = ((area.height - half) as usize).saturating_sub(2);
        let memory = &session.vm.memory;

        let mut mem_lines = Vec::new();
        for line_idx in 0..max_lines {
            let addr = start_addr.wrapping_add((line_idx * bytes_per_line) as u64);
            let mut hex = String::new();
            let mut ascii = String::new();
            for byte_idx in 0..bytes_per_line {
                let a = addr.wrapping_add(byte_idx as u64);
                if let Some(byte) = memory.read_u8(a) {
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
                format!("{:#010x}:  {:48}  {}", addr, hex, ascii),
                if !self.focus_regs {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::White)
                },
            )));
        }

        let mem_block = Block::default()
            .title(" Memory ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        let mem_widget = Paragraph::new(mem_lines).block(mem_block);
        frame.render_widget(
            mem_widget,
            Rect {
                x: area.x,
                y: area.y + half,
                width: area.width,
                height: area.height - half,
            },
        );
    }

    fn on_key_shortcut(
        &mut self,
        _session: &mut LocalDebugSession,
        _key: char,
        _status: &mut String,
    ) {
        match _key {
            '\t' => self.focus_regs = !self.focus_regs,
            _ => {}
        }
    }

    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}
