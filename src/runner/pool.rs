//! 任务池 — 管理所有任务的生命周期和状态转换。
//!
//! 覆盖 P3-PL-001 任务池基本设计、P3-PL-002 状态码范围。

use std::collections::HashSet;
use crate::runner::task::{Task, TaskId, TaskStatus};

/// 任务池。持有所有任务，提供状态查询和就绪判定。
#[derive(Debug, Clone)]
pub struct TaskPool {
    tasks: Vec<Task>,
}

impl TaskPool {
    /// 创建任务池。
    pub fn new(tasks: Vec<Task>) -> Self {
        Self { tasks }
    }

    /// 获取任务引用。
    pub fn get(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// 获取任务可变引用。
    pub fn get_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// 返回所有状态为 Ready 且依赖已完成的就绪任务。
    pub fn ready_tasks(&self) -> Vec<TaskId> {
        let done_ids: HashSet<TaskId> = self.tasks.iter()
            .filter(|t| t.status.is_terminal())
            .map(|t| t.id)
            .collect();

        self.tasks.iter()
            .filter(|t| {
                if t.status != TaskStatus::Ready {
                    return false;
                }
                t.deps.iter().all(|dep| done_ids.contains(dep))
            })
            .map(|t| t.id)
            .collect()
    }

    /// 将所有依赖已完成的 Init 任务转为 Ready。
    /// 在每次调度循环前调用。
    pub fn activate_ready_tasks(&mut self) {
        let done_ids: HashSet<TaskId> = self.tasks.iter()
            .filter(|t| t.status.is_terminal())
            .map(|t| t.id)
            .collect();

        for task in self.tasks.iter_mut() {
            if task.status == TaskStatus::Init
                && task.deps.iter().all(|dep| done_ids.contains(dep))
            {
                task.status = TaskStatus::Ready;
            }
        }
    }

    /// 所有任务是否都已完成或出错。
    pub fn all_done(&self) -> bool {
        self.tasks.iter().all(|t| t.status.is_terminal())
    }

    /// 动态添加任务（用于 TASK_FORK）。
    pub fn add_task(&mut self, task: Task) {
        self.tasks.push(task);
    }

    /// 检查指定任务是否已完成（Done 或 Error）。
    pub fn task_is_done(&self, id: TaskId) -> bool {
        self.tasks.iter()
            .any(|t| t.id == id && t.status.is_terminal())
    }

    /// 是否存在 Suspended 状态的任务。
    pub fn has_suspended(&self) -> bool {
        self.tasks.iter().any(|t| t.status == TaskStatus::Suspended)
    }

    /// 唤醒所有等待指定任务完成的 Suspended 任务。
    pub fn wake_joiners(&mut self, done_id: TaskId, return_value: u64) {
        for task in self.tasks.iter_mut() {
            if task.status == TaskStatus::Suspended
                && task.vm.join_waiting_for == Some(done_id)
            {
                task.vm.join_waiting_for = None;
                task.vm.state = crate::runner::VmStateKind::Running;
                task.vm.write_reg(crate::base::isa::reg::A0, return_value);
                task.status = TaskStatus::Ready;
            }
        }
    }

    /// 计算拓扑层级。返回 (level, task_ids) 按 level 升序排列。
    /// level 0 = 无依赖（最深），level N = 根（最浅）。
    pub fn compute_levels(&self) -> Vec<(u32, Vec<TaskId>)> {
        let n = self.tasks.len();
        if n == 0 {
            return Vec::new();
        }

        // 每个任务的 deps 集合
        let dep_sets: Vec<HashSet<TaskId>> = self.tasks.iter()
            .map(|t| t.deps.iter().copied().collect())
            .collect();

        // 从无依赖的任务开始，逐层移除
        let mut removed: HashSet<TaskId> = HashSet::new();
        let mut levels: Vec<(u32, Vec<TaskId>)> = Vec::new();

        loop {
            // 找出当前层所有依赖都已移除的任务
            let mut current: Vec<TaskId> = Vec::new();
            for task in &self.tasks {
                if removed.contains(&task.id) {
                    continue;
                }
                let deps_removed = dep_sets[task.id as usize]
                    .iter()
                    .all(|dep| removed.contains(dep));
                if deps_removed {
                    current.push(task.id);
                }
            }

            if current.is_empty() {
                break;
            }

            let level = levels.len() as u32;
            for &id in &current {
                removed.insert(id);
            }
            levels.push((level, current));
        }

        levels
    }

    /// 获取任务总数。
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// 获取所有任务的迭代器。
    pub fn all_tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// 获取所有已完成的条目及其返回值。
    pub fn results(&self) -> Vec<(TaskId, TaskStatus, u64, u64)> {
        self.tasks.iter()
            .map(|t| (t.id, t.status, t.return_value, t.total_instrs))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::task::Task;
    use crate::runner::VmState;
    use crate::base::ir::{AtxeBinary, Header};

    fn mock_vm() -> VmState {
        let header = Header::new(0, 0);
        let binary = AtxeBinary {
            header,
            sections: Vec::new(),
            text: vec![0],
            rodata: vec![],
            task_table: vec![],
            debug_info: vec![],
            exn_table: vec![],
            zones: vec![],
        };
        VmState::from_atxe(&binary).unwrap()
    }

    fn make_task(id: u16, deps: Vec<u16>, status: TaskStatus) -> Task {
        Task {
            id,
            entry_offset: 0,
            status,
            deps,
            vm: mock_vm(),
            return_value: 0,
            total_instrs: 0,
            quantum_instrs: 0,
        }
    }

    #[test]
    fn ready_tasks_no_deps() {
        let tasks = vec![
            make_task(0, vec![], TaskStatus::Ready),
            make_task(1, vec![], TaskStatus::Ready),
        ];
        let pool = TaskPool::new(tasks);
        let ready = pool.ready_tasks();
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn ready_tasks_with_deps() {
        let tasks = vec![
            make_task(0, vec![], TaskStatus::Done),  // completed
            make_task(1, vec![0], TaskStatus::Ready),  // dep 0 done → ready
            make_task(2, vec![0], TaskStatus::Ready),  // dep 0 done → ready
            make_task(3, vec![1], TaskStatus::Ready),  // dep 1 NOT done yet → not ready
        ];
        let pool = TaskPool::new(tasks);
        let ready = pool.ready_tasks();
        assert_eq!(ready.len(), 2);
        assert!(ready.contains(&1));
        assert!(ready.contains(&2));
    }

    #[test]
    fn ready_tasks_dep_not_met() {
        let tasks = vec![
            make_task(0, vec![], TaskStatus::Init),     // not ready
            make_task(1, vec![0], TaskStatus::Ready),   // dep 0 not terminal → not ready
        ];
        let pool = TaskPool::new(tasks);
        let ready = pool.ready_tasks();
        assert!(ready.is_empty());
    }

    #[test]
    fn all_done_true() {
        let tasks = vec![
            make_task(0, vec![], TaskStatus::Done),
            make_task(1, vec![], TaskStatus::Error),
        ];
        let pool = TaskPool::new(tasks);
        assert!(pool.all_done());
    }

    #[test]
    fn all_done_false() {
        let tasks = vec![
            make_task(0, vec![], TaskStatus::Done),
            make_task(1, vec![], TaskStatus::Ready),
        ];
        let pool = TaskPool::new(tasks);
        assert!(!pool.all_done());
    }

    #[test]
    fn get_by_id() {
        let tasks = vec![
            make_task(5, vec![], TaskStatus::Ready),
        ];
        let pool = TaskPool::new(tasks);
        assert!(pool.get(5).is_some());
        assert!(pool.get(99).is_none());
    }

    #[test]
    fn get_mut_by_id() {
        let tasks = vec![
            make_task(0, vec![], TaskStatus::Ready),
        ];
        let mut pool = TaskPool::new(tasks);
        let t = pool.get_mut(0).unwrap();
        t.status = TaskStatus::Done;
        assert_eq!(pool.get(0).unwrap().status, TaskStatus::Done);
    }
}
