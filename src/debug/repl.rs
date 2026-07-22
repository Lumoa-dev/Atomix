//! 调试 REPL — 交互式调试会话。
//!
//! # 命令
//!
//! | 命令           | 格式                    | 说明                     |
//! |----------------|-------------------------|--------------------------|
//! | `step`         | `step [n]`             | 单步执行 n 条指令        |
//! | `regs`         | `regs`                  | 显示所有寄存器           |
//! | `disasm`       | `disasm [addr] [n]`    | 反汇编指定位置           |
//! | `mem`          | `mem <addr> [bytes]`   | hexdump 内存             |
//! | `x/`           | `x/<fmt> <addr> [n]`   | 格式化查看内存           |
//! | `break`        | `break <addr>`         | 设置断点                 |
//! | `break`        | `break <addr> if <e>`  | 条件断点                 |
//! | `continue`     | `continue`             | 运行到断点或结束         |
//! | `print`        | `print <表达式>`       | 打印表达式值             |
//! | `eval`         | `eval <表达式>`        | 计算表达式               |
//! | `set`          | `set <reg> = <表达式>` | 设置寄存器值             |
//! | `set`          | `set *<addr> = <表达式>`| 设置内存值              |
//! | `watch`        | `watch <addr> [size]`  | 设置数据监视点           |
//! | `display`      | `display <表达式>`     | 每次 step 后自动显示     |
//! | `display`      | `display`              | 列出所有 display 表达式  |
//! | `backtrace`    | `backtrace`            | 显示调用栈               |
//! | `frame`        | `frame [n]`            | 选择/显示当前帧          |
//! | `up`           | `up`                   | 选择上一帧               |
//! | `down`         | `down`                 | 选择下一帧               |
//! | `source`       | `source [n]`           | 显示源码                 |
//! | `help`         | `help`                 | 显示帮助                 |
//! | `quit`         | `quit`                 | 退出调试器               |

use crate::base::isa::{self, opcode};
use crate::debug::disassemble;
use crate::runner::VmState;
use crate::runner::VmStateKind;
use crate::runner::execute::execute_instruction;
use std::collections::HashMap;

/// 数据监视点。
#[derive(Debug, Clone, Copy)]
struct Watchpoint {
    pub addr: u64,
    pub size: u64,
}

/// 调试会话。
pub struct DebugSession {
    pub vm: VmState,
    pub breakpoints: HashMap<usize, u32>,
    pub conditional_breakpoints: HashMap<usize, String>,
    pub debug_map: Option<crate::debug::debug_segment::DebugMap>,
    pub source_path: Option<String>,
    source_lines: Vec<String>,
    /// 数据监视点。
    watchpoints: Vec<Watchpoint>,
    /// 每次 step 后自动显示的表达式。
    display_exprs: Vec<String>,
    /// 当前选中的调用栈帧索引（0 = 最内层）。
    selected_frame: usize,
}

impl DebugSession {
    pub fn new(vm: VmState) -> Self {
        Self {
            vm,
            breakpoints: HashMap::new(),
            conditional_breakpoints: HashMap::new(),
            debug_map: None,
            source_path: None,
            source_lines: Vec::new(),
            watchpoints: Vec::new(),
            display_exprs: Vec::new(),
            selected_frame: 0,
        }
    }

    pub fn set_source(&mut self, path: &str) {
        self.source_path = Some(path.to_string());
        if let Ok(content) = std::fs::read_to_string(path) {
            self.source_lines = content.lines().map(|l| l.to_string()).collect();
        }
    }

    pub fn set_debug_map_from_bytes(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.debug_map = crate::debug::debug_segment::DebugMap::from_bytes(bytes);
        }
    }

    // ─── 命令分发 ──────────────────────────────────

    pub fn execute_command(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return true;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            return true;
        }
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
                self.show_current_source_line();
                self.show_display_exprs();
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
            _ if command.starts_with("x/") => {
                let fmt_rest = command[2..].to_string();
                let rest_parts: Vec<&str> = parts[1..].iter().map(|s| *s).collect();
                let all_args = format!("{} {}", fmt_rest, rest_parts.join(" "));
                self.cmd_examine(&all_args.trim());
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
                    println!("用法: print <表达式>");
                } else {
                    self.cmd_print(&expr);
                }
                true
            }
            "set" => {
                let rest = parts[1..].join(" ");
                self.cmd_set(&rest);
                true
            }
            "break" | "b" => {
                if let Some(addr) = parts.get(1).and_then(|s| parse_addr(s)) {
                    let condition = if parts.len() > 2 && parts[2].to_lowercase() == "if" {
                        Some(parts[3..].join(" "))
                    } else {
                        None
                    };
                    self.cmd_break(addr, condition.as_deref());
                } else {
                    self.list_breakpoints();
                }
                true
            }
            "continue" | "c" => {
                self.cmd_continue();
                true
            }
            "watch" | "w" => {
                if let Some(addr) = parts.get(1).and_then(|s| parse_addr(s)) {
                    let size: u64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
                    self.cmd_watch(addr as u64, size);
                } else {
                    self.list_watchpoints();
                }
                true
            }
            "display" => {
                let expr = parts[1..].join(" ");
                self.cmd_display(&expr);
                true
            }
            "backtrace" | "bt" => {
                self.cmd_backtrace();
                true
            }
            "frame" | "f" => {
                if let Some(n) = parts.get(1).and_then(|s| s.parse().ok()) {
                    self.cmd_frame(n);
                } else {
                    self.show_frame();
                }
                true
            }
            "up" => {
                self.cmd_frame(self.selected_frame.saturating_add(1));
                true
            }
            "down" => {
                let n = if self.selected_frame > 0 {
                    self.selected_frame - 1
                } else {
                    0
                };
                self.cmd_frame(n);
                true
            }
            "source" | "src" => {
                let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
                self.cmd_source(n);
                true
            }
            _ => {
                println!("未知命令: {}（输入 help）", command);
                true
            }
        }
    }

    fn cmd_help(&self) {
        println!("Atomix 调试器命令:");
        println!("  step [n]        单步执行 n 条指令");
        println!("  regs            显示所有寄存器");
        println!("  disasm [addr] [n]  反汇编");
        println!("  mem <addr> [bytes]  hexdump 内存");
        println!("  x/<fmt> <addr> [n]  格式化查看 (x/d/s/c)");
        println!("  eval <expr>     计算表达式");
        println!("  print <expr>    打印表达式值");
        println!("  set <reg> = <expr>  设置寄存器");
        println!("  set *<addr>=<expr>  设置内存");
        println!("  break [addr]    设置/列出断点");
        println!("  break <addr> if <e>  条件断点");
        println!("  continue        运行到断点");
        println!("  watch <addr> [size]  数据监视点");
        println!("  display [<expr>]  step 后自动显示表达式");
        println!("  backtrace       调用栈回溯");
        println!("  frame [n]       选择/显示帧");
        println!("  up/down         帧选择");
        println!("  source [n]      源码视图");
        println!("  help/quit       帮助/退出");
    }

    // ─── 执行控制 ──────────────────────────────────

    fn cmd_step(&mut self, n: usize) {
        for _ in 0..n {
            if !self.vm.is_running() {
                println!("VM 已停止（状态: {:?}）", self.vm.state);
                return;
            }
            execute_instruction(&mut self.vm);
        }
        if self.vm.pc < self.vm.text.len() && self.vm.is_running() {
            println!(
                "→ {}",
                disassemble::format_instruction(self.vm.pc, self.vm.text[self.vm.pc])
            );
        } else if self.vm.pc < self.vm.text.len() {
            println!(
                "⏹ {}",
                disassemble::format_instruction(self.vm.pc, self.vm.text[self.vm.pc])
            );
        }
    }

    fn cmd_continue(&mut self) {
        let max_steps = 1_000_000;
        let mut steps = 0;
        while self.vm.is_running() && steps < max_steps {
            let pc_before = self.vm.pc;
            execute_instruction(&mut self.vm);
            steps += 1;

            // 检查数据监视点（LOAD/STORE 指令）
            if self.check_watchpoints(pc_before) {
                println!("⏸ 监视点命中于 {:#06x}", pc_before);
                return;
            }

            if !self.vm.is_running() || self.vm.state == VmStateKind::Halted {
                if self.breakpoints.contains_key(&pc_before) {
                    let mut condition_skipped = false;
                    if let Some(cond) = self.conditional_breakpoints.get(&pc_before) {
                        if !cond.is_empty() {
                            match crate::debug::eval::eval_expr(cond, &self.vm) {
                                Ok(val) if val == 0 => {
                                    if let Some(&orig) = self.breakpoints.get(&pc_before) {
                                        self.vm.text[pc_before] = orig;
                                        self.vm.pc = pc_before;
                                        self.vm.state = VmStateKind::Running;
                                        execute_instruction(&mut self.vm);
                                        self.vm.text[pc_before] = isa::encode_ji(opcode::TRAP, 0);
                                    }
                                    condition_skipped = true;
                                }
                                Ok(_) => {}
                                Err(e) => println!("⚠ 条件求值错误 ({}): 继续执行", e),
                            }
                        }
                    }
                    if condition_skipped {
                        continue;
                    }

                    println!("⏸ 命中断点于 {:#06x}", pc_before);
                    if let Some(&orig) = self.breakpoints.get(&pc_before) {
                        self.vm.text[pc_before] = orig;
                        self.vm.pc = pc_before;
                        self.vm.state = VmStateKind::Running;
                        execute_instruction(&mut self.vm);
                        self.vm.text[pc_before] = isa::encode_ji(opcode::TRAP, 0);
                        if self.vm.pc < self.vm.text.len() && self.vm.is_running() {
                            println!(
                                "→ {}",
                                disassemble::format_instruction(
                                    self.vm.pc,
                                    self.vm.text[self.vm.pc]
                                )
                            );
                        }
                    }
                    return;
                }
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

    // ─── 数据监视点 ─────────────────────────────────

    fn cmd_watch(&mut self, addr: u64, size: u64) {
        // 去重
        if self.watchpoints.iter().any(|w| w.addr == addr) {
            println!("监视点已存在于 {:#x}", addr);
            return;
        }
        self.watchpoints.push(Watchpoint { addr, size });
        println!("监视点已设置: {:#x} ({} 字节)", addr, size);
    }

    fn list_watchpoints(&self) {
        if self.watchpoints.is_empty() {
            println!("（无监视点）");
            return;
        }
        println!("监视点列表:");
        for (i, wp) in self.watchpoints.iter().enumerate() {
            println!(
                "  #{}  {:#x}-{:#x} ({} bytes)",
                i,
                wp.addr,
                wp.addr + wp.size - 1,
                wp.size
            );
        }
    }

    /// 检查刚执行的指令是否命中监视点。命中返回 true。
    fn check_watchpoints(&self, pc_before: usize) -> bool {
        if self.watchpoints.is_empty() || pc_before >= self.vm.text.len() {
            return false;
        }
        let instr = self.vm.text[pc_before];
        let op = (instr >> 24) as u8;
        // 只检查 LOAD (0x13) 和 STORE (0x14)
        if op != 0x13 && op != 0x14 {
            return false;
        }
        let table = crate::runner::decode::dispatch_table();
        let entry = &table[op as usize];
        let ops = crate::runner::decode::decode(instr, entry.enc);
        let addr = if op == 0x13 {
            // LOAD rd, [rs1 + imm]
            self.vm
                .read_reg(ops.rs1 as usize)
                .wrapping_add(ops.imm as i16 as u64)
        } else {
            // STORE [rd + imm], rs1
            self.vm
                .read_reg(ops.rd as usize)
                .wrapping_add(ops.imm as i16 as u64)
        };
        self.watchpoints
            .iter()
            .any(|wp| addr >= wp.addr && addr < wp.addr.wrapping_add(wp.size))
    }

    // ─── 显示表达式 ─────────────────────────────────

    fn cmd_display(&mut self, expr: &str) {
        if expr.is_empty() {
            if self.display_exprs.is_empty() {
                println!("（无 display 表达式）");
            } else {
                println!("Display 表达式:");
                for (i, e) in self.display_exprs.iter().enumerate() {
                    match crate::debug::eval::eval_expr(e, &self.vm) {
                        Ok(val) => println!(
                            "  {}: {} = {}",
                            i,
                            e,
                            crate::debug::eval::format_result(val)
                        ),
                        Err(_) => println!("  {}: {} = <错误>", i, e),
                    }
                }
            }
            return;
        }
        self.display_exprs.push(expr.to_string());
        println!("{} 已添加到 display 列表", expr);
    }

    fn show_display_exprs(&self) {
        for e in &self.display_exprs {
            match crate::debug::eval::eval_expr(e, &self.vm) {
                Ok(val) => println!("  {} = {}", e, crate::debug::eval::format_result(val)),
                Err(_) => {}
            }
        }
    }

    // ─── 帧选择 ─────────────────────────────────────

    fn cmd_frame(&mut self, n: usize) {
        let max_frame = self.vm.call_stack.len();
        if n > max_frame {
            println!("帧 {} 超出范围（共 {} 帧）", n, max_frame);
            return;
        }
        self.selected_frame = n;
        self.show_frame();
    }

    fn show_frame(&self) {
        let cs = &self.vm.call_stack;
        if cs.is_empty() {
            println!("当前帧: #0 (entry) pc={:#06x}", self.vm.pc);
            return;
        }
        let idx = self.selected_frame.min(cs.len());
        if idx == 0 {
            // 当前执行帧
            println!("当前帧: #0 pc={:#06x}", self.vm.pc);
        } else {
            let frame = &cs[cs.len() - idx];
            println!(
                "帧 #{}: return_pc={:#06x} sp={:#x}",
                idx, frame.return_pc, frame.sp
            );
        }
    }

    // ─── 寄存器 / 内存 / 反汇编 ─────────────────────

    fn cmd_regs(&self) {
        println!("寄存器:");
        for i in 0..isa::REG_COUNT {
            let name = isa::reg_name(i).to_uppercase();
            let val = self.vm.read_reg(i);
            println!("  {:>8}(R{:>2}): {:#018x}  ({})", name, i, val, val as i64);
        }
        println!("  {:>8}: {:#06x}", "PC", self.vm.pc);
        println!("  状态: {:?}", self.vm.state);
        println!(
            "  帧: #{} (共 {} 帧)",
            self.selected_frame,
            self.vm.call_stack.len()
        );
    }

    fn cmd_disasm(&self, addr: usize, n: usize) {
        if addr >= self.vm.text.len() {
            println!("地址 {:#06x} 超出 .text 段", addr);
            return;
        }
        for line in disassemble::disassemble_range(&self.vm.text, addr, n) {
            println!("{}", line);
        }
    }

    fn cmd_mem(&self, addr: u64, bytes: usize) {
        let end = addr.saturating_add(bytes as u64);
        if end as usize > self.vm.memory.data.len() {
            println!("地址超出沙箱内存");
            return;
        }
        let mut offset = addr;
        while offset < end {
            let line_end = (offset + 16).min(end);
            let mut hex = String::new();
            let mut ascii = String::new();
            for a in offset..line_end {
                if let Some(byte) = self.vm.memory.read_u8(a) {
                    hex.push_str(&format!("{:02x} ", byte));
                    ascii.push(if byte.is_ascii_graphic() || byte == b' ' {
                        byte as char
                    } else {
                        '.'
                    });
                }
            }
            println!("{:#010x}:  {:48}  {}", offset, hex, ascii);
            offset = line_end;
        }
    }

    // ─── 格式化查看 x/<fmt> ─────────────────────────

    fn cmd_examine(&self, args: &str) {
        // 格式: <fmt> <addr> [n]
        // fmt: x=hex, d=decimal, u=unsigned, c=char, s=string
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.is_empty() {
            println!("用法: x/<fmt> <addr> [n]");
            return;
        }

        let fmt = parts[0].chars().next().unwrap_or('x');
        let addr = parts
            .get(1)
            .and_then(|s| parse_addr(s))
            .unwrap_or(self.vm.pc);
        let count: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);

        match fmt {
            'x' => {
                for i in 0..count {
                    let a = (addr as u64).wrapping_add((i * 8) as u64);
                    if let Some(val) = self.vm.memory.read_u64(a) {
                        println!("  {:#010x}: {:#018x}", a, val);
                    } else {
                        println!("  {:#010x}: <越界>", a);
                    }
                }
            }
            'd' => {
                for i in 0..count {
                    let a = (addr as u64).wrapping_add((i * 8) as u64);
                    if let Some(val) = self.vm.memory.read_u64(a) {
                        println!("  {:#010x}: {}", a, val as i64);
                    } else {
                        println!("  {:#010x}: <越界>", a);
                    }
                }
            }
            'c' => {
                for i in 0..count {
                    let a = (addr as u64).wrapping_add(i as u64);
                    if let Some(byte) = self.vm.memory.read_u8(a) {
                        let ch = if byte.is_ascii_graphic() || byte == b' ' {
                            byte as char
                        } else {
                            '.'
                        };
                        println!("  {:#010x}: '{}' ({})", a, ch, byte);
                    } else {
                        println!("  {:#010x}: <越界>", a);
                    }
                }
            }
            's' => {
                let mut s = String::new();
                for i in 0..256 {
                    let a = (addr as u64).wrapping_add(i as u64);
                    match self.vm.memory.read_u8(a) {
                        Some(0) => break,
                        Some(b) => s.push(if b.is_ascii_graphic() || b == b' ' {
                            b as char
                        } else {
                            '.'
                        }),
                        None => break,
                    }
                }
                println!("  {:#010x}: \"{}\"", addr as u64, s);
            }
            _ => println!("不支持的格式 '{}'（支持: x/d/c/s）", fmt),
        }
    }

    // ─── 设置寄存器 / 内存 ──────────────────────────

    fn cmd_set(&mut self, expr: &str) {
        // set a0 = 42 或 set *0x1000 = 42
        let eq_pos = expr.find('=');
        if eq_pos.is_none() {
            println!("用法: set <reg> = <expr> 或 set *<addr> = <expr>");
            return;
        }
        let target = expr[..eq_pos.unwrap()].trim();
        let value_expr = expr[eq_pos.unwrap() + 1..].trim();

        let value = match crate::debug::eval::eval_expr(value_expr, &self.vm) {
            Ok(v) => v,
            Err(e) => {
                println!("值表达式错误: {}", e);
                return;
            }
        };

        if target.starts_with('*') {
            // 设置内存: *addr = value
            let addr_expr = target[1..].trim();
            match crate::debug::eval::eval_expr(addr_expr, &self.vm) {
                Ok(addr) => {
                    if self.vm.memory.write_u64(addr, value) {
                        println!("  *{:#x} = {} ({:#x})", addr, value as i64, value);
                    } else {
                        println!("  无法写入地址 {:#x}", addr);
                    }
                }
                Err(e) => println!("地址表达式错误: {}", e),
            }
        } else {
            // 设置寄存器: reg = value
            let idx = match parse_reg_name(target) {
                Some(i) => i,
                None => {
                    println!("未知寄存器: {}", target);
                    return;
                }
            };
            if idx == 0 {
                println!("R0 (zero) 是只读寄存器");
            } else if idx == 14 {
                println!("R14 (task_id) 是只读寄存器");
            } else {
                self.vm.write_reg(idx, value);
                let name = isa::reg_name(idx).to_uppercase();
                println!("  {} = {} ({:#x})", name, value as i64, value);
            }
        }
    }

    // ─── 求值 / 打印 ────────────────────────────────

    fn cmd_eval(&self, expr: &str) {
        match crate::debug::eval::eval_expr(expr, &self.vm) {
            Ok(val) => println!("  {} = {}", expr, crate::debug::eval::format_result(val)),
            Err(e) => println!("表达式错误: {}", e),
        }
    }

    fn cmd_print(&self, name: &str) {
        // 先尝试表达式求值
        match crate::debug::eval::eval_expr(name, &self.vm) {
            Ok(val) => {
                println!("  {} = {}", name, crate::debug::eval::format_result(val));
                return;
            }
            Err(_) => {}
        }
        // 降级到寄存器名解析
        if let Some(idx) = parse_reg_name(name) {
            let val = self.vm.read_reg(idx);
            let name_upper = isa::reg_name(idx).to_uppercase();
            println!("  {}(R{}): {:#018x} ({})", name_upper, idx, val, val as i64);
        } else if name.to_lowercase() == "pc" {
            println!("  PC = {:#06x}", self.vm.pc);
        } else {
            println!("无法解析: {}", name);
        }
    }

    // ─── 断点 ───────────────────────────────────────

    fn cmd_break(&mut self, addr: usize, condition: Option<&str>) {
        if addr >= self.vm.text.len() {
            println!("地址 {:#06x} 超出 .text 段", addr);
            return;
        }
        if self.breakpoints.contains_key(&addr) {
            if let Some(cond) = condition {
                self.conditional_breakpoints.insert(addr, cond.to_string());
                println!("条件断点已更新于 {:#06x}: if {}", addr, cond);
            } else {
                println!("断点已存在于 {:#06x}", addr);
            }
            return;
        }
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
            let cond = self
                .conditional_breakpoints
                .get(&addr)
                .map(|c| format!(" if {}", c))
                .unwrap_or_default();
            let desc = if addr < self.vm.text.len() {
                format!(
                    "  ({})",
                    disassemble::format_instruction(addr, self.vm.text[addr])
                )
            } else {
                String::new()
            };
            println!("  {:#06x}{}{}", addr, cond, desc);
        }
    }

    // ─── 调用栈 ─────────────────────────────────────

    fn cmd_backtrace(&self) {
        if self.vm.call_stack.is_empty() {
            println!("调用栈为空");
            println!("  #0  current pc={:#06x}", self.vm.pc);
            return;
        }
        println!("调用栈（共 {} 帧）:", self.vm.call_stack.len() + 1);
        // 当前帧
        let marker = if self.selected_frame == 0 { "→" } else { " " };
        println!("  {} #0  pc={:#06x} (current)", marker, self.vm.pc);
        // 历史帧
        for (i, frame) in self.vm.call_stack.iter().rev().enumerate() {
            let depth = i + 1;
            let marker = if self.selected_frame == depth {
                "→"
            } else {
                " "
            };
            println!(
                "  {} #{}  return_pc={:#06x}",
                marker, depth, frame.return_pc
            );
        }
    }

    // ─── 源码视图 ───────────────────────────────────

    fn cmd_source(&self, n: usize) {
        let line = self
            .debug_map
            .as_ref()
            .and_then(|m| m.line_for_pc(self.vm.pc));
        let display_line = line.unwrap_or(1) as usize;
        if self.source_lines.is_empty() {
            println!("（未加载源文件）");
            return;
        }
        let start = display_line.saturating_sub(n / 2).max(1);
        let end = (start + n).min(self.source_lines.len() + 1);
        for lnum in start..end {
            let marker = if Some(lnum as u32) == line {
                "→"
            } else {
                " "
            };
            if lnum <= self.source_lines.len() {
                println!("{} {:>4} │ {}", marker, lnum, self.source_lines[lnum - 1]);
            }
        }
    }

    fn show_current_source_line(&self) {
        if let Some(ref map) = self.debug_map
            && let Some(line) = map.line_for_pc(self.vm.pc)
            && !self.source_lines.is_empty()
            && (line as usize) <= self.source_lines.len()
        {
            println!(
                "  {:>4} │ {} (line {})",
                line,
                self.source_lines[line as usize - 1].trim(),
                line
            );
        }
    }
}

// ─── 辅助函数 ────────────────────────────────────────

fn parse_addr(s: &str) -> Option<usize> {
    if s.starts_with("0x") || s.starts_with("0X") {
        usize::from_str_radix(&s[2..], 16).ok()
    } else {
        s.parse().ok()
    }
}

pub fn parse_reg_name(name: &str) -> Option<usize> {
    Some(match name.to_lowercase().as_str() {
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
        _ => return None,
    })
}

// ─── REPL 启动 ───────────────────────────────────────

pub fn run_repl(session: &mut DebugSession) {
    let mut rl =
        rustyline::Editor::<(), rustyline::history::DefaultHistory>::new().unwrap_or_else(|_| {
            rustyline::Editor::<(), rustyline::history::DefaultHistory>::new().unwrap()
        });

    println!("Atomix 调试器 — 输入 help 查看命令");
    println!("当前 PC: {:#06x}", session.vm.pc);
    if session.vm.pc < session.vm.text.len() {
        println!(
            "  {}",
            disassemble::format_instruction(session.vm.pc, session.vm.text[session.vm.pc])
        );
    }

    loop {
        let line_result = rl.readline("> ");
        match line_result {
            Ok(input) => {
                let _ = rl.add_history_entry(input.as_str());
                if !session.execute_command(&input.trim()) {
                    break;
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted)
            | Err(rustyline::error::ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                eprintln!("读取错误: {}", e);
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
            sections: vec![],
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
        let mut s = DebugSession::new(make_test_vm(text));
        s.execute_command("step");
        assert_eq!(s.vm.pc, 1);
        assert_eq!(s.vm.read_reg(reg::A0), 42);
    }

    #[test]
    fn session_step_multiple() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::T0 as u8, 0, 10),
            isa::encode_r2i(opcode::MOVI, reg::T1 as u8, 0, 20),
            isa::encode_r3(opcode::ADD, reg::T2 as u8, reg::T0 as u8, reg::T1 as u8, 0),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut s = DebugSession::new(make_test_vm(text));
        s.execute_command("step 3");
        assert_eq!(s.vm.read_reg(reg::T2), 30);
    }

    #[test]
    fn session_breakpoint() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 1),
            isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 2),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut s = DebugSession::new(make_test_vm(text));
        s.execute_command("break 1");
        assert!(s.breakpoints.contains_key(&1));
        s.execute_command("continue");
        assert_eq!(s.vm.pc, 2);
        assert_eq!(s.vm.read_reg(reg::A0), 1);
    }

    #[test]
    fn session_continue_halt() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut s = DebugSession::new(make_test_vm(text));
        s.execute_command("continue");
        assert!(!s.vm.is_running());
        assert_eq!(s.vm.read_reg(reg::A0), 42);
    }

    #[test]
    fn session_set_register() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = DebugSession::new(make_test_vm(text));
        s.execute_command("set t0 = 77");
        assert_eq!(s.vm.read_reg(reg::T0), 77);
    }

    #[test]
    fn session_set_memory() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = DebugSession::new(make_test_vm(text));
        let addr = s.vm.memory.alloc(16);
        s.execute_command(&format!("set *{} = 0xDEAD", addr));
        assert_eq!(s.vm.memory.read_u64(addr), Some(0xDEAD));
    }

    #[test]
    fn session_watchpoint() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 64),
            isa::encode_r1i(opcode::ECALL, 0, 0), // ALLOC → a0 = addr
            isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0), // t0 = addr
            isa::encode_r2i(opcode::MOVI, reg::T1 as u8, 0, 42), // t1 = 42
            isa::encode_r2i(opcode::STORE, reg::T0 as u8, reg::T1 as u8, 0), // [t0] = 42
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut s = DebugSession::new(make_test_vm(text));
        // 先 step 到 STORE 之前 (pc=3: MOVI t1=42)
        s.execute_command("step 4");
        let store_addr = s.vm.read_reg(reg::T0);
        s.execute_command(&format!("watch {} 8", store_addr));
        // verify watchpoint was set
        assert!(!s.watchpoints.is_empty());
    }

    #[test]
    fn session_display() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut s = DebugSession::new(make_test_vm(text));
        s.execute_command("display a0");
        assert_eq!(s.display_exprs.len(), 1);
        assert_eq!(s.display_exprs[0], "a0");
    }

    #[test]
    fn session_frame() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = DebugSession::new(make_test_vm(text));
        s.execute_command("frame");
        assert_eq!(s.selected_frame, 0);
    }

    #[test]
    fn session_examine() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let s = DebugSession::new(make_test_vm(text));
        s.cmd_examine("x 0 1");
        // 不崩溃即通过
    }

    #[test]
    fn session_help() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = DebugSession::new(make_test_vm(text));
        s.execute_command("help");
        assert_eq!(s.vm.pc, 0);
    }

    #[test]
    fn session_quit() {
        let text = vec![isa::encode_ji(opcode::TRAP, 0)];
        let mut s = DebugSession::new(make_test_vm(text));
        assert!(!s.execute_command("quit"));
    }

    #[test]
    fn parse_addr_hex() {
        assert_eq!(parse_addr("0x42"), Some(0x42));
        assert_eq!(parse_addr("0xFF"), Some(0xFF));
        assert_eq!(parse_addr("100"), Some(100));
    }

    #[test]
    fn parse_reg_name_all() {
        assert_eq!(parse_reg_name("a0"), Some(4));
        assert_eq!(parse_reg_name("sp"), Some(1));
        assert_eq!(parse_reg_name("r10"), Some(10));
    }
}
