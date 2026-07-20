//! 调试 REPL — 交互式调试会话。
//!
//! 提供一个命令行界面，用户可以通过命令控制 VM 执行、检查状态。
//!
//! # 命令
//!
//! | 命令      | 格式              | 说明                     |
//! |-----------|-------------------|--------------------------|
//! | `step`    | `step [n]`        | 单步执行 n 条指令        |
//! | `regs`    | `regs`            | 显示所有寄存器           |
//! | `disasm`  | `disasm [addr] [n]`| 反汇编指定位置           |
//! | `mem`     | `mem <addr> [bytes]`| hexdump 内存            |
//! | `break`   | `break <addr>`    | 设置断点                 |
//! | `continue`| `continue`        | 运行到断点或结束         |
//! | `print`   | `print <reg>`     | 打印单个寄存器           |
//! | `help`    | `help`            | 显示帮助                 |
//! | `quit`    | `quit`            | 退出调试器               |

use crate::base::isa::{self, opcode};
use crate::debug::disassemble;
use crate::runner::execute::execute_instruction;
use crate::runner::VmState;
use crate::runner::VmStateKind;
use std::collections::HashMap;

/// 调试会话。
pub struct DebugSession {
    /// 当前调试的 VM。
    pub vm: VmState,
    /// 断点表：地址 → 原始指令字。
    pub breakpoints: HashMap<usize, u32>,
    /// 条件断点：地址 → 条件表达式（空字符串 = 无条件）。
    pub conditional_breakpoints: HashMap<usize, String>,
    /// .debug 段解析后的行号映射。
    pub debug_map: Option<crate::debug::debug_segment::DebugMap>,
    /// 源文件路径（用于源码视图）。
    pub source_path: Option<String>,
    /// 缓存的源文件内容。
    source_lines: Vec<String>,
}

impl DebugSession {
    /// 从已加载的 VmState 创建调试会话。
    pub fn new(vm: VmState) -> Self {
        Self {
            vm,
            breakpoints: HashMap::new(),
            conditional_breakpoints: HashMap::new(),
            debug_map: None,
            source_path: None,
            source_lines: Vec::new(),
        }
    }

    /// 加载 .debug 段（从 VmState 的 .atxe 数据恢复）。
    pub fn load_debug_info(&mut self) {
        if !self.vm.rodata.is_empty() {
            // rodata 之后是 .debug 段？实际上 .debug 段在 AtxeBinary 中单独存储
            // 但 VmState 不保留完整 AtxeBinary 结构，只保留 text/rodata/exn_table
            // .debug 信息需要额外传递
        }
    }

    /// 设置源文件路径并加载。
    pub fn set_source(&mut self, path: &str) {
        self.source_path = Some(path.to_string());
        if let Ok(content) = std::fs::read_to_string(path) {
            self.source_lines = content.lines().map(|l| l.to_string()).collect();
        }
    }

    /// 从 .debug 段字节加载映射。
    pub fn set_debug_map_from_bytes(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.debug_map = crate::debug::debug_segment::DebugMap::from_bytes(bytes);
        }
    }

    /// 执行一条命令。返回 true 继续，false 退出。
    pub fn execute_command(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return true;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let command = parts[0].to_lowercase();

        match command.as_str() {
            "quit" | "exit" | "q" => false,
            "help" | "h" | "?" => {
                self.cmd_help();
                true
            }
            "step" | "s" => {
        let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
        self.cmd_step(n);
        // 显示当前源码行
        self.show_current_source_line();
        true
            }
            "regs" | "registers" | "r" => {
                self.cmd_regs();
                true
            }
            "disasm" | "d" => {
                let addr: usize = parts
                    .get(1)
                    .and_then(|s| parse_addr(s))
                    .unwrap_or(self.vm.pc);
                let n: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
                self.cmd_disasm(addr, n);
                true
            }
            "mem" | "m" => {
                if let Some(addr) = parts.get(1).and_then(|s| parse_addr(s)) {
                    let bytes: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
                    self.cmd_mem(addr as u64, bytes);
                } else {
                    println!("用法: mem <addr> [bytes]");
                }
                true
            }
            "break" | "b" => {
                if let Some(addr) = parts.get(1).and_then(|s| parse_addr(s)) {
                    // 检查是否有 if 条件
                    let condition = if parts.len() > 2 && parts[2].to_lowercase() == "if" {
                        Some(parts[3..].join(" "))
                    } else {
                        None
                    };
                    self.cmd_break(addr, condition.as_deref());
                } else {
                    // 列出所有断点
                    self.list_breakpoints();
                }
                true
            }
            "continue" | "c" => {
                self.cmd_continue();
                true
            }
            "eval" | "e" => {
                let expr = parts[1..].join(" ");
                if !expr.is_empty() {
                    self.cmd_eval(&expr);
                } else {
                    println!("用法: eval <表达式>");
                }
                true
            }
            "print" | "p" => {
                let expr = parts[1..].join(" ");
                if expr.is_empty() {
                    println!("用法: print <寄存器|表达式>");
                } else {
                    self.cmd_print(&expr);
                }
                true
            }
            "source" | "src" => {
                let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
                self.cmd_source(n);
                true
            }
            "backtrace" | "bt" => {
                self.cmd_backtrace();
                true
            }
            _ => {
                println!("未知命令: {}（输入 help 查看可用命令）", command);
                true
            }
        }
    }

    // ── 命令实现 ──────────────────────────────────────

    fn cmd_help(&self) {
        println!("Atomix 调试器命令:");
        println!("  step [n]    单步执行 n 条指令（默认 1）");
        println!("  regs        显示所有 16 个寄存器");
        println!("  disasm [addr] [n]  反汇编 n 条指令（默认 PC, 8 条）");
        println!("  mem <addr> [bytes]  hexdump 内存");
        println!("  break <addr>        设置断点（无参 = 列出断点）");
        println!("  break <addr> if <expr>  设置条件断点");
        println!("  continue    运行到断点或结束");
        println!("  eval <expr>  计算表达式（如 a0 + 42, *0x1000）");
        println!("  print <expr> 打印表达式值（寄存器/表达式）");
        println!("  backtrace   显示调用栈回溯");
        println!("  source [n]  显示当前 PC 附近的 n 行源码");
        println!("  help        显示此帮助");
        println!("  quit        退出调试器");
    }

    fn cmd_step(&mut self, n: usize) {
        let start_pc = self.vm.pc;
        for _ in 0..n {
            if !self.vm.is_running() {
                println!("VM 已停止（状态: {:?}）", self.vm.state);
                return;
            }
            execute_instruction(&mut self.vm);
        }
        // 显示当前 PC 的指令
        if self.vm.pc < self.vm.text.len() && self.vm.is_running() {
            let s = disassemble::format_instruction(self.vm.pc, self.vm.text[self.vm.pc]);
            println!("→ {}", s);
        } else if self.vm.pc < self.vm.text.len() {
            let s = disassemble::format_instruction(self.vm.pc, self.vm.text[self.vm.pc]);
            println!("⏹ {}", s);
        }
        // 每步的指令数摘要
        let executed = (self.vm.pc as i64 - start_pc as i64).unsigned_abs();
        if n > 1 {
            println!("（执行了 {} 条指令）", executed);
        }
    }

    fn cmd_regs(&self) {
        println!("寄存器:");
        for i in 0..isa::REG_COUNT {
            let name = isa::reg_name(i).to_uppercase();
            let val = self.vm.read_reg(i);
            // 带符号和十六进制显示
            println!("  {:>8}(R{:>2}): {:#018x}  ({})", name, i, val, val as i64);
        }
        println!("  {:>8}: {:#06x}", "PC", self.vm.pc);
        println!("  状态: {:?}", self.vm.state);
    }

    fn cmd_disasm(&self, addr: usize, n: usize) {
        if addr >= self.vm.text.len() {
            println!("地址 {:#06x} 超出 .text 段（长度 {})", addr, self.vm.text.len());
            return;
        }
        let lines = disassemble::disassemble_range(&self.vm.text, addr, n);
        for line in &lines {
            println!("{}", line);
        }
    }

    fn cmd_mem(&self, addr: u64, bytes: usize) {
        let end = addr.saturating_add(bytes as u64);
        if end as usize > self.vm.memory.data.len() {
            println!("地址范围超出沙箱内存（大小 {}）", self.vm.memory.data.len());
            return;
        }
        // hexdump: 每行 16 字节
        let mut offset = addr;
        while offset < end {
            let line_end = (offset + 16).min(end);
            let mut hex = String::new();
            let mut ascii = String::new();
            for a in offset..line_end {
                if let Some(byte) = self.vm.memory.read_u8(a) {
                    hex.push_str(&format!("{:02x} ", byte));
                    if byte.is_ascii_graphic() || byte == b' ' {
                        ascii.push(byte as char);
                    } else {
                        ascii.push('.');
                    }
                }
            }
            println!("{:#010x}:  {:48}  {}", offset, hex, ascii);
            offset = line_end;
        }
    }

    fn cmd_break(&mut self, addr: usize, condition: Option<&str>) {
        if addr >= self.vm.text.len() {
            println!("地址 {:#06x} 超出 .text 段", addr);
            return;
        }
        if self.breakpoints.contains_key(&addr) {
            // 如果已有断点，可能只是更新条件
            if let Some(cond) = condition {
                self.conditional_breakpoints.insert(addr, cond.to_string());
                println!("条件断点已更新于 {:#06x}: if {}", addr, cond);
            } else {
                println!("断点已存在于 {:#06x}", addr);
            }
            return;
        }
        // 保存原指令，写入 TRAP
        let original = self.vm.text[addr];
        self.breakpoints.insert(addr, original);
        if let Some(cond) = condition {
            self.conditional_breakpoints.insert(addr, cond.to_string());
        }
        self.vm.text[addr] = isa::encode_ji(opcode::TRAP, 0);
        match condition {
            Some(cond) => println!("条件断点已设置于 {:#06x}: if {}", addr, cond),
            None => println!("断点已设置于 {:#06x}", addr),
        }
    }

    fn list_breakpoints(&self) {
        if self.breakpoints.is_empty() {
            println!("（无断点）");
            return;
        }
        println!("断点列表:");
        for (&addr, _orig) in &self.breakpoints {
            let cond = self.conditional_breakpoints.get(&addr)
                .map(|c| format!(" if {}", c))
                .unwrap_or_default();
            let instr_desc = if addr < self.vm.text.len() {
                let s = disassemble::format_instruction(addr, self.vm.text[addr]);
                format!("  ({})", s)
            } else {
                String::new()
            };
            println!("  {:#06x}{}{}", addr, cond, instr_desc);
        }
    }

    fn cmd_continue(&mut self) {
        let max_steps = 1_000_000; // 防止无限循环
        let mut steps = 0;
        while self.vm.is_running() && steps < max_steps {
            let pc_before = self.vm.pc;
            execute_instruction(&mut self.vm);
            steps += 1;

            // 检查是否命中 TRAP（可能是断点或 halt）
            if !self.vm.is_running() || self.vm.state == VmStateKind::Halted {
                if self.breakpoints.contains_key(&pc_before) {
                    // 检查条件断点：如果条件不满足，跳过恢复的单步后继续执行
                    let mut condition_skipped = false;

                    if let Some(cond) = self.conditional_breakpoints.get(&pc_before) {
                        if !cond.is_empty() {
                            match crate::debug::eval::eval_expr(cond, &self.vm) {
                                Ok(val) if val == 0 => {
                                    // 条件不满足：恢复指令，单步，重新设断点，继续
                                    if let Some(&orig) = self.breakpoints.get(&pc_before) {
                                        self.vm.text[pc_before] = orig;
                                        self.vm.pc = pc_before;
                                        self.vm.state = VmStateKind::Running; // 恢复状态
                                        execute_instruction(&mut self.vm);
                                        self.vm.text[pc_before] = isa::encode_ji(opcode::TRAP, 0);
                                    }
                                    condition_skipped = true;
                                }
                                Ok(_) => {} // 条件满足，停
                                Err(e) => {
                                    println!("⚠ 条件求值错误 ({}): 继续执行", e);
                                }
                            }
                        }
                    }

                    if condition_skipped {
                        continue; // 继续执行主循环
                    }

                    // 命中断点：先把状态恢复为 Running 再单步
                    println!("⏸ 命中断点于 {:#06x}", pc_before);
                    if let Some(&orig) = self.breakpoints.get(&pc_before) {
                        self.vm.text[pc_before] = orig;
                        self.vm.pc = pc_before;
                        self.vm.state = VmStateKind::Running; // 恢复状态
                        execute_instruction(&mut self.vm);
                        self.vm.text[pc_before] = isa::encode_ji(opcode::TRAP, 0);
                        if self.vm.pc < self.vm.text.len() && self.vm.is_running() {
                            let s = disassemble::format_instruction(
                                self.vm.pc,
                                self.vm.text[self.vm.pc],
                            );
                            println!("→ {}", s);
                        }
                    }
                    return;
                }
                // 正常 halt
                if self.vm.state == VmStateKind::Halted {
                    println!("⏹ VM 已停止（执行了 {} 条指令）", steps);
                    return;
                }
                if matches!(self.vm.state, VmStateKind::Error(_)) {
                    println!("⛔ VM 错误: {:?}", self.vm.state);
                    return;
                }
                if self.vm.state == VmStateKind::Suspended {
                    println!("⏸ VM 已挂起（执行了 {} 条指令）", steps);
                    return;
                }
            }
        }
        if steps >= max_steps {
            println!("⚠ 达到最大步数限制 {}", max_steps);
        }
    }

    fn cmd_eval(&self, expr: &str) {
        match crate::debug::eval::eval_expr(expr, &self.vm) {
            Ok(val) => {
                let formatted = crate::debug::eval::format_result(val);
                println!("  {} = {}", expr, formatted);
            }
            Err(e) => {
                println!("表达式错误: {}", e);
            }
        }
    }

    fn cmd_print(&self, name: &str) {
        // 先尝试作为表达式求值
        match crate::debug::eval::eval_expr(name, &self.vm) {
            Ok(val) => {
                let formatted = crate::debug::eval::format_result(val);
                println!("  {} = {}", name, formatted);
                return;
            }
            Err(_) => {
                // 降级到旧的寄存器名解析
            }
        }

        let idx = match name.to_lowercase().as_str() {
            "zero" | "r0" => 0,
            "sp" | "r1" => 1,
            "fp" | "r2" => 2,
            "ra" | "r3" => 3,
            "a0" | "r4" => 4,
            "a1" | "r5" => 5,
            "a2" | "r6" => 6,
            "a3" | "r7" => 7,
            "t0" | "r8" => 8,
            "t1" | "r9" => 9,
            "t2" | "r10" => 10,
            "t3" | "r11" => 11,
            "t4" | "r12" => 12,
            "t5" | "r13" => 13,
            "task_id" | "r14" => 14,
            "tmp" | "r15" => 15,
            "pc" => {
                println!("  PC = {:#06x}", self.vm.pc);
                return;
            }
            _ => {
                if let Ok(n) = name.parse::<usize>() && n < 16 {
                    let val = self.vm.read_reg(n);
                    let n2 = isa::reg_name(n).to_uppercase();
                    println!("  {}(R{}): {:#018x} ({})", n2, n, val, val as i64);
                    return;
                }
                println!("未知寄存器: {}（可用: zero, sp, fp, ra, a0-a3, t0-t5, task_id, tmp, pc）", name);
                return;
            }
        };
        let val = self.vm.read_reg(idx);
        let name_upper = isa::reg_name(idx).to_uppercase();
        println!("  {}(R{}): {:#018x} ({})", name_upper, idx, val, val as i64);
    }

    fn cmd_backtrace(&self) {
        if self.vm.call_stack.is_empty() {
            println!("调用栈为空");
            return;
        }
        println!("调用栈（共 {} 帧）:", self.vm.call_stack.len());
        for (i, frame) in self.vm.call_stack.iter().enumerate() {
            // 显示返回地址处的指令
            let instr_desc = if frame.return_pc < self.vm.text.len() {
                let s = disassemble::format_instruction(frame.return_pc, self.vm.text[frame.return_pc]);
                format!(" (→ {})", s)
            } else {
                String::new()
            };
            println!("  #{}  return_pc={:#06x}{}", i, frame.return_pc, instr_desc);
        }
        // 当前帧
        println!("  #{}  current pc={:#06x}", self.vm.call_stack.len(), self.vm.pc);
        if self.vm.pc < self.vm.text.len() {
            let s = disassemble::format_instruction(self.vm.pc, self.vm.text[self.vm.pc]);
            println!("       ({})", s);
        }
    }

    fn cmd_source(&self, n: usize) {
        // 查找当前 PC 对应的源码行
        let line = self.debug_map.as_ref().and_then(|m| m.line_for_pc(self.vm.pc));
        let display_line = line.unwrap_or(1) as usize;

        if self.source_lines.is_empty() {
            println!("（未加载源文件）");
            return;
        }

        let start = display_line.saturating_sub(n / 2).max(1);
        let end = (start + n).min(self.source_lines.len() + 1);

        for lnum in start..end {
            let marker = if Some(lnum as u32) == line { "→" } else { " " };
            if lnum <= self.source_lines.len() {
                println!("{} {:>4} │ {}", marker, lnum, self.source_lines[lnum - 1]);
            }
        }
    }

    /// 在 step 后显示当前源码行（如果有 .debug 映射）。
    fn show_current_source_line(&self) {
        if let Some(ref map) = self.debug_map
            && let Some(line) = map.line_for_pc(self.vm.pc)
            && !self.source_lines.is_empty() && (line as usize) <= self.source_lines.len()
        {
            println!("  {:>4} │ {} (line {})",
                line,
                self.source_lines[line as usize - 1].trim(),
                line,
            );
        }
    }
}

/// 解析地址参数（支持 0x 前缀十六进制和十进制）。
fn parse_addr(s: &str) -> Option<usize> {
    if s.starts_with("0x") || s.starts_with("0X") {
        usize::from_str_radix(&s[2..], 16).ok()
    } else {
        s.parse().ok()
    }
}

/// 启动交互式调试 REPL。
///
/// 使用 rustyline 提供行编辑和历史记录支持。
pub fn run_repl(session: &mut DebugSession) {
    let mut rl = rustyline::Editor::<(), rustyline::history::DefaultHistory>::new()
        .unwrap_or_else(|_| {
            eprintln!("警告: 无法创建行编辑器");
            rustyline::Editor::<(), rustyline::history::DefaultHistory>::new().unwrap()
        });

    println!("Atomix 调试器 — 输入 help 查看命令");
    println!("当前 PC: {:#06x}", session.vm.pc);

    if session.vm.pc < session.vm.text.len() {
        let s = disassemble::format_instruction(session.vm.pc, session.vm.text[session.vm.pc]);
        println!("  {}", s);
    }

    loop {
        let line_result = rl.readline("> ");
        match line_result {
            Ok(input) => {
                let _ = rl.add_history_entry(input.as_str());
                let trimmed = input.trim().to_string();
                if !session.execute_command(&trimmed) {
                    break;
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted)
            | Err(rustyline::error::ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                eprintln!("读取输入错误: {}", e);
                break;
            }
        }
    }

    println!("调试器退出。");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::{AtxeBinary, Header};
    use crate::base::isa::{self, opcode, reg};

    fn make_test_vm(text: Vec<u32>) -> VmState {
        let header = Header::new(0, text.len() as u16);
        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata: vec![],
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        VmState::from_atxe(&binary).unwrap()
    }

    #[test]
    fn session_step_one() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        assert_eq!(session.vm.pc, 0);
        session.execute_command("step");
        assert_eq!(session.vm.pc, 1);
        assert_eq!(session.vm.read_reg(reg::A0), 42);
    }

    #[test]
    fn session_step_multiple() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::T0 as u8, 0, 10),
            isa::encode_r2i(opcode::MOVI, reg::T1 as u8, 0, 20),
            isa::encode_r3(opcode::ADD, reg::T2 as u8, reg::T0 as u8, reg::T1 as u8, 0),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        session.execute_command("step 3");
        assert_eq!(session.vm.pc, 3);
        assert_eq!(session.vm.read_reg(reg::T2), 30);
    }

    #[test]
    fn session_regs() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        session.execute_command("step");
        // regs 命令不改变状态，只是输出
        session.execute_command("regs");
        // 验证 a0 在 VM 中是 42
        assert_eq!(session.vm.read_reg(reg::A0), 42);
    }

    #[test]
    fn session_print() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 99),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        session.execute_command("step");
        session.execute_command("print a0");
        assert_eq!(session.vm.read_reg(reg::A0), 99);
    }

    #[test]
    fn session_breakpoint() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 1),
            isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 2),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        // 在地址 1 设断点
        session.execute_command("break 1");
        assert!(session.breakpoints.contains_key(&1));
        // 继续运行，应停在地址 1
        session.execute_command("continue");
        assert_eq!(session.vm.pc, 2); // 断点命中后单步执行了原指令，PC 前进到 2
        assert_eq!(session.vm.read_reg(reg::A0), 1); // 第一条指令已执行
    }

    #[test]
    fn session_disasm() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        session.execute_command("disasm 0 2");
        // 不改变状态
        assert_eq!(session.vm.pc, 0);
    }

    #[test]
    fn session_mem() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        session.execute_command("mem 0 16");
        // 不改变状态
        assert_eq!(session.vm.pc, 0);
    }

    #[test]
    fn session_continue_halt() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        session.execute_command("continue");
        assert!(!session.vm.is_running());
        assert_eq!(session.vm.read_reg(reg::A0), 42);
    }

    #[test]
    fn session_quit() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        let result = session.execute_command("quit");
        assert!(!result); // quit 返回 false
    }

    #[test]
    fn session_help() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let vm = make_test_vm(text);
        let mut session = DebugSession::new(vm);
        session.execute_command("help");
        // help 不改变状态，不崩溃
        assert_eq!(session.vm.pc, 0);
    }

    #[test]
    fn parse_addr_hex() {
        assert_eq!(parse_addr("0x42"), Some(0x42));
        assert_eq!(parse_addr("0xFF"), Some(0xFF));
        assert_eq!(parse_addr("100"), Some(100));
        assert_eq!(parse_addr("0xGG"), None); // 无效 hex
    }
}
