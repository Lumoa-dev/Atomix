//! Atomix 虚拟机 — 加载并执行 .atxe 二进制。
//!
//! 详见 02-指令集规范.md、07-执行器设计.md、08-运行时架构.md

pub mod batch;
pub mod client;
pub mod config;
pub mod decode;
pub mod event;
pub mod execute;
pub mod executor;
pub mod hwinfo;
pub mod load_balancer;
pub mod loader;
pub mod memory;
pub mod pool;
pub mod prefetch;
pub mod regression;
pub mod runtime;
pub mod server;
pub(crate) mod sched;
pub mod slot;
pub mod task;
pub mod task_meta;

use crate::base::ir::AtxeBinary;
use crate::base::isa::{Profile, reg};
use crate::runner::memory::SandboxMemory;

// ─── VM 状态 ───────────────────────────────────────────

/// 调用栈帧。
#[derive(Debug, Clone, Copy)]
pub struct CallFrame {
    /// 返回地址（CALL 的下一条指令）。
    pub return_pc: usize,
    /// 调用前的栈指针。
    pub sp: u64,
}

/// VM 核心状态。持有任务执行所需的所有运行时数据。
///
/// 手动实现 Clone（跳过 `open_files`，文件描述符不跨 VM 实例共享）。
#[derive(Debug)]
pub struct VmState {
    /// 16 个 64 位通用寄存器。
    pub regs: [u64; 16],
    /// 程序计数器（当前指令在 .text 中的索引）。
    pub pc: usize,
    /// .text 段 — 指令序列。
    pub text: Vec<u32>,
    /// .rodata 段 — 只读数据。
    pub rodata: Vec<u8>,
    /// .exn 段 — 异常表（原始字节）。
    pub exn_table: Vec<u8>,
    /// 沙箱线性内存（含堆和栈）。
    pub memory: SandboxMemory,
    /// 总内存大小（字节）。
    pub mem_size: u64,
    /// 运行状态。
    pub state: VmStateKind,
    /// 执行 profile。
    pub profile: Profile,
    /// 已消耗的指令配额。
    pub quantum: u32,
    /// 当前任务 ID。
    pub task_id: u16,
    /// TASK_JOIN 目标子任务 ID（调度器使用，None=未等待）。
    pub join_waiting_for: Option<u16>,
    /// TASK_FORK 产生的子任务 VmState（调度器取走入队，None=无待处理 fork）。
    pub pending_child: Option<Box<Self>>,
    /// .debug 段原始字节（PC ↔ 源码行映射）。
    pub debug_info: Vec<u8>,
    /// 调用栈帧列表。
    pub call_stack: Vec<CallFrame>,
    /// 打开的文件描述符表（FS_OPEN/FS_READ/FS_WRITE/FS_CLOSE 使用）。
    pub open_files: Vec<Option<std::fs::File>>,
    /// 打开的 TCP 套接字表（TCP_CONNECT/TCP_SEND/TCP_RECV/TCP_CLOSE 使用）。
    pub open_sockets: Vec<Option<std::net::TcpStream>>,
    /// TCP 监听器表（TCP_LISTEN/TCP_ACCEPT 使用）。
    pub listeners: Vec<Option<std::net::TcpListener>>,
}

/// VM 运行状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmStateKind {
    /// 正常运行。
    Running,
    /// 已停止（成功完成）。
    Halted,
    /// 异常终止。
    Error(String),
    /// 挂起（等待子任务或 IO）。
    Suspended,
}

impl VmState {
    /// 从已解码的 .atxe 创建 VM 状态。
    pub fn from_atxe(binary: &AtxeBinary) -> Result<Self, String> {
        let profile = binary.header.profile();
        let entry = binary.header.entry as usize;

        if entry >= binary.text.len() {
            return Err(format!(
                "entry point {} out of bounds (text length {})",
                entry,
                binary.text.len()
            ));
        }

        // 初始化沙箱内存
        let rodata_len = binary.rodata.len();
        let heap_size = 65536u64; // 64 KB 默认堆
        let stack_size = 4096u64; // 4 KB 栈
        let total = (rodata_len as u64 + heap_size + stack_size).max(8192) as usize;

        let mut memory = SandboxMemory::new(total);
        // .rodata 映射到地址空间底部
        if rodata_len > 0 {
            memory.data[..rodata_len].copy_from_slice(&binary.rodata);
        }
        memory.text_start = 0;
        memory.text_size = 0;
        let stack_base = (total - stack_size as usize) as u64;
        memory.stack_base = stack_base;
        memory.stack_size = stack_size;
        if rodata_len > 0 {
            memory.heap_base = rodata_len as u64;
        }
        memory.watermark_high = (total as u64) * 75 / 100;
        memory.usage = 0;

        let mut vm = Self {
            regs: [0u64; 16],
            pc: entry,
            text: binary.text.clone(),
            rodata: binary.rodata.clone(),
            exn_table: binary.exn_table.clone(),
            debug_info: binary.debug_info.clone(),
            memory,
            mem_size: total as u64,
            state: VmStateKind::Running,
            profile,
            quantum: 0,
            task_id: 0,
            join_waiting_for: None,
            pending_child: None,
            call_stack: Vec::new(),
            open_files: Vec::new(),
            open_sockets: Vec::new(),
            listeners: Vec::new(),
        };

        // 初始化 SP（栈顶，向下增长）
        vm.regs[reg::SP] = stack_base + stack_size;

        Ok(vm)
    }

    /// 从 .atxe 字节加载并创建 VM 状态。
    pub fn load_atxe(bytes: &[u8]) -> Result<Self, String> {
        let binary = AtxeBinary::from_bytes(bytes)
            .ok_or_else(|| "无效的 .atxe 文件：magic 不正确或数据损坏".to_string())?;

        // 版本检查
        if binary.header.version != 0x0001 {
            return Err(format!(
                "版本不兼容：.atxe v{:04x}，VM 需要 v0001",
                binary.header.version
            ));
        }

        Self::from_atxe(&binary)
    }

    /// 读取当前指令。
    pub fn fetch(&self) -> u32 {
        self.text[self.pc]
    }

    /// 检查是否仍在运行。
    pub fn is_running(&self) -> bool {
        self.state == VmStateKind::Running
    }

    /// 读取寄存器（R0 硬编码为 0）。
    pub fn read_reg(&self, idx: usize) -> u64 {
        if idx == reg::ZERO { 0 } else { self.regs[idx] }
    }

    /// 写入寄存器（R0 写入无效，R14 只读）。
    pub fn write_reg(&mut self, idx: usize, val: u64) {
        if idx != reg::ZERO && idx != reg::TASK_ID {
            self.regs[idx] = val;
        }
    }
}

impl Clone for VmState {
    fn clone(&self) -> Self {
        // 手动克隆，跳过 open_files（文件描述符不跨 VM 实例共享）
        Self {
            regs: self.regs,
            pc: self.pc,
            text: self.text.clone(),
            rodata: self.rodata.clone(),
            exn_table: self.exn_table.clone(),
            debug_info: self.debug_info.clone(),
            memory: self.memory.clone(),
            mem_size: self.mem_size,
            state: self.state.clone(),
            profile: self.profile,
            quantum: self.quantum,
            task_id: self.task_id,
            join_waiting_for: self.join_waiting_for,
            pending_child: None,
            call_stack: self.call_stack.clone(),
            open_files: Vec::new(), // 新 VM 实例不继承打开的文件
            open_sockets: Vec::new(), // 不继承套接字
            listeners: Vec::new(),    // 不继承监听器
        }
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ir::{AtxeBinary, Header};

    fn make_test_atxe(text: Vec<u32>) -> Vec<u8> {
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
        binary.to_bytes()
    }

    #[test]
    fn load_valid_atxe() {
        let text = vec![crate::base::isa::encode_r2i(
            crate::base::isa::opcode::MOVI,
            8,
            0,
            42,
        )];
        let bytes = make_test_atxe(text);
        let vm = VmState::load_atxe(&bytes);
        assert!(vm.is_ok());
    }

    #[test]
    fn load_invalid_magic() {
        let bytes = vec![0u8; 100];
        let vm = VmState::load_atxe(&bytes);
        assert!(vm.is_err());
    }

    #[test]
    fn version_mismatch() {
        let mut header = Header::new(0, 6);
        header.version = 0x9999;
        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text: vec![0],
            rodata: vec![],
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let bytes = binary.to_bytes();
        let vm = VmState::load_atxe(&bytes);
        assert!(vm.is_err());
        assert!(vm.unwrap_err().contains("版本"));
    }

    #[test]
    fn entry_out_of_bounds() {
        let mut header = Header::new(100, 6); // entry at 100
        header.total_instrs = 5; // but only 5 instructions
        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text: vec![0; 5],
            rodata: vec![],
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let bytes = binary.to_bytes();
        let vm = VmState::load_atxe(&bytes);
        assert!(vm.is_err());
    }

    #[test]
    fn zero_register_hardwired() {
        let mut vm = VmState::load_atxe(&make_test_atxe(vec![0])).unwrap();
        assert_eq!(vm.read_reg(0), 0);
        vm.write_reg(0, 42);
        assert_eq!(vm.read_reg(0), 0);
    }

    #[test]
    fn task_id_readonly() {
        let mut vm = VmState::load_atxe(&make_test_atxe(vec![0])).unwrap();
        vm.task_id = 7;
        vm.regs[14] = 7;
        vm.write_reg(14, 99);
        assert_eq!(vm.regs[14], 7); // unchanged
    }

    #[test]
    fn fetch_returns_instruction() {
        let text = vec![0x11223344];
        let vm = VmState::load_atxe(&make_test_atxe(text)).unwrap();
        assert_eq!(vm.fetch(), 0x11223344);
    }
}
