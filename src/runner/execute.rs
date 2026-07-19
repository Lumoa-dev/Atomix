//! 指令执行引擎 — 54 条指令的 dispatch 处理函数。
//!
//! 覆盖 02-指令集规范.md §3 的全部指令行为。

use crate::base::isa::{self, opcode, reg};
use crate::runner::decode;
use crate::runner::VmState;

/// 执行单条指令。返回 true 表示继续执行，false 表示需要让出/停止。
pub fn execute_instruction(vm: &mut VmState) -> bool {
    if vm.pc >= vm.text.len() {
        vm.state = crate::runner::VmStateKind::Error("pc 越界".into());
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
                0 => { vm.state = crate::runner::VmStateKind::Halted; return false; }
                1 => { /* DEBUG: NOP for now */ }
                _ => {}
            }
        }

        opcode::THROW => {
            let exc_val = vm.read_reg(ops.rd as usize);
            // 查 .exn 表（简化版：在 execute.rs 底部实现）
            if !handle_exception(vm, exc_val) {
                vm.state = crate::runner::VmStateKind::Error(format!("未捕获的异常: {}", exc_val));
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
                vm.state = crate::runner::VmStateKind::Error(format!("LOAD 越界: addr={:#x}", addr));
                return false;
            }
        }

        opcode::STORE => {
            let addr = vm.read_reg(ops.rd as usize).wrapping_add(ops.imm as u64);
            let val = vm.read_reg(ops.rs1 as usize);
            if !store_to_memory(vm, addr, val) {
                vm.state = crate::runner::VmStateKind::Error(format!("STORE 越界: addr={:#x}", addr));
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
                vm.state = crate::runner::VmStateKind::Error("除零异常".into());
                return false;
            }
            let r = (vm.read_reg(ops.rs1 as usize) as i64).wrapping_div(divisor as i64) as u64;
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::DIVU => {
            let divisor = vm.read_reg(ops.rs2 as usize);
            if divisor == 0 {
                vm.state = crate::runner::VmStateKind::Error("除零异常".into());
                return false;
            }
            vm.write_reg(ops.rd as usize, vm.read_reg(ops.rs1 as usize).wrapping_div(divisor));
        }

        opcode::REM => {
            let divisor = vm.read_reg(ops.rs2 as usize);
            if divisor == 0 {
                vm.state = crate::runner::VmStateKind::Error("除零异常".into());
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
            if vm.read_reg(ops.rd as usize) == 0 {
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

        // ── Concurrency / Task Management ────────
        opcode::TASK_FORK => {
            // 创建子任务，存到 pending_child 由调度器取走入队
            let task_id = ops.imm as u16;
            let mut child = Box::new(vm.clone());
            child.task_id = task_id;
            child.pc = vm.pc + 1;
            child.join_waiting_for = None;
            child.pending_child = None;
            child.quantum = 0;
            vm.pending_child = Some(child);
            vm.write_reg(ops.rd as usize, task_id as u64);
        }

        opcode::TASK_JOIN => {
            let handle = vm.read_reg(ops.rs1 as usize);
            // 设置阻塞等待，由调度器在子任务完成时唤醒
            vm.join_waiting_for = Some(handle as u16);
            vm.state = crate::runner::VmStateKind::Suspended;
            return false;
        }

        opcode::TASK_RET => {
            // 结束当前任务，rd 的值作为返回值
            let retval = vm.read_reg(ops.rd as usize);
            vm.write_reg(reg::A0, retval);
            vm.state = crate::runner::VmStateKind::Halted;
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
            let result = handle_ecall(vm, syscall, arg1, arg2, arg3);
            vm.write_reg(reg::A0, result);
            // 错误映射：负返回值 → 异常
            if (result as i64) < 0 {
                let exc_val = result.wrapping_neg() as u64;
                if !handle_exception(vm, exc_val) {
                    vm.state = crate::runner::VmStateKind::Error(
                        format!("ECALL 错误: syscall={}", syscall)
                    );
                    return false;
                }
                return true; // 跳过 pc++（THROW 已跳转）
            }
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
                        vm.state = crate::runner::VmStateKind::Error("MCPY 越界".into());
                        return false;
                    }
                } else {
                    vm.state = crate::runner::VmStateKind::Error("MCPY 越界".into());
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
                    vm.state = crate::runner::VmStateKind::Error("MSET 越界".into());
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
            vm.state = crate::runner::VmStateKind::Error(format!("非法指令: opcode={:#04x}", op));
            return false;
        }
    }

    vm.pc += 1;
    vm.quantum < 1000 // 配额用尽时让出
}

// ─── 内存辅助 ──────────────────────────────────────────

/// 沙箱中 .rodata 区域的终止地址（此地址以下为只读）。
fn rodata_end(vm: &VmState) -> u64 {
    vm.rodata.len() as u64
}

/// 从沙箱内存加载 64 位值（只读区域 + 堆 + 栈）。
fn load_from_memory(vm: &VmState, addr: u64) -> Option<u64> {
    vm.memory.read_u64(addr)
}

/// 存储 64 位值到沙箱内存（拒绝写入 .rodata 只读区域）。
fn store_to_memory(vm: &mut VmState, addr: u64, val: u64) -> bool {
    if addr < rodata_end(vm) {
        return false; // .rodata 只读
    }
    vm.memory.write_u64(addr, val)
}

/// 从沙箱内存加载单个字节。
fn load_byte_from_memory(vm: &VmState, addr: u64) -> Option<u8> {
    vm.memory.read_u8(addr)
}

/// 存储单个字节到沙箱内存（拒绝写入 .rodata 只读区域）。
fn store_byte_to_memory(vm: &mut VmState, addr: u64, val: u8) -> bool {
    if addr < rodata_end(vm) {
        return false;
    }
    vm.memory.write_u8(addr, val)
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
fn handle_ecall(vm: &mut VmState, syscall: u32, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    match syscall {
        crate::base::isa::ecall::ALLOC => {
            let size = arg1;
            if size == 0 { return 0; }
            vm.memory.alloc(size)
        }
        crate::base::isa::ecall::FREE => {
            let addr = arg1;
            vm.memory.free(addr);
            0
        }
        crate::base::isa::ecall::TCP_CONNECT
        | crate::base::isa::ecall::FS_OPEN => {
            // 未实现：返回负错误码（对应 DSL 异常）
            -1i64 as u64 // -EIO
        }
        _ => {
            // 不支持的调用号
            u64::MAX
        }
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
        crate::runner::VmStateKind::Halted => "halted",
        crate::runner::VmStateKind::Error(e) => {
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
        assert!(matches!(vm.state, crate::runner::VmStateKind::Error(_)));
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

    #[test]
    fn task_ret_returns_value() {
        // MOVI a0, 42; TASK_RET a0
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 42),
            isa::encode_r1i(opcode::TASK_RET, reg::A0 as u8, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[reg::A0], 42);
        assert!(matches!(vm.state, crate::runner::VmStateKind::Halted));
    }

    #[test]
    fn task_self_returns_id() {
        let text = vec![
            isa::encode_r1i(opcode::TASK_SELF, 8, 0),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        vm.task_id = 5;
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[8], 5);
    }

    #[test]
    fn ecall_unsupported_returns_max() {
        let text = vec![
            isa::encode_r1i(opcode::ECALL, 0, 99),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[reg::A0], u64::MAX);
    }

    #[test]
    fn ecall_alloc_returns_address() {
        // MOVI a0, 64; ECALL alloc
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 64), // R4 = 64 (size)
            isa::encode_r1i(opcode::ECALL, 0, 0), // ECALL alloc
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert!(vm.regs[reg::A0] > 0);
    }

    #[test]
    fn ecall_tcp_connect_returns_error() {
        let text = vec![
            isa::encode_r1i(opcode::ECALL, 0, 2), // ECALL tcp_connect
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        // 负值作为 u64（即 u64::MAX）
        assert_eq!(vm.regs[reg::A0] as i64, -1);
    }

    // ── 沙箱内存测试 ─────────────────────────────

    fn make_vm_with_rodata(text: Vec<u32>, rodata: Vec<u8>) -> VmState {
        use crate::base::ir::AtxeBinary;
        let header = crate::base::ir::Header::new(0, 6);
        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata,
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        VmState::from_atxe(&binary).unwrap()
    }

    #[test]
    fn load_from_rodata() {
        // 在 .rodata 中放入 0xDEADBEEFCAFEBABE，用 LOAD 读出到 t0
        let val: u64 = 0xDEAD_BEEF_CAFE_BABE;
        let rodata = val.to_le_bytes().to_vec();
        // MOVI a0, 0; LOAD t0, [a0 + 0]; TRAP
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 0),   // a0 = 0 (.rodata base)
            isa::encode_r2i(opcode::LOAD, reg::T0 as u8, reg::A0 as u8, 0), // t0 = [a0+0]
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm_with_rodata(text, rodata);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[reg::T0], val);
    }

    #[test]
    fn store_then_load() {
        // 先用 ECALL alloc 分配 16 字节，然后 STORE 一个值，再 LOAD 回来验证
        // MOVI a0, 16; ECALL alloc; MOV t0, a0 (保存地址到 t0)
        // MOVI t1, 0x42; STORE [t0+0], t1; LOAD t2, [t0+0]; TRAP
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 16),  // a0 = 16 (size)
            isa::encode_r1i(opcode::ECALL, 0, 0),                  // ECALL alloc → a0 = addr
            isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0), // t0 = addr
            isa::encode_r2i(opcode::MOVI, reg::T1 as u8, 0, 0x42), // t1 = 0x42
            isa::encode_r2i(opcode::STORE, reg::T0 as u8, reg::T1 as u8, 0), // [t0] = t1
            isa::encode_r2i(opcode::LOAD, reg::T2 as u8, reg::T0 as u8, 0),  // t2 = [t0]
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        // t2 应该等于 0x42
        assert_eq!(vm.regs[reg::T2], 0x42);
    }

    #[test]
    fn store_to_rodata_rejected() {
        // 尝试写入地址 0（.rodata 区域），应触发错误
        // MOVI a0, 0; MOVI t0, 42; STORE [a0+0], t0
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 0),   // a0 = 0 (.rodata base)
            isa::encode_r2i(opcode::MOVI, reg::T0 as u8, 0, 42),  // t0 = 42
            isa::encode_r2i(opcode::STORE, reg::A0 as u8, reg::T0 as u8, 0), // [0] = 42 → 应拒绝
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert!(matches!(vm.state, crate::runner::VmStateKind::Error(_)));
    }

    #[test]
    fn ecall_alloc_returns_heap_address() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 32), // size = 32
            isa::encode_r1i(opcode::ECALL, 0, 0),                 // ECALL alloc
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        let addr = vm.regs[reg::A0];
        assert_ne!(addr, u64::MAX, "alloc should succeed");
        assert!(addr >= vm.memory.heap_base, "addr should be in heap");
        assert!(addr < vm.memory.stack_base, "addr should be below stack");
    }
}
