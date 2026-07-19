# Atomix Runner 完整架构设计

> 架构版本: v0.3（全面修订）
> 最后更新: 2026-07-20
> 状态: **设计冻结 — 待仿真验证后进入实现**
> 文档中涉及到的算法已经在 /sim/ 下完成仿真

---

## 1. 总体架构

### 1.1 核心命题

Atomix Runner 在**资源紧缺环境**下寄生运行。核心约束：

- 物理内存有限（可能低至 64–256 MB）
- CPU 核心有限（1–4 核常见）
- 磁盘/网络延迟不可控
- 但**吞吐量必须够高**才有竞争力

因此架构设计的三个基石：**内存颗粒度精确、热路径零Runtime介入、线程数与内存并行度解耦**。

### 1.2 组件关系

Runner 由四个核心模块构成，分三层部署：

| 层 | 组件 | 职责 | 线程归属 |
|:---|:-----|:-----|:---------|
| **监管层** | Runtime | 任务池管理、N_batch 决策、内存槽位分配、事件处理、回归模型维护 | 主线程 |
| **执行层** | Executor 线程池（N_batch 个） | 指令取指/解码/执行、Quantum 计时、OOM 自检、事件上报 | 每 Executor 一个线程 |
| **存储层** | 磁盘仓库 | .atxe 二进制持久化、TaskMeta 持久化、统计数据持久化 | 独立 I/O 线程 |

交互关系：

- **Runtime** 通过**事件通道**（lock-free SPSC）与 Executor 通信，正常执行时零介入
- **Runtime** 通过**内存映射**将沙箱内存页映射到 Executor 的地址空间
- 每个 **Executor** 持有独立的 VmState（寄存器、沙箱内存、PC），与线程 1:1 绑定
- **编译预测**（`.atxe` 头部的 `memory_profile`）为 SlotManager 提供初始分配依据

详细数据流见 §3（任务生命周期）和 §5（事件通道）。

---

## 2. Executor 定义

### 2.1 Executor = VM = 线程（1:1:1）

**Executor 是持有 VmState 并驱动其执行指令的执行体。**

```
Executor {
    寄存器文件: [u64; 16]                 
    PC: usize                            
    SandboxMemory: Vec<u8>               
      ├── .text (从 .atxe 加载)           
      ├── .rodata (从 .atxe 加载)         
      ├── 堆区 (运行时按需增长)             
      └── 栈区 (运行时按需增长)             
    Quantum 计数器: u32                   
    状态: Running / Suspended / Halted    
    事件上报点: &AtomicU64 (Runtime分配)    
    任务元信息: task_id, slot_id           
}
```

每个 Executor：
- **独占一个线程**（线程数 = N_batch，由 BatchManager 决定）
- **独占一个 VmState**（没有"切换 VM"的概念，不需要 context save/restore）
- **直接操作自己的 SandboxMemory**（没有中间层、没有间接调用）
- **是"挂机托管"的**——Runtime 把任务和数据分配好之后，Executor 独立执行，正常时不需要和 Runtime 通信

### 2.2 线程数 VS N_batch

N_batch = Executor 线程数 = 同时存在于内存中的 VmState 数量。

线程数固定规则：
- 当 N_batch ≤ CPU 核心数：每个 Executor 独占一个核心，零 OS 上下文切换
- 当 N_batch > CPU 核心数：OS 时间片轮转，但量子充足（~1ms）下开销可忽略

N_batch 由 BatchManager 动态计算，但 Executor 线程数变化时**不销毁线程**——多余的线程进入 idle 状态等待新任务。

### 2.3 Executor 主循环

```rust
// Executor 线程的主函数
fn executor_main(mut exec: Executor, event_tx: &AtomicU64) {
    loop {
        match exec.state {
            Running => {
                let should_yield = exec.run_quantum(QUANTUM);
                if should_yield {
                    // quantum 耗尽，上报 Yield 事件
                    event_tx.store(Event::Yield(exec.task_id), Ordering::Release);
                    // 等待 Runtime 分配下一个 quantum（或新任务）
                    exec.wait_for_signal();
                }
            }
            Halted => {
                event_tx.store(Event::TaskDone(exec.task_id, exec.retval), Ordering::Release);
                exec.wait_for_new_task();  // Runtime 分配下一个任务
            }
            Suspended(OOM) => {
                event_tx.store(Event::Oom(exec.task_id, exec.memory_usage), Ordering::Release);
                exec.wait_for_expand();    // Runtime 分配更多内存
            }
            Suspended(Blocked) => {
                event_tx.store(Event::Blocked(exec.task_id), Ordering::Release);
                exec.wait_for_wakeup();    // Runtime 在条件满足时唤醒
            }
            Error(e) => {
                event_tx.store(Event::TaskError(exec.task_id, e), Ordering::Release);
                exec.wait_for_new_task();  // 错误任务终止，分配下一个
            }
        }
    }
}
```

### 2.4 Executor 热路径

Executor 在正常执行时**没有 Runtime 介入的空间**。一条指令的执行路径（release build）仅涉及：

```
ADD Rd, Rs1, Rs2
  → fetch:  读 self.text[self.pc]              1 次 Vec 索引
  → decode: 拆 opcode + rd + rs1 + rs2         位运算
  → exec:   self.regs[rd] = self.regs[rs1] + self.regs[rs2]   2 次读 + 1 次写
  → pc++:   self.pc += 1                       整数加法
  → quantum: self.quantum += 1                  整数加法
  → 检查:   self.quantum < QUANTUM             整数比较
```

无锁、无分配、无 Runtime 调用、无 indirect dispatch。

OOM 检查不在热路径上——它只插在 `ECALL alloc` 和 `STORE`（栈扩展）之前。这些分配操作本身就不是热路径。

---

## 3. 任务生命周期

### 3.1 完整流转

一个任务从接入到完成经历以下阶段：

1. **接入**（HTTP / 文件）→ 分配 task_id，写入磁盘，记录 `disk_offset`，读取 `memory_profile`
2. **排队** → TaskMeta 进入 TaskPool（约 65 字节），等待 BatchManager 决策
3. **预载** → Runtime 根据执行进度，异步将下一任务的 .atxe 从磁盘读到内存缓冲
4. **槽位分配** → SlotManager 分配虚地址空间 + 物理内存页
5. **执行** → .atxe 加载到 Executor 的 VmState，进入 `run_quantum` 循环
6. **完成/清理** → 回写结果到磁盘，释放物理内存，回收槽位，更新统计样本

### 3.2 TaskMeta — 轻量任务元数据

**关键设计**：任务本身（.atxe 二进制）**不在内存中**。只在磁盘上。内存中的只有 TaskMeta：

```rust
struct TaskMeta {
    task_id: u16,              // 4 字节（4B）
    name: [u8; 32],            // 32 字节
    disk_offset: u64,          // 8 字节 — .atxe 文件中的偏移
    disk_size: u32,            // 4 字节 — .atxe 文件大小
    memory_addr: AtomicU64,    // 8 字节 — 运行时的内存地址（运行时填充/更新）
    status: AtomicU8,          // 1 字节 — Init/Ready/Running/Done/Error
    entry_point: u32,          // 4 字节 — 入口 PC 偏移
    compiler_peak_mb: f32,     // 4 字节 — 编译预测峰值（MB）
    actual_peak_mb: f32,       // 4 字节 — 实际峰值（执行后填入）
}
// 总计 ≈ 65 字节 / 任务

// 100 万个任务 → 65 MB     ✅ 可接受（按需分批加载）
// 1 亿个任务   → 6.5 GB    ⚠️ 需按 backlog 分批加载 TaskMeta
```

### 3.3 双地址管理

每个任务始终维护两个地址：

| 地址 | 含义 | 生命周期 | 是否变更 |
|:-----|:-----|:---------|:---------|
| **disk_addr** | .atxe 二进制在磁盘仓库中的位置 | 任务入池到清理 | ❌ 固定 |
| **mem_addr** | 运行时 SandboxMemory 的基址 | 仅在 Running 期间有效 | ✅ 每次加载可能不同 |

**关键**：mem_addr 每次任务加载到 Executor 时都可能不同（取决于 SlotManager 的实时分配策略）。因此 Executor 不保存"这个任务应该在哪"——Runtime 在分配槽位时把 mem_addr 写入 TaskMeta，Executor 通过 `memory_addr` 原子变量读取。

### 3.4 预载机制

Runtime 知道每个 Executor 的当前执行进度，也知道 TaskPool 中下一个就绪任务是谁。因此可以在当前任务执行期间**异步预加载下一任务的 .atxe**：

```
时间线：

T1: Executor[0] 还在执行任务 A（还剩 ~300 条指令）
    → Runtime 从 TaskPool 中选出下一个就绪任务 B
    → 开始异步拉取任务 B 的 .atxe（网络 → 磁盘）

T2: 任务 A 完成
    → 任务 B 的 .atxe 已在本机磁盘
    → 直接从磁盘加载到 Slot[0]（不需要等网络）
    → Executor[0] 加载 VmState，开始执行任务 B

对比：
  · 无预载：任务完成 → 现拉网络 → 等传输 → 加载 → 执行
  · 有预载：网络延迟被隐藏在执行时间背后
```

预载深度可动态调整：
- 网络延迟高 → 预载 2–3 个
- 任务执行时间短 → 预载 2–3 个
- 网络好 + 任务时间长 → 预载 1 个足够

### 3.5 "不进内存"原则

```
任务在以下状态下，.atxe 二进制不在物理内存中：

  · 刚入池（Init）            → 在磁盘
  · 等待依赖（Blocked）        → 在磁盘
  · 就绪但无空槽位（Ready）     → 在磁盘
  · 执行完成（Done）           → 在磁盘（结果已回写）
  · 异常终止（Error）          → 在磁盘（错误已记录）

只有以下状态，任务在物理内存中：

  · 正在执行（Running）        → 在 Executor 的 SandboxMemory 中
  · OOM 挂起等待扩容（OOM）    → 仍在 SandboxMemory 中（被冻结）
```

**这意味着**：100 万个任务入池，内存里最多只有 N_batch 个 VmState。其他 >999,900 个任务只有 65 字节的 TaskMeta。

---

## 4. 内存模型

### 4.1 整体架构

内存分为三个区域：

| 区域 | 占比 | 用途 |
|:-----|:-----|:------|
| **活动槽位** | N_batch 个 | 每个运行中的任务占一个槽位，物理内存按需分配 |
| **滑道预留** | 2 × slot_size | 任务 OOM 时滑入扩展，永不分配给新任务 |
| **安全冗余** | 15% | 宿主 OS 预留，防止整机 OOM |

物理内存总量 P，可分配内存 M = P × (1 - β_safety)。M 减去滑道后，剩余的由 N_batch 个槽位共享。

### 4.2 虚到实地址分配

**核心设计**：虚地址预分配是管理结构，物理内存按需分配。

BatchManager 算出 N_batch 后，SlotManager 预分配 N_batch 个虚地址槽位 + 2 个滑道槽位。虚地址空间按固定间隔排列（如每槽 16MB），但实际物理内存只分配任务所需的量：

- 任务 A（编译器预测 10MB）→ 分配 10MB 物理内存，映射到 Slot[0] 的虚地址
- 任务 B（编译器预测 8MB）→ 分配 8MB 物理内存，映射到 Slot[1] 的虚地址

槽位之间的虚地址空洞留着给内存扩展用。**物理内存不浪费**。

### 4.3 槽位管理

```rust
struct Slot {
    slot_id: u16,
    vaddr: u64,                  // 虚地址基址
    physical_size: u64,          // 已分配的物理内存大小
    max_size: u64,               // 最大可扩展大小（槽位上限）
    task_id: Option<u16>,        // 当前占用任务（None = 空闲）
    status: SlotStatus,          // Free / Occupied / Dead / Slipway
}
```

槽位分配逻辑：

1. 预估峰值 = comp_mb × max(δ, 1.2)，其中 δ = actual_peak / compile_peak 的滑动平均
2. 在空闲槽位中找 max_size ≥ 预估峰值的
3. 有则分配，physical_size = 预估峰值
4. 无则任务进等待队列，等槽位释放后重试

### 4.4 滑道溢出机制

当任务执行中内存超过槽位上限（OOM）：

1. Executor 暂停执行，上报 OOM 事件（含当前用量 + 预估扩展量）
2. Runtime 检查滑道是否有足够空间
3. 有空间 → 任务滑入滑道槽位，原槽位标记 Dead，扩展 physical_size
4. 无空间 → 触发 AIMD 反馈降低 N_batch（见 §7）
5. Executor 恢复执行

滑道始终保留 2 个槽位（1.5x ~ 3x 标准槽位大小），永不分配给新任务。

### 4.5 物理内存按需分配

SandboxMemory 底层使用惰性分配——`Vec<u8>` 初始容量 = 编译预测峰值 × 修正系数，不预分配多余空间。如果运行时超出，通过 `resize()` 扩展（触发 OOM 流程）。

**不做页粒度懒映射**：那需要 MMU 或复杂的页表模拟，破坏热路径性能。物理内存一次性分配到位，但只分配"够用"的量。

### 4.6 死区合并（Defrag）

任务 OOM 滑移或完成后，原槽位变成 Dead。相邻 Dead + Free 槽位可合并为一个更大的空闲槽位：

- 初始: `[Slot0: task A] [Slot1: task B] [Slot2: free] [Slip: free]`
- task A OOM 后: `[Slot0: DEAD] [Slot1: task B] [Slot2: free] [Slip0: task A]`
- task B 完成后: `[Slot0: DEAD] [Slot1+2: merged free] [Slip0: task A]`

合并时机由 ROI 评估决定：

```
defrag_cost = 迁移任务数 × 迁移时间
defrag_benefit = 回收的死区大小 × 预期驻留时间 + 碎片率下降收益

当 defrag_benefit > defrag_cost × 2 时执行。
```

死区合并在 Runtime 空闲时作为后台任务执行，不阻塞正常调度。

### 4.7 仿真验证

内存模型的关键指标是：大内存任务场景下 OOM 能否被抑制、吞吐量能否保持。

**内存压力场景**（80% 任务 100-800MB，到达率 8/s）：

![吞吐量对比](../sim/reports/内存压力/throughput_comparison.png)
![时间序列](../sim/reports/内存压力/time_series.png)

| 变体 | 吞吐量 | OOM% | 平均 N_batch | 平均延迟 |
|------|--------|------|-------------|---------|
| Baseline | 3.9/s | 0.00% | 5.6 | 19.3s |
| FullOpt | **4.1/s** | 0.00% | 5.6 | 20.2s |
| FullAdaptive | 3.9/s | 0.00% | 5.5 | 19.3s |

N_batch 降至 5-6（远低于 CPU 上限 ~46），说明控制器正确感知了内存压力并收缩了并发度。0% OOM 率验证了滑道溢出机制的有效性。

**高碎片回收场景**（任务内存范围 50-500MB，频繁 OOM）：

| 变体 | 吞吐量 | 平均延迟 | N_batch | 相比 Baseline |
|------|--------|---------|---------|--------------|
| Baseline | 5.5/s | 10.1s | 4.3 | — |
| FullOpt | 5.9/s | 14.3s | 4.2 | +7% |
| FullAdaptive | **6.4/s** | 12.7s | 4.1 | **+16%** |

FullAdaptive 在高碎片场景下吞吐领先 16%，因其自适应平滑能更快响应碎片率变化。

---

## 5. 事件通道

### 5.1 阻塞型 ECALL 策略

**阻塞型 ECALL 直接阻塞当前 Executor 线程，不做特殊处理。**

```
Executor 遇到 tcp_recv / fs_read 等阻塞型 ECALL：
  → 阻塞在系统调用上
  → 该 Executor 的线程挂起
  → 其他 Executor 线程不受影响（各线程独立）
  → 系统调用返回后继续执行

不引入异步 IO 框架、不返回 Blocked 事件、不回收线程。
```

**理由**：
- Executor = 线程 = VM（1:1:1），阻塞一个不影响其他
- 引入异步 IO 的复杂度远大于收益（需要 IO 框架、回调、状态机）
- 当前阶段不需要"省这个线程"——线程本身就在那，阻塞了也不占 CPU
- 如果将来需要优化（如线程数太少、阻塞频繁），可以后续升级为 io_uring / epoll 事件驱动方案，但不影响现有架构

### 5.2 无锁 SPSC 设计

**原则**：Executor 只写自己的事件点，Runtime 只读所有事件点。单生产者单消费者，天然无锁。

Runtime 启动时创建固定大小的事件数组 `exec_events: [AtomicU64; N_batch]`：
- `Executor[i]` 只写 `exec_events[i]`（`AtomicU64::store`）
- `Runtime` 只读 `exec_events[0..N_batch-1]`（`AtomicU64::load`）

不存在两个线程竞争同一个地址的情况。

### 5.3 事件编码

每个事件用一个 u64 编码：

| 位域 | 大小 | 含义 |
|:-----|:-----|:-----|
| 63-48 | 16 bit | task_id |
| 47-40 | 8 bit | event 类型 |
| 39-32 | 8 bit | 保留 |
| 31-0 | 32 bit | payload |

event 类型：
- `0x01` = **Yield**：quantum 耗尽，请求继续
- `0x02` = **TaskDone**：任务正常完成，payload = 返回值
- `0x03` = **TaskError**：任务异常终止，payload = 错误码
- `0x04` = **Oom**：OOM 挂起，payload = 当前内存用量
- `0x05` = **Heartbeat**：定期心跳，payload = 指令数
- `0x00` = None：无事件

阻塞型 ECALL（tcp_recv、fs_read 等）直接阻塞当前 Executor 线程，不需要上报事件。详见 §5.1。

### 5.4 Runtime 事件处理

Runtime 主循环是非阻塞事件驱动：

1. 轮询所有 Executor 的事件点（`load(Acquire)`），非零则消费（`store(0, Release)`）并处理
2. 处理非事件任务：检查预载时机、死区合并时机、更新 BatchManager 统计
3. 无事件时通过条件变量休眠（`sleep(100μs)`），Executor 上报事件时唤醒

**关键**：Runtime 的"轮询"不是忙等——事件通道为空时休眠，Executor 通过条件变量边缘触发唤醒。

### 5.5 Executor 统计信息共享

Executor 每 quantum 结束时把运行时统计写入共享内存区域。Runtime 通过它获取全貌，无需锁：

```rust
struct ExecutorStats {
    pc: AtomicU32,              // 当前 PC
    memory_usage: AtomicU64,    // 当前内存用量
    total_instrs: AtomicU64,    // 总指令数
    total_quantums: AtomicU32,  // 已完成 quantum 数
}
// exec_stats: [ExecutorStats; N_batch]   ← 每个 Executor 独占一个元素
```

Executor 写自己的条目（`store`），Runtime 读所有条目（`load`）。单写单读，无需锁。

---

## 6. 调度与负载均衡

### 6.1 任务分配模型

当 Executor 完成当前任务（TaskDone）或 quantum 耗尽（Yield）时，Runtime 为其分配下一个任务：

| 触发时机 | 分配行为 |
|:---------|:---------|
| Executor 上报 TaskDone | 立即分配下一个就绪任务 |
| Executor 上报 Yield | 同一任务继续下一 quantum |
| 新任务入池 | 如果有空闲 Executor，立即分配 |

### 6.2 负载均衡算法

**目标**：避免"劳模线程"——某些 Executor 忙死，某些 Executor 闲着。

算法：加权最少任务 + 抗偏斜

1. 如果有 Executor 处于 idle → 优先分配给它
2. 否则计算每个 Executor 的负载：`load[i] = quantum_remaining[i] + pending_io[i] × io_weight`
3. 选择负载最低的 Executor
4. 如果多个 Executor 负载接近（差距 < 10%）→ 随机选一个（避免"惊群"效应）
5. 高积压时（就绪任务 ≥ N_batch × 2）→ 批量分配，每次 2-3 个任务

N_batch = 2 时退化为轮询分配。

### 6.3 预载调度算法

预载在 Runtime 主循环中决策：对每个活跃 Executor，估算其剩余执行时间。如果剩余时间 > 网络延迟 × 1.5，异步拉取下一个任务的 .atxe。

预载深度动态调整：

```
prefetch_depth = clamp(ceil(avg_exec_time_ms / network_latency_ms), 1, 3)
```

网络好时预载少，网络差时预载多。

### 6.4 任务树（TASK_FORK）调度

基本原则：子任务不特殊优先，和普通任务一样从 TaskPool 申请额度。

父任务遇到 `TASK_FORK` 后的执行流程：

1. Runtime 识别任务树，从最深层叶子节点开始调度
2. 子任务进入 TaskPool，标记为普通任务，无任何优先级提升
3. 所有子任务完成前，父任务处于 `Suspended(WaitingChildren)` 状态
4. 全部子任务完成后，父任务恢复执行

高积压场景下，父任务等待超过阈值后会冻结状态并迁移回磁盘，空出槽位给子任务。

### 6.5 仿真验证

调度算法的核心指标是任务执行时间悬殊时的吞吐量和公平性。

**不平衡负载场景**（任务时间跨度 10ms-20000ms，到达率 20/s）：

![吞吐量对比](../sim/reports/不平衡负载/throughput_comparison.png)

| 变体 | 吞吐量 | 平均延迟 | P99 | N_batch |
|------|--------|---------|-----|---------|
| Baseline | 18.4/s | 2.8s | 22.5s | 13.6 |
| FullOpt | 18.5/s | 2.9s | 23.9s | 13.4 |
| FullAdaptive | 18.5/s | 2.9s | 25.8s | 13.2 |
| Lin+WGM+AimdH | **18.8/s** | 3.4s | 37.8s | 12.1 |

各变体吞吐量接近（±2%），说明在任务时间悬殊时，负载均衡器（而非平滑/合并策略）起主要作用。Lin+WGM 吞吐略高但因 N_batch 更低导致延迟略增。

**大批量小任务预载场景**（50 tasks/s，1-50ms CPU）：

| 变体 | 吞吐量 | 平均延迟 | P99 | N_batch |
|------|--------|---------|-----|---------|
| **全部** | **51.1/s** | **36ms** | **77ms** | **~46** |

所有变体表现完全一致。任务极小（1-50ms），预载调度器始终能提前就绪，延迟极低（36ms）。N_batch 接近硬件上限 ~46。

## 7. 自适应控制体系

### 7.1 N_batch 计算（修订版）

**硬上限 H** 保持原有公式：

```
H = ⌊ min(C, M, I, N) ⌋

C = (CPU_cores × α_cpu) / CPU_per_task
M = (MEM_free  × α_mem) / MEM_per_task_effective
I = (IOPS_avail × α_io) / IOPS_per_task
N = (NET_avail  × α_net) / NET_per_task
```

**修订点**：`MEM_per_task_effective` 不再是固定值，而是由回归模型动态校正：

```
MEM_per_task_effective =
    if regression_model.ready:  α × compiler_peak_mb + β
    else:                       compiler_peak_mb × 1.5   // 冷启动保守估计
```

**软上限 S** 保持因子合并 + 加权几何平均，因子列表增加一个置信因子 θ：

```
因子列表：[β(积压), λ(速度), σ(体积), γ(方差), θ(置信)]
权重分配：[0.20, 0.20, 0.20, 0.15, 0.25]

θ(r²) = 0.70 + 0.30 × r²
```

θ 获得最高权重（0.25），因为内存精度直接影响 OOM 风险。r² → 0 时 θ → 0.70 收缩 N_batch，r² → 1 时 θ → 1.00 恢复正常。

### 7.2 编译器内存预测

每个 .atxe 文件头携带 `memory_profile`，提供编译器估算的峰值内存：

```rust
struct MemoryProfile {
    code_mb: f32,      // .text 段大小（精确）
    rodata_mb: f32,    // .rodata 段大小（精确）
    stack_mb: f32,     // 栈峰值（上界）
    heap_mb: f32,      // 堆峰值（上界）
    peak_mb: f32,      // 总和上界 = code + rodata + max(stack, heap)
}
```

### 7.3 线性回归修正

**样本收集**：每个任务执行完成后，Runtime 记录一条样本 `(compiler_peak_mb, actual_peak_mb)`，持久化到 `<data_dir>/stats/memory_samples.csv`。

**回归训练**：样本数 ≥ 50 时触发 OLS 训练：

```
actual_peak = α × compiler_peak + β + ε
```

使用条件：
- 样本数 ≥ 50 且 r² ≥ 0.6 → 使用回归修正
- 否则退回到保守估计（×1.5）
- 每 200 个新样本重新训练一次

线性回归够用的原因：编译预测 `peak_mb` 本身是上界估计，实际峰值与编译预测之间存在线性关系——预测偏高但成比例。回归就是学这个"偏高系数"。

### 7.4 冷启动协议（修订版）

原设计的冷启动（N_batch = 2 起步）针对"所有任务全进内存"的模型。现在任务和数据分离，冷启动的重点从"怕内存爆"变成"怕回归模型没有数据"：

| 阶段 | 条件 | N_batch 上限 | MEM 估计 |
|:-----|:-----|:-------------|:---------|
| 0 — 首任务 | 无数据 | 1 | compiler_peak × 1.5 |
| 1 — 前 5 个 | 少量数据 | min(2, H) | δ × compiler_peak（滑动平均） |
| 2 — 5~50 个 | 积累期 | min(H, S) | δ × compiler_peak |
| 3 — ≥50 个 | 回归就绪 | min(H, S) | 回归修正值 |

**关键区别**：爬坡速度取决于任务完成数（每个任务都提供数据），回归模型成熟后自动收敛。不再需要硬编码的 N_batch 爬坡序列。

### 7.5 仿真验证

自适应控制体系是 Runner 的核心算法，涵盖 N_batch 计算、因子平滑、OOM 反馈、滑道调整。以下三个场景覆盖不同的负载特征。

**稳态混合负载**（恒定 15 tasks/s，4 象限混合）：

![吞吐量对比](../sim/reports/稳态混合负载/throughput_comparison.png)
![帕累托前沿](../sim/reports/稳态混合负载/pareto_frontier.png)

| 变体 | 吞吐量 | 平均延迟 | P99 | N_batch |
|------|--------|---------|-----|---------|
| Baseline | 12.4/s | 2.97s | 16.5s | 10.6 |
| FullOpt | 12.7/s | 2.39s | 19.5s | 10.6 |
| FullAdaptive | 13.0/s | 2.77s | 29.2s | 10.5 |
| Lin+WGM+AimdH | **13.6/s** | 3.19s | 30.0s | 8.9 |

稳态下算法差异约 10%。Lin+WGM（线性插值 + 加权几何平均）吞吐最高但延迟方差较大。

**突发冲击**（5× 突发 15s）：

| 变体 | 吞吐量 | 平均延迟 | P99 | N_batch |
|------|--------|---------|-----|---------|
| Baseline | 14.0/s | 2.21s | 14.1s | 11.3 |
| FullOpt | 14.1/s | 2.03s | 14.7s | 11.4 |
| FullAdaptive | 14.3/s | 2.02s | 14.4s | 11.3 |
| Lin+WGM+AimdH | **15.2/s** | 3.21s | 30.3s | 9.8 |

突发场景下 Lin+WGM 吞吐领先 8%，但以更高延迟为代价。

**震荡测试**（负载快速切换）：

| 变体 | 吞吐量 | 平均延迟 | P99 | N_batch |
|------|--------|---------|-----|---------|
| Baseline | 16.8/s | 4.39s | 17.2s | 7.4 |
| FullOpt | 17.0/s | 3.43s | 20.9s | 7.5 |
| FullAdaptive | **17.7/s** | 4.29s | 56.0s | 7.2 |

震荡场景下 FullAdaptive 吞吐最高——自适应平滑的在线陡峭度调节在此场景发挥作用。

---

## 8. 算法规范

### 8.1 负载均衡算法

```
输入:
  ready: Vec<TaskId>      // 就绪任务列表
  exec_load: [f64; N]     // 每个 Executor 的当前负载估计
  exec_idle: [bool; N]    // 每个 Executor 是否空闲

输出:
  assignment: HashMap<TaskId, usize>  // 任务 → Executor 索引

if N == 2:
    // 2 线程直接轮询
    for (i, task) in ready.iter().enumerate():
        assignment[task] = i % 2
else:
    for task in ready:
        // 优先分配给空闲 Executor
        idle_indices: Vec<usize> = (0..N).filter(|i| exec_idle[*i]).collect()
        if !idle_indices.is_empty():
            selected = idle_indices[rand(idle_indices.len())]
        else:
            // 选负载最低的
            min_load = exec_load.iter().min_by(|a, b| a.partial_cmp(b))
            candidates = (0..N).filter(|i| exec_load[i] ≈ min_load ± 10%)
            selected = candidates[rand(candidates.len())]
        
        assignment[task] = selected
        exec_load[selected] += task.estimated_instrs

return assignment
```

### 8.2 预载调度算法

```
输入:
  active: Vec<ExecutorInfo>       // 活跃 Executor 信息
  task_pool: TaskPool             // 任务池
  network_rtt_ms: f64             // 网络往返延迟估计

每个 Event::TaskDone 或 Event::Yield 处理后触发:

for exec in active:
    if exec.status != Running:
        continue
    
    remaining_instrs = exec.quantum_remaining()
    remaining_time_ms = remaining_instrs × AVG_IPC_RATE_NS / 1_000_000
    
    if remaining_time_ms < PREFETCH_THRESHOLD_MS:  // 默认 50ms
        continue
    
    next_tasks = task_pool.peek_next(exec.task_id, prefetch_depth)
    for task in next_tasks:
        if !task.is_on_disk():
            task.start_async_fetch(network_rtt_ms)
```

### 8.3 死区合并算法

```
输入:
  slots: Vec<Slot>                // 所有槽位（含滑道）
  active_executors: Vec<Status>   // 当前活跃度

触发时机:
  · Runtime 事件循环空闲时
  · 每次槽位状态变更后（TaskDone / OOM Slip）

算法:
  1. 计算当前碎片率:
     dead_total = Σ dead_slot.size
     free_total = Σ free_slot.size
     total = Σ all_slot.size
     fragmentation = (dead_total + free_total) / total

  2. 如果 fragmentation < 30% → 不合并（收益不够）
  
  3. 枚举所有可合并的方案:
     for each dead_slot d:
         for each adjacent slot s:
             if s is Free or Dead:
                 merged_size = d.size + s.size
                 // 评估把 d 中的任务移到 merged 是否值得
                 cost = estimate_migrate_cost(d.task)
                 benefit = merged_size × expected_idle_time(d)
                 if benefit > cost × 2:
                     execute_merge(d, s)

  4. 执行迁移:
     migrate_task(d.task → target_slot)
     mark d as Free
     merge adjacent free slots

复杂度: O(S²) 其中 S = 槽位数（通常 ≤ 64），可接受
```

### 8.4 线性回归预测算法

```
struct RegressionModel {
    alpha: f64,          // 斜率
    beta: f64,           // 截距
    r_squared: f64,      // 拟合优度
    sample_count: u64,   // 样本数
    last_trained_at: u64, // 上次训练时的样本数
}

impl RegressionModel {
    fn predict(compiler_peak: f64) -> f64 {
        if self.sample_count < MIN_SAMPLES || self.r_squared < MIN_R_SQUARED {
            return compiler_peak * SAFETY_MULTIPLIER;  // 退回到 ×1.5
        }
        let predicted = self.alpha * compiler_peak + self.beta;
        predicted.max(compiler_peak * 0.5)  // 不能低于编译器预测的一半
              .min(compiler_peak * 3.0)     // 但不能无限高
    }

    fn train(samples: &[(f64, f64)]) {
        // OLS 训练
        let n = samples.len() as f64;
        let sum_x: f64 = samples.iter().map(|(x, _)| x).sum();
        let sum_y: f64 = samples.iter().map(|(_, y)| y).sum();
        let mean_x = sum_x / n;
        let mean_y = sum_y / n;
        
        let num: f64 = samples.iter().map(|(x, y)| (x - mean_x) * (y - mean_y)).sum();
        let den: f64 = samples.iter().map(|(x, _)| (x - mean_x).powi(2)).sum();
        
        if den.abs() < 1e-10 { return; }  // 除零保护
        
        self.alpha = num / den;
        self.beta = mean_y - self.alpha * mean_x;
        
        // 计算 r²
        let ss_res: f64 = samples.iter().map(|(x, y)| (y - (self.alpha * x + self.beta)).powi(2)).sum();
        let ss_tot: f64 = samples.iter().map(|(_, y)| (y - mean_y).powi(2)).sum();
        self.r_squared = 1.0 - ss_res / ss_tot;
        self.sample_count = samples.len() as u64;
    }
}
```

### 8.5 冷启动算法

```
冷启动状态机:

enum ColdStartPhase {
    Bootstrap,       // 阶段 0: 第 1 个任务
    WarmUp,          // 阶段 1: 2~5 个任务
    Accumulate,      // 阶段 2: 6~49 个任务
    Stable,          // 阶段 3: ≥50 个任务且 r² ≥ 0.6
}

每个任务完成后的状态转换:

match phase {
    Bootstrap => {
        n_batch = 1
        delta = actual_peak / compiler_peak
        phase = WarmUp
    }
    WarmUp => {
        delta = EMA(delta, actual_peak / compiler_peak, 0.3)
        n_batch = min(2, H)
        if completed_count >= 5: phase = Accumulate
    }
    Accumulate => {
        delta = EMA(delta, actual_peak / compiler_peak, 0.1)
        n_batch = min(H, S_with_delta)
        if completed_count >= 50: try_train_regression()
        if regression.ready: phase = Stable
    }
    Stable => {
        n_batch = min(H, S_with_regression)
        // 每 200 个任务重新训练一次
	    if completed_count - last_train_count >= 200: try_train_regression()
	    }
	}
```

### 8.6 仿真验证

算法层面的两个关键场景：

**冷启动预测误差**（编译器预测偏差 2-3×，到达率 5/s）：

![吞吐量对比](../sim/reports/冷启动预测误差/throughput_comparison.png)
![因子分解](../sim/reports/冷启动预测误差/factor_decomposition.png)

| 变体 | 吞吐量 | 平均延迟 | P99 | N_batch |
|------|--------|---------|-----|---------|
| Baseline | 3.9/s | 4.75s | 58.9s | 6.6 |
| FullAdaptive | **4.1/s** | 4.67s | 59.4s | 6.1 |
| Lin+WGM+AimdH | **4.3/s** | 5.91s | 59.8s | 5.2 |

冷启动场景下各变体表现接近。FullAdaptive 的自适应平滑和 Lin+WGM 的保守策略在冷启动初期对预测误差的容忍度更高。回归模型积累 50+ 样本后吞吐逐步改善。

**高碎片回收**（频繁 OOM，50-500MB 任务）：

| 变体 | 吞吐量 | 平均延迟 | P99 | N_batch | 相比 Baseline |
|------|--------|---------|-----|---------|--------------|
| Baseline | 5.5/s | 10.1s | 57.0s | 4.3 | — |
| FullOpt | 5.9/s | 14.3s | 83.7s | 4.2 | +7% |
| FullAdaptive | **6.4/s** | 12.7s | 71.3s | 4.1 | **+16%** |

FullAdaptive 在高碎片场景下吞吐领先 16%，因其自适应平滑能更快响应碎片率变化导致的分配失败。该场景直接验证了 §4.6 死区合并算法的必要性。

---

## 9. 持久化存储布局

```
<data_dir>/
├── repo/                          # .atxe 仓库
│   ├── 0000.atxe                  # task_id = 0 的 .atxe
│   ├── 0001.atxe
│   └── ...
│
├── meta/                          # TaskMeta 持久化
│   ├── tasks.db                   # SQLite / 自定义二进制
│   └── task_index.idx
│
├── stats/                         # 统计数据
│   ├── memory_samples.csv         # 编译预测 VS 实际峰值
│   ├── batch_decisions.json       # 每次 N_batch 决策记录
│   ├── executor_stats.json        # Executor 运行统计
│   └── regression_model.json      # 线性回归模型参数
│
└── runner.toml                    # Runner 配置文件
```

---

## 10. 未解决的问题（下一轮讨论）

| 问题 | 说明 | 依赖 |
|:-----|:-----|:-----|
| **Heartbeat 频率** | Executor 的心跳上报频率多少？太快浪费，太慢 Runtime 不敏感 | 仿真调参 |
| **死区合并触发阈值** | 碎片率多少触发？多长时间检查一次？ | 仿真验证 |
| **回归模型更新时机** | 固定 200 样本还是根据误差变化动态触发？ | 数据积累后 |

---

## 11. 附录：仿真框架

仿真框架位于 [`sim/`](../sim/)，用离散时间步进模拟 Runner 的完整任务生命周期。运行方式：

```bash
# 快速验证（2 场景 × 4 变体）
python -m sim.main --quick

# 单场景详细验证
python -m sim.main --scenario <场景名> --detailed

# 全量跑（10 场景 × 9 变体）
python -m sim.main
```

### 算法变体对照

| 变体 | 平滑 | 合并 | OOM 反馈 | 滑道 |
|------|------|------|----------|------|
| Baseline | Discrete | Mul | Hard | 1.5x |
| Sigmoid Only | Sigmoid | Mul | Hard | 1.5x |
| MinBottleneck | Discrete | Min | Hard | 1.5x |
| AIMD+Hysteresis | Discrete | Mul | AIMD+Hys | 1.5x |
| Sig+AimdH | Sigmoid | Mul | AIMD+Hys | 1.5x |
| Lin+WGM+AimdH | Linear | WGM | AIMD+Hys | Dynamic |
| FullOpt | Sigmoid | WGM | AIMD+Hys | Dynamic |
| FullAdaptive | Adaptive | WGM | AIMD+Hys | P95 |
| FullOpt_v0.3 | Sigmoid | WGM | AIMD+Hys | Dynamic |

各场景的仿真验证结果已嵌入对应章节（§4.7 内存模型、§6.5 调度、§7.5 自适应控制、§8.6 算法规范）。完整报告数据（含 CSV 和所有图表）位于 [`sim/reports/`](../sim/reports/)。

> 完整报告数据位于 [`sim/reports/`](../sim/reports/)，包含各场景的原始 CSV、JSON 汇总和所有图表。
> 仿真框架位于 [`sim/`](../sim/)，运行 `python -m sim.main --quick` 快速验证。*
