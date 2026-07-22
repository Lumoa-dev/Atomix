//! IR 汇编器 — 将 IR 各段组装为 .atxe 二进制产物。
//!
//! 覆盖 02-指令集规范.md §4 和 04-编译管线.md §7。

use crate::base::ir::{AtxeBinary, Header};
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
    zones: &[(ZoneKind, String, usize, usize)], // (kind, name, text_start, text_end)
    exn_entries: &[ExnEntry],
) -> Vec<u8> {
    let mut header = Header::new(entry, 6);
    header.total_instrs = emit.text.len() as u32;
    header.compute_memory_profile(emit.text.len() * 4, rodata.len());

    let zone_tuples: Vec<(ZoneKind, String)> =
        zones.iter().map(|(k, n, _, _)| (*k, n.clone())).collect();

    let binary = AtxeBinary {
        header,
        sections: Vec::new(),
        text: emit.text.clone(),
        rodata: rodata.to_vec(),
        task_table: build_task_section(&zone_tuples),
        debug_info: build_debug_section(emit),
        exn_table: build_exn_section(exn_entries),
        zones: build_zones_section(zones, emit),
    };

    binary.to_bytes()
}

/// .exn 段条目（见 02-指令集规范.md §4.6）。
#[derive(Debug, Clone, Copy)]
pub struct ExnEntry {
    pub start_pc: u32,
    pub end_pc: u32,
    pub handler_pc: u32,
    pub filter: u16, // 0=All, 1=IsError, 2=IsTimeout
}

/// 构建 .exn 段（每条目 12 字节）。
pub fn build_exn_section(entries: &[ExnEntry]) -> Vec<u8> {
    let mut data = Vec::with_capacity(entries.len() * 12);
    for entry in entries {
        data.extend_from_slice(&entry.start_pc.to_le_bytes()); // 4B
        data.extend_from_slice(&entry.end_pc.to_le_bytes()); // 4B
        data.extend_from_slice(&entry.handler_pc.to_le_bytes()); // 4B
        data.extend_from_slice(&entry.filter.to_le_bytes()); // 2B
        data.extend_from_slice(&0u16.to_le_bytes()); // padding 2B
    }
    data
}

/// 构建 .task 段。
pub fn build_task_section(zones: &[(ZoneKind, String)]) -> Vec<u8> {
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

/// 构建 .zones 段（每条目 12 字节：zone_id 2B + lifecycle 1B + flags 1B + text_start 4B + text_end 4B）。
/// zones 参数为 (kind, name, text_start_instr, text_end_instr) 元组。
pub fn build_zones_section(
    zones: &[(ZoneKind, String, usize, usize)],
    _emit: &InstrEmitter,
) -> Vec<u8> {
    let mut data = Vec::new();
    // 按 zone_id 固定编号映射：区外=0, TOOLS=1, INPUT=2, WORKS=3, TASK=4, OUT=5, TEST=6
    let zone_id_map: &[(ZoneKind, u16)] = &[
        (ZoneKind::Tools, 1),
        (ZoneKind::Input, 2),
        (ZoneKind::Works, 3),
        (ZoneKind::Task, 4),
        (ZoneKind::Out, 5),
    ];

    // 区外(0) 和 TEST(6) 没有 body → text_start=text_end=0
    for zone_id in [0u16, 6u16] {
        let (lifecycle, flags) = zone_lifecycle(zone_id);
        data.extend_from_slice(&zone_id.to_le_bytes());
        data.push(lifecycle);
        data.push(flags);
        data.extend_from_slice(&0u32.to_le_bytes()); // text_start
        data.extend_from_slice(&0u32.to_le_bytes()); // text_end
    }

    // 实际有 body 的 zone
    for (kind, _name, text_start, text_end) in zones {
        if let Some(&zone_id) = zone_id_map
            .iter()
            .find(|(k, _)| k == kind)
            .map(|(_, id)| id)
        {
            let (lifecycle, flags) = zone_lifecycle(zone_id);
            data.extend_from_slice(&zone_id.to_le_bytes());
            data.push(lifecycle);
            data.push(flags);
            data.extend_from_slice(&(*text_start as u32).to_le_bytes());
            data.extend_from_slice(&(*text_end as u32).to_le_bytes());
        }
    }

    data
}

/// 构建 .debug 段 — PC → 源码行号映射。
///
/// 格式（与 docs/12-debugger-设计.md §9 兼容）：
/// - Magic "ADBG" (4B)
/// - version u16 LE (2B)
/// - flags u16 LE (2B)
/// - entry_count u32 LE (4B)
/// - entries: DebugEntry[entry_count]（每条 28 字节）
/// - string_pool: 连续存放的空终止字符串
///
/// 当前仅发射 LINE 类型条目（pc_start, source_line）。
pub fn build_debug_section(emit: &InstrEmitter) -> Vec<u8> {
    if emit.line_map.is_empty() {
        return Vec::new();
    }

    let mut data = Vec::new();
    // Header
    data.extend_from_slice(b"ADBG"); // magic
    data.extend_from_slice(&1u16.to_le_bytes()); // version
    data.extend_from_slice(&0u16.to_le_bytes()); // flags
    data.extend_from_slice(&(emit.line_map.len() as u32).to_le_bytes()); // entry_count

    // string_pool 偏移占位（先写 entries，再写 string_pool）
    let entries_start = 12; // 12B header
    let mut string_pool = Vec::new();

    // 先计算 entries 区域大小，预留空间
    let entry_size = 28u32;
    let entries_bytes = (emit.line_map.len() as u32) * entry_size;
    data.resize(data.len() + entries_bytes as usize, 0);

    // 填充 entries
    for (i, &(pc, line)) in emit.line_map.iter().enumerate() {
        let entry_off = entries_start + (i as u32) * entry_size;
        // pc_start (4B)
        data[entry_off as usize..][..4].copy_from_slice(&(pc as u32).to_le_bytes());
        // pc_end (4B) — 暂不填充，可后续计算
        // source_line (4B)
        data[entry_off as usize + 8..][..4].copy_from_slice(&line.to_le_bytes());
        // source_col (2B)
        data[entry_off as usize + 12..][..2].copy_from_slice(&1u16.to_le_bytes());
        // kind (1B) — 4 = LINE
        data[entry_off as usize + 14] = 4;
        // depth (1B)
        data[entry_off as usize + 15] = 0;
        // func_name_off, var_name_off, type_name_off, ast_node_off (4B each)
        // 全部设为 0（LINE 条目不需要这些字段）
        // 从偏移 16 到 28 都是 0
        for j in 16..28 {
            data[entry_off as usize + j] = 0;
        }
    }

    // string_pool（无论如何都要附加一个空字符串给 0 偏移引用）
    if string_pool.is_empty() {
        string_pool.push(0); // 空字符串偏移 0
    }
    data.extend_from_slice(&string_pool);

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
        let zones = vec![(ZoneKind::Task, "main".into(), 0, 3)];

        let result = assemble(&emit, &rodata, 0, &zones, &[]);
        assert!(!result.is_empty());

        // 验证可解码
        let decoded = AtxeBinary::from_bytes(&result);
        assert!(decoded.is_some());
        let decoded = decoded.unwrap();
        assert!(decoded.header.version == 0x0001);
        assert_eq!(decoded.header.total_instrs, 3);
    }

    #[test]
    fn exn_section_entry_size() {
        let entries = vec![ExnEntry {
            start_pc: 0,
            end_pc: 10,
            handler_pc: 20,
            filter: 1,
        }];
        let data = build_exn_section(&entries);
        assert_eq!(data.len(), 16); // 12 bytes + 4 padding? Actually 4+4+4+2+2=16
        // Actually doc says: start_pc 4B + end_pc 4B + handler_pc 4B + filter 2B + padding 2B = 16
        assert_eq!(data.len(), 16);
    }

    #[test]
    fn zones_section_size() {
        let emit = InstrEmitter::new();
        let zones = vec![(ZoneKind::Task, "main".into(), 0, 10)];
        let zones_data = build_zones_section(&zones, &emit);
        // 2 fixed zones (zone 0 and 6) + 1 actual zone = 3 entries
        assert_eq!(zones_data.len(), 3 * 12);
    }

    #[test]
    fn lifecycle_mapping() {
        assert_eq!(zone_lifecycle(0), (0, 0)); // persistent
        assert_eq!(zone_lifecycle(4), (1, 1)); // exec_unload + prune
        assert_eq!(zone_lifecycle(5), (2, 0)); // lazy
    }
}
