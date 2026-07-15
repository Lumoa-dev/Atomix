# 附录 C：钩子与 IS\* 值参考

> 配套文档: 详见 WORKS语法.md、通用语法.md

---

## 实现层级说明

钩子系统按实现层级分为三类，不同层级对应不同的实现复杂度和 VM 侵入性：

| 层级 | 说明 | 实现方式 | 数量 |
|------|------|----------|------|
| **P0（核心钩子）** | 生命周期关键节点，VM 原生支持 | VM 内联指令序列，固定触发点 | ~15 |
| **P1（常用钩子）** | 常用场景，编译器生成支持 | 编译器在 IR 中插入条件跳转链 | ~25 |
| **P2（扩展钩子）** | 高级/特殊场景，库层实现 | 由标准库或用户自定义逻辑触发 | ~13 |
| **IS\* 值** | 上下文只读变量 | 编译器映射到寄存器或栈偏移，零运行时开销 | 72 |

P1/P2 钩子不增加 VM 核心复杂度——它们在编译期展开为条件判断 + 跳转指令序列，VM 执行时不感知"钩子"概念。轻量原则约束的是 VM 二进制体积和运行时内存占用，非语法层功能数量。

## 一、生命周期钩子

钩子名全大写，无前缀。钩子链格式灵活（详见 WORKS 语法 §4.1）。

### 阶段 1：实例创建/初始化

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `DEFINE` | P2 | WORKS 模板被编译器加载时（类级） | — |
| `INHERIT` | P2 | 从父 WORKS 继承时 | `ISPARENT` |
| `NEW` | P1 | 实例创建、内存分配完成时 | `ISTASKID` |
| `INIT` | P0 | 属性初始化完毕时 | `ISTASKID` |
| `CONSTRUCT` | P1 | 整个构造完成、可被引用时 | `ISTASKID` |

### 阶段 2：执行准备

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `LOAD` | P1 | 调度器取出准备执行时 | `ISTASKID`、`ISWAITTIME` |
| `PREPARE` | P1 | 执行资源分配完成时 | `ISTASKID` |
| `READY` | P1 | 完全就绪、代码体即将执行前 | `ISTASKID`、`ISSTEPNAME` |
| `START` | P0 | 实例开始执行主代码体时 | `ISTASKID`、`ISSTARTTIME` |

### 阶段 3：运行

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `RUN` | P1 | 每轮主语句前 | `ISSTEPNAME`、`ISLINE` |
| `STEP` | P0 | 每个 CALL 形成的 Step 前 | `ISSTEPNAME`、`ISARGS`、`ISSTEPINDEX` |
| `STEP_AFTER` | P0 | 每个 Step 执行完毕后 | `ISSTEPNAME`、`ISRETURN`、`ISELAPSED` |
| `STEP_ERROR` | P1 | 某个 Step 抛出异常时 | `ISSTEPNAME`、`ISERROR` |
| `SUSPEND` | P2 | 实例被调度器挂起时 | `ISSUSPENDREASON`、`ISELAPSED` |
| `RESUME` | P2 | 实例从挂起恢复时 | `ISELAPSED` |
| `INTERRUPT` | P2 | 执行被外部中断时 | `ISINTERRUPTCODE`、`ISLINE` |

### 阶段 4：方法调用

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `CALL` | P0 | 方法被调用时（调用前） | `ISMETHOD`、`ISARGS`、`ISCALLER` |
| `CALL_AFTER` | P1 | 方法调用成功返回后 | `ISMETHOD`、`ISRETURN`、`ISELAPSED` |
| `CALL_ERROR` | P1 | 方法调用抛出异常时 | `ISMETHOD`、`ISARGS`、`ISERROR` |
| `CALL_RETURN` | P1 | return 语句执行时 | `ISMETHOD`、`ISRETURN` |
| `PUB_CALL` | P1 | 公开方法被外部调用时 | `ISMETHOD`、`ISARGS`、`ISCALLER` |
| `PRIV_CALL` | P2 | 私有方法被内部调用时 | `ISMETHOD`、`ISARGS` |

### 阶段 5：属性访问

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `GET` | P0 | 属性被读取前 | `ISPROPERTY`、`ISPROPTYPE` |
| `GET_AFTER` | P2 | 属性读取完成后 | `ISPROPERTY`、`ISPROPVALUE` |
| `GET_ERROR` | P2 | 属性读取出错时 | `ISPROPERTY`、`ISERROR` |
| `SET` | P0 | 属性被写入前 | `ISPROPERTY`、`ISPROPVALUE` |
| `SET_AFTER` | P2 | 属性写入完成后 | `ISPROPERTY` |
| `SET_ERROR` | P2 | 属性写入失败时 | `ISPROPERTY`、`ISERROR` |

### 阶段 6：子任务管理

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `FORK` | P0 | 派生子任务时 | `ISCHILDID`、`ISCHILDCOUNT` |
| `JOIN` | P0 | 等待子任务返回时 | `ISCHILDID`、`ISCHILDCOUNT` |
| `JOIN_AFTER` | P1 | 子任务完成、取回结果后 | `ISCHILDID`、`ISRETURN` |
| `CHILD_START` | P2 | 任意直接子任务开始执行时 | `ISCHILDID`、`ISCHILDNAME` |
| `CHILD_DONE` | P2 | 任意直接子任务完成时 | `ISCHILDID`、`ISCHILDRETURN` |
| `CHILD_ERROR` | P2 | 任意直接子任务出错时 | `ISCHILDID`、`ISERROR` |
| `CHILD_FORK` | P2 | 子任务又派生孙任务时 | `ISCHILDID`、`ISDEPTH` |
| `JOIN_ALL` | P1 | 等待所有子任务完成时 | `ISCHILDREN`、`ISCHILDCOUNT` |

### 阶段 7：管道/装饰器

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `PIPE_BEFORE` | P2 | 管道操作 `$` 数据流转前 | `ISPROPVALUE`、`ISPIPEINDEX` |
| `PIPE_AFTER` | P2 | 管道操作 `$` 数据流转后 | `ISPROPVALUE`、`ISPIPECOUNT` |
| `PIPE_ERROR` | P2 | 管道处理出错时 | `ISERROR`、`ISPIPEINDEX` |
| `PIPE_DONE` | P2 | 整条管道链完成时 | `ISPIPECOUNT`、`ISELAPSED` |
| `DECORATOR_BEFORE` | P2 | 装饰器处理数据前 | `ISDECORATORNAME`、`ISPROPVALUE` |
| `DECORATOR_AFTER` | P2 | 装饰器处理完成后 | `ISDECORATORNAME`、`ISELAPSED` |

### 阶段 8：完成

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `DONE` | P0 | 主代码体正常执行完成时 | `ISELAPSED`、`ISRETURN` |
| `ERROR` | P0 | 未捕获异常时 | `ISERROR`、`ISERRORTYPE`、`ISERRORSTACK` |
| `FINALLY` | P0 | 无论成功失败，DONE/ERROR 后触发 | `ISSTATUS`、`ISELAPSED` |
| `TIMEOUT` | P1 | 超过预设超时时 | `ISTIMEOUT`、`ISELAPSED` |
| `CANCEL` | P1 | 执行被外部取消时 | `ISCANCELREASON`、`ISELAPSED` |
| `HALT` | P1 | 遇到 TRAP 0 指令时 | `ISLINE` |
| `RETURN` | P1 | 通过 TASK_RET 返回结果时 | `ISRETURN`、`ISELAPSED` |

### 阶段 9：销毁/清理

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `DEL` | P0 | 实例即将销毁时 | `ISELAPSED` |
| `CLEANUP` | P1 | 资源释放过程中 | `ISCLEANUPTARGET` |
| `DISPOSE` | P2 | 内存即将回收时 | `ISELAPSED` |
| `UNLOAD` | P2 | 类定义被虚拟机卸载时（类级） | — |

### 特殊场景

| 钩子 | 层级 | 触发时机 | 可用 IS\* |
|------|------|----------|-----------|
| `CONFIG` | P1 | WAIT 参数覆盖发生时 | `ISPROPERTY`、`ISPROPVALUE`、`ISDEFAULT` |
| `RETRY` | P1 | 失败重试逻辑触发时 | `ISRETRYCOUNT`、`ISRETRYLIMIT`、`ISERROR` |
| `VALIDATE` | P1 | 数据经过装饰器 `[validate]` 时 | `ISPROPVALUE`、`ISWARNING` |

### 空钩子

```
VOID_0(<NAME>)  ~  VOID_9(<NAME>)
```

10 个预留空钩子，括号内指定扩展名，默认无行为。

---

## 二、IS\* 上下文值

### 异常相关

| IS\* | 含义 | 类型 |
|------|------|------|
| `ISERROR` | 当前异常对象 | 错误类型 |
| `ISERRORTYPE` | 异常类型标识 | 类型标识 |
| `ISERRORMESSAGE` | 异常消息 | str |
| `ISERRORSTACK` | 调用堆栈 | str |
| `ISERRORLINE` | 异常行号 | i32 |
| `ISERRORCODE` | 错误码 | i32 |
| `ISCHILDERROR` | 子任务异常对象 | 错误类型 |

### 时间相关

| IS\* | 含义 | 类型 |
|------|------|------|
| `ISTIMEOUT` | 超时设定/实际值 | duration |
| `ISELAPSED` | 已耗时 | duration |
| `ISSTARTTIME` | 开始时间戳 | 时间戳 |
| `ISREMAINING` | 剩余可用时间 | duration |
| `ISWAITTIME` | 队列等待时长 | duration |
| `ISQUEUETIME` | 入队时间戳 | 时间戳 |
| `ISSUSPENDTIME` | 挂起持续时间 | duration |
| `ISDURATION` | 操作耗时 | duration |
| `ISCURRENTTIME` | 当前系统时间 | 时间戳 |

### 计数相关

| IS\* | 含义 | 类型 |
|------|------|------|
| `ISCALLCOUNT` | 方法调用累计次数 | i32 |
| `ISRETRYCOUNT` | 已重试次数 | i32 |
| `ISRETRYLIMIT` | 最大重试次数 | i32 |
| `ISDEPTH` | 任务树嵌套深度 | i32 |
| `ISCHILDCOUNT` | 子任务数 | i32 |
| `ISPIPECOUNT` | 管道处理步数 | i32 |
| `ISPIPEINDEX` | 管道当前步序号 | i32 |
| `ISSTEPINDEX` | Step 序号 | i32 |
| `ISTOTALSTEPS` | 总 Step 数 | i32 |
| `ISITERATION` | FOR 循环迭代次数 | i32 |

### 调用上下文

| IS\* | 含义 | 类型 |
|------|------|------|
| `ISMETHOD` | 当前方法名 | str |
| `ISARGS` | 参数列表 | list |
| `ISPARAMS` | 具名字典参数 | dict |
| `ISRETURN` | 返回值 | 任意 |
| `ISCALLER` | 调用者标识 | str |
| `ISSTEPNAME` | 当前 Step 名 | str |
| `ISWORKNAME` | WORKS 模板名 | str |
| `ISSELF` | 实例引用 | 实例引用 |

### 属性上下文

| IS\* | 含义 | 类型 |
|------|------|------|
| `ISPROPERTY` | 属性名 | str |
| `ISPROPVALUE` | 属性值 | 任意 |
| `ISPROPTYPE` | 属性声明类型 | 类型标识 |
| `ISPROPACCESS` | 访问模式（read/write） | str |
| `ISDEFAULT` | 属性默认值 | 任意 |

### 任务上下文

| IS\* | 含义 | 类型 |
|------|------|------|
| `ISTASKID` | 当前任务 ID | u64 |
| `ISPHASE` | 执行阶段标识 | str |
| `ISSTATUS` | 任务状态 | 枚举 |
| `ISPARENT` | 父任务 ID | u64 |
| `ISCHILDID` | 子任务 ID | u64 |
| `ISCHILDREN` | 子任务 ID 列表 | list |
| `ISCHILDNAME` | 子任务模板名 | str |
| `ISCHILDRETURN` | 子任务返回值 | 任意 |
| `ISGRANDCHILDID` | 孙任务 ID | u64 |
| `ISTASKTREE` | 任务树概要 | 结构 |

### 数据相关

| IS\* | 含义 | 类型 |
|------|------|------|
| `ISBIGSIZE` | 数据大小/尺寸 | 尺寸量 |
| `ISWARNING` | 警告级别 | i32 |
| `ISDATASIZE` | 数据实际字节数 | i64 |
| `ISDATATYPE` | 运行时类型 | 类型标识 |
| `ISDATASTATE` | 处理状态 | str |
| `ISDATACHECKSUM` | 校验和 | str/i64 |

### 系统/环境

| IS\* | 含义 | 类型 |
|------|------|------|
| `ISLINE` | 当前源码行号 | i32 |
| `ISFILE` | 当前源文件路径 | str |
| `ISMODE` | 运行模式（dev/prod） | str |
| `ISDEBUG` | 调试模式是否启用 | bool |
| `ISENV` | 宿主环境信息 | dict |
| `ISVERSION` | 运行时版本 | str |
| `ISCANCELREASON` | 取消原因 | str |
| `ISINTERRUPTCODE` | 中断码 | i32 |
| `ISSUSPENDREASON` | 挂起原因 | str |
| `ISCLEANUPTARGET` | 清理资源名 | str |
| `ISDECORATORNAME` | 装饰器名 | str |
| `ISCONCURRENCYID` | 并发批次 ID | u64 |
| `ISQUOTA` | 并发额度信息 | dict |
