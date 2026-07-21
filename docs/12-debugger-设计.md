# Atomix Debugger 设计文档

> 版本: v0.3（本地 + 远程完整设计）
> 最后更新: 2026-07-22
> 配套线框图: `docs/debug-design/*.svg`（28 个页面）

---

## 1. 概述

`atomix-debug` 是 Atomix 工具链的调试器。编译器与 VM 的模块直接编译进 debug 二进制，通过 `pub mod` 按需导入，无需跨进程通信。

### 1.1 使用方式

| 模式 | 入口 | 说明 |
|------|------|------|
| 本地 TUI | `atomix task <file>` | 完整调试环境，19 页面，50+ 命令 |
| 本地 CLI | `atomix task <file> --<flag>` | 一键操作，不进入 TUI |
| 远程 TUI | `atomix task <file> --origin <alias>` | 远程监控环境，12 页面 |
| 远程 CLI | `atomix origin *` | 连接管理与快速状态查询 |

本地与远程是两套独立的 TUI 会话。同一 debug 二进制在启动时根据参数决定进入本地还是远程模式，会话中不可切换——如需切换，退出后以另一参数重新启动。

### 1.2 核心原则

- **默认运行收集**：启动时自动完整执行，收集 Step 状态、变量变化、IS* 时间线、数据流向。后续查看/展开/追踪均基于已收集数据，无需再次运行。
- **纯键盘操作**：不支持鼠标。
- **底层复用**：编译器（词法、语法、语义分析、debug info 生成）+ 执行器（指令执行、寄存器/内存、解码/反汇编）按需导入。

---

## 2. TUI 布局

```
┌─ 标题栏 ────────────────────────────────────────────────────┐
│  atomix debug — <文件名>                                     │
├─ 面包屑 ────────────────────────────────────────────────────┤
│  Home ▸ STEP 2: validate                                     │
├─ 左侧主视图（≈ 70%）──┬─ 右侧状态面板 ──────────────────────┤
│                        │  IS* Context（持久）                 │
│                        │  ───────────────                     │
│  页面内容（18 页面之一） │  Variables / Watch                  │
│                        │  ───────────────                     │
│                        │  动态面板（用户切换）                  │
├─ 命令栏 ──────────────┴──────────────────────────────────────┤
│  > command                                                   │
├─ help（help 命令时弹出）─────────────────────────────────────┤
│  键盘快捷键 & 命令参考                                        │
└──────────────────────────────────────────────────────────────┘
```

**布局规则：**
- 面包屑始终可见，显示当前页面路径及返回栈
- 命令栏始终固定在底部
- 右侧上区 = IS* 上下文 + 常量（持久展示）
- 右侧下区 = 动态面板，通过 `:regs`、`:mem` 等命令切换内容
- help 面板在命令栏上方弹出

---

## 3. 页面体系

共 18 个页面，通过面包屑导航。`exit` 返回上一层，`exit:home` 回到首页。

### 3.0 导航模型

页面历史以栈形式维护：

```
Home                          ← 根页面
 ├─ Step: validate            ← step:see 进入
 │    ├─ fn: check_format     ← Enter 进入子调用
 │    │    └─ :df             ← 数据时间轴
 │    └─ :hooks               ← 钩子时间轴
 ├─ :src                      ← 源码视图
 └─ :deps                     ← 任务依赖树
```

- `exit` — 弹出栈顶
- `exit:home` — 清空栈
- `exit:fn <name>` / `exit:step <name>` — 跳转到栈内指定页面

---

### 3.1 Home — Step 执行日志

**入口**：默认打开。

**内容**：以 Step 为单位的执行日志，按段落排列（SYSTEM → INPUT → TASK → OUT）。TASK 段中每个 CALL 即为一个 Step。默认全部折叠，↑↓ 选择，Enter 进入详情。

- ✓ 及耗时 → 已执行 Step
- ✗ 及错误摘要 → 错误 Step
- — → 被跳过的 Step
- 当前选中行高亮

**线框图**：`docs/debug-design/home-page.svg`

---

### 3.2 Step Detail — Step 详情

**入口**：`step:see name:<name>` / `step:see id:<n>` / 在 Home 页按 Enter。

**内容**：Step 的完整输入/输出及内部子调用链。

- 顶部：CALL 语句和行号
- 输入参数列表
- 子调用列表（fn 调用、WORKS 调用各自标识）
- WORKS 生命周期作陈述性展示（例：`INIT → START → DONE`）
- 输出变量列表
- 底部附带源码片段，当前行高亮

**线框图**：`docs/debug-design/step-detail.svg`

---

### 3.3 Data Timeline — 数据时间轴

**入口**：`:df` / `:dataflow`。

**内容**：横向时间轴树，展示变量从 INPUT → 各 Step → OUT 的完整生命周期。

- 变量为圆点节点，函数/WORKS 调用为方角节点
- 边 = 数据流向
- 合并点（多输入 → 单输出）可视化
- 断裂路径（✗）明确标注
- ←→ 平移，↑↓ 切换变量，+/- 缩放

**线框图**：`docs/debug-design/data-lineage.svg`

---

### 3.4 Hook Timeline — 钩子时间轴

**入口**：`:hooks` / `:lifecycle`。

**内容**：横向时间轴树，展示 WORKS 实例的钩子执行序列。仅显示定义了行为链的钩子。

- 钩子为圆角节点，动作为方角节点
- 边 = 钩子链，条件标注在边上
- 同一钩子的多个分支（扇出）黄色标注
- 重复触发不打回头箭头——在时间轴上继续向右延伸，虚线表示
- ←→ 平移，↑↓ 切换分支，+/- 缩放

**线框图**：`docs/debug-design/works-lifecycle.svg`

---

### 3.5 Task Dependency — 任务依赖树

**入口**：`:deps` / `:tasks`。

**内容**：横向层次树，展示 WAIT 产生的 FORK/JOIN 任务依赖关系。

- FORK 边（橙色）= 父任务派生子任务
- JOIN 边（青色）= 子任务结果返回父任务
- 深层节点优先执行，同深度可并行
- 底部展示调度批次顺序

**线框图**：`docs/debug-design/task-dependency.svg`

---

### 3.6 Source View — 源码视图

**入口**：`:src` / `show atx`。

**内容**：只读源码视图，含行号、断点装订线、当前执行行高亮。

- 语法高亮（关键字、字符串、类型标注）
- 已执行行右侧注释标记
- 断点以红点显示于装订线
- ↑↓ 移动光标，`b` 打断点，`B` 条件断点，`g` 跳行
- 右侧面板显示断点列表、当前行详情、作用域变量

**不支持编辑**——编辑能力留待后续评估。

**线框图**：`docs/debug-design/source-view.svg`

---

### 3.7 Watch Replay — 回放

**入口**：`step:run <name> watch <speed>`。

**内容**：以指定速度慢速重放 Step 的执行过程。

- 进度条 + 子操作清单（已完成 / 当前 / 待执行）
- Space 暂停/继续，←→ 调速（0.25x – 4x）
- 右侧面板实时更新 IS*、变量值、内存分配

**线框图**：`docs/debug-design/watch-replay.svg`

---

### 3.8 INPUT Detail — 输入数据源

**入口**：在 Home 页选中 INPUT 段按 Enter。

**内容**：所有输入常量及其数据源详情。

- 每个常量一行：名称、类型、加载状态、耗时、数据源类型
- 被 WAIT 覆盖的常量标注"被覆盖"及覆盖值
- 显示消费者（哪些 Step 使用该常量）

**线框图**：`docs/debug-design/input-detail.svg`

---

### 3.9 OUT Detail — 产出交付

**入口**：在 Home 页选中 OUT 段按 Enter。

**内容**：所有产出变量及其交付状态。

- 每个产出变量一行：名称、类型、交付状态、耗时/大小、目标类型
- 未交付的标注原因（如"所在 Step 未执行"）
- 右侧面板显示选中产出目标的详细信息

**线框图**：`docs/debug-design/out-detail.svg`

---

### 3.10 Binary View — 二进制视图

**入口**：`:binary`。

**内容**：`.text` 段的原始 hex dump。

- 每行 1 条指令（4 字节）
- 列：Offset / Hex(LE) / Binary / ASCII
- 断点行以红点标记，当前 PC 行绿色高亮
- 底部显示各段大小统计

**线框图**：`docs/debug-design/binary-view.svg`

---

### 3.11 IR / Disassembly — 反汇编视图

**入口**：`:disasm` / `:ir`。

**内容**：指令级反汇编，带操作数和源码注释。

- 列：PC / Bytes(LE) / Opcode / Operands / Source Comment
- 操作码颜色分类：蓝色=ARITH、紫色=MEM、橙色=CTRL、红色=SYSTEM
- 右侧面板显示选中指令的编码详情

**线框图**：`docs/debug-design/ir-disasm.svg`

---

### 3.12 Registers & Memory — 寄存器与内存

**入口**：`:regs` / `:mem`。

**内容**：上半部分 16 个通用寄存器，下半部分内存 hex dump。

- 寄存器：名称、hex 值、十进制值、类型标注
- 可编辑（`set a0 = 100`）
- Tab 切换焦点（寄存器 ↔ 内存）
- 内存区域支持 goto 地址跳转

**线框图**：`docs/debug-design/regs-mem.svg`

---

### 3.13 Exception Detail — 异常详情

**入口**：在 Home 页选中 ✗ Step 按 Enter，或从异常通知跳转。

**内容**：异常发生时的完整上下文。

- 异常类型、错误码、错误消息
- 源位置（Zone、Step、行号、函数）
- 异常时刻的 IS* 变量快照
- 异常时刻的调用栈
- 异常时刻的作用域变量
- 右侧面板：异常是否向上传播、是否被 TRY 块捕获、对后续 Step 和 OUT 的影响

**线框图**：`docs/debug-design/exception-detail.svg`

---

### 3.14 Zone Status — Zone 状态

**入口**：`:zones`。

**内容**：所有 Zone 的加载状态一览。

- Zone 名称、生命周期类型、加载状态、大小、函数数量、PC 范围、依赖
- 生命周期类型说明（PERSISTENT / EXEC_UNLOAD / LAZY / PRUNE）
- Zone 依赖关系图
- 内存占用统计（总计 / 持久加载 / 执行后卸载 / 延迟加载）

**线框图**：`docs/debug-design/zone-status.svg`

---

### 3.15 Call Stack — 调用栈

**入口**：`:bt` / `:callstack`。

**内容**：完整调用栈帧列表。

- 当前帧绿色高亮
- 每帧：函数名、PC、行号
- ↑↓ 选择帧，Enter 查看帧详情
- `u` / `d` 上下切换帧

**线框图**：`docs/debug-design/callstack.svg`

---

### 3.16 Breakpoints — 断点管理

**入口**：`break:list` / `:breaks`。

**内容**：所有断点的集中管理视图。

- 每行：编号、类型（PC/HOOK/WATCH）、位置、条件表达式、命中次数、状态
- `d` 删除，`e` 启用/禁用切换，`c` 编辑条件
- 支持批量操作（全部清除、全部启用）

**线框图**：`docs/debug-design/breakpoints.svg`

---

### 3.17 IS* Context — IS* 全览

**入口**：`:is`。

**内容**：全部 72 个 IS* 变量按 7 个分组展示。

- 分组：异常、计数、调用上下文、系统/环境、时间、任务、数据
- Tab 切换分组，`/` 搜索
- 右侧面板显示选中 IS* 变量的详细信息

**线框图**：`docs/debug-design/is-context.svg`

---

### 3.18 Segment Info — 段信息

**入口**：`:segments`。

**内容**：`.atxe` 二进制文件的完整段结构。

- 文件布局（Header → Section Table → 各段数据）
- 每段显示：名称、大小、偏移范围、类型码、标志位
- `.debug` 段展开显示 ADBG 条目详情
- `.exn` 段显示异常处理器（如有）

**线框图**：`docs/debug-design/segment-info.svg`

---

### 3.19 Performance Analysis — 性能分析

**入口**：`:perf` / `:profile`。

**内容**：基于当前执行数据的性能统计面板。

- 指令执行分布：按 opcode 分类统计（ARITH / MEM / CTRL / SYSTEM 各自数量及占比）
- Hot Path：Top-N 最常执行的 PC 地址及对应源码位置
- Step 耗时排名：各 Step 执行时间从长到短排序
- 内存概况：分配/释放次数、峰值使用量、当前占用量
- 所有数据来自已收集的 ExecutionTrace，无需重新执行

**线框图**：`docs/debug-design/perf-analysis.svg`

---

## 4. 命令体系

命令采用**冒号分级**格式：

| 格式 | 含义 | 示例 |
|------|------|------|
| `主命令` | 一级操作 | `step`、`continue`、`exit`、`quit` |
| `主命令:子命令 参数` | 分级操作 | `step:see validate`、`break:line 43` |
| `:模式命令` | 当前模式内部命令 | `:go`、`:again` |
| `:视图名` | 视图切换 | `:src`、`:df`、`:hooks` |

### 4.1 执行控制

| 命令 | 说明 |
|------|------|
| `step` | 执行到下一个 Step |
| `step:into` | 进入当前 Step 内部 |
| `step:out` | 跳出当前 Step |
| `continue` / `c` | 运行到下一个断点或任务结束 |
| `:go` | 单步推进（watch / one 模式） |
| `:again` | 重做上一步 |

### 4.2 Step 查看与重跑

| 命令 | 说明 |
|------|------|
| `step:see <name>` | 查看 Step 详情（按名称） |
| `step:see <n>` | 查看 Step 详情（按序号） |
| `step:run <name>` | 重跑指定 Step |
| `step:run <name> watch <speed>` | watch 模式重跑（速度 0.25 – 4.0） |
| `step:run <name> one` | 单步模式重跑 |

### 4.3 导航

| 命令 | 说明 |
|------|------|
| `exit` | 返回上一层 |
| `exit:home` | 回到首页 |
| `exit:fn <name>` | 回到历史上某函数页 |
| `exit:step <name>` | 回到历史上某 Step 页 |

### 4.4 断点

| 命令 | 说明 |
|------|------|
| `break:line <n>` | 按行号设置 PC 断点 |
| `break:fn <TOOLS::fn>` | 按函数路径设置断点 |
| `break:hook <HOOK>` | 全局钩子断点 |
| `break:hook <WORKS::HOOK>` | 指定 WORKS 的钩子断点 |
| `break:line <n> if <cond>` | 条件断点 |
| `break:list` | 列出所有断点 |
| `break:del <id>` | 删除指定断点 |
| `break:clear` | 清空所有断点 |
| `break:enable all` | 全部启用 |
| `watch <var>` | 监视变量变化 |

源码视图键盘操作：`↑↓` 移光标，`b` 打断点，`B` 条件断点，`g` 跳行。

### 4.5 信息查询

| 命令 | 说明 |
|------|------|
| `info` | 当前上下文概览 |
| `info:task` | 任务完整信息 |
| `info:zones` | Zone 加载状态 |
| `info:functions` | 作用域函数列表 |
| `info:variables` | 作用域变量及类型 |
| `info:file` | 源文件信息 |

### 4.6 表达式求值

| 命令 | 说明 |
|------|------|
| `print <expr>` / `p <expr>` | 求值并打印 |
| `print/f <expr>` | 格式化打印（`/x` 十六进制 `/d` 十进制 `/s` 字符串） |
| `print/t <expr>` | 打印值及类型 |

### 4.7 设置

| 命令 | 说明 |
|------|------|
| `set <reg> = <value>` | 设置寄存器值 |
| `set *<addr> = <value>` | 写入内存地址 |
| `set:var <name> = <value>` | 修改变量值 |

### 4.8 搜索

| 命令 | 说明 |
|------|------|
| `find <text>` | 在源码中搜索 |
| `find:next` / `find:prev` | 下一处 / 上一处 |
| `find:mem <pattern>` | 在内存中搜索字节模式 |
| `find:var <name>` | 搜索变量定义及使用位置 |

### 4.9 反汇编

| 命令 | 说明 |
|------|------|
| `disasm` | 反汇编当前 PC 附近 8 条 |
| `disasm <addr>` | 从指定地址开始 |
| `disasm <addr> <n>` | 从指定地址，n 条 |

### 4.10 内存操作

| 命令 | 说明 |
|------|------|
| `mem:dump <addr> <len>` | hexdump |
| `mem:diff <a1> <a2> <len>` | 比较两块内存 |
| `mem:fill <addr> <len> <val>` | 填充内存区域 |
| `mem:watch <addr> <len>` | 监视内存区域变化 |

### 4.11 调用栈

| 命令 | 说明 |
|------|------|
| `bt` / `backtrace` | 显示完整调用栈 |
| `frame <n>` | 切换到第 n 帧 |
| `frame:up` / `frame:down` | 上/下切换帧 |
| `frame:info` | 当前帧详情 |

### 4.12 IS* 上下文

| 命令 | 说明 |
|------|------|
| `is` | 显示全部 IS* 变量 |
| `is <name>` | 查询单个 IS* 变量 |
| `is:watch <name>` | 监视 IS* 变量变化 |

### 4.13 执行历史

| 命令 | 说明 |
|------|------|
| `history` | 显示命令历史 |
| `history <n>` | 最近 n 条 |
| `!<n>` | 重复第 n 条历史命令 |

### 4.14 显示格式

| 命令 | 说明 |
|------|------|
| `display <expr>` | 每次 step 后自动打印表达式 |
| `display` | 列出所有自动显示项 |
| `display:del <n>` | 删除第 n 项 |
| `display:clear` | 清空 |

### 4.15 日志与导出

| 命令 | 说明 |
|------|------|
| `log:start <file>` | 开始记录调试日志 |
| `log:stop` | 停止记录 |
| `log:status` | 日志记录状态 |
| `export:state <file>` | 导出 VM 状态快照 |
| `export:dataflow <file>` | 导出数据追踪图 SVG |

### 4.16 配置

| 命令 | 说明 |
|------|------|
| `set:fmt hex` / `set:fmt dec` | 设置默认数值显示格式 |
| `set:depth <n>` | 设置嵌套展开深度 |
| `set:speed <n>` | 设置 watch 默认速度 |

### 4.17 视图切换

| 命令 | 页面 |
|------|------|
| `:src` | 源码视图 |
| `:df` / `:dataflow` | 数据时间轴 |
| `:hooks` / `:lifecycle` | 钩子时间轴 |
| `:deps` / `:tasks` | 任务依赖树 |
| `:binary` | 二进制视图 |
| `:disasm` / `:ir` | 反汇编视图 |
| `:regs` | 寄存器面板 |
| `:mem` | 内存面板 |
| `:zones` | Zone 状态 |
| `:bt` / `:callstack` | 调用栈 |
| `:breaks` | 断点管理 |
| `:is` | IS* 全览 |
| `:segments` | 段信息 |

### 4.18 元命令

| 命令 | 说明 |
|------|------|
| `help` / `h` / `?` | 弹出帮助面板 |
| `quit` / `q` | 退出调试器 |

### 4.19 全局键盘快捷键

| 键 | 作用 |
|----|------|
| `↑ ↓ ← →` | 导航节点 / 选项 / 光标 |
| `Enter` | 选中 / 进入 |
| `Esc` | 返回上一层 |
| `Tab` | 切换焦点面板（左 ↔ 右） |
| `+ / -` | 缩放（时间轴视图） |
| `Space` | 暂停 / 继续（watch 模式） |
| `f` | 追踪路径（图视图） |
| `t` | 切换执行轨迹高亮 |

---

## 5. CLI 命令

### 5.1 本地调试 CLI

```
atomix task <file>                    # 进入本地 TUI 交互模式
atomix task <file> --no-run           # 进入本地 TUI，跳过默认运行
atomix task <file> --step <name>      # 运行并直接查看指定 Step
atomix task <file> --print <expr>     # 运行后打印表达式值
atomix task <file> --check            # 检查断点命中情况
atomix task <file> --break-line <n>   # 运行前设置断点
atomix task <file> --export-state <f> # 导出 VM 快照
atomix task <file> --export-dataflow  # 导出数据追踪图
atomix task <file> --log <file>       # 运行并记录日志
atomix task <file> --list-steps       # 列出所有 Step
atomix task <file> --list-vars        # 列出变量及最终值
atomix task <file> --list-is          # 列出 IS* 最终状态
atomix task <file> --disasm <addr>    # 反汇编指定地址
atomix task <file> --mem-dump <a> <l> # 内存 dump
```

### 5.2 远程 CLI（`atomix origin`）

远程 CLI 以 `atomix origin` 为入口，所有远程操作均可通过命令行直接完成，无需进入 TUI。

每个命令通过 `--alias <name>` 指定目标 Runner；若省略，读取环境变量 `ATOMIX_ORIGIN` 作为默认别名。两者都未设置则报错并提示可用连接列表。

```
atomix origin
│
├── 连接管理
│   ├── connect <alias> --ip <addr> [--port <port>]
│   ├── disconnect [<alias>]
│   └── list
│
├── status [<alias>]                         # Runner 概览（dashboard CLI 版）
│
├── task
│   ├── list [--alias <a>] [--status pending|running|done|error]
│   │       [--limit <n>] [--sort id|name|status|elapsed]
│   ├── show <id> [--alias <a>] [--regs] [--mem] [--bt] [--is]
│   ├── submit <file> [--alias <a>]
│   │       [--name <n>] [--mode release|debug]
│   │       [--opt O0|O1|O2] [--timeout <sec>]
│   │       [--wait] [--output <path>]       # --wait：等待完成并下载产出
│   ├── cancel <id> [--alias <a>]
│   ├── output <id> [--alias <a>] [--file <path>]
│   └── log <id> [--alias <a>] [--lines <n>]
│
├── runner
│   ├── config [--alias <a>] [--get <key>] [--set <key>=<val>]
│   └── stats [--alias <a>] [--live]
│
├── pool [--alias <a>] [--status <s>]        # 任务池分布 + 依赖 DAG
│
├── controller [--alias <a>] [--history]     # 控制器参数
│
├── slots [--alias <a>] [--compact]          # 内存槽位布局
│
├── log [--alias <a>]
│   ├── tail [--task <id>] [--level error|warn|info|debug]
│   │       [--lines <n>] [--follow]
│   ├── level <level>
│   └── clear
│
├── perf [--alias <a>]
│   ├── all                                  # 全部指标
│   ├── cpu                                  # CPU 使用率
│   ├── memory                               # 内存使用率
│   ├── throughput                           # 任务吞吐量
│   └── controller                           # 控制器参数趋势
│
└── export [--alias <a>]
    ├── state <id> <file>                   # 导出任务状态快照
    └── snapshot <file>                     # 导出 Runner 全貌快照
```

### 5.3 本地 Runner 管理

本地 Runner 进程管理独立于 `atomix origin`，用于在本地启动/停止 daemon：

```
atomix runner daemon [--listen <addr>] [--config <file>]
atomix runner status
atomix runner stop
```

### 5.4 远程 TUI 命令

远程 TUI 共用本地 TUI 的导航和元命令体系（`exit`、`exit:home`、`quit`、`help`）。以下为远程独有命令：

| 命令 | 说明 |
|------|------|
| `connect <alias>` | 连接到指定远程 Runner |
| `disconnect` | 断开当前连接 |
| `submit <file>` | 提交 .atx 文件到远程 |
| `r` | 刷新当前页面数据 |
| `f <filter>` | 设置日志过滤器 |
| `l` | 切换日志级别过滤 |
| `p` | 暂停/恢复日志实时推送 |
| `c` | 清空日志缓冲 |
| `s` | 保存配置更改 |

### 5.5 远程视图切换

| 命令 | 页面 |
|------|------|
| `:connections` | Connection Manager |
| `:dashboard` | Runner Dashboard |
| `:tasks` | Task List |
| `:pool` | Task Pool（依赖 DAG） |
| `:controller` | Controller Panel |
| `:slots` | Memory Slots |
| `:submit` | Submit Task |
| `:config` | Runner Config |
| `:logs` | Runner Logs |
| `:slots-anim` | Memory Slot Animation |
| `:perf` | Performance Analysis（远程） |
| `:task <id>` | Task Snapshot |

---

## 6. 架构

### 6.1 模块复用

编译器和执行器的模块通过 Rust 的 `pub mod` 机制按需导入，不引入 RPC 或 IPC 开销。

```
atomix-debug 二进制
  │
  ├─ compiler::lexer          词法分析（源码高亮、表达式解析）
  ├─ compiler::parser         语法分析（eval 表达式编译）
  ├─ compiler::semantic       语义分析（类型推断、变量解析）
  ├─ compiler::codegen        debug info 生成（.debug 段）
  │    └─ assembly::build_debug_section()
  │    └─ instr::InstrEmitter (line_map)
  │
  ├─ runner::VmState          VM 状态管理
  ├─ runner::execute          指令执行引擎
  ├─ runner::decode           指令解码 / 反汇编
  ├─ runner::memory           沙箱内存管理
  │
  ├─ debug::repl              TUI 前端
  ├─ debug::session           DebugSession trait + LocalDebugSession
  ├─ debug::eval              表达式求值
  ├─ debug::disassemble       反汇编格式化
  └─ debug::debug_segment     .debug 段解析
```

### 6.2 数据流

```
atomix task demo.atx
  │
  ├─ 1. 编译 (compiler::compile)
  │     → .atxe 二进制（含 .debug 段）
  │
  ├─ 2. 加载 (VmState::load_atxe)
  │     → 构造 VM 沙箱、寄存器初始化、栈空间分配
  │
  ├─ 3. 默认执行 (execute_instruction 循环)
  │     → 收集 Step 状态、变量事件、IS* 时间线、PC↔行号映射
  │     → 存入 ExecutionTrace
  │
  └─ 4. 启动 TUI
        → 所有查看/导航操作基于 ExecutionTrace 数据
        → step:run 时重新驱动 VM 执行
```

### 6.3 关键数据结构

```rust
/// 一次完整执行的记录
struct ExecutionTrace {
    steps: Vec<StepRecord>,
    variable_events: Vec<VariableEvent>,
    is_timeline: Vec<IsEvent>,
    hook_timeline: Vec<HookEvent>,
}

struct StepRecord {
    name: String,
    status: StepStatus,
    elapsed_us: u64,
    sub_calls: Vec<SubCall>,
    input_vars: Vec<String>,
    output_vars: Vec<String>,
    pc_range: (usize, usize),
    source_line: u32,
}
```

---

## 7. 远程模式

远程模式通过 ATXP 协议（详见 `docs/05-通信协议.md`）连接远程 Runner，提供任务监控、Runner 状态、内存槽位、控制器参数等运行时观察能力。

> **注意**：远程模式**不支持**断点、单步、watch、时间轴、数据追踪等深度调试功能。

### 7.1 远程页面（10 个）

远程页面复用本地 TUI 的布局框架（面包屑 + 命令栏 + 左右分栏）。

#### 7.1.1 Connection Manager

**入口**：`:connections`，或启动时未指定 `--origin`。

**内容**：已保存的远程连接列表、连接状态、增删操作。

**线框图**：`docs/debug-design/remote-connections.svg`

---

#### 7.1.2 Runner Dashboard

**入口**：`:dashboard`，或连接后默认显示。

**内容**：Runner 概览卡片（状态、任务数、内存、CPU、指令数、槽位）+ 事件时间线。

**线框图**：`docs/debug-design/remote-dashboard.svg`

---

#### 7.1.3 Task List

**入口**：`:tasks`。

**内容**：任务池中所有任务的状态列表。每行：ID、名称、状态、指令数、内存、耗时、依赖。

**线框图**：`docs/debug-design/remote-task-list.svg`

---

#### 7.1.4 Task Snapshot

**入口**：`:task <id>`，或 Task List 中 Enter。

**内容**：单个任务的寄存器、内存、调用栈、IS* 快照。仅对非 Running 状态的任务可用。

**线框图**：`docs/debug-design/remote-task-snapshot.svg`

---

#### 7.1.5 Controller Panel

**入口**：`:controller`。

**内容**：自适应控制器完整状态（批次、积压、OOM 反馈、4 个 Sigmoid 因子、回归模型参数）。

**线框图**：`docs/debug-design/remote-controller.svg`

---

#### 7.1.6 Memory Slots

**入口**：`:slots`。

**内容**：内存槽位布局（NORMAL / SLIPWAY / DEAD）、使用率、水位线、碎片率。

**线框图**：`docs/debug-design/remote-slots.svg`

---

#### 7.1.7 Submit Task

**入口**：`:submit`。

**内容**：任务提交表单（源文件、任务名、模式、优化级别、输出模式、超时）。

**线框图**：`docs/debug-design/remote-submit.svg`

---

#### 7.1.8 Runner Config

**入口**：`:config`。

**内容**：Runner 配置查看与修改（可写字段：max_concurrent、quantum、trace_level、deny_commands）。

**线框图**：`docs/debug-design/remote-config.svg`

---

#### 7.1.9 Task Pool

**入口**：`:pool`。

**内容**：任务池状态分布（Pending / Ready / Running / Error）、依赖 DAG 可视化（深度层级）、调度批次预测。

**线框图**：`docs/debug-design/remote-task-pool.svg`

---

#### 7.1.10 Runner Logs

**入口**：`:logs`。

**内容**：Runner 日志流。支持按 task_id 过滤、按日志级别过滤、暂停/恢复实时推送。

**线框图**：`docs/debug-design/remote-logs.svg`

---

#### 7.1.11 Memory Slot Animation — 内存槽动画

**入口**：`:slots-anim`，或从 Memory Slots 页切换进入。

**内容**：实时动画展示 Runner 内存槽位的分配与回收过程。

- 每个槽位（NORMAL / SLIPWAY / DEAD）以方块表示，颜色标识状态
- 动画播放：展示槽位分配、合并、碎片整理的全过程
- 播放控制：暂停/继续、速度调节（0.5x – 4x）
- 类似俄罗斯方块的视觉组合效果，直观验证内存调度是否正确
- 远程独有功能，依赖实时数据流

**线框图**：`docs/debug-design/remote-slots-anim.svg`

---

#### 7.1.12 Performance Analysis — 性能分析

**入口**：`:perf`。

**内容**：Runner 运行性能指标的时间序列视图。

- CPU / 内存使用率趋势图
- 任务吞吐量（completed/sec）时间序列
- 任务队列深度变化
- 控制器自适应参数历史（Sigmoid 因子 α/β/γ/δ 变化曲线）
- 内存槽位使用率趋势
- 所有图表支持时间范围缩放

**线框图**：`docs/debug-design/remote-perf.svg`

---

## 8. 不在此范围

以下功能留待后续独立设计：

- **代码编辑能力** — 当前 debugger 为只读视图；编辑能力需配套语言服务器、增量编译、热重载等基础设施，后续评估

---

> **本地页面线框图（19）**：`docs/debug-design/home-page.svg`、`step-detail.svg`、`data-lineage.svg`、`works-lifecycle.svg`、`task-dependency.svg`、`source-view.svg`、`watch-replay.svg`、`input-detail.svg`、`out-detail.svg`、`binary-view.svg`、`ir-disasm.svg`、`regs-mem.svg`、`exception-detail.svg`、`zone-status.svg`、`callstack.svg`、`breakpoints.svg`、`is-context.svg`、`segment-info.svg`、`perf-analysis.svg`
>
> **远程页面线框图（12）**：`docs/debug-design/remote-connections.svg`、`remote-dashboard.svg`、`remote-task-list.svg`、`remote-task-snapshot.svg`、`remote-controller.svg`、`remote-slots.svg`、`remote-submit.svg`、`remote-config.svg`、`remote-task-pool.svg`、`remote-logs.svg`、`remote-slots-anim.svg`、`remote-perf.svg`
>
> **协议文档**：`docs/05-通信协议.md`（ATXP v0.5）
