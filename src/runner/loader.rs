//! .task 段解析器 — 将编译后的 .task 段二进制解析为任务条目列表。
//!
//! 覆盖 02-指令集规范.md §4.4 和 P3-SCH-007。

/// 解析后的任务条目。
#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub task_id: u16,
    pub entry_offset: u32,
    pub dep_count: u16,
    pub dep_list: Vec<u16>,
}

/// 解析 .task 段。每条目结构：
///   task_id(2B) + entry_offset(4B) + dep_count(2B) + dep_list(dep_count × 2B)
pub fn parse_task_section(data: &[u8]) -> Result<Vec<TaskEntry>, String> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    let mut offset = 0usize;

    while offset + 8 <= data.len() {
        let task_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let entry_offset = u32::from_le_bytes([
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
        ]);
        let dep_count = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        offset += 8;

        let dep_size = dep_count as usize * 2;
        let mut dep_list = Vec::with_capacity(dep_count as usize);
        if offset + dep_size <= data.len() {
            for i in 0..dep_count as usize {
                let dep_id = u16::from_le_bytes([data[offset + i * 2], data[offset + i * 2 + 1]]);
                dep_list.push(dep_id);
            }
            offset += dep_size;
        } else {
            return Err(format!(
                ".task 段截断: task_id={} 的依赖列表超出边界",
                task_id
            ));
        }

        entries.push(TaskEntry {
            task_id,
            entry_offset,
            dep_count,
            dep_list,
        });
    }

    if offset != data.len() {
        return Err(format!(
            ".task 段末尾有 {} 个字节的残留数据",
            data.len() - offset
        ));
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty() {
        let entries = parse_task_section(&[]).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_single_no_deps() {
        // task_id=0, entry_offset=0, dep_count=0
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_le_bytes()); // task_id
        data.extend_from_slice(&0u32.to_le_bytes()); // entry_offset
        data.extend_from_slice(&0u16.to_le_bytes()); // dep_count
        let entries = parse_task_section(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].task_id, 0);
        assert_eq!(entries[0].entry_offset, 0);
        assert!(entries[0].dep_list.is_empty());
    }

    #[test]
    fn parse_multiple_with_deps() {
        let mut data = Vec::new();
        // Task 0: entry=0, deps=[]
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        // Task 1: entry=10, deps=[0]
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&10u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes()); // dep_count=1
        data.extend_from_slice(&0u16.to_le_bytes()); // dep_id=0
        // Task 2: entry=20, deps=[0, 1]
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&20u32.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes()); // dep_count=2
        data.extend_from_slice(&0u16.to_le_bytes()); // dep_id=0
        data.extend_from_slice(&1u16.to_le_bytes()); // dep_id=1

        let entries = parse_task_section(&data).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].task_id, 0);
        assert_eq!(entries[0].entry_offset, 0);
        assert_eq!(entries[1].task_id, 1);
        assert_eq!(entries[1].entry_offset, 10);
        assert_eq!(entries[1].dep_list, vec![0]);
        assert_eq!(entries[2].dep_list, vec![0, 1]);
    }

    #[test]
    fn parse_truncated_data() {
        let data = vec![0u8, 0, 0, 0, 0, 0, 0, 0, 1, 0]; // 10 bytes: one entry + 2 bytes partial
        let result = parse_task_section(&data);
        assert!(result.is_err());
    }

    #[test]
    fn parse_dep_list_truncated() {
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_le_bytes()); // task_id
        data.extend_from_slice(&0u32.to_le_bytes()); // entry_offset
        data.extend_from_slice(&3u16.to_le_bytes()); // dep_count=3, but only 2 bytes following
        data.extend_from_slice(&0u16.to_le_bytes()); // only one dep_id
        let result = parse_task_section(&data);
        assert!(result.is_err());
    }
}
