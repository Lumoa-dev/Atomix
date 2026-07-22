//! ATXP 协议帧层 — 16 字节帧头 + Protobuf 载荷。
//!
//! 协议细节见 docs/05-通信协议.md 和 docs/atxp.proto。
//!
//! # 帧格式
//!
//! | 偏移 | 大小 | 字段        | 说明                        |
//! |------|------|-------------|-----------------------------|
//! | 0    | 2    | magic       | 0x4154 ("AT")               |
//! | 2    | 1    | version     | 协议版本 (0x01)             |
//! | 3    | 1    | msg_type    | 消息类型 ID                 |
//! | 4    | 4    | seq_id      | 序列号 (u32 LE)             |
//! | 8    | 1    | flags       | 标志位                      |
//! | 9    | 4    | payload_len | 载荷长度 (u32 LE)           |
//! | 13   | 1    | req_type    | 请求的消息类型 (响应时回显) |
//! | 14   | 2    | checksum    | CRC-16-CCITT 校验和          |

/// 帧头大小：16 字节。
pub const FRAME_HEADER_SIZE: usize = 16;

/// 消息类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgType {
    Capabilities = 0x01,
    Ack = 0x02,
    Error = 0x03,
    Command = 0x04,
    Query = 0x05,
    QueryResult = 0x06,
    Event = 0x07,
    Stream = 0x08,
    Inject = 0x09,
    InjectResult = 0x0A,
    Heartbeat = 0x0B,
    Disconnect = 0x0C,
    Submit = 0x0D,
    SubmitResult = 0x0E,
    TaskOutput = 0x0F,
    OutputRequest = 0x10,
}

impl MsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Capabilities),
            0x02 => Some(Self::Ack),
            0x03 => Some(Self::Error),
            0x04 => Some(Self::Command),
            0x05 => Some(Self::Query),
            0x06 => Some(Self::QueryResult),
            0x07 => Some(Self::Event),
            0x08 => Some(Self::Stream),
            0x09 => Some(Self::Inject),
            0x0A => Some(Self::InjectResult),
            0x0B => Some(Self::Heartbeat),
            0x0C => Some(Self::Disconnect),
            0x0D => Some(Self::Submit),
            0x0E => Some(Self::SubmitResult),
            0x0F => Some(Self::TaskOutput),
            0x10 => Some(Self::OutputRequest),
            _ => None,
        }
    }
}

/// 帧标志位。
pub mod flags {
    pub const COMPRESSED: u8 = 0x01;
    pub const ACK_REQUESTED: u8 = 0x02;
    pub const IS_ERROR: u8 = 0x04;
    pub const IS_RESPONSE: u8 = 0x08;
}

/// ATXP 帧头。
#[derive(Debug, Clone)]
pub struct FrameHeader {
    pub version: u8,
    pub msg_type: u8,
    pub seq_id: u32,
    pub flags: u8,
    pub payload_len: u32,
    pub req_type: u8, // 响应时回显请求的 msg_type
    pub checksum: u16,
}

impl FrameHeader {
    /// 创建新帧头（自动计算 checksum）。
    pub fn new(msg_type: u8, seq_id: u32, payload: &[u8]) -> Self {
        let payload_len = payload.len() as u32;
        let mut hdr = Self {
            version: 0x01,
            msg_type,
            seq_id,
            flags: 0,
            payload_len,
            req_type: 0,
            checksum: 0,
        };
        hdr.checksum = hdr.compute_checksum(payload);
        hdr
    }

    /// 标记为响应帧。
    pub fn as_response(&mut self, req_type: u8) {
        self.flags |= flags::IS_RESPONSE;
        self.req_type = req_type;
    }

    /// 计算 CRC-16-CCITT 校验和。
    pub fn compute_checksum(&self, payload: &[u8]) -> u16 {
        // 计算 bytes[2..14] + payload 的 CRC
        let mut buf = Vec::with_capacity(12 + payload.len());
        buf.push(self.version);
        buf.push(self.msg_type);
        buf.extend_from_slice(&self.seq_id.to_le_bytes());
        buf.push(self.flags);
        buf.extend_from_slice(&self.payload_len.to_le_bytes());
        buf.push(self.req_type);
        buf.extend_from_slice(payload);
        calc_crc(&buf)
    }
}

/// 将帧头编码为 16 字节。
pub fn encode_header(hdr: &FrameHeader) -> [u8; FRAME_HEADER_SIZE] {
    let mut buf = [0u8; FRAME_HEADER_SIZE];
    buf[0..2].copy_from_slice(b"AT"); // magic
    buf[2] = hdr.version;
    buf[3] = hdr.msg_type;
    buf[4..8].copy_from_slice(&hdr.seq_id.to_le_bytes());
    buf[8] = hdr.flags;
    buf[9..13].copy_from_slice(&hdr.payload_len.to_le_bytes());
    buf[13] = hdr.req_type;
    buf[14..16].copy_from_slice(&hdr.checksum.to_le_bytes());
    buf
}

/// 从 16 字节解码帧头。
pub fn decode_header(bytes: &[u8; FRAME_HEADER_SIZE]) -> Option<FrameHeader> {
    if &bytes[0..2] != b"AT" {
        return None;
    }
    let hdr = FrameHeader {
        version: bytes[2],
        msg_type: bytes[3],
        seq_id: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        flags: bytes[8],
        payload_len: u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]),
        req_type: bytes[13],
        checksum: u16::from_le_bytes([bytes[14], bytes[15]]),
    };
    Some(hdr)
}

/// 编码完整帧（帧头 + 载荷）。
pub fn encode_frame(msg_type: u8, seq_id: u32, payload: &[u8]) -> Vec<u8> {
    let hdr = FrameHeader::new(msg_type, seq_id, payload);
    let mut buf = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
    buf.extend_from_slice(&encode_header(&hdr));
    buf.extend_from_slice(payload);
    buf
}

/// 尝试从字节流中解析一帧。返回 (帧头, 载荷字节, 剩余字节)。
pub fn decode_frame(data: &[u8]) -> Option<(FrameHeader, Vec<u8>, &[u8])> {
    if data.len() < FRAME_HEADER_SIZE {
        return None;
    }
    let hdr_bytes: [u8; FRAME_HEADER_SIZE] = data[..FRAME_HEADER_SIZE].try_into().ok()?;
    let hdr = decode_header(&hdr_bytes)?;

    let total = FRAME_HEADER_SIZE + hdr.payload_len as usize;
    if data.len() < total {
        return None; // 数据不足，等待更多
    }

    let payload = data[FRAME_HEADER_SIZE..total].to_vec();
    let rest = &data[total..];

    // 验证 checksum
    let expected_crc = hdr.compute_checksum(&payload);
    if expected_crc != hdr.checksum {
        return None; // CRC 不匹配
    }

    Some((hdr, payload, rest))
}

// ─── CRC-16-CCITT ──────────────────────────────────

/// 计算 CRC-16-CCITT (0xFFFF 初始值, 0x1021 多项式)。
fn calc_crc(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

// ─── Protobuf 生成类型的重导出 ───────────────────

/// 包含 prost 生成的 ATXP 消息类型。
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/atxp.rs"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_header_roundtrip() {
        let payload = b"hello";
        let hdr = FrameHeader::new(0x04, 1, payload);
        let encoded = encode_header(&hdr);

        let decoded = decode_header(&encoded).unwrap();
        assert_eq!(decoded.version, 0x01);
        assert_eq!(decoded.msg_type, 0x04);
        assert_eq!(decoded.seq_id, 1);
        assert_eq!(decoded.payload_len, 5);
    }

    #[test]
    fn encode_decode_frame() {
        let payload = vec![0x01, 0x02, 0x03];
        let frame = encode_frame(0x05, 42, &payload);
        let (hdr, decoded_payload, rest) = decode_frame(&frame).unwrap();
        assert_eq!(hdr.msg_type, 0x05);
        assert_eq!(hdr.seq_id, 42);
        assert_eq!(decoded_payload, payload);
        assert!(rest.is_empty());
    }

    #[test]
    fn checksum_verification() {
        let payload = b"test data";
        let hdr = FrameHeader::new(0x01, 0, payload);
        let crc = hdr.compute_checksum(payload);
        assert_ne!(crc, 0);
        assert_ne!(crc, 0xFFFF);

        // 篡改数据后 CRC 应不匹配
        let tampered = b"test datA";
        let crc2 = hdr.compute_checksum(tampered);
        assert_ne!(crc, crc2);
    }

    #[test]
    fn bad_magic_rejected() {
        let mut frame = encode_frame(0x01, 0, b"");
        frame[0] = 0x00; // 破坏 magic
        assert!(decode_frame(&frame).is_none());
    }

    #[test]
    fn partial_data_returns_none() {
        let frame = encode_frame(0x01, 0, b"1234");
        assert!(decode_frame(&frame[..15]).is_none()); // 缺 1 字节
    }

    #[test]
    fn msg_type_conversion() {
        assert_eq!(MsgType::from_u8(0x01).unwrap() as u8, 0x01);
        assert_eq!(MsgType::from_u8(0x0B).unwrap() as u8, 0x0B);
        assert!(MsgType::from_u8(0xFF).is_none());
    }

    #[test]
    fn compute_checksum_consistency() {
        let payload = b"";
        let hdr = FrameHeader::new(0x01, 0, payload);
        let crc1 = hdr.compute_checksum(payload);
        let crc2 = calc_crc(&[
            hdr.version,
            hdr.msg_type,
            0,
            0,
            0,
            0,
            hdr.flags,
            0,
            0,
            0,
            0,
            hdr.req_type,
        ]);
        assert_eq!(crc1, crc2);
    }
}
