//! .debug 段解析器 — 将 .atxe 中的 ADBG 格式调试信息解码为可用的映射表。
//!
//! 格式（兼容 docs/12-debugger-设计.md §9）：
//! - Magic "ADBG" (4B)
//! - version u16 LE (2B)
//! - flags u16 LE (2B)
//! - entry_count u32 LE (4B)
//! - entries: DebugEntry[entry_count]（每条 28 字节）
//! - string_pool: 连续存放的空终止字符串

/// 解析后的调试条目（PC ↔ 源码行号映射）。
#[derive(Debug, Clone)]
pub struct DebugEntry {
    pub pc_start: u32,
    pub kind: u8,
    pub source_line: u32,
    pub depth: u8,
}

/// PC → 源码行号映射表。
#[derive(Debug, Clone)]
pub struct DebugMap {
    entries: Vec<DebugEntry>,
}

impl DebugMap {
    /// 从 .debug 段原始字节解析。
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 {
            return None;
        }

        // 检查 magic
        if &bytes[0..4] != b"ADBG" {
            return None;
        }

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != 1 {
            return None;
        }

        let entry_count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let entry_size = 28usize;
        let entries_start = 12;

        let expected_size = entries_start + entry_count * entry_size;
        if bytes.len() < expected_size {
            return None;
        }

        let mut entries = Vec::with_capacity(entry_count);
        for i in 0..entry_count {
            let off = entries_start + i * entry_size;
            let pc_start = u32::from_le_bytes([
                bytes[off],
                bytes[off + 1],
                bytes[off + 2],
                bytes[off + 3],
            ]);
            let source_line = u32::from_le_bytes([
                bytes[off + 8],
                bytes[off + 9],
                bytes[off + 10],
                bytes[off + 11],
            ]);
            let kind = bytes[off + 14];
            let depth = bytes[off + 15];

            entries.push(DebugEntry {
                pc_start,
                kind,
                source_line,
                depth,
            });
        }

        Some(Self { entries })
    }

    /// 根据 PC 查找对应的源码行号。
    pub fn line_for_pc(&self, pc: usize) -> Option<u32> {
        let pc_u32 = pc as u32;
        // entries 按 pc_start 升序排列，二分查找最后一个 ≤ pc 的条目
        let mut best: Option<u32> = None;
        for entry in &self.entries {
            if entry.kind == 4 && entry.pc_start <= pc_u32 {
                best = Some(entry.source_line);
            } else if entry.pc_start > pc_u32 {
                break;
            }
        }
        best
    }

    /// 获取所有 LINE 类型条目。
    pub fn line_entries(&self) -> Vec<&DebugEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == 4)
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_returns_none() {
        assert!(DebugMap::from_bytes(&[]).is_none());
    }

    #[test]
    fn parse_invalid_magic() {
        let bytes = b"XXXX\x01\x00\x00\x00\x01\x00\x00\x00".to_vec();
        assert!(DebugMap::from_bytes(&bytes).is_none());
    }

    #[test]
    fn parse_valid_debug() {
        let mut data = Vec::new();
        // magic "ADBG"
        data.extend_from_slice(b"ADBG");
        // version = 1
        data.extend_from_slice(&1u16.to_le_bytes());
        // flags = 0
        data.extend_from_slice(&0u16.to_le_bytes());
        // entry_count = 1
        data.extend_from_slice(&1u32.to_le_bytes());

        // 一个 LINE 条目 (kind=4)
        // pc_start = 0
        data.extend_from_slice(&0u32.to_le_bytes());
        // pc_end = 0 (未使用)
        data.extend_from_slice(&0u32.to_le_bytes());
        // source_line = 5
        data.extend_from_slice(&5u32.to_le_bytes());
        // source_col = 1
        data.extend_from_slice(&1u16.to_le_bytes());
        // kind = 4 (LINE)
        data.push(4);
        // depth = 0
        data.push(0);
        // func_name_off, var_name_off, type_name_off, ast_node_off = 0
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        // string_pool (空字符串)
        data.push(0);

        let map = DebugMap::from_bytes(&data);
        assert!(map.is_some());
        let map = map.unwrap();
        assert_eq!(map.line_for_pc(0), Some(5));
        assert_eq!(map.line_for_pc(10), Some(5));
    }

    #[test]
    fn line_for_pc_before_first_entry() {
        let mut data = Vec::new();
        data.extend_from_slice(b"ADBG");
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());

        // LINE entry at pc=10, line=20
        data.extend_from_slice(&10u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&20u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.push(4);
        data.push(0);
        data.extend_from_slice(&[0u8; 16]);

        let map = DebugMap::from_bytes(&data).unwrap();
        assert_eq!(map.line_for_pc(0), None); // before first entry
        assert_eq!(map.line_for_pc(10), Some(20)); // at entry
        assert_eq!(map.line_for_pc(15), Some(20)); // after entry
    }
}
