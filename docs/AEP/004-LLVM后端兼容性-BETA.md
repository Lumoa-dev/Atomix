# AEP-004 (BETA): LLVM 后端兼容性
Atomix Enhancement Proposal — 试验性提案

| 字段 | 内容 |
|------|------|
| **状态** | Draft (Beta) |
| **优先级** | P3 |
| **关联文档** | 02-指令集规范.md, 04-编译管线.md, 06-外围工具.md, 01-总纲与哲学.md |
| **提出日期** | 2026-07-19 |
| **标签** | `beta` — 本提案属于探索性研究，不承诺实现，不纳入里程碑 |

## 1. 动机

Atomix 目前只有 VM 解释执行一条路径。LLVM 后端的引入意味着：

- Atomix IR 可以编译为原生机器码（x86_64 / ARM64）
- 计算密集型任务的执行速度可能提升 10-50 倍
- 可以复用 LLVM 社区 20+ 年的优化积累（GVN、向量化、内联、自动并行化）

但这是一条"天方夜谭"级别的路线——挑战极大。本提案仅做技术可行性分析。

## 2. 路线分析

### 2.1 路线 A：Atomix IR → LLVM IR

```
.atx → [Atomix 编译器] → .atxe (Atomix IR) → [LLVM 升降级] → LLVM IR → [LLVM opt] → 原生码
```

**核心问题**：Atomix IR 是 32 位定长指令，LLVM IR 是 SSA 形式。
需要将 54 条 opcode 逐条映射为 LLVM IR 指令序列。

| Atomix IR | LLVM IR 映射 | 难度 |
|-----------|-------------|:----:|
| ADD Rd, Rs1, Rs2 | `%rd = add i64 %rs1, %rs2` | ★☆☆ |
| LOAD Rd, [Rs+imm] | `%ptr = getelementptr i8, i8* %base, i64 %rs+imm; %rd = load i64, i8* %ptr` | ★☆☆ |
| CALL offset | `call void @func()` | ★☆☆ |
| THROW | `invoke void @throw() to label %normal unwind label %handler` | ★★★ |
| ECALL | `call i64 @ecall(i32 %syscall, i64 %arg1, ...)` | ★★☆ |
| TASK_FORK / TASK_JOIN | 无直接对应，需运行时支持 | ★★★★★ |
| 浮点运算 | 直接映射到 LLVM 浮点指令 | ★☆☆ |
| 沙箱边界检查 | `if (addr > bound) { trap(); }` | ★★☆ |

**优点**：直接复用 LLVM 中后端，无需自己写寄存器分配和指令选择。
**缺点**：Atomix 的并发模型（TASK_FORK/JOIN）和异常模型（THROW/exn 表）与 LLVM 的异常模型差异大。

### 2.2 路线 B：AST → LLVM IR（跳过 .atxe）

```
.atx → [Atomix 编译器] → AST → [LLVM 代码生成] → LLVM IR → [LLVM opt] → 原生码
```

跳过 .atxe 中间表示，直接从 AST 生成 LLVM IR。

**优点**：类型信息更完整，优化机会更多。
**缺点**：需要写一整套新的代码生成器，与现有 .atxe 路线并行维护。

### 2.3 路线 C：AOT 编译（JIT 先行）

不追求完整的 LLVM 后端，先做 JIT：

```
.atxe → [Atomix VM] → [执行追踪] → [热路径识别] → [LLVM JIT] → 原生码
```

类似 Java JVM 的 JIT 编译策略。

**优点**：可以先不做整个编译器后端，只优化执行热点。
**缺点**：LLVM JIT 集成复杂度高（orcjit / lli）。

## 3. 关键挑战

| 挑战 | 描述 | 严重性 |
|------|------|:------:|
| **异常模型** | Atomix 的 THROW/exn 表查找与 LLVM `invoke`/`landingpad` 模型需适配 | 🔴 高 |
| **并发原语** | TASK_FORK/TASK_JOIN/TASK_RET 无 LLVM 对应项，需要运行时支持 | 🔴 高 |
| **沙箱** | LLVM 生成的原生码需要插入边界检查指令 | 🟡 中 |
| **ECALL** | 系统调用需要 FFI 边界，LLVM 可以调用 C ABI | 🟢 低 |
| **浮点一致性** | IEEE 754 双精度在 LLVM 中行为相同 | 🟢 低 |
| **指令数膨胀** | 一条 Atomix 指令可能展开为 3-10 条 LLVM IR 指令 | 🟡 中 |
| **调试信息** | LLVM 生成的 DWARF 调试信息与 .atx 源码行号对齐 | 🟡 中 |

## 4. 最小可行产品（MVP）定义

BETA 提案的阶段性目标不是"完整 LLVM 后端"，而是：

```
阶段 1（3-6 人月）：
  - 一个 .atxe → LLVM IR 的转换工具（atxe2llvm）
  - 覆盖 30/54 条指令（排除并发和异常相关指令）
  - 输出可被 `llc` 编译为 .o 文件

阶段 2（6-12 人月）：
  - 集成异常处理（exn 表 → landingpad）
  - 集成 ECALL（FFI 到 Atomix 运行时库）

阶段 3（12-24 人月）：
  - 集成 TASK_FORK/JOIN（通过 pthread 或 coroutine 运行时）
  - AOT 编译模式 `atomix build --aot`
```

## 5. 向后兼容性

LLVM 后端作为可选编译目标，不影响现有的 VM 解释执行路径。
`atomix build` 默认仍产出 `.atxe`。
`atomix build --aot` 产出原生可执行文件。

## 6. BETA 声明

本提案标为 `BETA` 的原因：

1. **可行性未验证**——Atomix 的并发模型与 LLVM 的线性控制流模型存在根本性冲突，可能无法在不修改 VM 语义的情况下解决
2. **投入产出比不确定**——VM 解释执行在 2C2G 的轻量场景下可能已经足够，LLVM 后端的复杂度是否值得？
3. **存在替代方案**——在投入 LLVM 后端之前，优化现有 VM 执行循环（AEP-003）可能获得更高的性价比
4. **社区生态未建立**——当前 Atomix 还在早期，引入 LLVM 依赖会增加构建复杂度

**建议**：在 Phase 3（运行时稳定）之后，重新评估此提案的必要性和可行性。
