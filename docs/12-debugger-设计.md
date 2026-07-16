# Atomix Debugger 设计文档 (atomix-debug)

> 版本: v0.1 (需求框架)
> 最后更新: 2026-07-16
> 依赖: ATXP 协议 v0.3 (`docs/通信协议.md`)
> 配套文档: 外围工具.md, 命令行规范.md

---

## 1. 概述

`atomix-debug.exe` 是 Atomix 工具链的**深度检查后端**，通过 ATXP 协议与 `atomix-runner.exe` 通信，提供从底层二进制到高层源码的全方位调试能力。

### 1.1 定位

```
atomix CLI
  └── atomix task <name> ...  ──→  atomix-debug.exe  ──ATXP──→  atomix-runner.exe
                                  (检查/控制/逆向/可视化)         (执行引擎)
```

- **本地模式**：共享内存（16MB 轨迹流 + 命令/事件环形缓冲），全功能可用
- **远程模式**：TCP（含 TLS 可选），功能受限于请求-响应模式

### 1.2 核心能力一览

| 能力域 | 功能 | 依赖的 ATXP 端点 |
|--------|------|-----------------|
| 三层逆向 | 二进制→指令集→AST→源码 | `segments/{seg}`, `sourcemap`, `symbols` |
| 执行控制 | 断点/单步/继续/数据监视 | `breakpoints`, `status` |
| 状态检查 | 寄存器/内存/栈/IS*上下文/类型 | `regs`, `mem`, `stack`, `context`, `types` |
| 可视化 | 数据流/内存槽/钩子/Zone/依赖图/时间线 | `dataflow`, `slots`, `timeline`, `zones` |
| 录制回放 | .atxr 录制与回放 | Command + ATXR format |
| 性能分析 | 控制器状态/任务统计/热点 | `controller`, `stats`, trace streaming |
| 表达式 | 在当前上下文 eval | `eval` |

---

## 2. 三层逆向工程视图

这是 atomix-debug 最有特色的能力：从 `.atxe` 编译产物逐层还原为可读的代码表示。

### 2.1 概述

```
Layer 1: 二进制视图 (Binary View)
    ↑ disassemble
Layer 2: 指令集视图 (ISA View)  
    ↑ .debug segment + sourcemap
Layer 3: AST 视图 (AST View)
    ↑ sourcemap
Layer 4: 源码视图 (Source View)
```

每一层使用不同的 ATXP 端点获取数据：

| 视图 | 数据来源 | ATXP 端点 |
|------|---------|----------|
| 二进制 | `.text` 段原始字节 | `segments/text` GET |
| 指令集 | 32位定长指令解码 | `segments/text` GET + 本地反汇编 |
| AST | AST 节点映射信息 | `segments/debug` GET (`.debug` 段解析) |
| 源码 | 源文件内容 + pc↔行号映射 | `sourcemap` GET |

### 2.2 二进制视图

**展示内容：** PC 偏移 + Hex dump + ASCII sidebar

```
偏移      Hex                                   ASCII
0x0000:  13 01 00 00  93 01 10 00  23 20 41 00  .... .... # A.
0x000C:  83 20 00 00  67 80 00 00               . ... g...
```

**数据获取流程：**

```
1. Query segments/text GET → SegmentData { raw_data: bytes }
2. 按 32-bit (4B) 对齐分行显示
3. 当前 PC 高亮
4. 断点地址标记
```

### 2.3 指令集视图（反汇编）

**展示内容：** 每条 32-bit 指令 → 助记符 + 操作数

```
0x0000:  ADDI   R1, R0, 0        // sp = 0
0x0004:  ADDI   R1, R1, 16       // sp += 16
0x0008:  SW     R2, 0(R1)        // [sp+0] = fp
0x000C:  LW     R2, 0(R1)        // R2 = [sp+0]
0x0010:  JALR   R0, R3, 0        // ret
```

**数据获取流程：**

```
1. Query segments/text GET → raw .text bytes
2. 本地反汇编引擎:
   - 查 opcode lookup table (256 entries)
   - 根据编码模板 (R3/R2I/R1I/JI) 解析操作数
3. 叠加符号信息:
   - Query symbols GET → 函数名标注 (入口 PC → func_name)
   - 跳转目标标注 (JMP/JAL target → label/function name)
4. 标注:
   - ← PC (当前执行位置)
   - ● BP (断点)
   - ◆ WP (数据监视点)
```

**反汇编查找表：**

```
struct OpcodeEntry {
    mnemonic:   &str,        // "ADDI", "JAL", ...
    template:   EncTemplate, // R3 / R2I / R1I / JI
    category:   OpCategory,  // ARITH / MEM / CTRL / ...
    desc:       &str,        // 指令说明
}
```

### 2.4 AST 视图

**展示内容：** IR 指令序列 → AST 节点树

```
Function: process_data (0x0042-0x00A0)
└── Block
    ├── VarDecl: int x
    ├── VarDecl: str result
    ├── Assign: x = INPUT.value
    │   ├── Identifier: x
    │   └── MemberAccess: INPUT.value
    ├── Call: validate(x)
    │   └── Identifier: x
    └── Return: result
```

**数据获取流程：**

```
1. Query segments/debug GET → DebugSegment { entries: [...] }
   每个 entry 有 kind=DEBUG_AST_NODE 的记录:
   { pc: 0x0042, kind: AST_NODE, ast_node: "FunctionDecl:process_data" }
   { pc: 0x0048, kind: AST_NODE, ast_node: "VarDecl:x:int" }
   { pc: 0x0050, kind: AST_NODE, ast_node: "Assign" }
   ...

2. 按 pc 范围构建 AST 树
3. AST 节点类型参考编译管线中的 40+ AST 节点类型
```

**`.debug` 段的 AST 条目格式（提议）：**

```
每个 AST 映射条目:
  pc_start:  u32 LE    // 此 AST 节点对应的起始指令
  pc_end:    u32 LE    // 此 AST 节点对应的结束指令
  kind:      u8        // AST 节点类型 (见枚举)
  depth:     u8        // 树深度 (用于缩进)
  name_len:  u16 LE    // 名称长度
  name:      UTF-8     // 节点名称/类型
  extra_len: u16 LE    // 附加信息长度
  extra:     UTF-8     // 附加信息 (类型注解, 值等)
```

### 2.5 源码视图

**展示内容：** 原始 .atx 源文件，当前执行行高亮

```
 40 │ TASK cleanup {
 41 │     RAW = INPUT.file("data.csv")
 42 │ →   CALL parse(RAW)         ← 当前执行位置 (PC=0x0042)
 43 │     IF ISERROR:
 44 │         CALL handle_error()
 45 │     END
 46 │ }
```

**数据获取流程：**

```
1. Query sourcemap GET → SourceMap { source_file, entries: [...] }
   每个 entry: { pc_start, pc_end, source_line, source_col, source_text }

2. 根据当前 PC 查找对应的 source_line
3. 从 source_text 或源文件中读取并显示
4. 高亮当前行 (PC 匹配)
5. 标记断点行 (bp.pc → source_line 映射)
```

---

## 3. 执行控制

### 3.1 断点管理

```
操作                           ATXP 消息
─────────────────────────────────────────────────────
设置 PC 断点                  Command + SET + Breakpoint { type: PC, pc: 0x42 }
设置条件断点                  Command + SET + Breakpoint { pc: 0x42, condition: "x > 10" }
设置数据监视点                Command + SET + Breakpoint { type: MEM_WRITE, mem_addr: 0x1000, mem_size: 4 }
列出所有断点                  Query + GET → BreakpointList
删除断点                      Command + SET + endpoint ".../del/{id}"
启用/禁用                     Command + SET + Breakpoint { id: 3, enabled: false }
订阅断点事件                  SUBSCRIBE → Event { type: BREAKPOINT_HIT / WATCHPOINT_HIT }
```

### 3.2 执行流程控制

```
暂停                          Command + SET + TaskStatus { state: SUSPENDED }
继续                          Command + SET + TaskStatus { state: RUNNING }
单步 (1条指令)                Command + SET + step_count: 1
单步 (到下一行源码)           debugger 内部循环: 单步 → 检查 sourcemap → pc是否在新行
单步 (跳过函数)               debugger 内部: 在当前 CALL 后设临时断点 → continue
```

---

## 4. 状态检查

### 4.1 寄存器视图

```
R0  (zero):    0x0000000000000000
R1  (sp):      0x00007F00_00001000  → 栈指针
R2  (fp):      0x00007F00_00001000  → 帧指针
R3  (ra):      0x0000000000000080  → 返回地址
R4  (a0):      0x000000000000002A  (42) "userCount: int"
R5  (a1):      0x00007F00_00000100  → "data.csv: str*"
R6  (a2):      0x0000000000000000
R7  (a3):      0x0000000000000000
R8  (t0):      0x0000000000000001  (true) "isValid: bool"
R9  (t1):      0x0000000000000000
...
R14 (task_id): 0xA1B2C3D4E5F60001
R15 (tmp):     0x0000000000000000

PC: 0x00000042
```

类型标注通过 `RegisterSnapshot.annotations` 和 `types` 端点获取。

### 4.2 内存视图

- 分页显示（每页 256 字节）
- 地址可跳转（输入绝对地址或符号名）
- 支持多种显示格式：hex dump / 有符号整数 / 无符号整数 / float / ASCII
- 数据监视点高亮
- 实时刷新（本地模式通过共享内存，远程模式通过定时轮询）

### 4.3 调用栈视图

```
Frame 0: parse(RAW="data.csv")        pc=0x0042  [cleanup.atx:42]
Frame 1: cleanup()                     pc=0x0020  [cleanup.atx:38]
Frame 2: (entry)                       pc=0x0000  [cleanup.atx:36]
```

点击栈帧 → 自动切换到该帧的寄存器/内存/源码视图。

### 4.4 IS* 上下文面板

全天候显示运行时自省值，按类别分组：

```
┌─ 异常 ─────────────────────┐  ┌─ 调用上下文 ──────────────┐
│ ISERROR:       false       │  │ ISMETHOD:    parse       │
│ ISERRORTYPE:   —           │  │ ISSTEPNAME:  parse_data  │
│ ISERRORMESSAGE: —          │  │ ISWORKNAME:  DataParser  │
│ ISCHILDERROR:  false       │  │ ISARGS:      ["data.csv"]│
└────────────────────────────┘  └──────────────────────────┘

┌─ 计数 ─────────────────────┐  ┌─ 系统 ───────────────────┐
│ ISSTEPINDEX:   2           │  │ ISDEBUG:      true       │
│ ISTOTALSTEPS:  5           │  │ ISMODE:       dev        │
│ ISDEPTH:       1           │  │ ISFILE:       cleanup.atx│
│ ISCALLCOUNT:   42          │  │ ISLINE:       42         │
└────────────────────────────┘  └──────────────────────────┘
```

数据来源：`Query runner/tasks/{tid}/context GET`，建议 500ms 刷新间隔。

### 4.5 变量监视窗

用户自定义监视列表：

```
表达式              类型      值
──────────────────────────────────
x                   int       42
user.name           str       "Alice"
items[0]            dict      {id: 1, name: "foo"}
len(items)          int       3
```

每个监视项通过 `eval` 端点求值：

```
Query runner/tasks/{tid}/eval EVAL
  params: EvalRequest { expression: "user.name" }
→ EvalResult { type: "str", value: "\"Alice\"" }
```

---

## 5. 可视化

### 5.1 数据流图

**数据来源：** `Query runner/tasks/{tid}/dataflow GET`

**展示形式：** 有向图（Source → Transform → Sink），边标注数据标签和字节数

```
┌──────────┐    data.csv     ┌──────────┐    parsed     ┌──────────┐
│  INPUT   │ ──────────────→ │  parse() │ ────────────→ │  OUT     │
│  (file)  │    1,024 B      │ (WORKS)  │    512 B      │ (output) │
└──────────┘                 └──────────┘               └──────────┘
```

### 5.2 内存槽可视化（俄罗斯方块滑道）

**数据来源：** `Query runner/tasks/{tid}/slots GET`，本地模式下还可 SUBSCRIBE `MEMORY_SLOT` 流

**展示形式：** 垂直槽位列表，每个槽位显示：
- 槽位 ID + 任务名
- 使用量进度条（used / size）
- 水位线标记
- 颜色：NORMAL(绿) / SLIPWAY(黄) / DEAD(灰) / RESERVED(红)

```
槽位 ──────────────────────────────────────────
 #0  [████████░░░░] 40%  task_A     NORMAL
 #1  [████████████] 95%  task_B ⚠   WARNING
 #2  [░░░░░░░░░░░░]  0%  (空闲)     NORMAL
 #3  [██████░░░░░░] 30%  task_C     SLIPWAY ← 备用滑道
 #4  [██░░░░░░░░░░] 10%  (合并中)   DEAD
 #5  [████████████] 100% task_D 🔴  RESERVED
```

### 5.3 钩子生命周期图

**数据来源：** SUBSCRIBE 钩子事件（HOOK_STEP, HOOK_CALL, HOOK_RETURN, ...），叠加 `context` 端点

**展示形式：** 时间轴 + 泳道图，显示每个 WORKS 实例的 9 阶段钩子触发序列

```
时间 ──────────────────────────────────────────→
DataParser:
  INIT ──→ START ──→ STEP ──→ CALL:validate()
                        │
                        ├── CALL_AFTER: ✅
                        │
                        └── DONE ──→ DEL ──→ FINALLY
```

点击钩子节点 → 展开该钩子的 IS* 上下文快照。

### 5.4 Zone 加载状态

**数据来源：** `Query runner/tasks/{tid}/zones GET`

```
Zone          生命周期       状态      大小
───────────────────────────────────────────
TOOLS         PERSISTENT     ● 已加载   12 KB
INPUT         EXEC_UNLOAD    ● 已加载    4 KB
WORKS         PERSISTENT     ● 已加载   48 KB
TASK          EXEC_UNLOAD    ◐ 加载中   —
OUT           LAZY           ○ 未加载   —
```

### 5.5 任务依赖图

**数据来源：** `Query runner/tasks/{tid}/segments/task GET` → TaskSegment

**展示形式：** DAG（有向无环图），最深层优先高亮

```
         [E: send_report]
              ↑
    ┌─────────┴─────────┐
    │                   │
 [C: aggregate]    [D: format]
    ↑                   ↑
    └──────┬────────────┘
           │
      [B: validate]
           ↑
      [A: extract]  ← 当前执行
```

### 5.6 执行时间线

**数据来源：** `Query runner/tasks/{tid}/timeline GET`

**展示形式：** 横向时间轴，不同颜色标记不同事件类型（CALL=蓝, RET=绿, ECALL=橙, OOM=红, HOOK=紫, BREAKPOINT=黄）

```
0ms ──────────────────────────────────────── 150ms
 │  ██  ██    ████  ██  ██  ████    ██  ██   │
 │  CALL RET  ECALL CALL RET ECALL  CALL RET  │
```

---

## 6. 录制与回放

### 6.1 录制

```bash
atomix task record cleanup --output ./debug/cleanup.atxr --level full
```

内部通过 ATXP Command 触发：

```
Command {
    endpoint: "runner/tasks/cleanup",
    operation: SET,
    data: RecordingCommand {
        action: START_RECORDING,
        output_path: "./debug/cleanup.atxr",
        level: FULL,
        snapshots: BREAKPOINT_ONLY
    }
}
```

### 6.2 回放

加载录制文件后，debugger 进入回放模式，提供标准播放控件：

```
┌─────────────────────────────────────────────┐
│  ⏮   ⏪   ▶/⏸   ⏩   ⏭   速度: 1.0x  [━░░░░] │
│  00:01:42.350 / 00:05:30.000               │
└─────────────────────────────────────────────┘
```

回放时所有 debugger 视图（寄存器/内存/栈/源码）跟随时间轴更新，完全模拟实时调试体验。

---

## 7. 性能分析

### 7.1 控制器面板

**数据来源：** `Query runner/controller GET`

展示自适应控制器的实时状态：

```
┌─ 批次管理 ─────────────┐  ┌─ OOM 反馈 ─────────────┐
│ N_batch:        12     │  │ α_mem:          0.375  │
│ Hard Ceiling:   21     │  │ OOM Count:      3      │
│ Soft Ceiling:   15     │  │ OOM State:      INC    │
│ Backlog:        34     │  │                          │
│ High Backlog:   YES ⚠  │  │                          │
│ Cold Start:     NO      │  └────────────────────────┘
└────────────────────────┘  ┌─ 槽位 ─────────────────┐
┌─ 四因子 ───────────────┐  │ Total:    32            │
│ β (积压):   0.82       │  │ Used:     24 ●●●●●●●   │
│ λ (速度):   0.65       │  │ Slipway:   4 ●●        │
│ σ (容量):   0.71       │  │ Dead:      4 ●●        │
│ γ (波动):   0.23       │  │ Slot:     16.0 MB      │
│ Merged:     0.58       │  │ Slipway:   1.5x        │
└────────────────────────┘  └────────────────────────┘
```

### 7.2 任务统计面板

**数据来源：** `Query runner/tasks/{tid}/stats GET`

```
指令总数:     1,234,567    异常次数:       3
ECALL 次数:   42           CPU 时间:     12.5ms
阻塞次数:     8            墙上时间:     45.0ms
OOM 次数:     1            峰值内存:    4.2 MB
量子耗尽:     156          IO 读:       1.2 MB
上下文切换:   164          IO 写:       0.5 MB
```

### 7.3 热点分析

**数据来源：** 本地模式下通过 trace streaming 获取全量轨迹，远程模式下通过 `stats` 端点 + 采样 trace

**展示形式：** 按函数/指令聚合的执行次数和耗时：

```
函数                  调用次数    指令数    耗时(ms)   占比
─────────────────────────────────────────────────────
parse()               1,000      450,000    18.2      40.4%
validate()            1,000      320,000    12.8      28.4%
aggregate()             500      200,000     8.1      18.0%
format()                200       80,000     3.2       7.1%
其他                     —        50,000     2.7       6.1%
```

---

## 8. 表达式求值（REPL）

### 8.1 交互模式

```
atomix> task cleanup --repl

attached to task: cleanup (pc=0x0042)
mode: dev | step: parse_data (2/5)

>>> x
42 (int)

>>> x + 1
43 (int)

>>> user.name
"Alice" (str)

>>> items[0]
{id: 1, name: "foo", value: 100} (dict[str, any])

>>> len(items)
3 (int)

>>> items | filter(.value > 50) | map(.name)
["foo"] (list[str])

>>> :help
  :regs      — 显示寄存器
  :stack     — 显示调用栈
  :context   — 显示所有 IS* 上下文变量
  :types     — 显示当前作用域类型信息
  :step      — 单步执行
  :continue  — 继续执行
  :quit      — 退出
```

### 8.2 实现

每次 `>>>` 输入触发：

```
Query runner/tasks/{tid}/eval EVAL
  params: EvalRequest { expression: "user.name", timeout_ms: 500 }
→ EvalResult { type: "str", value: "\"Alice\"", error: "", duration_ns: 120000 }
```

- 求值在 Runner 的 VM 沙箱内执行（不可 ECALL/网络/文件/TASK_FORK）
- 500ms 超时自动中断
- 远程模式下受 `deny_commands` 约束

---

## 9. .debug 段格式定义

### 9.1 现状

当前 `.debug` 段格式定义是"由工具链自定义"，未标准化。这导致 debugger 无法可靠解析。

### 9.2 提议格式

采用紧凑的自定义格式（非 DWARF，避免引入复杂依赖），直接编码 pc↔源码↔AST 的三向映射：

```
.debug 段结构:
┌─────────┬──────┬─────────────────────────────────┐
│ 0x00    │ 4B   │ magic: "ADBG" (0x47424441)      │
│ 0x04    │ 2B   │ version: u16 LE = 0x0001        │
│ 0x06    │ 2B   │ flags: u16 LE                   │
│ 0x08    │ 4B   │ entry_count: u32 LE             │
│ 0x0C    │ N    │ entries: DebugEntry[entry_count] │
│ ...     │ M    │ string_pool: 所有字符串连续存放   │
└─────────┴──────┴─────────────────────────────────┘

每个 DebugEntry (定长 28 字节):
┌─────────┬──────┬─────────────────────────────────┐
│ 0x00    │ 4B   │ pc_start: u32 LE                │
│ 0x04    │ 4B   │ pc_end: u32 LE                  │
│ 0x08    │ 4B   │ source_line: u32 LE             │
│ 0x0C    │ 2B   │ source_col: u16 LE              │
│ 0x0E    │ 1B   │ kind: u8                        │
│ 0x0F    │ 1B   │ depth: u8                       │
│ 0x10    │ 4B   │ func_name_off: u32 LE           │ string_pool 偏移
│ 0x14    │ 4B   │ var_name_off: u32 LE            │ string_pool 偏移
│ 0x18    │ 4B   │ type_name_off: u32 LE           │ string_pool 偏移
│ 0x1C    │ 4B   │ ast_node_off: u32 LE            │ string_pool 偏移
└─────────┴──────┴─────────────────────────────────┘

kind 枚举:
  0 = FUNC_START   (func_name 有效)
  1 = FUNC_END     (func_name 有效)
  2 = VAR_DECL     (var_name + type_name 有效)
  3 = VAR_END      (var_name 有效)
  4 = LINE         (source_line 有效)
  5 = AST_NODE     (ast_node 有效)
```

**string_pool：** 所有字符串（函数名、变量名、类型名、AST 节点类型）以 null-terminated C string 格式连续存储。Entry 中的 `*_off` 字段是该字符串在 pool 中的偏移量。

---

## 10. 用户界面方案（参考）

### 10.1 TUI 模式（终端）

```
┌─ 源码 (cleanup.atx) ───────────────────────────┐
│ 40 │ TASK cleanup {                            │
│ 41 │     RAW = INPUT.file("data.csv")          │
│ 42 │ →   CALL parse(RAW)        ● BP          │
│ 43 │     IF ISERROR:                          │
│ 44 │         CALL handle_error()              │
│ 45 │     END                                  │
│ 46 │ }                                        │
├─ 指令集 ───────────────────────────────────────┤
│ 0x0042: ADDI  R4, R0, 42    // a0 = 42        │
│ 0x0046: JAL   R3, 0x0080    // call parse     │
│ 0x004A: SW    R4, 0(R1)     // save result    │
├─ 寄存器 ─────────────────┬─ IS* 上下文 ────────┤
│ R4(a0): 42 "userCount"  │ ISSTEPNAME: parse   │
│ R5(a1): → "data.csv"    │ ISDEPTH:    1       │
│ R8(t0): true "isValid"  │ ISDEBUG:    true    │
│ PC:     0x0042          │ ISMODE:     dev     │
├─ 调用栈 ─────────────────┴─────────────────────┤
│ Frame 0: parse(RAW)    pc=0x0042              │
│ Frame 1: cleanup()     pc=0x0020              │
├─ 变量监视 ─────────────────────────────────────┤
│ x:           42 (int)                         │
│ user.name:   "Alice" (str)                    │
│ items.len:   3 (int)                          │
├─ REPL ────────────────────────────────────────┤
│ >>> items[0].name                             │
│ "foo" (str)                                   │
│ >>>                                           │
└───────────────────────────────────────────────┘
```

### 10.2 GUI 模式（考虑未来）

可考虑基于 [egui](https://github.com/emilk/egui) (Rust 原生 immediate mode GUI) 或 Tauri (Web 技术栈) 构建图形界面，提供：
- 多面板自由布局
- 图形化数据流/依赖图/内存槽
- 拖拽式变量监视
- 时间轴拖拽回放

---

> 本文档为 atomix-debug 的**需求框架**。具体实现细节（TUI 框架选型、面板渲染逻辑、键盘快捷键等）在后续开发阶段细化。
