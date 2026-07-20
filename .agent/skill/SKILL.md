---
name: atomix-dev
description: SOP for Atomix project development — compiler, VM, runtime, toolchain
trigger: >
  Activate when working on the Atomix project: modifying source code,
  writing tests, generating .atxe builds, or updating design docs.
---

# Atomix 开发 Skill

## 触发条件

当用户提及以下任意内容时激活：
- Atomix 项目代码的修改/新增
- 编译器、VM、运行时、调试器、工具链相关工作
- `.atx` / `.atxe` 文件相关操作
- 设计文档（`docs/`）更新
- 需求追踪文档（`14-Atomix-Rust-实现任务文档.md`）更新

---

## 0. 前置：更新代码索引

每次开始工作时，第一步永远是：

```bash
.agent/tools/atomix-map.exe
```

这会在 `.agent/codebase-map.json` 生成树状代码索引（含每个文件的 pub 声明、行数、测试数、内容快照）。**不要全量扫源码，先读索引。**

---

## 1. 理解上下文（2 分钟内完成）

```
┌─ 读文件                               → 读完知道什么        ─┐
│  ① .agent/AGENTS.md（项目手册）         项目定位、架构决策      │
│  ② .agent/codebase-map.json | head -200  代码全貌、关键入口    │
│  ③ git status / git diff --stat      当前变更范围             │
│  ④ git log --oneline -5              最近提交上下文           │
└──────────────────────────────────────────────────────────────┘
```

**原则**：不读完整源文件，除非索引告诉你某个文件是你要改的目标。

---

## 2. 定位目标代码

从 `codebase-map.json` 的索引中查找目标：

```bash
# 查关键字在哪些文件出现
grep -n "关键字" .agent/codebase-map.json

# 根据索引中的 pub 声明判断是否需要读完整文件
```

只在以下情况读完整源文件：
- 索引片段显示该文件包含你要修改的函数/类型
- 你要新增的代码需要复用其中的类型

---

## 3. 编码与测试

### 3.1 构建

```bash
cargo build                    # debug
cargo build --bin <名称>        # 只编某个 binary
```

### 3.2 测试

```bash
cargo test                      # 全量（~180 个，应全过）
cargo test <模块路径>            # 如 cargo test compiler::lexer
cargo test -- --nocapture       # 看 println
```

### 3.3 运行

```bash
cargo run -- build <file.atx>                             # 编译
cargo run -- runner run <file.atx>                        # 编译+执行
cargo run -- task <file.atx>                              # 调试模式
cargo run --bin atomix-runner -- daemon --listen :9000    # 守护进程
```

---

## 4. 编码约束

| 项目 | 要求 |
|------|------|
| Edition | 2024 |
| unsafe | 编译器核心**零 unsafe**；VM 尽量规避；ECALL 层可控 |
| 异步 | 仅 runner 用 tokio；VM 循环同步 |
| 依赖 | 最小依赖；核心路径零 std 依赖 |
| 格式 | `cargo fmt` + `cargo clippy`（deny 模式） |
| 文档 | 所有 `pub` 项必须有 `///`，引用设计文档章节 |

### 命名

```
文件/目录      snake_case
类型/枚举      PascalCase
函数/变量      snake_case
常量           SCREAMING_SNAKE_CASE
错误变体       PascalCase + Error（如 LexError）
```

### Commit

```
<type>(<scope>): <描述>

Closes: P1-XXX-XXX

type: feat / fix / docs / refactor / perf / test / chore
scope: compiler:lexer / compiler:ir / vm:isa / runtime:scheduler / cli / stdlib
```

---

## 5. 对照需求文档

需求总纲在 `docs/14-Atomix-Rust-实现任务文档.md`。

每项需求有唯一编号（如 `P1-LEX-001`）和优先级（P0/P1/P2）：
- **P0**：必须通过
- **P1**：核心功能
- **P2**：增强/优化

如果实现/修改了某个需求，在 commit message 中标注编号。

---

## 6. 当遇到问题时

| 情况 | 做法 |
|------|------|
| 不知道某个模块的内部逻辑 | 读 `codebase-map.json` 中该文件的 pub 声明和首部注释 |
| 测试失败 | 先读失败测试的断言信息，定位到对应代码模块 |
| 不理解设计意图 | 查 `docs/` 下对应的设计文档 |
| 需要跨模块修改 | 先确认两个模块的接口类型从索引中是否可读，再读接口定义文件 |

---

## 7. 同步更新 AGENTS.md

如果修改涉及架构变动（新增文件、修改核心流程、新增功能模块），同步更新 `.agent/AGENTS.md` 中的对应章节，保持项目手册与代码一致。
