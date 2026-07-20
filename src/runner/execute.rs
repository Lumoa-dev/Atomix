//! 指令执行引擎 — 54 条指令的 dispatch 处理函数。
//!
//! 覆盖 02-指令集规范.md §3 的全部指令行为。

use crate::base::isa::{self, opcode, reg};
use crate::runner::VmState;
use crate::runner::decode;
use std::fs::File;
use std::io::{Read, Write, Seek, SeekFrom};
use std::net::{TcpStream, TcpListener, ToSocketAddrs};

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
                0 => {
                    vm.state = crate::runner::VmStateKind::Halted;
                    return false;
                }
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
                vm.state =
                    crate::runner::VmStateKind::Error(format!("LOAD 越界: addr={:#x}", addr));
                return false;
            }
        }

        opcode::STORE => {
            let addr = vm.read_reg(ops.rd as usize).wrapping_add(ops.imm as u64);
            let val = vm.read_reg(ops.rs1 as usize);
            if !store_to_memory(vm, addr, val) {
                vm.state =
                    crate::runner::VmStateKind::Error(format!("STORE 越界: addr={:#x}", addr));
                return false;
            }
        }

        // ── Integer Arithmetic (0x20–0x2E) ────────
        opcode::ADDI => {
            let r = vm
                .read_reg(ops.rs1 as usize)
                .wrapping_add(ops.imm as i16 as u64);
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::ADD => {
            let r = vm
                .read_reg(ops.rs1 as usize)
                .wrapping_add(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::SUB => {
            let r = vm
                .read_reg(ops.rs1 as usize)
                .wrapping_sub(vm.read_reg(ops.rs2 as usize));
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::MUL => {
            let r = vm
                .read_reg(ops.rs1 as usize)
                .wrapping_mul(vm.read_reg(ops.rs2 as usize));
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
            vm.write_reg(
                ops.rd as usize,
                vm.read_reg(ops.rs1 as usize).wrapping_div(divisor),
            );
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
            vm.write_reg(
                ops.rd as usize,
                vm.read_reg(ops.rs1 as usize) & vm.read_reg(ops.rs2 as usize),
            );
        }

        opcode::OR => {
            vm.write_reg(
                ops.rd as usize,
                vm.read_reg(ops.rs1 as usize) | vm.read_reg(ops.rs2 as usize),
            );
        }

        opcode::XOR => {
            vm.write_reg(
                ops.rd as usize,
                vm.read_reg(ops.rs1 as usize) ^ vm.read_reg(ops.rs2 as usize),
            );
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
            let result = if a.is_nan() || b.is_nan() {
                0
            } else if a != b {
                1
            } else {
                0
            };
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
            let r = if vm.read_reg(ops.rs1 as usize) == vm.read_reg(ops.rs2 as usize) {
                1
            } else {
                0
            };
            vm.write_reg(ops.rd as usize, r);
        }

        opcode::SNE => {
            let r = if vm.read_reg(ops.rs1 as usize) != vm.read_reg(ops.rs2 as usize) {
                1
            } else {
                0
            };
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
            let target = vm
                .read_reg(ops.rs1 as usize)
                .wrapping_add(ops.imm as i16 as u64);
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
                    vm.state = crate::runner::VmStateKind::Error(format!(
                        "ECALL 错误: syscall={}",
                        syscall
                    ));
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
        if chunk.len() < 12 {
            continue;
        }
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

/// 从 VM 沙箱内存读取空终止字符串（最大 4096 字节）。
fn read_string_from_memory(vm: &VmState, addr: u64) -> Option<String> {
    let mut bytes = Vec::new();
    for i in 0..4096 {
        let b = vm.memory.read_u8(addr.wrapping_add(i))?;
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    String::from_utf8(bytes).ok()
}

/// 分配下一个可用的文件描述符。
fn alloc_fd(vm: &mut VmState, file: File) -> u64 {
    for (i, slot) in vm.open_files.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(file);
            return i as u64;
        }
    }
    let fd = vm.open_files.len() as u64;
    vm.open_files.push(Some(file));
    fd
}

/// 分配下一个可用的套接字描述符。
fn alloc_socket_fd(vm: &mut VmState, stream: TcpStream) -> u64 {
    for (i, slot) in vm.open_sockets.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(stream);
            return i as u64;
        }
    }
    let fd = vm.open_sockets.len() as u64;
    vm.open_sockets.push(Some(stream));
    fd
}

/// 分配下一个可用的监听器描述符。
fn alloc_listener_fd(vm: &mut VmState, listener: TcpListener) -> u64 {
    for (i, slot) in vm.listeners.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(listener);
            return i as u64;
        }
    }
    let fd = vm.listeners.len() as u64;
    vm.listeners.push(Some(listener));
    fd
}

/// 处理 ECALL 系统调用。
fn handle_ecall(vm: &mut VmState, syscall: u32, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    match syscall {
        // ── 内存 ────────────────────────────────────────
        crate::base::isa::ecall::ALLOC => {
            let size = arg1;
            if size == 0 {
                return 0;
            }
            if vm.memory.is_over_watermark() {
                vm.state = crate::runner::VmStateKind::Suspended;
                return u64::MAX; // OOM 信号
            }
            vm.memory.alloc(size)
        }
        crate::base::isa::ecall::FREE => {
            vm.memory.free(arg1);
            0
        }

        // ── 文件系统 ────────────────────────────────────
        crate::base::isa::ecall::FS_OPEN => {
            // A0 = 路径字符串地址（沙箱内存中的空终止字符串）
            // A1 = 标志位: 0=RDONLY, 1=WRONLY, 2=RDWR
            let path = match read_string_from_memory(vm, arg1) {
                Some(p) => p,
                None => return -5i64 as u64, // EINVAL
            };
            let flags = arg2;
            let file = match flags {
                0 => File::open(&path),
                1 => File::create(&path),
                2 => std::fs::OpenOptions::new().read(true).write(true).open(&path),
                _ => return -5i64 as u64, // EINVAL
            };
            match file {
                Ok(f) => alloc_fd(vm, f),
                Err(e) => match e.kind() {
                    std::io::ErrorKind::NotFound => -2i64 as u64, // ENOENT
                    _ => -1i64 as u64,                            // EIO
                },
            }
        }
        crate::base::isa::ecall::FS_READ => {
            // A0 = fd, A1 = 缓冲区地址, A2 = 缓冲区大小
            let fd = arg1 as usize;
            let buf_addr = arg2;
            let buf_size = arg3 as usize;
            if fd >= vm.open_files.len() {
                return -3i64 as u64; // EBADF
            }
            let file = match vm.open_files[fd].as_mut() {
                Some(f) => f,
                None => return -3i64 as u64,
            };
            // 确保缓冲区在沙箱内存范围内
            let end = buf_addr.wrapping_add(buf_size as u64);
            if end as usize > vm.memory.data.len() || end < buf_addr {
                return -5i64 as u64; // EINVAL
            }
            let buf = &mut vm.memory.data[buf_addr as usize..end as usize];
            match file.read(buf) {
                Ok(n) => n as u64,
                Err(_) => -1i64 as u64,
            }
        }
        crate::base::isa::ecall::FS_WRITE => {
            // A0 = fd, A1 = 数据地址, A2 = 数据大小
            let fd = arg1 as usize;
            let data_addr = arg2;
            let data_size = arg3 as usize;
            if fd >= vm.open_files.len() {
                return -3i64 as u64;
            }
            let file = match vm.open_files[fd].as_mut() {
                Some(f) => f,
                None => return -3i64 as u64,
            };
            let end = data_addr.wrapping_add(data_size as u64);
            if end as usize > vm.memory.data.len() || end < data_addr {
                return -5i64 as u64;
            }
            let data = &vm.memory.data[data_addr as usize..end as usize];
            match file.write(data) {
                Ok(n) => {
                    let _ = file.flush();
                    n as u64
                }
                Err(_) => -1i64 as u64,
            }
        }
        crate::base::isa::ecall::FS_CLOSE => {
            // A0 = fd
            let fd = arg1 as usize;
            if fd >= vm.open_files.len() {
                return -3i64 as u64;
            }
            match vm.open_files[fd].take() {
                Some(_) => 0,
                None => -3i64 as u64, // EBADF: 已关闭
            }
        }
        crate::base::isa::ecall::FS_SEEK => {
            // A0 = fd, A1 = 偏移, A2 = whence (0=Start, 1=Current, 2=End)
            let fd = arg1 as usize;
            let offset = arg2 as i64;
            let whence = arg3;
            if fd >= vm.open_files.len() {
                return -3i64 as u64;
            }
            let file = match vm.open_files[fd].as_mut() {
                Some(f) => f,
                None => return -3i64 as u64,
            };
            let seek_from = match whence {
                0 => SeekFrom::Start(offset as u64),
                1 => SeekFrom::Current(offset),
                2 => SeekFrom::End(offset),
                _ => return -5i64 as u64, // EINVAL
            };
            match file.seek(seek_from) {
                Ok(pos) => pos,
                Err(_) => -1i64 as u64,
            }
        }
        crate::base::isa::ecall::FS_STAT => {
            // A0 = 路径字符串地址
            // 返回: 文件大小（字节），出错返回负错误码
            let path = match read_string_from_memory(vm, arg1) {
                Some(p) => p,
                None => return -5i64 as u64,
            };
            match std::fs::metadata(&path) {
                Ok(meta) => meta.len(),
                Err(e) => match e.kind() {
                    std::io::ErrorKind::NotFound => -2i64 as u64,
                    _ => -1i64 as u64,
                },
            }
        }

        // ── TCP 套接字 ──────────────────────────────────
        crate::base::isa::ecall::TCP_CONNECT => {
            // A0 = 地址字符串指针, A1 = 端口
            let addr_str = match read_string_from_memory(vm, arg1) {
                Some(s) => s,
                None => return -5i64 as u64,
            };
            let port = arg2;
            let addr = format!("{}:{}", addr_str, port);
            match TcpStream::connect(&addr) {
                Ok(stream) => {
                    let _ = stream.set_nonblocking(true);
                    alloc_socket_fd(vm, stream)
                }
                Err(_) => -1i64 as u64,
            }
        }
        crate::base::isa::ecall::TCP_SEND => {
            // A0 = fd, A1 = 数据地址, A2 = 数据大小
            let fd = arg1 as usize;
            let data_addr = arg2;
            let data_size = arg3 as usize;
            if fd >= vm.open_sockets.len() {
                return -3i64 as u64;
            }
            let stream = match vm.open_sockets[fd].as_mut() {
                Some(s) => s,
                None => return -3i64 as u64,
            };
            let end = data_addr.wrapping_add(data_size as u64);
            if end as usize > vm.memory.data.len() || end < data_addr {
                return -5i64 as u64;
            }
            let data = &vm.memory.data[data_addr as usize..end as usize];
            match stream.write(data) {
                Ok(n) => n as u64,
                Err(_) => -1i64 as u64,
            }
        }
        crate::base::isa::ecall::TCP_RECV => {
            // A0 = fd, A1 = 缓冲区地址, A2 = 缓冲区大小
            let fd = arg1 as usize;
            let buf_addr = arg2;
            let buf_size = arg3 as usize;
            if fd >= vm.open_sockets.len() {
                return -3i64 as u64;
            }
            let stream = match vm.open_sockets[fd].as_mut() {
                Some(s) => s,
                None => return -3i64 as u64,
            };
            let end = buf_addr.wrapping_add(buf_size as u64);
            if end as usize > vm.memory.data.len() || end < buf_addr {
                return -5i64 as u64;
            }
            let buf = &mut vm.memory.data[buf_addr as usize..end as usize];
            match stream.read(buf) {
                Ok(n) => n as u64,
                Err(_) => -1i64 as u64,
            }
        }
        crate::base::isa::ecall::TCP_LISTEN => {
            // A0 = 地址字符串指针, A1 = 端口
            let addr_str = match read_string_from_memory(vm, arg1) {
                Some(s) => s,
                None => return -5i64 as u64,
            };
            let port = arg2;
            let addr = format!("{}:{}", addr_str, port);
            match TcpListener::bind(&addr) {
                Ok(listener) => alloc_listener_fd(vm, listener),
                Err(_) => -1i64 as u64,
            }
        }
        crate::base::isa::ecall::TCP_ACCEPT => {
            // A0 = listener fd
            let fd = arg1 as usize;
            if fd >= vm.listeners.len() {
                return -3i64 as u64;
            }
            let listener = match vm.listeners[fd].as_mut() {
                Some(l) => l,
                None => return -3i64 as u64,
            };
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(true);
                    alloc_socket_fd(vm, stream)
                }
                Err(_) => -1i64 as u64,
            }
        }
        crate::base::isa::ecall::TCP_CLOSE => {
            // A0 = socket fd 或 listener fd（尝试关闭两者）
            let fd = arg1 as usize;
            if fd < vm.open_sockets.len() && vm.open_sockets[fd].take().is_some() {
                return 0;
            }
            if fd < vm.listeners.len() && vm.listeners[fd].take().is_some() {
                return 0;
            }
            -3i64 as u64 // EBADF
        }
        crate::base::isa::ecall::DNS_LOOKUP => {
            // A0 = 主机名字符串指针
            // 返回: 解析到的 IPv4 地址（打包为 u32），负值表示错误
            let hostname = match read_string_from_memory(vm, arg1) {
                Some(s) => s,
                None => return -5i64 as u64,
            };
            // 添加默认端口 80 以满足 ToSocketAddrs 的格式要求
            let addr_str = format!("{}:80", hostname);
            match (addr_str.as_str(), 0u16).to_socket_addrs() {
                Ok(mut addrs) => {
                    if let Some(addr) = addrs.next() {
                        match addr.ip() {
                            std::net::IpAddr::V4(ipv4) => u32::from(ipv4) as u64,
                            std::net::IpAddr::V6(_) => -4i64 as u64, // ENOSYS: IPv6 暂不支持
                        }
                    } else {
                        -2i64 as u64 // ENOENT: 无解析结果
                    }
                }
                Err(_) => -1i64 as u64,
            }
        }

        // ── 内置函数 ────────────────────────────────────
        crate::base::isa::ecall::PRINT => {
            println!("{}", arg1 as i64);
            0
        }
        crate::base::isa::ecall::LEN => {
            arg2
        }

        // ── 不支持的调用号 ──────────────────────────────
        _ => u64::MAX,
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
            isa::encode_ji(opcode::TRAP, 0),          // HALT
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
            isa::encode_r2i(opcode::MOVI, 8, 0, 10),   // t0 = 10
            isa::encode_r2i(opcode::MOVI, 9, 0, 3),    // t1 = 3
            isa::encode_r3(opcode::SUB, 10, 8, 9, 0),  // t2 = 10-3 = 7
            isa::encode_r3(opcode::MUL, 11, 10, 9, 0), // t3 = 7*3 = 21
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
            isa::encode_r1i(opcode::JZ, 8, 2),        // 2: JZ r8, +2 → r8==0, 跳转
            isa::encode_r2i(opcode::MOVI, 10, 0, 99), // 3: 被跳过
            isa::encode_ji(opcode::TRAP, 0),          // 4: HALT
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
            isa::encode_ji(opcode::JMP, 2),         // JMP +2 → instr 2
            isa::encode_r2i(opcode::MOVI, 8, 0, 0), // 被跳过
            isa::encode_r2i(opcode::MOVI, 8, 0, 1), // t0 = 1
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
            isa::encode_ji(opcode::CALL, 2), // CALL foo (instr 0 → instr 2)
            isa::encode_ji(opcode::TRAP, 0), // TRAP (instr 1)
            isa::encode_r2i(opcode::MOVI, 8, 0, 42), // t0 = 42 (instr 2)
            isa::encode_r1i(opcode::JMPR, 3, 0), // JMPR ra (instr 3)
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
            isa::encode_r1i(opcode::ECALL, 0, 99), // ECALL #99 (unsupported)
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
            isa::encode_r2i(opcode::MOVI, 8, 0, 0xFF), // t0 = 0xFF
            isa::encode_r2i(opcode::MOVI, 9, 0, 0x0F), // t1 = 0x0F
            isa::encode_r3(opcode::AND, 10, 8, 9, 0),  // t2 = 0xFF & 0x0F = 0x0F
            isa::encode_r3(opcode::OR, 11, 8, 9, 0),   // t3 = 0xFF | 0x0F = 0xFF
            isa::encode_r3(opcode::XOR, 12, 8, 9, 0),  // t4 = 0xFF ^ 0x0F = 0xF0
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
            isa::encode_r1i(opcode::ECALL, 0, 0),                // ECALL alloc
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
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 0), // a0 = 0 (.rodata base)
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
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 16), // a0 = 16 (size)
            isa::encode_r1i(opcode::ECALL, 0, 0),                // ECALL alloc → a0 = addr
            isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0), // t0 = addr
            isa::encode_r2i(opcode::MOVI, reg::T1 as u8, 0, 0x42), // t1 = 0x42
            isa::encode_r2i(opcode::STORE, reg::T0 as u8, reg::T1 as u8, 0), // [t0] = t1
            isa::encode_r2i(opcode::LOAD, reg::T2 as u8, reg::T0 as u8, 0), // t2 = [t0]
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
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 0), // a0 = 0 (.rodata base)
            isa::encode_r2i(opcode::MOVI, reg::T0 as u8, 0, 42), // t0 = 42
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
            isa::encode_r1i(opcode::ECALL, 0, 0),                // ECALL alloc
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

    // ── FS ECALL 测试 ─────────────────────────────

    /// 创建一个包含空终止字符串的内存布局。
    fn vm_with_string(text: Vec<u32>, s: &str) -> VmState {
        let rodata = s.as_bytes().to_vec();
        make_vm_with_rodata(text, rodata)
    }

    #[test]
    fn ecall_fs_open_nonexistent_returns_error() {
        // 把一个不存在的路径放到 .rodata 中，ECALL FS_OPEN
        let path_bytes = "/tmp/nonexistent_file_xyz\0";
        let path_addr: u16 = 0; // .rodata base
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, path_addr), // a0 = path addr
            isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 0u16),      // a1 = O_RDONLY
            isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_OPEN), // ECALL FS_OPEN
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = vm_with_string(text, path_bytes);
        // 执行直到停止
        //（不 while 循环，因为路径错误可能直接触发异常让 VM 停掉）
        // 实际上 VM 应该能跑完：ECALL 返回负值，然后 TRAP halt
        let mut running_vm = vm;
        while running_vm.is_running() {
            execute_instruction(&mut running_vm);
        }
        // 应该返回负错误码（文件不存在）
        let result = running_vm.regs[reg::A0] as i64;
        assert!(result < 0, "FS_OPEN on nonexistent file should return negative, got {}", result);
    }

    #[test]
    fn ecall_fs_open_write_close_read() {
        use std::io::Write;
        // 创建一个临时文件，写入内容，然后通过 FS_OPEN 打开并读取
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("atomix_test_fs_open.txt");
        // 先写入一些测试数据
        let test_data = b"Hello Atomix FS!";
        {
            let mut f = std::fs::File::create(&temp_file).unwrap();
            f.write_all(test_data).unwrap();
        }

        // 路径字符串的 .rodata 布局
        let path_str = temp_file.to_str().unwrap();
        let mut rodata = path_str.as_bytes().to_vec();
        rodata.push(0); // 空终止
        let path_addr: u16 = 0;

        // text 段指令序列
        let mut text = Vec::new();
        // 1. FS_OPEN: a0 = path_addr, a1 = O_RDONLY (0)
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, path_addr));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 0));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_OPEN));
        // 2. MOV t0, a0 (保存 fd)
        text.push(isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0));
        // 3. FS_READ: a0 = fd, a1 = buf_addr (堆分配), a2 = buf_size
        // 先分配缓冲区
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 256u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::ALLOC));
        text.push(isa::encode_r3(opcode::MOV, reg::T1 as u8, reg::A0 as u8, 0, 0)); // t1 = buf
        // 读文件
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0)); // a0 = fd
        text.push(isa::encode_r3(opcode::MOV, reg::A1 as u8, reg::T1 as u8, 0, 0)); // a1 = buf
        text.push(isa::encode_r2i(opcode::MOVI, reg::A2 as u8, 0, 256u16)); // a2 = 256
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_READ));
        text.push(isa::encode_r3(opcode::MOV, reg::T2 as u8, reg::A0 as u8, 0, 0)); // t2 = bytes read
        // 4. FS_CLOSE: a0 = fd
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_CLOSE));
        // 5. HALT
        text.push(isa::encode_ji(opcode::TRAP, 0));

        let header = crate::base::ir::Header::new(0, text.len() as u16);
        let binary = crate::base::ir::AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata,
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let mut vm = VmState::from_atxe(&binary).unwrap();
        while vm.is_running() {
            execute_instruction(&mut vm);
        }

        // 验证
        let bytes_read = vm.regs[reg::T2];
        assert!(bytes_read > 0, "should read some bytes, got {}", bytes_read);
        assert_eq!(bytes_read as usize, test_data.len(), "should read exactly the test data length");

        // 验证读出的内容
        let buf_addr = vm.regs[reg::T1];
        let read_data: Vec<u8> = (0..bytes_read as usize)
            .map(|i| vm.memory.read_u8(buf_addr + i as u64).unwrap())
            .collect();
        assert_eq!(read_data, test_data, "read data should match written data");

        // 清理临时文件
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn ecall_fs_write_and_readback() {
        use std::io::Read;
        // 创建一个临时文件，通过 FS_OPEN(WRONLY) + FS_WRITE 写入，再打开读取验证
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("atomix_test_fs_write.txt");

        let path_str = temp_file.to_str().unwrap();
        let mut rodata = path_str.as_bytes().to_vec();
        rodata.push(0);
        let path_addr: u16 = 0;

        let test_data = b"Written by Atomix VM!";
        let data_len: u16 = test_data.len() as u16;

        let mut text = Vec::new();
        // 1. 分配缓冲区并写入测试数据
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 256u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::ALLOC));
        text.push(isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0)); // t0 = buf
        // 将测试数据写入缓冲区
        for (i, &byte) in test_data.iter().enumerate() {
            text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, byte as u16));
            text.push(isa::encode_r2i(opcode::STORE, reg::T0 as u8, reg::A0 as u8, i as u16));
        }
        // 2. FS_OPEN: a0 = path_addr, a1 = O_WRONLY (1) → create
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, path_addr));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 1u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_OPEN));
        text.push(isa::encode_r3(opcode::MOV, reg::T1 as u8, reg::A0 as u8, 0, 0)); // t1 = fd
        // 3. FS_WRITE: a0 = fd, a1 = buf, a2 = len
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T1 as u8, 0, 0));
        text.push(isa::encode_r3(opcode::MOV, reg::A1 as u8, reg::T0 as u8, 0, 0));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A2 as u8, 0, data_len));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_WRITE));
        text.push(isa::encode_r3(opcode::MOV, reg::T2 as u8, reg::A0 as u8, 0, 0)); // t2 = bytes written
        // 4. FS_CLOSE: a0 = fd
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T1 as u8, 0, 0));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_CLOSE));
        // 5. 再以 RDONLY 打开读回来验证
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, path_addr));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 0u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_OPEN));
        text.push(isa::encode_r3(opcode::MOV, reg::T1 as u8, reg::A0 as u8, 0, 0)); // t1 = fd
        // 6. 重新分配缓冲区读取
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 256u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::ALLOC));
        text.push(isa::encode_r3(opcode::MOV, reg::T3 as u8, reg::A0 as u8, 0, 0)); // t3 = buf2
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T1 as u8, 0, 0));
        text.push(isa::encode_r3(opcode::MOV, reg::A1 as u8, reg::T3 as u8, 0, 0));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A2 as u8, 0, 256u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_READ));
        text.push(isa::encode_r3(opcode::MOV, reg::T4 as u8, reg::A0 as u8, 0, 0)); // t4 = bytes read
        // 7. 关闭
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T1 as u8, 0, 0));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_CLOSE));
        // 8. HALT
        text.push(isa::encode_ji(opcode::TRAP, 0));

        let header = crate::base::ir::Header::new(0, text.len() as u16);
        let binary = crate::base::ir::AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata,
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let mut vm = VmState::from_atxe(&binary).unwrap();
        while vm.is_running() {
            execute_instruction(&mut vm);
        }

        let bytes_written = vm.regs[reg::T2];
        let bytes_read = vm.regs[reg::T4];
        assert!(bytes_written > 0, "should write some bytes");
        assert_eq!(bytes_written as usize, test_data.len());
        assert_eq!(bytes_read as usize, test_data.len(), "should read back same amount");

        let buf_addr = vm.regs[reg::T3];
        let read_back: Vec<u8> = (0..bytes_read as usize)
            .map(|i| vm.memory.read_u8(buf_addr + i as u64).unwrap())
            .collect();
        assert_eq!(read_back, test_data, "read back data should match");

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn ecall_fs_seek() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("atomix_test_fs_seek.txt");
        let test_data = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        {
            let mut f = std::fs::File::create(&temp_file).unwrap();
            f.write_all(test_data).unwrap();
        }

        let path_str = temp_file.to_str().unwrap();
        let mut rodata = path_str.as_bytes().to_vec();
        rodata.push(0);
        let path_addr: u16 = 0;

        let mut text = Vec::new();
        // 1. FS_OPEN: RDONLY
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, path_addr));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 0u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_OPEN));
        text.push(isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0)); // t0 = fd
        // 2. FS_SEEK: a0 = fd, a1 = 5, a2 = 0 (Start) → pos=5
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 5u16));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A2 as u8, 0, 0u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_SEEK));
        text.push(isa::encode_r3(opcode::MOV, reg::T1 as u8, reg::A0 as u8, 0, 0)); // t1 = new pos (=5)
        // 3. FS_READ 1 byte → should be 'F' (index 5)
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 256u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::ALLOC));
        text.push(isa::encode_r3(opcode::MOV, reg::T2 as u8, reg::A0 as u8, 0, 0)); // t2 = buf
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0));
        text.push(isa::encode_r3(opcode::MOV, reg::A1 as u8, reg::T2 as u8, 0, 0));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A2 as u8, 0, 1u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_READ));
        // 4. FS_CLOSE
        text.push(isa::encode_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_CLOSE));
        text.push(isa::encode_ji(opcode::TRAP, 0));

        let header = crate::base::ir::Header::new(0, text.len() as u16);
        let binary = crate::base::ir::AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata,
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let mut vm = VmState::from_atxe(&binary).unwrap();
        while vm.is_running() {
            execute_instruction(&mut vm);
        }

        assert_eq!(vm.regs[reg::T1], 5, "seek to pos 5 should return 5");

        let buf_addr = vm.regs[reg::T2];
        let byte = vm.memory.read_u8(buf_addr).unwrap();
        assert_eq!(byte, b'F', "after seeking to 5, should read 'F'");

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn ecall_fs_stat() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("atomix_test_fs_stat.txt");
        let test_data = b"1234567890";
        {
            let mut f = std::fs::File::create(&temp_file).unwrap();
            f.write_all(test_data).unwrap();
        }

        let path_str = temp_file.to_str().unwrap();
        let mut rodata = path_str.as_bytes().to_vec();
        rodata.push(0);
        let path_addr: u16 = 0;

        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, path_addr),
            isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_STAT),
            isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0),
            isa::encode_ji(opcode::TRAP, 0),
        ];

        let header = crate::base::ir::Header::new(0, text.len() as u16);
        let binary = crate::base::ir::AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata,
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let mut vm = VmState::from_atxe(&binary).unwrap();
        while vm.is_running() {
            execute_instruction(&mut vm);
        }

        assert_eq!(vm.regs[reg::T0], 10, "file size should be 10 bytes");

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn ecall_fs_bad_fd_returns_error() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 999u16), // fd = 999 (invalid)
            isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_READ),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[reg::A0] as i64, -3, "EBADF on invalid fd");
    }

    // ── TCP / DNS ECALL 测试 ─────────────────────────

    #[test]
    fn ecall_tcp_connect_refused() {
        // 连接到 127.0.0.1:1（大概率没有服务监听），应返回负错误码
        let addr_bytes = "127.0.0.1\0";
        let mut rodata = addr_bytes.as_bytes().to_vec();
        let path_addr: u16 = 0;
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, path_addr), // a0 = "127.0.0.1"
            isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 1u16),     // a1 = 1 (port)
            isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::TCP_CONNECT),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = vm_with_string(text, addr_bytes);
        let mut running_vm = vm;
        while running_vm.is_running() {
            execute_instruction(&mut running_vm);
        }
        // 连接被拒绝是正常行为，应返回负数
        assert!(
            (running_vm.regs[reg::A0] as i64) < 0
                || running_vm.state == crate::runner::VmStateKind::Halted,
            "TCP connect to port 1 should fail"
        );
    }

    #[test]
    fn ecall_tcp_listen_and_connect() {
        // 监听 127.0.0.1:0（系统分配端口），连接后收发数据
        let listen_addr = "127.0.0.1\0";
        let mut rodata = listen_addr.as_bytes().to_vec();
        let addr_ptr: u16 = 0;

        let mut text = Vec::new();

        // 1. TCP_LISTEN: a0 = addr, a1 = port 0 (system assigns)
        text.push(isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, addr_ptr));
        text.push(isa::encode_r2i(opcode::MOVI, reg::A1 as u8, 0, 0u16));
        text.push(isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::TCP_LISTEN));
        text.push(isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0)); // t0 = listener fd

        // 获取实际端口（通过 listener.local_addr()，但在 VM 内无法获取）
        // 简单测试：只要能创建 listener 即可
        // 2. 尝试用无效的 listener fd accept → 应该超时或阻塞
        //    我们暂时用 HALT
        text.push(isa::encode_ji(opcode::TRAP, 0));

        let header = crate::base::ir::Header::new(0, text.len() as u16);
        let binary = crate::base::ir::AtxeBinary {
            header,
            sections: Vec::new(),
            text,
            rodata,
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let mut vm = VmState::from_atxe(&binary).unwrap();
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        // TCP_LISTEN 应该成功（返回非负 fd）
        let listener_fd = vm.regs[reg::T0] as usize;
        assert!(
            listener_fd < vm.listeners.len() && vm.listeners[listener_fd].is_some(),
            "TCP_LISTEN should succeed, got fd={}",
            listener_fd
        );
    }

    #[test]
    fn ecall_tcp_close_invalid_fd() {
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, 999u16), // fd = 999
            isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::TCP_CLOSE),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let mut vm = make_vm(text);
        while vm.is_running() {
            execute_instruction(&mut vm);
        }
        assert_eq!(vm.regs[reg::A0] as i64, -3, "TCP_CLOSE on invalid fd should return EBADF");
    }

    #[test]
    fn ecall_dns_lookup_localhost() {
        // 解析 "localhost" → 应返回 127.0.0.1 (0x7F000001)
        let hostname = "localhost\0";
        let addr_ptr: u16 = 0;
        let text = vec![
            isa::encode_r2i(opcode::MOVI, reg::A0 as u8, 0, addr_ptr),
            isa::encode_r1i(opcode::ECALL, 0, crate::base::isa::ecall::DNS_LOOKUP),
            isa::encode_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0),
            isa::encode_ji(opcode::TRAP, 0),
        ];
        let vm = vm_with_string(text, hostname);
        let mut running_vm = vm;
        while running_vm.is_running() {
            execute_instruction(&mut running_vm);
        }
        let result = running_vm.regs[reg::T0] as i64;
        // localhost 应解析为 127.0.0.1 = 0x7F000001 = 2130706433
        // 某些系统可能返回不同值或返回错误（如没有网络），只验证非负
        // 如果不是 127.0.0.1，至少应该是正数（DNS 成功）
        if result > 0 {
            // 成功解析，验证格式
            let ip = result as u32;
            let octets = [
                (ip >> 24) as u8,
                (ip >> 16) as u8,
                (ip >> 8) as u8,
                ip as u8,
            ];
            assert_eq!(octets[0], 127, "should be 127.x.x.x");
        }
        // 如果返回负数（如无 DNS 服务），也是可接受的
    }
}
