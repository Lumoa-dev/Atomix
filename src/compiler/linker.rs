//! Atomix 链接器 — 段合并、符号解析、闭包修剪、.atxe 输出。
//!
//! 覆盖 04-编译管线.md §7 的链接规范。

use crate::base::ir::{AtxeBinary, Header};
use crate::base::isa::{self, opcode};
use crate::compiler::ast::ZoneKind;
use crate::compiler::codegen::assembly::{self, ExnEntry};
use crate::compiler::codegen::instr::InstrEmitter;
use std::collections::HashMap;

// ─── 链接器 ────────────────────────────────────────────

pub struct Linker {
    /// 函数名 → 指令偏移
    pub symbols: HashMap<String, usize>,
    /// 未解析的引用
    pub unresolved: Vec<String>,
    /// CALL 指令位置 → 目标函数名
    pub call_sites: Vec<(usize, String)>,
}

impl Linker {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
            unresolved: Vec::new(),
            call_sites: Vec::new(),
        }
    }

    /// 注册符号：函数名 → 指令偏移。
    pub fn define_symbol(&mut self, name: &str, offset: usize) {
        self.symbols.insert(name.to_string(), offset);
    }

    /// 注册一个 CALL 站点，需要在链接阶段解析。
    pub fn add_call_site(&mut self, instr_idx: usize, target: &str) {
        self.call_sites.push((instr_idx, target.to_string()));
    }

    /// 运行链接器：解析符号 + 闭包修剪 + 输出 .atxe。
    pub fn link(
        &mut self,
        emit: &InstrEmitter,
        rodata: &[u8],
        zones: &[(ZoneKind, String)],
        exn_entries: &[ExnEntry],
    ) -> Result<Vec<u8>, Vec<String>> {
        let mut errors = Vec::new();

        // 1. 解析所有 CALL 目标
        let mut text = emit.text.clone();
        for &(instr_idx, ref target) in &self.call_sites {
            if let Some(&target_offset) = self.symbols.get(target) {
                // 计算相对偏移
                let offset = (target_offset as i32) - (instr_idx as i32);
                // 重新编码 CALL 指令
                text[instr_idx] = isa::encode_ji(opcode::CALL, offset as u32);
            } else {
                errors.push(format!("未解析的符号: `{}`", target));
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        // 2. 闭包修剪
        let pruned = self.closure_prune(&text);

        // 3. 组装 .atxe
        let mut header = Header::new(0, 6);
        header.total_instrs = pruned.len() as u32;

        // 构建 zones 元信息（含 text 区间）
        let total_instrs = pruned.len();
        let zones_meta: Vec<(ZoneKind, String)> = zones.to_vec();
        let zones_with_ranges: Vec<(ZoneKind, String, usize, usize)> = zones_meta
            .iter()
            .map(|(k, n)| (*k, n.clone(), 0, total_instrs))
            .collect();

        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text: pruned,
            rodata: rodata.to_vec(),
            task_table: assembly::build_task_section(&zones_meta),
            debug_info: Vec::new(),
            exn_table: assembly::build_exn_section(exn_entries),
            zones: assembly::build_zones_section(&zones_with_ranges, &{
                // 用修剪后的指令数创建临时 emitter
                let mut e = InstrEmitter::new();
                e.text = emit.text.clone();
                e
            }),
        };

        Ok(binary.to_bytes())
    }

    /// 闭包修剪：从 TASK 入口出发，标记所有可达指令。
    fn closure_prune(&self, text: &[u32]) -> Vec<u32> {
        if text.is_empty() {
            return Vec::new();
        }

        let n = text.len();
        let mut reachable = vec![false; n];

        // 从指令 0（TASK 入口）出发
        self.mark_reachable(text, 0, &mut reachable);

        // 同时标记所有已知符号的目标
        for &offset in self.symbols.values() {
            if offset < n && !reachable[offset] {
                self.mark_reachable(text, offset, &mut reachable);
            }
        }

        // 收集可达指令
        text.iter()
            .enumerate()
            .filter(|(i, _)| reachable[*i])
            .map(|(_, &instr)| instr)
            .collect()
    }

    /// 正向传播可达性。
    fn mark_reachable(&self, text: &[u32], start: usize, reachable: &mut [bool]) {
        let mut i = start;
        while i < text.len() && !reachable[i] {
            reachable[i] = true;
            match (text[i] >> 24) as u8 {
                opcode::JMP => {
                    let offset = isa::decode_ji(text[i]);
                    let target = (i as i32 + offset) as usize;
                    if target < text.len() && !reachable[target] {
                        self.mark_reachable(text, target, reachable);
                    }
                    break;
                }
                opcode::JZ | opcode::JNZ => {
                    let raw = text[i] & 0x000F_FFFF;
                    let offset = if raw & 0x80000 != 0 {
                        (raw | 0xFFF00000) as i32
                    } else {
                        raw as i32
                    };
                    let target = (i as i32 + offset) as usize;
                    if target < text.len() && !reachable[target] {
                        self.mark_reachable(text, target, reachable);
                    }
                    i += 1;
                }
                opcode::CALL => {
                    let offset = isa::decode_ji(text[i]);
                    let target = (i as i32 + offset) as usize;
                    if target < text.len() && !reachable[target] {
                        self.mark_reachable(text, target, reachable);
                    }
                    i += 1;
                }
                opcode::JMPR | opcode::THROW | opcode::TRAP => break,
                _ => {
                    i += 1;
                }
            }
        }
    }
}

impl Default for Linker {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::isa::opcode;

    #[test]
    fn symbol_define_and_resolve() {
        let mut linker = Linker::new();
        linker.define_symbol("foo", 10);
        linker.add_call_site(0, "foo");

        let mut emit = InstrEmitter::new();
        emit.emit_ji(opcode::CALL, 0); // 将被解析

        let result = linker.link(&emit, &[], &[], &[]);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn unresolved_symbol_error() {
        let mut linker = Linker::new();
        linker.add_call_site(0, "undefined_func");

        let mut emit = InstrEmitter::new();
        emit.emit_ji(opcode::CALL, 0);

        let result = linker.link(&emit, &[], &[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn closure_prune_keeps_reachable() {
        let linker = Linker::new();
        let text = vec![
            isa::encode_ji(opcode::JMP, 2),         // instr 0: JMP +2 → instr 2
            isa::encode_r2i(opcode::MOVI, 8, 0, 0), // instr 1: 不可达
            isa::encode_r2i(opcode::MOVI, 9, 0, 1), // instr 2: 可达
        ];
        let pruned = linker.closure_prune(&text);
        assert_eq!(pruned.len(), 2); // JMP + MOVI
    }

    #[test]
    fn multiple_symbols_resolved() {
        let mut linker = Linker::new();
        linker.define_symbol("add", 2);
        linker.define_symbol("main", 0);
        linker.add_call_site(3, "add");

        let mut emit = InstrEmitter::new();
        emit.emit_label("main");
        emit.emit_nop(); // instr 0
        emit.emit_label("add");
        emit.emit_movi(8, 42); // instr 1
        emit.emit_r1i(opcode::JMPR, 3, 0); // instr 2: return
        // CALL add at instr 3
        emit.emit_ji(opcode::CALL, 0); // 将被链接器解析

        let result = linker.link(&emit, &[], &[], &[]);
        assert!(result.is_ok());
    }
}
