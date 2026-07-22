//! 查看器页面 — Source View、Binary View、Disasm View、Regs/Mem
//!
//! 对应设计文档 §3.6（Source View）、§3.10（Binary View）、§3.11（IR/Disasm）、§3.12（Regs/Mem）

use crate::base::isa;
use crate::debug::session::{DebugSession, LocalDebugSession};
use crate::debug::tui::pages::Page;
use crate::runner::decode;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Syntax-highlight a single source line into colored spans.
///
/// Handles keywords (blue bold), string literals (green),
/// type annotations (yellow dim), and comments (dark gray italic).
fn highlight_source(line: &str) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        return vec![Span::styled(
            line.to_string(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )];
    }

    const KEYWORDS: &[&str] = &[
        "let", "fn", "CALL", "if", "else", "return", "for", "while", "true", "false", "TOOLS",
        "INPUT", "TASK", "OUT", "WORKS", "ZONE", "IMPORT", "WAIT", "TRY", "HOOK",
    ];

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0;
    let bytes = line.as_bytes();
    let len = line.len();

    while pos < len {
        // ── String literal ──
        if bytes[pos] == b'"' {
            let start = pos;
            pos += 1;
            while pos < len && bytes[pos] != b'"' {
                pos += 1;
            }
            if pos < len {
                pos += 1; // skip closing quote
            }
            spans.push(Span::styled(
                line[start..pos].to_string(),
                Style::default().fg(Color::Green),
            ));
            continue;
        }

        // ── Word characters (alphanumeric / underscore) ──
        if bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_' {
            let start = pos;
            while pos < len
                && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_')
            {
                pos += 1;
            }
            let word = &line[start..pos];
            if KEYWORDS.contains(&word) {
                spans.push(Span::styled(
                    word.to_string(),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(word.to_string()));
            }
            continue;
        }

        // ── Colon — check for type annotation ──
        if bytes[pos] == b':' {
            let start = pos;
            pos += 1;
            // skip whitespace between `:` and type name
            while pos < len && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            let type_start = pos;
            while pos < len
                && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_')
            {
                pos += 1;
            }
            let type_word = &line[type_start..pos];
            if ["int", "float", "bool", "string"].contains(&type_word) {
                spans.push(Span::styled(
                    line[start..pos].to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::DIM),
                ));
            } else {
                spans.push(Span::raw(line[start..pos].to_string()));
            }
            continue;
        }

        // ── Other non-word character (whitespace, punctuation) ──
        let start = pos;
        pos += 1;
        // group consecutive non-word, non-colon, non-quote chars
        while pos < len
            && !bytes[pos].is_ascii_alphanumeric()
            && bytes[pos] != b'_'
            && bytes[pos] != b'"'
            && bytes[pos] != b':'
        {
            pos += 1;
        }
        spans.push(Span::raw(line[start..pos].to_string()));
    }

    spans
}

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

            let gutter_span = Span::styled(
                format!("{} {:>4} │ ", gutter, lnum),
                line_style,
            );
            let text_spans = highlight_source(&text);
            let text_spans: Vec<Span> = if is_exec_line {
                text_spans
                    .into_iter()
                    .map(|s| {
                        let patched = s.style.patch(Style::default().bg(Color::DarkGray));
                        Span {
                            content: s.content,
                            style: patched,
                        }
                    })
                    .collect()
            } else {
                text_spans
            };
            let mut line_spans = vec![gutter_span];
            line_spans.extend(text_spans);
            display_lines.push(Line::from(line_spans));
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

/// Format only the operand portion of an instruction (no PC prefix).
fn format_operands_only(pc: usize, instr: u32) -> String {
    let opcode = (instr >> 24) as u8;
    let table = decode::dispatch_table();
    let entry = &table[opcode as usize];
    let ops = decode::decode(instr, entry.enc);
    let mnemonic = entry.name;
    use isa::EncTemplate;

    let reg = |r: u8| -> String {
        let name = isa::reg_name(r as usize);
        if name == "?" {
            format!("R{}", r)
        } else {
            name.to_uppercase()
        }
    };

    match entry.enc {
        EncTemplate::R3 => {
            format!("{}, {}, {}", reg(ops.rd), reg(ops.rs1), reg(ops.rs2))
        }
        EncTemplate::R2I => match mnemonic {
            "MOVI" => format!("{}, {}", reg(ops.rd), ops.imm as i16),
            "LOAD" => format!("{}, [{}+{}]", reg(ops.rd), reg(ops.rs1), ops.imm as i16),
            "STORE" => format!("[{}+{}], {}", reg(ops.rd), ops.imm as i16, reg(ops.rs1)),
            _ => format!("{}, {}, {}", reg(ops.rd), reg(ops.rs1), ops.imm as i16),
        },
        EncTemplate::R1I => match mnemonic {
            "TRAP" => format!("{}", ops.imm as i16),
            "ECALL" => {
                let sname = crate::debug::disassemble::syscall_name(ops.imm);
                format!("{}, {} ; {}", ops.rd, ops.imm, sname)
            }
            "TASK_FORK" | "TASK_JOIN" | "TASK_RET" | "TASK_SELF" => {
                format!("{}, {}", reg(ops.rd), ops.imm as i16)
            }
            _ => format!("{}, {}", reg(ops.rd), ops.imm as i16),
        },
        EncTemplate::JI => {
            if mnemonic == "illegal" {
                format!("; illegal instr {:#010x}", instr)
            } else {
                let offset = ops.imm as i32;
                let target = (pc as i32).wrapping_add(offset);
                format!("{:#x}", target)
            }
        }
    }
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
        let max_visible = (area.height as usize).saturating_sub(3);
        let max_instrs = text.len();

        let start = pc.saturating_sub(max_visible / 2);
        let start = start.min(max_instrs.saturating_sub(max_visible));
        let end = (start + max_visible).min(max_instrs);

        let table = decode::dispatch_table();

        // ── Column header ──
        let header_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let header = Line::from(vec![
            Span::styled(format!("{:<7}", "PC"), header_style),
            Span::raw("| "),
            Span::styled(format!("{:<9}", "Bytes(LE)"), header_style),
            Span::raw("| "),
            Span::styled(format!("{:<8}", "Opcode"), header_style),
            Span::raw("| "),
            Span::styled("Operands", header_style),
            Span::raw(" | Source Comment"),
        ]);

        let mut lines = vec![header, Line::from(Span::raw(""))];

        for i in start..end {
            let instr = text[i];
            let op = (instr >> 24) as u8;
            let is_pc = i == pc;
            let entry = &table[op as usize];

            let marker_style = if is_pc {
                Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            // PC column — 6 hex digits right-aligned
            let pc_str = format!("{:06x}", i);

            // Bytes column — u32 as LE hex (8 hex digits)
            let bytes_str = format!("{:08x}", instr);

            // Opcode column — colored mnemonic
            let op_style = Style::default().fg(Self::opcode_color(op));
            let mnemonic_str = entry.name;

            // Operands column
            let ops_str = format_operands_only(i, instr);

            // Source comment from debug_map
            let src_line = session
                .debug_map
                .as_ref()
                .and_then(|m| m.line_for_pc(i));
            let comment = src_line.and_then(|ln| {
                let idx = ln as usize;
                if idx > 0 && idx <= session.source_lines.len() {
                    let text = session.source_lines[idx - 1].trim().to_string();
                    if text.is_empty() { None } else { Some(text) }
                } else {
                    None
                }
            });

            let pipe = Span::raw(" | ");

            // Determine if this row should have a dark gray background
            let row_style = if is_pc {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            // Build row as a Vec<Span> then wrap in Line with row_style
            let mut row_parts: Vec<Span> = Vec::new();

            // PC
            row_parts.push(Span::styled(
                format!("{:<7}", pc_str),
                marker_style,
            ));
            row_parts.push(pipe.clone());

            // Bytes
            row_parts.push(Span::styled(
                format!("{:<9}", bytes_str),
                Style::default().fg(Color::Magenta),
            ));
            row_parts.push(pipe.clone());

            // Opcode (colored)
            row_parts.push(Span::styled(
                format!("{:<8}", mnemonic_str),
                op_style,
            ));
            row_parts.push(pipe.clone());

            // Operands
            row_parts.push(Span::raw(ops_str));

            // Source comment
            if let Some(comment_text) = comment {
                row_parts.push(Span::styled(
                    format!("  ; {}", comment_text),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            lines.push(Line::from(row_parts).style(row_style));
        }

        let op_info = if pc < text.len() {
            let opcode_val = (text[pc] >> 24) as u8;
            let entry = &table[opcode_val as usize];
            format!("opcode={:#04x} ({})", opcode_val, entry.name)
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
