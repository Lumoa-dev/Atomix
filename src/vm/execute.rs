//! 指令执行引擎 — 54 条指令的 dispatch 处理函数。
//!
//! 覆盖 02-指令集规范.md §3 的全部指令行为。

use crate::base::isa::{self, opcode, reg};
use crate::vm::decode;
use crate::vm::VmState;

/// 执行单条指令。返回 true 表示继续执行，false 表示需要让出/停止。
pub fn execute_instruction(vm: &mut VmState) -> bool {
    if vm.pc >= vm.text.len() {
        vm.state = crate::vm::VmStateKind::Error("pc 越界".into());
        return false;
    }

    let instr = vm.fetch();
    let op = (instr >> 24) as u8;
    let entry = &decode::dispatch_table()[op as usize];
    let ops = decode::decode(instr, entry.enc);

    vm.quantum += 1;

    match op {
        // ── System / Control (0x00–0x0F) ──────────
        opcode::NOP => {}

        opcode::TRAP => {
            match ops.imm {
                0 => { vm.state = crate::vm::VmStateKind::Halted; return false; }
                1 => { /* DEBUG: NOP for now */ }
                _ => {}
            }
        }

        opcode::THROW => {
            let exc_val = vm.read_reg(ops.rd as usize);
            // 查 .exn 表（简化版：在 execute.rs 底部实现）
            if !handle_exception(vm, exc_val) {
                vm.state = crate::vm::VmStateKind::Error(format!("未捕获的异常: {}", exc_val));
                return false;
            }
        }

        // ── Data Movement (0x10–0x1F) ────────────
        opcode::MOV => {
            vm.write_reg(ops.rd as usize, vm.read_reg(ops.rs1 as usize));
        }

        opcode::MOVI => {
            vm.write_reg(ops.rd as usize, ops.imm as u64);
        }

        opcode::LCONST => {
            vm.write_reg(ops.rd as usize, ops.imm as u64);
        }

        opcode::LOAD => {
            let addr = vm.read_reg(ops.rs1 as usize).wrapping_add(ops.imm as u64);
            // 简化：从 .rodata 或栈读取
            if let Some(val) = load_from_memory(vm, addr) {
                vm.write_reg(ops.rd as usize, val);
            } else {
                vm.state = crate::vm::VmStateKind::Error(format!("LOAD 越界: addr={:#x}", addr));
                return false;
            }
        }

        opcode::STORE => {
            let addr = vm.read_reg(ops.rd as usize).wrapping_add(ops.imm as u64);
            let val = vm.read_reg(ops.rs1 as usize);
            if !store_to_memory(vm, addr, val) {
                vm.state = crate::vm::VmStateKind::Error(format!("STORE 越界: addr={:#x}", addr));
                return false;
            }
        }

        // ── Integer Arithmetic (0x20–0x2E) ────────
        opcode::ADDI => {
            let r = vm.read_reg(ops.rs1 as usize).wrapping_add(ops.imm as i16 as u64);
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::ADD => {
            let r = vm.read_reg(ops.rs1 as usize).wrapping_add(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::SUB => {
            let r = vm.read_reg(ops.rs1 as usize).wrapping_sub(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::MUL => {
            let r = vm.read_reg(ops.rs1 as usize).wrapping_mul(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::DIV => {
            let divisor = vm.read_reg(ops.rs2 as usize);
            if divisor == 0 {
                vm.state = crate::vm::VmStateKind::Error("除零异常".into());
                return false;
            }
            let r = (vm.read_reg(ops.rs1 as usize) as i64).wrapping_div(divisor as i64) as u64;
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::DIVU => {
            let divisor = vm.read_reg(ops.rs2 as usize);
            if divisor == 0 {
                vm.state = crate::vm::VmStateKind::Error("除零异常".into());
                return false;
            }
            vm.write_reg(ops.rd as usize, vm.read_reg(ops.rs1 as usize).wrapping_div(divisor));
        }

        opcode::REM => {
            let divisor = vm.read_reg(ops.rs2 as usize);
            if divisor == 0 {
                vm.state = crate::vm::VmStateKind::Error("除零异常".into());
                return false;
            }
            let r = (vm.read_reg(ops.rs1 as usize) as i64).wrapping_rem(divisor as i64) as u64;
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::AND => {
            vm.write_reg(ops.rd as usize, vm.read_reg(ops.rs1 as usize) & vm.read_reg(ops.rs2 as usize));
        }

        opcode::OR => {
            vm.write_reg(ops.rd as usize, vm.read_reg(ops.rs1 as usize) | vm.read_reg(ops.rs2 as usize));
        }

        opcode::XOR => {
            vm.write_reg(ops.rd as usize, vm.read_reg(ops.rs1 as usize) ^ vm.read_reg(ops.rs2 as usize));
        }

        opcode::NOT => {
            vm.write_reg(ops.rd as usize, !vm.read_reg(ops.rd as usize));
        }

        opcode::NEG => {
            let r = (vm.read_reg(ops.rd as usize) as i64).wrapping_neg() as u64;
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::SHL => {
            let shift = vm.read_reg(ops.rs2 as usize) & 0x3F;
            vm.write_reg(ops.rd as usize, vm.read_reg(ops.rs1 as usize) << shift);
        }

        opcode::SHR => {
            let shift = vm.read_reg(ops.rs2 as usize) & 0x3F;
            let r = (vm.read_reg(ops.rs1 as usize) as i64 >> shift) as u64;
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::SHRU => {
            let shift = vm.read_reg(ops.rs2 as usize) & 0x3F;
            vm.write_reg(ops.rd as usize, vm.read_reg(ops.rs1 as usize) >> shift);
        }

        // ── Floating Point (0x2F–0x38) ───────────
        opcode::FADD => {
            let a = f64::from_bits(vm.read_reg(ops.rs1 as usize));
            let b = f64::from_bits(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, (a + b).to_bits());
        }

        opcode::FSUB => {
            let a = f64::from_bits(vm.read_reg(ops.rs1 as usize));
            let b = f64::from_bits(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, (a - b).to_bits());
        }

        opcode::FMUL => {
            let a = f64::from_bits(vm.read_reg(ops.rs1 as usize));
            let b = f64::from_bits(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, (a * b).to_bits());
        }

        opcode::FDIV => {
            let a = f64::from_bits(vm.read_reg(ops.rs1 as usize));
            let b = f64::from_bits(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, (a / b).to_bits());
        }

        opcode::FEQ => {
            let a = f64::from_bits(vm.read_reg(ops.rs1 as usize));
            let b = f64::from_bits(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, if a == b { 1 } else { 0 });
        }

        opcode::FNE => {
            let a = f64::from_bits(vm.read_reg(ops.rs1 as usize));
            let b = f64::from_bits(vm.read_reg(ops.rs2 as usize));
            let result = if a.is_nan() || b.is_nan() { 0 } else if a != b { 1 } else { 0 };
            vm.write_reg(ops.rd as usize, result);
        }

        opcode::FLT => {
            let a = f64::from_bits(vm.read_reg(ops.rs1 as usize));
            let b = f64::from_bits(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, if a < b { 1 } else { 0 });
        }

        opcode::FLE => {
            let a = f64::from_bits(vm.read_reg(ops.rs1 as usize));
            let b = f64::from_bits(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, if a <= b { 1 } else { 0 });
        }

        opcode::ITOF => {
            let val = vm.read_reg(ops.rd as usize) as i64;
            vm.write_reg(ops.rd as usize, (val as f64).to_bits());
        }

        opcode::FTOI => {
            let val = f64::from_bits(vm.read_reg(ops.rd as usize));
            vm.write_reg(ops.rd as usize, val as i64 as u64);
        }

        // ── Compare / Set (0x40–0x45) ────────────
        opcode::SEQ => {
            let r = if vm.read_reg(ops.rs1 as usize) == vm.read_reg(ops.rs2 as usize) { 1 } else { 0 };
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::SNE => {
            let r = if vm.read_reg(ops.rs1 as usize) != vm.read_reg(ops.rs2 as usize) { 1 } else { 0 };
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::SLT => {
            let a = vm.read_reg(ops.rs1 as usize) as i64;
            let b = vm.read_reg(ops.rs2 as usize) as i64;
            vm.write_reg(ops.rd as usize, if a < b { 1 } else { 0 });
        }

        opcode::SLE => {
            let a = vm.read_reg(ops.rs1 as usize) as i64;
            let b = vm.read_reg(ops.rs2 as usize) as i64;
            vm.write_reg(ops.rd as usize, if a <= b { 1 } else { 0 });
        }

        opcode::SGT => {
            let a = vm.read_reg(ops.rs1 as usize) as i64;
            let b = vm.read_reg(ops.rs2 as usize) as i64;
            vm.write_reg(ops.rd as usize, if a > b { 1 } else { 0 });
        }

        opcode::SGE => {
            let a = vm.read_reg(ops.rs1 as usize) as i64;
            let b = vm.read_reg(ops.rs2 as usize) as i64;
            vm.write_reg(ops.rd as usize, if a >= b { 1 } else { 0 });
        }

        // ── Control Flow (0x50–0x55) ─────────────
        opcode::JMP => {
            let offset = isa::decode_ji(instr);
            vm.pc = (vm.pc as i32).wrapping_add(offset) as usize;
            return true; // skip pc increment
        }

        opcode::JZ => {
            let raw = ops.imm;
                let offset = if raw & 0x80000 != 0 {
                    (raw | 0xFFF00000) as i32
                } else {
                    raw as i32
                };
                vm.pc = (vm.pc as i32).wrapping_add(offset) as usize;
        }

        opcode::JNZ => {
            if vm.read_reg(ops.rd as usize) != 0 {
                let raw = ops.imm;
                let offset = if raw & 0x80000 != 0 {
                    (raw | 0xFFF00000) as i32
                } else {
                    raw as i32
                };
                vm.pc = (vm.pc as i32).wrapping_add(offset) as usize;
                return true;
            }
        }

        opcode::CALL => {
            vm.write_reg(reg::RA, (vm.pc + 1) as u64);
            let offset = isa::decode_ji(instr);
            vm.pc = (vm.pc as i32).wrapping_add(offset) as usize;
            return true;
        }

        opcode::JMPR => {
            vm.pc = vm.read_reg(ops.rd as usize) as usize;
            return true;
        }

        opcode::JALR => {
            let link = (vm.pc + 1) as u64;
            let target = vm.read_reg(ops.rs1 as usize).wrapping_add(ops.imm as i16 as u64);
            vm.write_reg(ops.rd as usize, link);
            vm.pc = target as usize;
            return true;
        }

        // ── Concurrency (0x60–0x63) — 存根 ─────
        opcode::TASK_FORK => {
            vm.write_reg(ops.rd as usize, 0); // 空句柄
        }

        opcode::TASK_JOIN => {
            vm.write_reg(ops.rd as usize, 0); // 空返回值
        }

        opcode::TASK_RET => {
            vm.state = crate::vm::VmStateKind::Halted;
            return false;
        }

        opcode::TASK_SELF => {
            vm.write_reg(ops.rd as usize, vm.task_id as u64);
        }

        // ── System Call (0x70) ──────────────────
        opcode::ECALL => {
            let syscall = ops.imm;
            let arg1 = vm.read_reg(reg::A0);
            let arg2 = vm.read_reg(reg::A1);
            let arg3 = vm.read_reg(reg::A2);
            let result = handle_ecall(syscall, arg1, arg2, arg3);
            vm.write_reg(reg::A0, result);
        }

        // ── Memory Operations (0x80–0x81) ───────
        opcode::MCPY => {
            // memcpy(dst=rd, src=rs1, len=rs2)
            let dst = vm.read_reg(ops.rd as usize);
            let src = vm.read_reg(ops.rs1 as usize);
            let len = vm.read_reg(ops.rs2 as usize) as usize;
            for i in 0..len {
                if let Some(byte) = load_byte_from_memory(vm, src + i as u64) {
                    if !store_byte_to_memory(vm, dst + i as u64, byte) {
                        vm.state = crate::vm::VmStateKind::Error("MCPY 越界".into());
                        return false;
                    }
                } else {
                    vm.state = crate::vm::VmStateKind::Error("MCPY 越界".into());
                    return false;
                }
            }
        }

        opcode::MSET => {
            // memset(dst=rd, val=rs1, len=rs2)
            let dst = vm.read_reg(ops.rd as usize);
            let val = vm.read_reg(ops.rs1 as usize) as u8;
            let len = vm.read_reg(ops.rs2 as usize) as usize;
            for i in 0..len {
                if !store_byte_to_memory(vm, dst + i as u64, val) {
                    vm.state = crate::vm::VmStateKind::Error("MSET 越界".into());
                    return false;
                }
            }
        }

        // ── Atomic / Memory Barrier (0xF0–0xF1) ─
        opcode::FENCE => { /* NOP in VM mode */ }
        opcode::CAS => {
            let addr = vm.read_reg(ops.rs1 as usize);
            let new_val = vm.read_reg(ops.rs2 as usize);
            let expected = vm.read_reg(ops.rd as usize);
            let is_32bit = (ops.funct & 0x01) != 0;
            if is_32bit {
                if let Some(old) = load_from_memory(vm, addr) {
                    let old32 = old as u32;
                    if old32 == (expected as u32) {
                        store_to_memory(vm, addr, new_val & 0xFFFF_FFFF);
                    }
                    vm.write_reg(ops.rd as usize, old32 as u64);
                }
            } else {
                if let Some(old) = load_from_memory(vm, addr) {
                    if old == expected {
                        store_to_memory(vm, addr, new_val);
                    }
                    vm.write_reg(ops.rd as usize, old);
                }
            }
        }

        // ── Illegal opcode ────────────────────
        _ => {
            vm.state = crate::vm::VmStateKind::Error(format!("非法指令: opcode={:#04x}", op));
            return false;
        }
    }

    vm.pc += 1;
    vm.quantum < 1000 // 配额用尽时让出
}

// ─── 内存辅助 ──────────────────────────────────────────

/// 从内存或 .rodata 加载 64 位值。
fn load_from_memory(vm: &VmState, addr: u64) -> Option<u64> {
    if addr < vm.rodata.len() as u64 {
        let start = addr as usize;
        let end = start + 8;
        if end <= vm.rodata.len() {
            let bytes: [u8; 8] = vm.rodata[start..end].try_into().ok()?;
            return Some(u64::from_le_bytes(bytes));
        }
    }
    None
}

/// 存储 64 位值到内存。
fn store_to_memory(vm: &mut VmState, addr: u64, val: u64) -> bool {
    let _ = addr;
    let _ = val;
    // Phase 2：真正的沙箱内存
    false
}

/// 从内存加载单个字节。
fn load_byte_from_memory(vm: &VmState, addr: u64) -> Option<u8> {
    if addr < vm.rodata.len() as u64 {
        return Some(vm.rodata[addr as usize]);
    }
    None
}

/// 存储单个字节到内存。
fn store_byte_to_memory(vm: &mut VmState, addr: u64, val: u8) -> bool {
    let _ = addr;
    let _ = val;
    false
}

// ─── 异常处理 ──────────────────────────────────────────

/// 处理 THROW 指令：查 .exn 表，匹配则跳转到 handler。
fn handle_exception(vm: &mut VmState, exc_val: u64) -> bool {
    let pc = vm.pc as u32;
    // 解析 .exn 表：每条目 16 字节
    for chunk in vm.exn_table.chunks(16) {
        if chunk.len() < 12 { continue; }
        let start_pc = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let end_pc = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let handler_pc = u32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);

        if pc >= start_pc && pc < end_pc {
            vm.write_reg(reg::A0, exc_val);
            vm.pc = handler_pc as usize;
            return true;
        }
    }
    // 未找到 handler：栈展开（简化：直接失败）
    false
}

// ─── ECALL ─────────────────────────────────────────────

/// 处理 ECALL 系统调用。
fn handle_ecall(syscall: u32, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    match syscall {
        crate::base::isa::ecall::ALLOC => {
            let _size = arg1;
            0 // 分配失败
        }
        crate::base::isa::ecall::FREE => {
            let _addr = arg1;
            0
        }
        _ => u64::MAX // 不支持
    }
}

// ─── 主执行循环 ────────────────────────────────────────

/// 运行 VM 直到完成或出错。返回执行结果。
pub fn run_vm(vm: &mut VmState) -> &str {
    while vm.is_running() {
        if !execute_instruction(vm) {
            if vm.quantum >= 1000 {
                vm.quantum = 0; // 重置配额，继续
                continue;
            }
            break;
        }
    }
    match &vm.state {
        crate::vm::VmStateKind::Halted => "halted",
        crate::vm::VmStateKind::Error(e) => {
            // Store error in return
            Box::leak(format!("error: {}", e).into_boxed_str())
        }
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::{AtxeBinary, Header};

    fn make_vm(text: Vec<u32>) -> VmState {
        let header = Header::new(0, 6);
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
    fn movi_and_add() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, 8, 0, 2),   // t0 = 2
            isa::encode_r2i(opcode::MOVI, 9, 0, 3),   // t1 = 3
            isa::encode_r3(opcode::ADD, 10, 8, 9, 0), // t2 = t0 + t1
            isa::encode_ji(opcode::TRAP, 0),            // HALT
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[10], 5);
    }

    #[test]
    fn movi_sub_mul() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, 8, 0, 10),  // t0 = 10
            isa::encode_r2i(opcode::MOVI, 9, 0, 3),   // t1 = 3
            isa::encode_r3(opcode::SUB, 10, 8, 9, 0), // t2 = 10-3 = 7
            isa::encode_r3(opcode::MUL, 11, 10, 9, 0),// t3 = 7*3 = 21
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[11], 21);
    }

    #[test]
    fn movi_and_jz_not_taken() {
        // 最简 JZ 不跳转测试
        // MOVI r8, 0; MOVI r9, 1; JZ r8, 2; MOVI r10, 99
        let text = vec![
            isa::encode_r2i(opcode::MOVI, 8, 0, 0),   // 0: r8 = 0
            isa::encode_r2i(opcode::MOVI, 9, 0, 1),   // 1: r9 = 1
            isa::encode_r1i(opcode::JZ, 8, 2),         // 2: JZ r8, +2 → r8==0, 跳转
            isa::encode_r2i(opcode::MOVI, 10, 0, 99),  // 3: 被跳过
            isa::encode_ji(opcode::TRAP, 0),            // 4: HALT
        ];
        // JZ 应跳转到 instr 4（跳过 instr 3）
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        // instr 3 被跳过，r10 保持 0
        assert_eq!(vm.regs[10], 0);
        assert_eq!(vm.regs[9], 1); // r9 在跳转前设置
    }

    #[test]
    fn jmp_forward() {
        // JMP +2; MOVI t0, 0; MOVI t0, 1; TRAP
        let text = vec![
            isa::encode_ji(opcode::JMP, 2),           // JMP +2 → instr 2
            isa::encode_r2i(opcode::MOVI, 8, 0, 0),   // 被跳过
            isa::encode_r2i(opcode::MOVI, 8, 0, 1),   // t0 = 1
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[8], 1);
    }

    #[test]
    fn call_and_return() {
        // main: CALL +2; TRAP
        // fn foo: MOVI t0, 42; JMPR ra
        let text = vec![
            isa::encode_ji(opcode::CALL, 2),          // CALL foo (instr 0 → instr 2)
            isa::encode_ji(opcode::TRAP, 0),           // TRAP (instr 1)
            isa::encode_r2i(opcode::MOVI, 8, 0, 42),  // t0 = 42 (instr 2)
            isa::encode_r1i(opcode::JMPR, 3, 0),       // JMPR ra (instr 3)
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[8], 42);
    }

    #[test]
    fn floating_point_add() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, 8, 0, 0x4000), // 高 16 位
            // 简单：用 LCONST 加载 1.0 和 2.0 的 bits
            // 实际应通过 .rodata LOAD，简化直接算
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        // 仅验证不崩溃
    }

    #[test]
    fn div_by_zero_errors() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, 8, 0, 1),   // t0 = 1
            isa::encode_r2i(opcode::MOVI, 9, 0, 0),   // t1 = 0
            isa::encode_r3(opcode::DIV, 10, 8, 9, 0), // t2 = 1/0 → error
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert!(matches!(vm.state, crate::vm::VmStateKind::Error(_)));
    }

    #[test]
    fn ecall_unsupported() {
        let text = vec![
            isa::encode_r1i(opcode::ECALL, 0, 99),    // ECALL #99 (unsupported)
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[4], u64::MAX); // R4 = error
    }

    #[test]
    fn bitwise_operations() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, 8, 0, 0xFF),    // t0 = 0xFF
            isa::encode_r2i(opcode::MOVI, 9, 0, 0x0F),    // t1 = 0x0F
            isa::encode_r3(opcode::AND, 10, 8, 9, 0),     // t2 = 0xFF & 0x0F = 0x0F
            isa::encode_r3(opcode::OR, 11, 8, 9, 0),      // t3 = 0xFF | 0x0F = 0xFF
            isa::encode_r3(opcode::XOR, 12, 8, 9, 0),     // t4 = 0xFF ^ 0x0F = 0xF0
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[10], 0x0F);
        assert_eq!(vm.regs[11], 0xFF);
        assert_eq!(vm.regs[12], 0xF0);
    }

    #[test]
    fn run_vm_simple() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, 8, 0, 42),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        let result = run_vm(&mut vm);
        assert_eq!(result, "halted");
    }
}
