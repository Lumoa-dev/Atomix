//! IR 汇编器 — 将 IR 各段组装为 .atxe 二进制产物。
//!
//! 覆盖 02-指令集规范.md §4 和 04-编译管线.md §7。

use crate::base::ir::{AtxeBinary, Header, SectionEntry, SectionType, HEADER_SIZE, SECTION_ENTRY_SIZE};
use crate::compiler::ast::ZoneKind;
use crate::compiler::codegen::instr::InstrEmitter;

// ─── 区域生命周期映射 ──────────────────────────────────

/// zone_id → lifecycle 映射表（见 02-指令集规范.md §4.7）
fn zone_lifecycle(zone_id: u16) -> (u8, u8) {
    match zone_id {
        0 => (0, 0), // 区外: persistent
        1 => (0, 0), // TOOLS: persistent
        2 => (1, 0), // INPUT: exec_unload
        3 => (0, 1), // WORKS: persistent + prune
        4 => (1, 1), // TASK: exec_unload + prune
        5 => (2, 0), // OUT: lazy
        6 => (0, 0), // TEST: persistent
        _ => (0, 0),
    }
}

// ─── 汇编器 ────────────────────────────────────────────

/// 将编译产物组装为 .atxe 二进制。
pub fn assemble(
    emit: &InstrEmitter,
    rodata: &[u8],
    entry: u32,
    zones: &[(ZoneKind, String)], // (kind, name) 列表
) -> Vec<u8> {
    let mut header = Header::new(entry, 6);
    header.total_instrs = emit.text.len() as u32;

    let binary = AtxeBinary {
        header,
        sections: Vec::new(),
        text: emit.text.clone(),
        rodata: rodata.to_vec(),
        task_table: build_task_section(zones),
        debug_info: Vec::new(),
        exn_table: Vec::new(),
        zones: build_zones_section(zones, emit),
    };

    binary.to_bytes()
}

/// 构建 .task 段。
fn build_task_section(zones: &[(ZoneKind, String)]) -> Vec<u8> {
    let mut data = Vec::new();
    for (i, (kind, name)) in zones.iter().enumerate() {
        if *kind == ZoneKind::Task {
            let task_id = i as u16;
            // task_id(2B) + entry_offset(4B) + dep_count(2B) + dep_list(变长)
            data.extend_from_slice(&task_id.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes()); // entry_offset（链接时填充）
            data.extend_from_slice(&0u16.to_le_bytes()); // dep_count
            let _ = name;
        }
    }
    data
}

/// 构建 .zones 段（每条目 16 字节）。
fn build_zones_section(zones: &[(ZoneKind, String)], emit: &InstrEmitter) -> Vec<u8> {
    let mut data = Vec::new();
    let total_instrs = emit.text.len();

    // 计算每个 zone 的 text 区间（简化：均分 .text）
    // Phase 1 简单实现：整体作为一个 TASK zone
    let zone_count = 7u16; // 区外 + TOOLS + INPUT + WORKS + TASK + OUT + TEST

    for zone_id in 0..zone_count {
        let (lifecycle, flags) = zone_lifecycle(zone_id);
        data.extend_from_slice(&zone_id.to_le_bytes());  // zone_id: 2B
        data.push(lifecycle);                             // lifecycle: 1B
        data.push(flags);                                 // flags: 1B
        // text_start / text_end: 简化为整个 .text
        data.extend_from_slice(&0u32.to_le_bytes());      // text_start
        data.extend_from_slice(&(total_instrs as u32).to_le_bytes()); // text_end
    }

    data
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::codegen::instr::InstrEmitter;

    #[test]
    fn assemble_basic() {
        let mut emit = InstrEmitter::new();
        emit.emit_movi(8, 42);
        emit.emit_movi(9, 10);
        emit.emit_r3(0x20, 10, 8, 9, 0); // ADD

        let rodata = vec![0u8; 16];
        let zones = vec![(ZoneKind::Task, "main".into())];

        let result = assemble(&emit, &rodata, 0, &zones);
        assert!(!result.is_empty());

        // 验证可解码
        let decoded = AtxeBinary::from_bytes(&result);
        assert!(decoded.is_some());
        let decoded = decoded.unwrap();
        assert!(decoded.header.version == 0x0001);
        assert_eq!(decoded.header.total_instrs, 3);
    }

    #[test]
    fn zones_section_size() {
        let emit = InstrEmitter::new();
        let zones = vec![(ZoneKind::Task, "main".into())];
        let zones_data = build_zones_section(&zones, &emit);
        // 7 entries × 12 bytes each (zone_id 2B + lifecycle 1B + flags 1B + text_start 4B + text_end 4B)
        assert_eq!(zones_data.len(), 7 * 12);
    }

    #[test]
    fn lifecycle_mapping() {
        assert_eq!(zone_lifecycle(0), (0, 0)); // persistent
        assert_eq!(zone_lifecycle(4), (1, 1)); // exec_unload + prune
        assert_eq!(zone_lifecycle(5), (2, 0)); // lazy
    }
}
