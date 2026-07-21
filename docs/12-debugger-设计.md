# Atomix Debugger 设计文档 (atomix-debug)

> 版本: v0.2 (本地调试完整设计)
> 最后更新: 2026-07-22
> 范围: **本地调试**（远程调试单独设计）
> 配套文件: `docs/debug-design/*.svg`（18 个页面线框图）

---

## 1. 概述

`atomix-debug` 是 Atomix 工具链的**本地调试器**。编译器与 VM 的部分模块直接编译进 debug 二进制，通过 `pub mod` 按需导入复用，无需跨进程通信。

### 1.1 两种使用方式

| 方式 | 入口 | 说明 |
|------|------|------|
| **TUI 交互模式** | `atomix task <file>` | 完整调试环境，7 个页面，40+ 命令 |
| **CLI 快捷命令** | `atomix task <file> --<flag>` | 一键操作，不进 TUI |

### 1.2 核心原则

- **默认运行收集**：打开时自动完整执行一遍，收集 Step 状态、变量变化、IS* 时间线、数据流向。后续查看/展开/追踪全在已收集数据上操作，无需再跑 VM。
- **纯键盘操作**：不支持鼠标。
- **底层复用**：编译器（解析、语义分析、debug info）+ 执行器（指令执行、寄存器/内存、解码/反汇编）按需导入，不重复造轮子。

---

## 2. TUI 布局

```
┌──────────────────────────────────────────────────┐
│  标题栏: atomix debug — <文件名>                   │
├──────────────────────────────────────────────────┤
│  面包屑: Home ▸ STEP 2: validate                  │  顶部固定
├───────────────────────────┬──────────────────────┤
│                            │  IS* Context          │
│   左侧主视图 (约 70%)       │  ────────────         │
│                            │  ISERROR   false      │
│   页面内容                  │  ISMODE    dev        │
│   (7 个页面之一)            │  ISSTEPNAME validate  │
│                            │  ...                  │
│                            │                       │
│                            │  Variables / Watch    │
│                            │  ────────────────     │
│                            │  RAW: bytes 1024B     │
│                            │  result: ...          │
│                            │                       │
│                            │  动态面板 (用户切换)    │
│                            │  :regs / :mem / :cpu  │
├───────────────────────────┴──────────────────────┤
│  > command                                        │  底部固定
│  ─────────────────────────────────────────────    │
│  help 面板（help 命令时弹出在命令栏上方）            │
└──────────────────────────────────────────────────┘
```

**布局规则：**
- 面包屑始终可见，显示当前页面路径
- 命令栏始终在底部
- 右侧上区 = IS* 上下文 + 常量（持久）
- 右侧下区 = 动态面板，`:regs` `:mem` `:cpu` 切换
- help 面板在命令栏上方弹出，不遮挡主视图

---

## 3. 页面体系（18 个页面）

所有页面通过面包屑导航，`exit` 返回上一层，`exit:home` 回到首页。

### 页面导航模型

```
Home  ← 根页面
 ├─ Step: validate       ← step:see 进入
 │    ├─ fn: check_format ← Enter 进入子调用
 │    │    └─ :df         ← 数据时间轴
 │    └─ :hooks           ← 钩子时间轴
 ├─ :src                  ← 源码视图
 └─ :deps                 ← 任务依赖树
```

页面历史以栈形式维护。`exit` 弹出栈顶，`exit:home` 清空栈，`exit:fn <name>` / `exit:step <name>` 跳到栈内指定页面。

---

### 3.1 ① Home — Step 执行日志

**进入**：默认打开。

**内容**：GitHub Actions 风格的 Step 日志，按段落排列：

```
TASK: demo                              ✓  15ms
──────────────────────────────────────────────
SYSTEM STEPS
  ▶ [load] Compile & link               ✓  2ms
  ▶ [init] Init runtime                 ✓  1ms
──────────────────────────────────────────────
▶ INPUT: RAW, TIMEOUT                   ✓  3ms
──────────────────────────────────────────────
  ▶ STEP 1: fetch_data()                ✓  2ms
  ▼ STEP 2: validate(RAW)               ✓  5ms   ← 选中
  ▶ STEP 3: transform(data)             ✗  ERROR
  ▶ STEP 4: send_report()               —  skipped
──────────────────────────────────────────────
▶ OUT: result                           ✓  1ms
```

- SYSTEM / INPUT / TASK / OUT 四个段落
- TASK 段每个 CALL 就是一个 Step
- 折叠/展开，默认全部折叠
- ↑↓ 选择 Step，Enter 进入详情

**对应文件**：`docs/debug-design/home-page.svg`

---

### 3.2 ② Step Detail — Step 详情

**进入**：`step:see name:<name>` / `step:see id:<n>` / 在 Home 页按 Enter。

**内容**：

```
← STEP: validate               ✓  5ms
────────────────────────────────────────────
CALL validate(RAW) @ line 43
────────────────────────────────────────────
  INPUT: RAW = <bytes: 1024>
  ─────────────────────────────────────────
  ▶ check_format(RAW)          ✓  1ms
      → valid: true
  ▶ DataChecker.run() [WORKS]  ✓  3ms
      → INIT → START → DONE
      → result: {ok: true, count: 42}
  ▶ normalize(result)          ✓  1ms
      → output: "processed"
  ─────────────────────────────────────────
  OUTPUT: status = "processed"
```

- 输入参数 → 子调用列表（每行 入→出）→ 输出
- fn 调用和 WORKS 调用区分显示
- WORKS 生命周期只做陈述性展示（INIT → START → DONE）
- Enter 可深入子调用详情
- 底部源码片段（当前行高亮）

**对应文件**：`docs/debug-design/step-detail.svg`

---

### 3.3 ③ Data Timeline — 数据时间轴

**进入**：`:df` / `:dataflow`。

**内容**：横向时间轴树，展示变量从 INPUT → 各 Step → OUT 的完整生命周期。

```
INPUT        STEP 1        STEP 2          STEP 3        OUT
 ● RAW ──── fetch_data() ─ validate() ─── transform() ✗
                          ├ check_format()
                          └ DataChecker.run()
 ● TIMEOUT ────────────── (unchanged) ──────────────── ● TIMEOUT=30
                            ↓ generate
                          ● valid: true ─┐
                            ↓ generate     │ merge
                          ● result: dict ─┤ normalize() ── ● status: "processed"
```

- 变量 ● 圆点，函数/WORKS ▭ 方角
- 边 = 数据流向
- 合并点（多输入→单输出）可视化
- 断裂（✗）清晰标注
- ←→ 平移、↑↓ 切变量、+/- 缩放

**对应文件**：`docs/debug-design/data-lineage.svg`

---

### 3.4 ④ Hook Timeline — 钩子时间轴

**进入**：`:hooks` / `:lifecycle`。

**内容**：横向时间轴树，展示 WORKS 的钩子执行序列。只显示定义了行为链的钩子（未定义行为的不显示）。

```
0ms              2ms              4ms              6ms
 INIT ──→ START ──→ validate() ──→ PROCESS ──→ transform() ──→ DONE ●
                  ├→ log_start() → end
                  └→ check_timeout() → STEP → process() → ...
                                                            
                                                  ERROR ──→ recover() → VOID_0 ─→ INIT' ─→ START' ...
                                                                    (重复触发, 时间轴继续向右)

                                                  FINALLY ──→ cleanup() ──→ DEL
```

- 钩子 ○ 圆角，动作 ▭ 方角
- 边 = 钩子链（条件标注在边上）
- 扇出（同一钩子多分支）黄色标注
- 重复触发不画回头箭头——直接在时间轴上继续往右画，用虚线表示
- ←→ 平移、↑↓ 切分支、+/- 缩放

**对应文件**：`docs/debug-design/works-lifecycle.svg`

---

### 3.5 ⑤ Task Dependency — 任务依赖树

**进入**：`:deps` / `:tasks`。

**内容**：横向层次树，展示 WAIT 产生的 FORK/JOIN 任务依赖关系。

```
Depth 3          Depth 2         Depth 1          Depth 0        Result
 D: fetch_users ─┐
                  ├─→ B: aggregate ─┐
 E: fetch_orders ─┘                  ├─→ A: process ──→ TASK: demo ──→ OUT
 F: validate_data ──→ C: format ────┘

调度顺序: Batch 1: D,E,F → Batch 2: B,C → Batch 3: A
```

- FORK 边（橙色）= 父派生子
- JOIN 边（青色）= 子返回父
- 深层节点先执行
- 同深度可并行
- 底部展示调度批次

**对应文件**：`docs/debug-design/task-dependency.svg`

---

### 3.6 ⑥ Source View — 源码视图

**进入**：`:src` / `show atx`。

**内容**：只读源码视图，含行号、装订线（断点红点）、当前执行行高亮。

```
 39  │
→43  │     CALL validate(RAW)          ← 当前执行行（绿色高亮）
 44  │     IF ISERROR:
 45  │         CALL handle_error()
 46  │     END
 50 ●     CALL transform(data)         ← 断点（红点）
```

- 语法高亮（关键字蓝色、字符串橙色等）
- 已执行行右侧灰色注释标记
- `↑↓` 移光标，`b` 打断点，`B` 条件断点，`g` 跳行
- 右侧面板：断点列表、当前行详情、作用域变量

**不支持编辑。** 编辑能力留待后续评估。

**对应文件**：`docs/debug-design/source-view.svg`

---

### 3.7 ⑦ Watch Replay — 回放

**进入**：`step:run <name> watch <speed>`。

**内容**：

```
⏳ Replay: step:run validate watch 0.5    Step 4 of 8 sub-operations
[========>                ] 50%

▶ fn: check_format(RAW)                          ✓ done
▶ WORKS: DataChecker.run()
    ● DONE: ISSTATUS=done  ISELAPSED=3ms         ← 当前
    RET: {ok: true, count: 42}
◌ fn: normalize(result)                          pending

Controls: Space=pause  ←→=speed  q=quit  :go=one step
```

- 进度条 + 子操作清单（已完成/当前/待执行）
- `Space` 暂停/继续，`←→` 调速（0.25x ~ 4x）
- 右侧面板实时更新 IS*、变量值、内存分配

**对应文件**：`docs/debug-design/watch-replay.svg`

---

### 3.8 ⑧ INPUT Detail — 输入数据源

**进入**：在 Home 页选中 INPUT 段按 Enter。

**内容**：列出所有输入常量及其数据源详情。

```
RAW       : bytes   ✓ loaded   2ms    FILE("data.csv") → ./data/data.csv  1024B
TIMEOUT   : int     ✓ loaded   1ms    DEFAULT = 30
CONFIG    : dict    △ overridden      HTTP("...") → WAIT override = custom_config
```

- 每个常量一行：名称、类型、状态、耗时、数据源类型及地址
- 被 WAIT 覆盖的常量标注 △
- 显示消费者（哪些 Step 使用了该常量）
- 右侧面板显示选中数据源的详细信息（路径、编码、读取字节数等）

**对应文件**：`docs/debug-design/input-detail.svg`

---

### 3.9 ⑨ OUT Detail — 产出交付

**进入**：在 Home 页选中 OUT 段按 Enter。

**内容**：列出所有产出变量及其交付状态。

```
result       : JSON   ✓ delivered   1ms/512B    FILE("output.json")
log          : TXT    ✓ delivered   0.5ms/128B  FILE("debug.log")
error_report : JSON   ✗ not delivered           HTTP("...") — STEP 3 failed
```

- 每个产出变量一行：名称、类型、状态、耗时/大小、目标类型及地址
- ✗ 未交付的标注原因
- 右侧面板显示选中产出目标的详细信息

**对应文件**：`docs/debug-design/out-detail.svg`

---

### 3.10 ⑩ Binary View — 二进制视图

**进入**：`:binary`。

**内容**：`.text` 段的原始 hex dump。

```
Offset    Hex (4B)         Binary                              ASCII
0x0000    13 01 00 10      00010011 00000001 00000000 00010000  ....
0x000C ●  83 20 00 00      10000011 00100000 00000000 00000000  . ..
→0x002C   13 04 00 00      00010011 00000100 00000000 00000000  ....
```

- 每行 = 1 条指令（4 字节）
- 列：offset / hex / binary / ASCII
- ● 红点 = 断点，→ 绿色 = 当前 PC
- 底部显示段信息（.text / .rodata / .debug 大小）

**对应文件**：`docs/debug-design/binary-view.svg`

---

### 3.11 ⑪ IR / Disassembly — 反汇编视图

**进入**：`:disasm` / `:ir`。

**内容**：指令反汇编，带操作数和源码注释。

```
PC       Bytes (LE)    Opcode   Operands              Source
0x0000   13 01 00 10   ADDI     sp, zero, 16          ; sp = stack_base + 16
0x000C ● 83 20 00 00   LOAD     fp, [sp + 0]          ; restore fp
→0x002C  13 04 00 00   ADDI     a0, zero, 0           ; entry: arg setup
```

- 操作码颜色分类（蓝色=ARITH、紫色=MEM、橙色=CTRL、红色=SYSTEM）
- 右侧面板显示选中指令的编码详情

**对应文件**：`docs/debug-design/ir-disasm.svg`

---

### 3.12 ⑫ Registers & Memory — 寄存器与内存

**进入**：`:regs` / `:mem`。

**内容**：上半部分 16 个寄存器，下半部分内存 hex dump。

```
R0  zero     0x0000000000000000  0                 (hardwired zero)
R1  sp       0x00007F0000001000  → stack pointer
R4  a0       0x000000000000002A  42               → "userCount: int"

Memory:
0x00001000  13 01 00 00  93 01 10 00  23 20 41 00  83 20 00 00  .... . .. # A. . ..
0x00001020  48 65 6C 6C  6F 00 00 00  64 61 74 61  2E 63 73 76  Hell o... data. csv
```

- 寄存器可选中编辑（`set a0 = 100`）
- 类型标注（从 debug info 推断）
- Tab 切换焦点（寄存器 ↔ 内存）
- 底部有 goto 地址输入框

**对应文件**：`docs/debug-design/regs-mem.svg`

---

### 3.13 ⑬ Exception Detail — 异常详情

**进入**：在 Home 页选中 ✗ Step 按 Enter，或从异常通知跳转。

**内容**：异常发生时的完整上下文。

```
✗ EXCEPTION: bad format — "expected JSON object, got array"

Error Info:  Type=ValueError  Code=402  Message="expected JSON..."
Source:      Zone=TASK  Step=transform(data)  Line=50  Function=TOOLS::transform()
IS* at error: ISERROR=true  ISERRORTYPE=ValueError  ISERRORCODE=402
Call Stack:  #0 TOOLS::transform() @ line 82  PC:0x0064
             #1 TASK:demo / STEP 3 @ PC:0x0050
Variables:   RAW=<bytes 1024B>  valid=true  result={ok,count:42}
```

- 右侧面板显示异常传播情况（是否被 TRY 捕获、影响哪些 Step/OUT）

**对应文件**：`docs/debug-design/exception-detail.svg`

---

### 3.14 ⑭ Zone Status — Zone 状态

**进入**：`:zones`。

**内容**：所有 Zone 的加载状态一览。

```
Zone     Lifecycle     Status     Size    Functions    PC Range      Deps
TOOLS    PERSISTENT    ● loaded   12 KB   3            0x0000-0x0020  —
INPUT    EXEC_UNLOAD   ● loaded   4 KB    — (const)    0x0024-0x0028  TOOLS
WORKS    PERSISTENT    ● loaded   48 KB   1            0x0030-0x0060  TOOLS,INPUT
TASK     EXEC_UNLOAD   ● loaded   8 KB    — (CALLs)    0x0064-0x00A0  TOOLS,WORKS,INPUT
OUT      LAZY          ○ lazy     2 KB    — (delivery) 0x00A4-0x00B0  TASK
```

- 生命周期类型说明（PERSISTENT/EXEC_UNLOAD/LAZY/PRUNE）
- Zone 依赖关系图
- 内存占用统计

**对应文件**：`docs/debug-design/zone-status.svg`

---

### 3.15 ⑮ Call Stack — 调用栈

**进入**：`:bt` / `backtrace`（在 TUI 中打开全页视图）。

**内容**：完整调用栈帧列表。

```
→ Frame 0  TOOLS::transform()         PC:0x0064  line 82
  Frame 1  TASK:demo / STEP 3: ...    PC:0x0050  CALL transform(RAW) @ line 50
  Frame 2  TASK:demo / STEP 2: ...    PC:0x0043  CALL validate(RAW) @ line 43
  Frame 3  TASK:demo / STEP 1: ...    PC:0x0038  CALL fetch_data() @ line 41
  Frame 4  TASK:demo (root/entry)     PC:0x002C
```

- 当前帧高亮
- ↑↓ 选择帧，Enter 查看帧详情
- u/d 上下切帧

**对应文件**：`docs/debug-design/callstack.svg`

---

### 3.16 ⑯ Breakpoints — 断点管理

**进入**：`break:list` 或 `:breaks`。

**内容**：所有断点的集中管理视图。

```
#  Type   Location                  Condition    Hits   Status
1  PC     line 39 (spacer)          —            0      ● active
2  PC     line 43 (CALL validate)   —            3      ● active
3  PC     line 50 (CALL transform)  ISERROR      1      ● active
4  HOOK   ERROR (global)            —            0      ● active
5  HOOK   DataChecker::START        ISDEBUG==true 0     ○ disabled
6  WATCH  RAW (variable)            —            0      ● active
7  WATCH  result (variable)         —            2      ● active
8  PC     TOOLS::check_format (fn)  —            0      ○ disabled
```

- d 删除、e 启用/禁用、c 编辑条件
- `break:clear` 清空、`break:enable all` 全部启用

**对应文件**：`docs/debug-design/breakpoints.svg`

---

### 3.17 ⑰ IS* Context — IS* 全览

**进入**：`:is`。

**内容**：72 个 IS* 变量按分组展示。

```
异常         计数           调用上下文
ISERROR ✗    ISCALLCOUNT 4   ISMETHOD validate
ISERRORTYPE — ISDEPTH 1      ISSTEPNAME validate
...          ...             ...

系统/环境    时间 & 任务      数据
ISMODE dev   ISELAPSED 5ms   ISDATASIZE 1024B
ISDEBUG ✓    ISTASKID f7a3   ...
```

- 按 7 个分组排列（异常/计数/调用/系统/时间/任务/数据）
- Tab 切换分组
- `/` 搜索 IS* 变量名

**对应文件**：`docs/debug-design/is-context.svg`

---

### 3.18 ⑱ Segment Info — 段信息

**进入**：`:segments`。

**内容**：`.atxe` 二进制文件的完整段结构。

```
.atxe File Layout

Header        20 bytes  magic="ATXE" version=0x0001 entry=0x002C
Section Table 72 bytes  6 entries × 12 bytes
.text         48 bytes  (12 instrs)  0x0000-0x002F  type=0x0001
.rodata       1024 bytes              0x0030-0x042F  type=0x0002
.task         64 bytes                0x0430-0x046F  type=0x0003
.debug        256 bytes  ADBG v1      0x0470-0x056F  type=0x0004
.exn          0 bytes    —             —              type=0x0005
.zones        32 bytes   5 zones      0x0570-0x058F  type=0x0006
```

- `.debug` 段展开显示条目详情
- `.exn` 段显示异常处理器（如有）

**对应文件**：`docs/debug-design/segment-info.svg`

---

## 4. 命令体系

统一格式：**冒号分级**

```
主命令                    step / continue / exit / quit / help / watch
主命令:子命令 参数 ...      step:see / exit:home / break:line 43
:模式命令                  :go / :again
:视图名                    :src / :df / :hooks / :deps / :regs / :mem
```

### 4.1 完整命令列表

#### 执行控制

| 命令 | 说明 |
|------|------|
| `step` | 执行到下一个 Step |
| `step:into` | 进入 Step 内部 |
| `step:out` | 跳出当前 Step |
| `continue` / `c` | 运行到断点或结束 |
| `:go` | 单步推进（watch/one 模式） |
| `:again` | 重做上一步 |

#### Step 查看与重跑

| 命令 | 说明 |
|------|------|
| `step:see <name>` | 查看 Step（按名） |
| `step:see <n>` | 查看 Step（按序号） |
| `step:run <name>` | 重跑 Step |
| `step:run <name> watch <speed>` | watch 模式 + 速度（0.25 ~ 4.0） |
| `step:run <name> one` | 单步模式 |

#### 导航

| 命令 | 说明 |
|------|------|
| `exit` | 返回上一层 |
| `exit:home` | 回到首页 |
| `exit:fn <name>` | 回到历史上某函数页 |
| `exit:step <name>` | 回到历史上某 Step 页 |

#### 断点

| 命令 | 说明 |
|------|------|
| `break:line <n>` | 按行号打断点 |
| `break:fn <TOOLS::fn>` | 按函数路径打断点 |
| `break:hook <HOOK>` | 在钩子上打断点 |
| `break:hook <WORKS::HOOK>` | 在特定 WORKS 钩子上打断点 |
| `break:line <n> if <cond>` | 条件断点 |
| `break:list` | 列出所有断点 |
| `break:del <id>` | 删除断点 |
| `break:clear` | 清空所有断点 |
| `watch <var>` | 监视变量变化 |

源码视图中键盘：`↑↓` 移光标，`b` 打断点，`B` 条件断点，`g` 跳行。

#### 信息查询

| 命令 | 说明 |
|------|------|
| `info` | 当前上下文概览 |
| `info:task` | 当前任务完整信息 |
| `info:zones` | Zone 加载状态 |
| `info:functions` | 当前作用域函数列表 |
| `info:variables` | 当前作用域变量 + 类型 |
| `info:file` | 当前源文件信息 |

#### 表达式求值

| 命令 | 说明 |
|------|------|
| `print <expr>` / `p <expr>` | 求值并打印 |
| `print/f <expr>` | 格式化打印（`/x` 十六进制 `/d` 十进制 `/s` 字符串） |
| `print/t <expr>` | 打印 + 类型信息 |

#### 设置

| 命令 | 说明 |
|------|------|
| `set <reg> = <value>` | 设置寄存器 |
| `set *<addr> = <value>` | 写入内存 |
| `set:var <name> = <value>` | 修改变量值 |

#### 搜索

| 命令 | 说明 |
|------|------|
| `find <text>` | 在源码中搜索 |
| `find:next` / `find:prev` | 下一个/上一个匹配 |
| `find:mem <pattern>` | 在内存中搜索字节 |
| `find:var <name>` | 搜索变量定义/使用处 |

#### 反汇编

| 命令 | 说明 |
|------|------|
| `disasm` | 反汇编当前 PC 附近 8 条 |
| `disasm <addr>` | 从指定地址开始 |
| `disasm <addr> <n>` | 从指定地址，n 条 |

#### 内存操作

| 命令 | 说明 |
|------|------|
| `mem:dump <addr> <len>` | hexdump |
| `mem:diff <a1> <a2> <len>` | 比较两块内存 |
| `mem:fill <addr> <len> <val>` | 填充 |
| `mem:watch <addr> <len>` | 监视区域变化 |

#### 调用栈

| 命令 | 说明 |
|------|------|
| `bt` / `backtrace` | 完整调用栈 |
| `frame <n>` | 切换到第 n 帧 |
| `frame:up` / `frame:down` | 上下切换帧 |
| `frame:info` | 当前帧详情 |

#### IS* 上下文

| 命令 | 说明 |
|------|------|
| `is` | 显示全部 IS* 变量 |
| `is <name>` | 查询单个（`is ISERROR`） |
| `is:watch <name>` | 监视 IS* 变量变化 |

#### 执行历史

| 命令 | 说明 |
|------|------|
| `history` | 显示命令历史 |
| `history <n>` | 最近 n 条 |
| `!<n>` | 重复执行第 n 条 |

#### 显示格式

| 命令 | 说明 |
|------|------|
| `display <expr>` | 每次 step 后自动打印 |
| `display` | 列出自动显示项 |
| `display:del <n>` | 删除第 n 项 |
| `display:clear` | 清空 |

#### 日志与导出

| 命令 | 说明 |
|------|------|
| `log:start <file>` | 开始日志记录 |
| `log:stop` | 停止记录 |
| `log:status` | 状态 |
| `export:state <file>` | 导出 VM 快照 |
| `export:dataflow <file>` | 导出数据追踪图 SVG |

#### 配置

| 命令 | 说明 |
|------|------|
| `set:fmt hex` / `set:fmt dec` | 默认数值格式 |
| `set:depth <n>` | 嵌套展开深度 |
| `set:speed <n>` | watch 默认速度 |

#### 视图切换

| 命令 | 说明 |
|------|------|
| `:src` | 源码视图 |
| `:df` | 数据时间轴 |
| `:hooks` | 钩子时间轴 |
| `:deps` | 任务依赖树 |
| `:binary` | 二进制视图（.text hex） |
| `:disasm` / `:ir` | 反汇编视图 |
| `:regs` | 寄存器面板 |
| `:mem` | 内存面板 |
| `:zones` | Zone 状态 |
| `:bt` / `:callstack` | 调用栈全页 |
| `:breaks` | 断点管理 |
| `:is` | IS* 全览 |
| `:segments` | 段信息 |

#### 元命令

| 命令 | 说明 |
|------|------|
| `help` / `h` / `?` | 弹出帮助面板 |
| `quit` / `q` | 退出调试器 |

### 4.2 全局键盘快捷键

| 键 | 作用 |
|----|------|
| `↑ ↓ ← →` | 导航节点/选项/光标 |
| `Enter` | 选中/进入 |
| `Esc` | 返回上一层 |
| `Tab` | 切换焦点面板（左 ↔ 右） |
| `+ / -` | 缩放（时间轴视图） |
| `Space` | 暂停/继续（watch 模式） |
| `f` | 追踪路径（图视图） |
| `t` | 切换执行轨迹高亮 |

---

## 5. CLI 快捷命令

部分能力可绕过 TUI，直接从命令行一键执行。

```
atomix task <file>                    # 进入 TUI 交互模式（默认运行）
atomix task <file> --no-run           # 进入 TUI，不自动运行
atomix task <file> --step <name>      # 运行并直接查看某个 Step
atomix task <file> --print <expr>     # 运行后打印表达式的值
atomix task <file> --check            # 检查断点命中情况
atomix task <file> --break-line <n>   # 运行前设断点，然后进入 TUI
atomix task <file> --export-state <f> # 导出 VM 快照到文件
atomix task <file> --export-dataflow  # 导出数据追踪图 SVG
atomix task <file> --log <file>       # 运行并将日志写入文件
atomix task <file> --list-steps       # 列出所有 Step 及其状态
atomix task <file> --list-vars        # 列出所有变量及其最终值
atomix task <file> --list-is          # 列出所有 IS* 最终状态
atomix task <file> --disasm <addr>    # 反汇编指定地址
atomix task <file> --mem-dump <a> <l> # 内存 dump
```

---

## 6. 架构

### 6.1 模块复用

```
atomix-debug 二进制
  │
  ├── compiler::lexer        # 词法分析（源码高亮、解析表达式）
  ├── compiler::parser       # 语法分析（源码高亮、eval 表达式编译）
  ├── compiler::semantic     # 语义分析（类型推断、变量解析）
  ├── compiler::codegen      # debug info 生成 (.debug 段)
  │    └── assembly::build_debug_section()
  │    └── instr::InstrEmitter (line_map)
  │
  ├── runner::VmState        # VM 状态（寄存器、内存、PC）
  ├── runner::execute        # 指令执行
  ├── runner::decode         # 指令解码/反汇编
  ├── runner::memory         # 沙箱内存
  │
  ├── debug::repl            # TUI 前端（ratatui）
  ├── debug::session         # DebugSession trait + LocalDebugSession
  ├── debug::eval            # 表达式求值
  ├── debug::disassemble     # 反汇编格式化
  └── debug::debug_segment   # .debug 段解析
```

### 6.2 数据流

```
atomix task demo.atx
  │
  ├─ ① 编译 (compiler::compile)
  │    → .atxe 二进制 (含 .debug 段)
  │
  ├─ ② 加载 (VmState::load_atxe)
  │    → VmState
  │
  ├─ ③ 默认执行 (execute_instruction 循环)
  │    → 收集: Step 状态、变量变化、IS* 时间线、PC→行号映射
  │    → 存入 ExecutionTrace
  │
  └─ ④ 打开 TUI
       → 所有查看/展开/追踪操作基于 ExecutionTrace 数据
       → step:run 时重新驱动 VM
```

### 6.3 关键数据结构

```rust
/// 一次执行的完整记录（默认运行后收集）
struct ExecutionTrace {
    steps: Vec<StepRecord>,
    variable_events: Vec<VariableEvent>,
    is_timeline: Vec<IsEvent>,
    hook_timeline: Vec<HookEvent>,
}

struct StepRecord {
    name: String,
    status: StepStatus,      // Done / Error / Skipped
    elapsed_us: u64,
    sub_calls: Vec<SubCall>,
    input_vars: Vec<String>,
    output_vars: Vec<String>,
    pc_range: (usize, usize),
    source_line: u32,
}
```

---

## 7. 不在此次范围（后续设计）

- 远程调试（ATXP 协议）
- TUI 内的代码编辑能力
- 执行录制与回放（.atxr）
- 内存槽可视化（俄罗斯方块滑道）
- 数据流动画
- 性能分析面板
- 三层逆向视图（二进制/指令集/AST）
- GUI 模式

---

> 页面线框图位于 `docs/debug-design/`：home-page.svg、step-detail.svg、data-lineage.svg、works-lifecycle.svg、task-dependency.svg、source-view.svg、watch-replay.svg、input-detail.svg、out-detail.svg、binary-view.svg、ir-disasm.svg、regs-mem.svg、exception-detail.svg、zone-status.svg、callstack.svg、breakpoints.svg、is-context.svg、segment-info.svg。
