//! .atxe binary format: header, section table, serialization.
//! Used by compiler (write) and runtime (read).

use crate::base::isa::Profile;

// ─── Magic ────────────────────────────────────────────────────────

pub const ATMX_MAGIC: u32 = 0x584D5441; // "ATMX" LE

// ─── Header + Memory Profile ────────────────────────────────────
//
// Offset  Size  Field
// 0x00    4B    magic       "ATMX"
// 0x04    2B    version     0x0001
// 0x06    2B    flags       bit0: debug, bit1: sandbox, bit3-4: profile
// 0x08    4B    entry       root task entry instruction offset
// 0x0C    4B    total_instrs total .text instruction count
// 0x10    2B    section_count
// 0x12    2B    flags_ext
// ─────────────────────────────────
// Total: 0x14 (20) bytes

pub const HEADER_SIZE: usize = 20;
pub const MEMORY_PROFILE_SIZE: usize = 20;

/// 编译器内存预测（P3-CS-002）。
#[derive(Debug, Clone, Copy)]
pub struct MemoryProfile {
    pub code_mb: f32,
    pub rodata_mb: f32,
    pub stack_mb: f32,
    pub heap_mb: f32,
    pub peak_mb: f32,
}

#[derive(Debug, Clone)]
pub struct Header {
    pub version: u16,
    pub flags: u16,
    pub entry: u32,
    pub total_instrs: u32,
    pub section_count: u16,
    pub memory_profile: Option<MemoryProfile>,
}

impl Header {
    pub fn new(entry: u32, section_count: u16) -> Self {
        Self {
            version: 0x0001,
            flags: 0,
            entry,
            total_instrs: 0,
            section_count,
            memory_profile: None,
        }
    }

    /// 从 .text 和 .rodata 大小计算内存预测。
    pub fn compute_memory_profile(&mut self, text_bytes: usize, rodata_bytes: usize) {
        let code_mb = (text_bytes as f32) / (1024.0 * 1024.0);
        let rodata_mb = (rodata_bytes as f32) / (1024.0 * 1024.0);
        let stack_mb: f32 = 1.0; // 保守估计 1MB
        let heap_mb: f32 = 4.0; // 保守估计 4MB
        let peak_mb = code_mb + rodata_mb + stack_mb.max(heap_mb);
        self.memory_profile = Some(MemoryProfile {
            code_mb,
            rodata_mb,
            stack_mb,
            heap_mb,
            peak_mb,
        });
    }

    pub fn debug_mode(&self) -> bool {
        self.flags & 0x01 != 0
    }

    pub fn set_debug_mode(&mut self, on: bool) {
        if on {
            self.flags |= 0x01;
        } else {
            self.flags &= !0x01;
        }
    }

    pub fn sandbox_enabled(&self) -> bool {
        self.flags & 0x02 != 0
    }

    pub fn set_sandbox(&mut self, on: bool) {
        if on {
            self.flags |= 0x02;
        } else {
            self.flags &= !0x02;
        }
    }

    pub fn profile(&self) -> Profile {
        Profile::from_flags(self.flags)
    }

    pub fn set_profile(&mut self, p: Profile) {
        self.flags = (self.flags & !0x18) | p.to_flags_bits();
    }

    /// Serialize header to bytes (little-endian).
    /// 如果 memory_profile 存在，追加 20 字节到 header 之后。
    pub fn to_bytes(&self) -> Vec<u8> {
        let has_profile = self.memory_profile.is_some();
        let size = if has_profile {
            HEADER_SIZE + MEMORY_PROFILE_SIZE
        } else {
            HEADER_SIZE
        };
        let mut buf = Vec::with_capacity(size);
        buf.extend_from_slice(&ATMX_MAGIC.to_le_bytes());
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.extend_from_slice(&self.flags.to_le_bytes());
        buf.extend_from_slice(&self.entry.to_le_bytes());
        buf.extend_from_slice(&self.total_instrs.to_le_bytes());
        buf.extend_from_slice(&self.section_count.to_le_bytes());
        buf.extend_from_slice(&(if has_profile { 1u16 } else { 0u16 }).to_le_bytes()); // flags_ext
        if let Some(mp) = &self.memory_profile {
            buf.extend_from_slice(&mp.code_mb.to_le_bytes());
            buf.extend_from_slice(&mp.rodata_mb.to_le_bytes());
            buf.extend_from_slice(&mp.stack_mb.to_le_bytes());
            buf.extend_from_slice(&mp.heap_mb.to_le_bytes());
            buf.extend_from_slice(&mp.peak_mb.to_le_bytes());
        }
        buf
    }

    /// Deserialize header from bytes. Returns None if magic mismatch.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != ATMX_MAGIC {
            return None;
        }
        let flags_ext = u16::from_le_bytes([data[18], data[19]]);
        let has_profile = flags_ext & 0x01 != 0;

        let memory_profile = if has_profile && data.len() >= HEADER_SIZE + MEMORY_PROFILE_SIZE {
            let off = HEADER_SIZE;
            Some(MemoryProfile {
                code_mb: f32::from_le_bytes([
                    data[off],
                    data[off + 1],
                    data[off + 2],
                    data[off + 3],
                ]),
                rodata_mb: f32::from_le_bytes([
                    data[off + 4],
                    data[off + 5],
                    data[off + 6],
                    data[off + 7],
                ]),
                stack_mb: f32::from_le_bytes([
                    data[off + 8],
                    data[off + 9],
                    data[off + 10],
                    data[off + 11],
                ]),
                heap_mb: f32::from_le_bytes([
                    data[off + 12],
                    data[off + 13],
                    data[off + 14],
                    data[off + 15],
                ]),
                peak_mb: f32::from_le_bytes([
                    data[off + 16],
                    data[off + 17],
                    data[off + 18],
                    data[off + 19],
                ]),
            })
        } else {
            None
        };

        Some(Self {
            version: u16::from_le_bytes([data[4], data[5]]),
            flags: u16::from_le_bytes([data[6], data[7]]),
            entry: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            total_instrs: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
            section_count: u16::from_le_bytes([data[16], data[17]]),
            memory_profile,
        })
    }
}

// ─── Section Types ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum SectionType {
    Text = 0x0001,
    Rodata = 0x0002,
    Task = 0x0003,
    Debug = 0x0004,
    Exn = 0x0005,
    Zones = 0x0006,
}

impl SectionType {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x0001 => Some(Self::Text),
            0x0002 => Some(Self::Rodata),
            0x0003 => Some(Self::Task),
            0x0004 => Some(Self::Debug),
            0x0005 => Some(Self::Exn),
            0x0006 => Some(Self::Zones),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Text => ".text",
            Self::Rodata => ".rodata",
            Self::Task => ".task",
            Self::Debug => ".debug",
            Self::Exn => ".exn",
            Self::Zones => ".zones",
        }
    }
}

// ─── Section Table Entry ──────────────────────────────────────────
//
// Offset  Size  Field
// 0x00    2B    section_type
// 0x02    2B    flags
// 0x04    4B    offset      from file start to section data
// 0x08    4B    length      section data length in bytes
// ─────────────────────────────────
// Total: 12 bytes per entry

pub const SECTION_ENTRY_SIZE: usize = 12;

#[derive(Debug, Clone)]
pub struct SectionEntry {
    pub section_type: SectionType,
    pub flags: u16,
    pub offset: u32,
    pub length: u32,
}

impl SectionEntry {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(SECTION_ENTRY_SIZE);
        buf.extend_from_slice(&(self.section_type as u16).to_le_bytes());
        buf.extend_from_slice(&self.flags.to_le_bytes());
        buf.extend_from_slice(&self.offset.to_le_bytes());
        buf.extend_from_slice(&self.length.to_le_bytes());
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < SECTION_ENTRY_SIZE {
            return None;
        }
        let ty = u16::from_le_bytes([data[0], data[1]]);
        Some(Self {
            section_type: SectionType::from_u16(ty)?,
            flags: u16::from_le_bytes([data[2], data[3]]),
            offset: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            length: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        })
    }
}

// ─── Complete .atxe file ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AtxeBinary {
    pub header: Header,
    pub sections: Vec<SectionEntry>,
    pub text: Vec<u32>, // raw instruction words (LE u32 per instruction)
    pub rodata: Vec<u8>,
    pub task_table: Vec<u8>,
    pub debug_info: Vec<u8>,
    pub exn_table: Vec<u8>,
    pub zones: Vec<u8>,
}

impl Header {
    /// 返回序列化后的 header 实际字节数（含可选的 memory profile）。
    pub fn serialized_size(&self) -> usize {
        HEADER_SIZE
            + if self.memory_profile.is_some() {
                MEMORY_PROFILE_SIZE
            } else {
                0
            }
    }
}

impl AtxeBinary {
    /// Serialize to complete .atxe file bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        // header
        let mut header = self.header.clone();
        header.total_instrs = self.text.len() as u32;

        // build section table
        let mut sections = Vec::new();
        let header_size = header.serialized_size() as u32;
        let mut data_offset = header_size + (6 * SECTION_ENTRY_SIZE) as u32;

        // helper: push a section
        let mut push_section = |ty: SectionType, data: &[u8], sections: &mut Vec<SectionEntry>| {
            let len = data.len() as u32;
            sections.push(SectionEntry {
                section_type: ty,
                flags: 0,
                offset: data_offset,
                length: len,
            });
            data_offset += len;
        };

        push_section(
            SectionType::Text,
            &self
                .text
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
            &mut sections,
        );
        push_section(SectionType::Rodata, &self.rodata, &mut sections);
        push_section(SectionType::Task, &self.task_table, &mut sections);
        push_section(SectionType::Debug, &self.debug_info, &mut sections);
        push_section(SectionType::Exn, &self.exn_table, &mut sections);
        push_section(SectionType::Zones, &self.zones, &mut sections);

        header.section_count = sections.len() as u16;

        // assemble
        let mut out = Vec::new();
        out.extend_from_slice(&header.to_bytes());
        for s in &sections {
            out.extend_from_slice(&s.to_bytes());
        }
        out.extend_from_slice(
            &self
                .text
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        );
        out.extend_from_slice(&self.rodata);
        out.extend_from_slice(&self.task_table);
        out.extend_from_slice(&self.debug_info);
        out.extend_from_slice(&self.exn_table);
        out.extend_from_slice(&self.zones);
        out
    }

    /// Deserialize from .atxe file bytes. Returns None on invalid magic.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }
        // 从 flags_ext 判断是否有 memory profile 扩展
        let flags_ext = u16::from_le_bytes([data[18], data[19]]);
        let has_profile = flags_ext & 0x01 != 0;
        let total_header_size = HEADER_SIZE + if has_profile { MEMORY_PROFILE_SIZE } else { 0 };
        if data.len() < total_header_size {
            return None;
        }
        let header = Header::from_bytes(&data[..total_header_size])?;

        let sec_count = header.section_count as usize;
        let sec_start = total_header_size;
        let sec_end = sec_start + sec_count * SECTION_ENTRY_SIZE;
        if data.len() < sec_end {
            return None;
        }

        let mut sections = Vec::with_capacity(sec_count);
        for i in 0..sec_count {
            let off = sec_start + i * SECTION_ENTRY_SIZE;
            sections.push(SectionEntry::from_bytes(
                &data[off..off + SECTION_ENTRY_SIZE],
            )?);
        }

        let mut text = Vec::new();
        let mut rodata = Vec::new();
        let mut task_table = Vec::new();
        let mut debug_info = Vec::new();
        let mut exn_table = Vec::new();
        let mut zones = Vec::new();

        for s in &sections {
            let start = s.offset as usize;
            let end = start + s.length as usize;
            if end > data.len() {
                return None;
            }
            let slice = &data[start..end];
            match s.section_type {
                SectionType::Text => {
                    for chunk in slice.chunks_exact(4) {
                        text.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
                    }
                }
                SectionType::Rodata => rodata.extend_from_slice(slice),
                SectionType::Task => task_table.extend_from_slice(slice),
                SectionType::Debug => debug_info.extend_from_slice(slice),
                SectionType::Exn => exn_table.extend_from_slice(slice),
                SectionType::Zones => zones.extend_from_slice(slice),
            }
        }

        Some(Self {
            header,
            sections,
            text,
            rodata,
            task_table,
            debug_info,
            exn_table,
            zones,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let mut h = Header::new(0x42, 3);
        h.set_debug_mode(true);
        h.set_profile(Profile::Embedded);
        let bytes = h.to_bytes();
        let h2 = Header::from_bytes(&bytes).unwrap();
        assert_eq!(h2.version, 0x0001);
        assert_eq!(h2.entry, 0x42);
        assert_eq!(h2.section_count, 3);
        assert!(h2.debug_mode());
        assert_eq!(h2.profile(), Profile::Embedded);
    }

    #[test]
    fn atxe_roundtrip() {
        let mut h = Header::new(0, 0);
        h.set_debug_mode(false);
        let atxe = AtxeBinary {
            header: h,
            sections: vec![],
            text: vec![0x20000000, 0x50000008],
            rodata: b"hello\0world".to_vec(),
            task_table: vec![0x01, 0x00, 0x00, 0x00],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        let bytes = atxe.to_bytes();
        let atxe2 = AtxeBinary::from_bytes(&bytes).unwrap();
        assert_eq!(atxe2.text, vec![0x20000000, 0x50000008]);
        assert_eq!(atxe2.rodata, b"hello\0world".to_vec());
    }

    #[test]
    fn bad_magic_rejected() {
        assert!(Header::from_bytes(&[0; 20]).is_none());
    }
}
