//! 负载均衡器 — 加权最少任务分配 + 抗偏斜。
//!
//! 覆盖设计文档 §6.2（负载均衡算法）。

use crate::runner::task::TaskId;
use std::collections::HashMap;

/// Executor 负载信息。
#[derive(Debug, Clone)]
pub struct ExecutorLoad {
    /// Executor 索引。
    pub idx: usize,
    /// 当前工作负载估计（剩余指令数 + pending_io × io_weight）。
    pub load: f64,
    /// 是否空闲。
    pub idle: bool,
}

/// 负载均衡器。
///
/// 算法（设计文档 §6.2）：
/// 1. 优先分配给空闲 Executor
/// 2. 否则选负载最低的
/// 3. 多个负载接近（差距 < 10%）→ 随机选（抗偏斜）
/// 4. N_batch = 2 时退化为轮询
/// 5. 高积压时（就绪任务 ≥ N_batch × 2）→ 批量分配 2-3 个
pub struct LoadBalancer {
    /// 轮询计数器（N=2 时使用）。
    round_robin_counter: usize,
    /// 伪随机计数器（抗偏斜用）。
    rand_counter: usize,
}

impl Default for LoadBalancer {
    fn default() -> Self {
        Self {
            round_robin_counter: 0,
            rand_counter: 0,
        }
    }
}

impl LoadBalancer {
    /// 创建负载均衡器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 为一批就绪任务分配 Executor。
    ///
    /// # 参数
    /// - `ready`: 就绪任务 ID 列表
    /// - `exec_loads`: 各 Executor 的当前负载
    /// - `n_batch`: 并发额度
    ///
    /// # 返回
    /// `HashMap<TaskId, usize>` — 任务 → Executor 索引
    pub fn assign(
        &mut self,
        ready: &[TaskId],
        exec_loads: &[ExecutorLoad],
        n_batch: usize,
    ) -> HashMap<TaskId, usize> {
        let mut assignment = HashMap::new();

        if ready.is_empty() || exec_loads.is_empty() {
            return assignment;
        }

        let n = exec_loads.len();

        // N_batch = 2 退化为轮询
        if n_batch == 2 || n == 2 {
            for &task in ready {
                let idx = self.round_robin_counter % n;
                assignment.insert(task, idx);
                self.round_robin_counter += 1;
            }
            return assignment;
        }

        // 批量分配：高积压时一次分配 2-3 个
        let batch_size = if ready.len() >= n_batch * 2 {
            3
        } else {
            1
        };

        // 备选负载列表（可变，每次分配后更新）
        let mut loads: Vec<ExecutorLoad> = exec_loads.to_vec();

        for chunk in ready.chunks(batch_size) {
            for &task in chunk {
                let selected = self.select_executor(&loads);
                if let Some(idx) = selected {
                    assignment.insert(task, idx);
                    // 增加该 Executor 的负载估计
                    if let Some(load) = loads.iter_mut().find(|l| l.idx == idx) {
                        load.load += 100.0; // 估计新增负载
                        load.idle = false;
                    }
                }
            }
        }

        assignment
    }

    /// 选一个 Executor：
    /// 1. 空闲优先
    /// 2. 否则负载最低
    /// 3. 多个接近则随机（使用内部计数器做伪随机）
    fn select_executor(&mut self, loads: &[ExecutorLoad]) -> Option<usize> {
        // 找空闲
        let idle: Vec<&ExecutorLoad> = loads.iter().filter(|l| l.idle).collect();
        if !idle.is_empty() {
            let idx = self.rand_counter % idle.len();
            self.rand_counter = self.rand_counter.wrapping_add(1);
            return Some(idle[idx].idx);
        }

        // 找负载最低的
        let min_load = loads
            .iter()
            .map(|l| l.load)
            .fold(f64::INFINITY, f64::min);

        // 负载接近的候选（差距 < 10%）
        let candidates: Vec<&ExecutorLoad> = loads
            .iter()
            .filter(|l| {
                if min_load.is_infinite() {
                    true
                } else {
                    l.load <= min_load * 1.10
                }
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        let idx = self.rand_counter % candidates.len();
        self.rand_counter = self.rand_counter.wrapping_add(1);
        Some(candidates[idx].idx)
    }
}

/// 构建 ExecutorLoad 列表的辅助函数。
pub fn build_executor_loads(
    total_instrs: &[u64],
    idle_mask: &[bool],
) -> Vec<ExecutorLoad> {
    total_instrs
        .iter()
        .enumerate()
        .map(|(i, &instrs)| ExecutorLoad {
            idx: i,
            load: instrs as f64,
            idle: idle_mask[i],
        })
        .collect()
}

// ─── 测试 ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_balancer_assigns_to_idle_first() {
        let mut lb = LoadBalancer::new();
        let loads = build_executor_loads(
            &[1000, 0, 2000], // exec 1 idle (load=0)
            &[false, true, false],
        );
        let ready = vec![1u16, 2u16];
        let assignment = lb.assign(&ready, &loads, 4);
        // 第一个任务应分配给空闲的 exec 1
        assert_eq!(assignment.get(&1), Some(&1));
    }

    #[test]
    fn load_balancer_round_robin_for_n2() {
        let mut lb = LoadBalancer::new();
        let loads = build_executor_loads(&[0, 0], &[true, true]);
        let ready = vec![1u16, 2u16, 3u16];
        let assignment = lb.assign(&ready, &loads, 2);
        assert_eq!(assignment.len(), 3);
        // 轮询: 1→0, 2→1, 3→0
        assert_eq!(assignment.get(&1), Some(&0));
        assert_eq!(assignment.get(&2), Some(&1));
        assert_eq!(assignment.get(&3), Some(&0));
    }

    #[test]
    fn load_balancer_empty_ready() {
        let mut lb = LoadBalancer::new();
        let loads = build_executor_loads(&[], &[]);
        let assignment = lb.assign(&[], &loads, 4);
        assert!(assignment.is_empty());
    }

    #[test]
    fn build_executor_loads_utility() {
        let loads = build_executor_loads(&[50, 100], &[true, false]);
        assert_eq!(loads.len(), 2);
        assert!(loads[0].idle);
        assert!(!loads[1].idle);
        assert_eq!(loads[0].load, 50.0);
    }
}
