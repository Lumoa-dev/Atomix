//! 事件通道 — Executor → Runtime 的 lock-free SPSC 通信。
//!
//! 覆盖设计文档 §5（事件通道）。

use std::sync::atomic::{AtomicU64, Ordering};

// ─── 事件类型 ───────────────────────────────────────

/// Executor 上报的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorEvent {
    /// 无事件。
    None,
    /// Quantum 耗尽，请求继续。
    Yield { task_id: u16 },
    /// 任务正常完成，payload = 返回值。
    TaskDone { task_id: u16, retval: u64 },
    /// 任务异常终止，payload = 错误码。
    TaskError { task_id: u16, errcode: u32 },
    /// OOM 挂起，payload = 当前内存用量。
    Oom { task_id: u16, memory_usage: u64 },
    /// 定期心跳，payload = 指令数。
    Heartbeat { task_id: u16, instrs: u32 },
}

// ─── u64 编码 ───────────────────────────────────────
//
// 位域布局（设计文档 §5.3）：
//   bits 63-48: task_id (16 bit)
//   bits 47-40: event 类型 (8 bit)
//   bits 39-32: 保留 (8 bit)
//   bits 31-0:  payload (32 bit)

const EVENT_NONE: u8 = 0x00;
const EVENT_YIELD: u8 = 0x01;
const EVENT_TASK_DONE: u8 = 0x02;
const EVENT_TASK_ERROR: u8 = 0x03;
const EVENT_OOM: u8 = 0x04;
const EVENT_HEARTBEAT: u8 = 0x05;

fn encode_event(task_id: u16, event_type: u8, payload: u32) -> u64 {
    (task_id as u64) << 48 | (event_type as u64) << 40 | (payload as u64)
}

fn decode_event(raw: u64) -> ExecutorEvent {
    let task_id = ((raw >> 48) & 0xFFFF) as u16;
    let event_type = ((raw >> 40) & 0xFF) as u8;
    let payload = (raw & 0xFFFF_FFFF) as u32;

    match event_type {
        EVENT_NONE => ExecutorEvent::None,
        EVENT_YIELD => ExecutorEvent::Yield { task_id },
        EVENT_TASK_DONE => ExecutorEvent::TaskDone {
            task_id,
            retval: payload as u64,
        },
        EVENT_TASK_ERROR => ExecutorEvent::TaskError {
            task_id,
            errcode: payload,
        },
        EVENT_OOM => ExecutorEvent::Oom {
            task_id,
            memory_usage: payload as u64,
        },
        EVENT_HEARTBEAT => ExecutorEvent::Heartbeat {
            task_id,
            instrs: payload,
        },
        _ => ExecutorEvent::None,
    }
}

// ─── 事件通道 ───────────────────────────────────────

/// 固定大小的 lock-free SPSC 事件通道。
///
/// - Executor[i] 只写 `events[i]`（`store(Release)`）
/// - Runtime 只读 `events[0..N-1]`（`load(Acquire)`）
/// - 不存在两个线程竞争同一个地址。
#[derive(Debug)]
pub struct EventChannel {
    events: Vec<AtomicU64>,
}

impl Clone for EventChannel {
    fn clone(&self) -> Self {
        let events: Vec<AtomicU64> = self
            .events
            .iter()
            .map(|a| AtomicU64::new(a.load(Ordering::Relaxed)))
            .collect();
        Self { events }
    }
}

impl EventChannel {
    /// 创建 N_batch 个事件槽位，初始化为 None。
    pub fn new(n: usize) -> Self {
        let events = (0..n).map(|_| AtomicU64::new(0)).collect();
        Self { events }
    }

    /// 返回槽位数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Executor 上报事件（Release 语义）。
    pub fn post(&self, idx: usize, event: ExecutorEvent) {
        if let Some(slot) = self.events.get(idx) {
            let raw = match event {
                ExecutorEvent::None => 0,
                ExecutorEvent::Yield { task_id } => encode_event(task_id, EVENT_YIELD, 0),
                ExecutorEvent::TaskDone { task_id, retval } => {
                    encode_event(task_id, EVENT_TASK_DONE, retval as u32)
                }
                ExecutorEvent::TaskError { task_id, errcode } => {
                    encode_event(task_id, EVENT_TASK_ERROR, errcode)
                }
                ExecutorEvent::Oom {
                    task_id,
                    memory_usage,
                } => encode_event(task_id, EVENT_OOM, memory_usage as u32),
                ExecutorEvent::Heartbeat { task_id, instrs } => {
                    encode_event(task_id, EVENT_HEARTBEAT, instrs)
                }
            };
            slot.store(raw, Ordering::Release);
        }
    }

    /// Runtime 消费一个事件（Acquire 语义）。
    pub fn poll(&self, idx: usize) -> ExecutorEvent {
        let slot = match self.events.get(idx) {
            Some(s) => s,
            None => return ExecutorEvent::None,
        };
        let raw = slot.load(Ordering::Acquire);
        if raw == 0 {
            return ExecutorEvent::None;
        }
        slot.store(0, Ordering::Relaxed); // 消费后清零
        decode_event(raw)
    }

    /// 轮询所有槽位，返回非空事件列表。
    pub fn poll_all(&self) -> Vec<(usize, ExecutorEvent)> {
        self.events
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                let raw = slot.load(Ordering::Acquire);
                if raw == 0 {
                    return None;
                }
                slot.store(0, Ordering::Relaxed);
                Some((i, decode_event(raw)))
            })
            .collect()
    }
}

// ─── Executor 统计信息 ──────────────────────────────

/// Executor 向 Runtime 暴露的运行时统计（lock-free 共享）。
#[derive(Debug, Clone, Default)]
pub struct ExecutorStats {
    /// 当前 PC。
    pub pc: u32,
    /// 当前内存用量（字节）。
    pub memory_usage: u64,
    /// 总执行指令数。
    pub total_instrs: u64,
    /// 已完成 quantum 数。
    pub total_quantums: u32,
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_yield() {
        let e = ExecutorEvent::Yield { task_id: 42 };
        let raw = encode_event(42, EVENT_YIELD, 0);
        let decoded = decode_event(raw);
        assert_eq!(decoded, e);
    }

    #[test]
    fn encode_decode_task_done() {
        let e = ExecutorEvent::TaskDone {
            task_id: 7,
            retval: 12345,
        };
        let raw = encode_event(7, EVENT_TASK_DONE, 12345);
        let decoded = decode_event(raw);
        assert_eq!(decoded, e);
    }

    #[test]
    fn encode_decode_oom() {
        let e = ExecutorEvent::Oom {
            task_id: 3,
            memory_usage: 0xA00000,
        };
        let raw = encode_event(3, EVENT_OOM, 0xA00000);
        let decoded = decode_event(raw);
        assert_eq!(decoded, e);
    }

    #[test]
    fn channel_post_poll() {
        let ch = EventChannel::new(4);
        ch.post(0, ExecutorEvent::Yield { task_id: 1 });
        ch.post(
            2,
            ExecutorEvent::TaskDone {
                task_id: 2,
                retval: 99,
            },
        );
        assert_eq!(ch.poll(0), ExecutorEvent::Yield { task_id: 1 });
        assert_eq!(ch.poll(1), ExecutorEvent::None);
        assert_eq!(
            ch.poll(2),
            ExecutorEvent::TaskDone {
                task_id: 2,
                retval: 99
            }
        );
    }

    #[test]
    fn channel_poll_all() {
        let ch = EventChannel::new(3);
        ch.post(
            0,
            ExecutorEvent::Heartbeat {
                task_id: 0,
                instrs: 100,
            },
        );
        ch.post(
            2,
            ExecutorEvent::TaskError {
                task_id: 2,
                errcode: 1,
            },
        );

        let events = ch.poll_all();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, 0);
        assert_eq!(events[1].0, 2);
        // 消费后应为空
        assert!(ch.poll_all().is_empty());
    }

    #[test]
    fn channel_out_of_bounds() {
        let ch = EventChannel::new(2);
        ch.post(5, ExecutorEvent::Yield { task_id: 0 }); // should not panic
        assert_eq!(ch.poll(5), ExecutorEvent::None);
    }

    #[test]
    fn zero_is_none() {
        let ch = EventChannel::new(1);
        assert_eq!(ch.poll(0), ExecutorEvent::None);
    }

    #[test]
    fn executor_stats_default() {
        let s = ExecutorStats::default();
        assert_eq!(s.pc, 0);
        assert_eq!(s.memory_usage, 0);
        assert_eq!(s.total_instrs, 0);
        assert_eq!(s.total_quantums, 0);
    }
}
