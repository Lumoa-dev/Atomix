//! .debug 段解析器 — 将 .atxe 中的 ADBG 格式调试信息解码为可用的映射表。
//!
//! 格式（兼容 docs/12-debugger-设计.md §9）：
//! - Magic "ADBG" (4B)
//! - version u16 LE (2B)
//! - flags u16 LE (2B)
//! - entry_count u32 LE (4B)
//! - entries: DebugEntry[entry_count]（每条 28 字节）
//! - string_pool: 连续存放的空终止字符串

/// 调试条目类型常量。
pub mod entry_kind {
    /// LINE — PC ↔ 源码行号映射。
    pub const LINE: u8 = 4;
    /// FUNC — 函数定义。
    pub const FUNC: u8 = 1;
    /// VAR — 变量声明。
    pub const VAR: u8 = 2;
    /// CALL — 调用点。
    pub const CALL: u8 = 3;
    /// SCOPE — 作用域开始。
    pub const SCOPE: u8 = 5;
}

/// 解析后的调试条目（PC ↔ 源码行号映射）。
#[derive(Debug, Clone)]
pub struct DebugEntry {
    pub pc_start: u32,
    pub pc_end: u32,
    pub kind: u8,
    pub source_line: u32,
    pub source_col: u16,
    pub depth: u8,
    pub func_name_off: u32,
    pub var_name_off: u32,
    pub type_name_off: u32,
    pub string_pool: Vec<u8>,
    raw_data: Vec<u8>,
}

impl DebugEntry {
    /// 获取函数名（仅 FUNC 类型）。
    pub fn func_name(&self) -> Option<&str> {
        if self.kind != entry_kind::FUNC {
            return None;
        }
        self.read_string(self.func_name_off as usize)
    }

    /// 获取变量名（仅 VAR 类型）。
    pub fn var_name(&self) -> Option<&str> {
        if self.kind != entry_kind::VAR || self.var_name_off == 0 {
            return None;
        }
        self.read_string(self.var_name_off as usize)
    }

    /// 获取类型名（仅 VAR 类型）。
    pub fn type_name(&self) -> Option<&str> {
        if self.type_name_off == 0 {
            return None;
        }
        self.read_string(self.type_name_off as usize)
    }

    fn read_string(&self, offset: usize) -> Option<&str> {
        if offset >= self.string_pool.len() {
            return None;
        }
        let remaining = &self.string_pool[offset..];
        let end = remaining.iter().position(|&b| b == 0).unwrap_or(remaining.len());
        Some(std::str::from_utf8(&remaining[..end]).unwrap_or(""))
    }
}

/// PC → 源码行号映射表。
#[derive(Debug, Clone)]
pub struct DebugMap {
    pub entries: Vec<DebugEntry>,
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

        let _flags = u16::from_le_bytes([bytes[6], bytes[7]]);
        let entry_count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let entry_size = 28usize;
        let entries_start = 12;

        let expected_size = entries_start + entry_count * entry_size;
        if bytes.len() < expected_size {
            return None;
        }

        let string_pool_start = entries_start + entry_count * entry_size;
        let string_pool = if string_pool_start < bytes.len() {
            bytes[string_pool_start..].to_vec()
        } else {
            Vec::new()
        };

        let mut entries = Vec::with_capacity(entry_count);
        for i in 0..entry_count {
            let off = entries_start + i * entry_size;
            let pc_start = u32::from_le_bytes([
                bytes[off],
                bytes[off + 1],
                bytes[off + 2],
                bytes[off + 3],
            ]);
            let pc_end = u32::from_le_bytes([
                bytes[off + 4],
                bytes[off + 5],
                bytes[off + 6],
                bytes[off + 7],
            ]);
            let source_line = u32::from_le_bytes([
                bytes[off + 8],
                bytes[off + 9],
                bytes[off + 10],
                bytes[off + 11],
            ]);
            let source_col = u16::from_le_bytes([bytes[off + 12], bytes[off + 13]]);
            let kind = bytes[off + 14];
            let depth = bytes[off + 15];
            let func_name_off = u32::from_le_bytes([
                bytes[off + 16],
                bytes[off + 17],
                bytes[off + 18],
                bytes[off + 19],
            ]);
            let var_name_off = u32::from_le_bytes([
                bytes[off + 20],
                bytes[off + 21],
                bytes[off + 22],
                bytes[off + 23],
            ]);
            let type_name_off = u32::from_le_bytes([
                bytes[off + 24],
                bytes[off + 25],
                bytes[off + 26],
                bytes[off + 27],
            ]);

            entries.push(DebugEntry {
                pc_start,
                pc_end,
                kind,
                source_line,
                source_col,
                depth,
                func_name_off,
                var_name_off,
                type_name_off,
                string_pool: string_pool.clone(),
                raw_data: bytes[off..off + entry_size].to_vec(),
            });
        }

        Some(Self { entries })
    }

    /// 根据 PC 查找对应的源码行号。
    pub fn line_for_pc(&self, pc: usize) -> Option<u32> {
        let pc_u32 = pc as u32;
        let mut best: Option<u32> = None;
        for entry in &self.entries {
            if entry.kind == entry_kind::LINE && entry.pc_start <= pc_u32 {
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
            .filter(|e| e.kind == entry_kind::LINE)
            .collect()
    }

    /// 获取所有 FUNC 类型条目。
    pub fn func_entries(&self) -> Vec<&DebugEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == entry_kind::FUNC)
            .collect()
    }

    /// 获取所有 VAR 类型条目。
    pub fn var_entries(&self) -> Vec<&DebugEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == entry_kind::VAR)
            .collect()
    }

    /// 获取所有 CALL 类型条目。
    pub fn call_entries(&self) -> Vec<&DebugEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == entry_kind::CALL)
            .collect()
    }

    /// 获取第 n 条 entry 的原始 28 字节数据。
    pub fn raw_entry(&self, index: usize) -> Option<&[u8]> {
        self.entries.get(index).map(|e| e.raw_data.as_slice())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 条目数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(pc_start: u32, source_line: u32, kind: u8) -> Vec<u8> {
        // 28-byte entry: pc_start(4) + pc_end(4) + source_line(4) + source_col(2)
        //              + kind(1) + depth(1) + func_name_off(4) + var_name_off(4) + type_name_off(4)
        let mut data = Vec::with_capacity(28);
        data.extend_from_slice(&pc_start.to_le_bytes());    // 0-3
        data.extend_from_slice(&0u32.to_le_bytes());         // 4-7: pc_end
        data.extend_from_slice(&source_line.to_le_bytes());  // 8-11
        data.extend_from_slice(&1u16.to_le_bytes());         // 12-13: source_col
        data.push(kind);                                     // 14
        data.push(0);                                        // 15: depth
        data.extend_from_slice(&0u32.to_le_bytes());         // 16-19: func_name_off
        data.extend_from_slice(&0u32.to_le_bytes());         // 20-23: var_name_off
        data.extend_from_slice(&0u32.to_le_bytes());         // 24-27: type_name_off
        assert_eq!(data.len(), 28);
        data
    }

    fn make_debug_bytes(entries: Vec<Vec<u8>>) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"ADBG");
        data.extend_from_slice(&1u16.to_le_bytes()); // version
        data.extend_from_slice(&0u16.to_le_bytes()); // flags
        data.extend_from_slice(&(entries.len() as u32).to_le_bytes()); // entry_count
        for entry in entries {
            data.extend_from_slice(&entry);
        }
        data.push(0); // empty string pool
        data
    }

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
        let data = make_debug_bytes(vec![
            make_entry(0, 5, entry_kind::LINE)
        ]);
        let map = DebugMap::from_bytes(&data);
        assert!(map.is_some());
        let map = map.unwrap();
        assert_eq!(map.line_for_pc(0), Some(5));
        assert_eq!(map.line_for_pc(10), Some(5));
    }

    #[test]
    fn line_for_pc_before_first_entry() {
        let data = make_debug_bytes(vec![
            make_entry(10, 20, entry_kind::LINE)
        ]);
        let map = DebugMap::from_bytes(&data).unwrap();
        assert_eq!(map.line_for_pc(0), None);
        assert_eq!(map.line_for_pc(10), Some(20));
        assert_eq!(map.line_for_pc(15), Some(20));
    }

    #[test]
    fn func_entry_parsing() {
        // 创建一个带函数名的 FUNC 条目需要 string_pool
        let mut data = Vec::new();
        data.extend_from_slice(b"ADBG");
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes()); // 1 entry

        let func_name_str = "my_function\0";
        // FUNC entry (28 bytes)
        data.extend_from_slice(&0u32.to_le_bytes());      // pc_start (0-3)
        data.extend_from_slice(&100u32.to_le_bytes());    // pc_end (4-7)
        data.extend_from_slice(&10u32.to_le_bytes());     // source_line (8-11)
        data.extend_from_slice(&1u16.to_le_bytes());       // source_col (12-13)
        data.push(entry_kind::FUNC);                       // kind (14)
        data.push(0);                                       // depth (15)
        data.extend_from_slice(&0u32.to_le_bytes());        // func_name_off = 0 (16-19)
        data.extend_from_slice(&0u32.to_le_bytes());        // var_name_off (20-23)
        data.extend_from_slice(&0u32.to_le_bytes());        // type_name_off (24-27)

        // string_pool
        data.extend_from_slice(func_name_str.as_bytes());

        let map = DebugMap::from_bytes(&data).unwrap();
        assert_eq!(map.len(), 1);
        let func_entries = map.func_entries();
        assert_eq!(func_entries.len(), 1);
        assert_eq!(func_entries[0].func_name(), Some("my_function"));
        assert_eq!(func_entries[0].source_line, 10);
    }

    #[test]
    fn entry_kind_constants() {
        assert_eq!(entry_kind::LINE, 4);
        assert_eq!(entry_kind::FUNC, 1);
        assert_eq!(entry_kind::VAR, 2);
        assert_eq!(entry_kind::CALL, 3);
        assert_eq!(entry_kind::SCOPE, 5);
    }

    #[test]
    fn filter_entries_by_kind() {
        let data = make_debug_bytes(vec![
            make_entry(0, 1, entry_kind::LINE),
            make_entry(10, 2, entry_kind::FUNC),
            make_entry(20, 3, entry_kind::VAR),
            make_entry(30, 4, entry_kind::LINE),
        ]);
        let map = DebugMap::from_bytes(&data).unwrap();
        assert_eq!(map.line_entries().len(), 2);
        assert_eq!(map.func_entries().len(), 1);
        assert_eq!(map.var_entries().len(), 1);
    }

    #[test]
    fn is_empty_check() {
        let data = make_debug_bytes(vec![]);
        let map = DebugMap::from_bytes(&data).unwrap();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn raw_entry_data() {
        let data = make_debug_bytes(vec![
            make_entry(42, 7, entry_kind::LINE)
        ]);
        let map = DebugMap::from_bytes(&data).unwrap();
        let raw = map.raw_entry(0);
        assert!(raw.is_some());
        assert_eq!(raw.unwrap().len(), 28);
    }
}
