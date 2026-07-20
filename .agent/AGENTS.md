# Atomix 项目手册（AI 参考）

## 一句话

Atomix 是一门**任务执行 DSL**，配备编译器（.atx → .atxe）、虚拟机（54 指令）、运行时（多任务调度 + 自适应并发）和调试器。核心场景：在 2C2G 服务器上跑出最大吞吐量。

---

## 目录结构

```
src/
├── base/          基础类型
│   ├── isa.rs     [指令集] 54 opcode + 4编码模板 + 16寄存器
│   ├── ir.rs      [IR格式] .atxe 二进制编解码
│   ├── atxp.rs    [通信] ATXP protobuf 类型
│   └── error.rs   错误类型
├── compiler/      编译器管线
│   ├── lexer.rs   [词法] ~95%完成，28测试
│   ├── token.rs   [Token] ~50关键字 + 符号
│   ├── parser.rs  [语法] 递归下降+Pratt，~90%，22测试
│   ├── ast.rs     [AST] 42+ 节点类型
│   ├── semantic.rs [语义] 符号表/类型/单态化，~85%，20测试
│   ├── symbol.rs  符号表管理
│   ├── type_checker.rs 类型推断
│   ├── builtins.rs 内置函数注册
│   ├── linker.rs  链接器（单文件）
│   └── codegen/   代码生成
│       ├── assembly.rs   .atxe 汇编输出
│       ├── expr.rs       表达式 IR 生成
│       ├── instr.rs      IR 指令发射器
│       ├── stmt.rs       语句 IR 生成
│       ├── optimizer.rs  O0/O1/O2/Os
│       └── reg_alloc.rs  线性扫描寄存器分配
├── runner/        VM + 运行时（22文件，~8000行）
│   ├── mod.rs     VmState、.atxe 加载
│   ├── execute.rs [核心] 54指令dispatch + ECALL，23测试
│   ├── decode.rs  指令解码器
│   ├── executor.rs Executor量子执行/线程池
│   ├── runtime.rs  [核心] 调度/冷启动/OOM，10测试
│   ├── pool.rs    任务池
│   ├── batch.rs   批次管理器/N_batch
│   ├── slot.rs    槽位管理
│   ├── sched.rs   调度器
│   ├── memory.rs  沙箱内存
│   ├── server.rs  ATXP TCP 服务端
│   ├── client.rs  ATXP TCP 客户端
│   ├── config.rs  runner.toml
│   ├── event.rs   事件通道(SPSC)
│   └── ...
├── debug/         调试器
│   ├── repl.rs    REPL: step/break/watch/backtrace，16测试
│   ├── disassemble.rs 反汇编
│   ├── eval.rs    表达式求值
│   └── debug_segment.rs  .debug段↔源码行
├── bin/           5个二进制入口
│   ├── atomix.rs  [主CLI] build/check/runner/task/origin
│   ├── atomix-build.rs  构建专用
│   ├── atomix-runner.rs [Runner] run + daemon
│   ├── atomix-debug.rs  调试器独立入口
│   └── atomix-map.rs    [索引] 代码地图生成
└── lib.rs
```

---

## 核心架构决策

### 指令集（ISA）
- 54 条 opcode，32 位定长，4 种编码模板（R3/R2I/R1I/JI）
- 16 个 64 位寄存器：R0=zero(硬编码0), R1=sp, R2=fp, R3=ra, R4-R7=a0-a3, R8-R13=t0-t5, R14=task_id(只读), R15=tmp
- 17 个 ECALL 系统调用：ALLOC/FREE/TCP(4)/FS(6)/DNS/PRINT/LEN
- 异常表 (.exn)：16 字节条目，THROW 时查表跳转 handler

### 内存模型
- 沙箱线性内存：布局 `[.rodata | 堆 | 栈]`
- 水位线：安全区(75%) → 警戒线(75-90%) → 保留区(90-100%)
- .rodata 只读，写入触发异常
- 文件描述符/套接字列表在 VmState 中维护

### 执行模型
- Executor = VM = Thread 1:1:1
- 协作式分片：默认 1000 指令/量子
- 挂起永远在指令边界（非抢占）
- 多线程：Arc<Mutex<TaskPool>> + mpsc channel

### 任务调度
- 依赖图层级调度（.task 段预计算）
- N_batch = min(H, S) 自适应并发
- H = min(C_cpu, C_mem, C_io, C_net) 四维硬上限
- S = H × β×λ×σ×γ 加权几何平均软上限
- 冷启动 4 阶段：Bootstrap(1任务) → WarmUp(2) → Accumulate(5-50) → Stable

### 远程通信（ATXP）
- Protobuf 协议，16 种消息类型
- TCP 传输（本地共享内存预留）
- Daemon 模式：Tokio 异步 TCP 服务器
- Client：同步 TCP + 连接管理

### 调试器
- REPL 命令：step/continue/break(条件)/watch/disasm/regs/mem/examine/eval/print/set/backtrace/frame/up/down/source/display
- 断点实现：指令替换 TRAP + 保存原始指令
- 监视点：LOAD/STORE 指令地址匹配

---

## 实现状态一览

| Category | 状态 | 关键缺口 |
|----------|------|---------|
| 词法分析 | ✅ ~95% | FStr插值解析不完整 |
| 语法分析 | ✅ ~90% | — |
| 语义分析 | ✅ ~85% | 类型推断边界情况 |
| IR生成 | ✅ ~70% | .task/.exn/.zones段简化 |
| 优化器 | 🔶 O0/O1通 | O2内联/循环优化缺失 |
| 链接器 | 🔶 单文件 | 多文件链接未实现 |
| VM指令 | ✅ 54/54 | — |
| ECALL | ✅ 17/17 | — |
| 内存管理 | 🔶 | 墙壁预分配/滑道缺失 |
| 运行时 | ✅ ~80% | 自适应因子S型函数未全 |
| ATXP协议 | ✅ | — |
| Daemon | ✅ | — |
| CLI | ✅ 主要命令 | pm/format/lint缺失 |
| 调试器 | ✅ 完整REPL | — |
| 标准库 | ❌ | 全部5个模块未实现 |
| 包管理 | ❌ | 全部5个需求未实现 |

---

## 关键文档索引

| 内容 | 位置 |
|------|------|
| 需求总纲+验收标准 | `docs/14-Atomix-Rust-实现任务文档.md`（v0.3，2007行） |
| 指令集规范 | `docs/02-指令集规范.md` |
| 编译管线 | `docs/04-编译管线.md` |
| 运行时架构 | `docs/07-Runner.md` |
| ATXP通信协议 | `docs/05-通信协议.md` |
| 调试器设计 | `docs/12-debugger-设计.md` |
| 语法设计全集 | `docs/语法设计/`（12文件） |
| AEP提案 | `docs/AEP/`（6提案） |
| CLI规范 | `docs/10-命令行规范.md` |
| 配置设计 | `docs/09-配置设计.md` |
| SDK设计 | `docs/13-sdk-设计.md` |
