# 附录 C：钩子与 IS\* 值参考

> 配套文档: [WORKS语法.md](../WORKS语法.md)、[通用语法.md](../通用语法.md)

---

## 一、生命周期钩子

钩子名全大写，无前缀。钩子链格式灵活（详见 WORKS 语法 §4.1）。

### 阶段 1：实例创建/初始化

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `DEFINE` | WORKS 模板被编译器加载时（类级） | — |
| `INHERIT` | 从父 WORKS 继承时 | `ISPARENT` |
| `NEW` | 实例创建、内存分配完成时 | `ISTASKID` |
| `INIT` | 属性初始化完毕时 | `ISTASKID` |
| `CONSTRUCT` | 整个构造完成、可被引用时 | `ISTASKID` |

### 阶段 2：执行准备

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `LOAD` | 调度器取出准备执行时 | `ISTASKID`、`ISWAITTIME` |
| `PREPARE` | 执行资源分配完成时 | `ISTASKID` |
| `READY` | 完全就绪、代码体即将执行前 | `ISTASKID`、`ISSTEPNAME` |
| `START` | 实例开始执行主代码体时 | `ISTASKID`、`ISSTARTTIME` |

### 阶段 3：运行

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `RUN` | 每轮主语句前 | `ISSTEPNAME`、`ISLINE` |
| `STEP` | 每个 CALL 形成的 Step 前 | `ISSTEPNAME`、`ISARGS`、`ISSTEPINDEX` |
| `STEP_AFTER` | 每个 Step 执行完毕后 | `ISSTEPNAME`、`ISRETURN`、`ISELAPSED` |
| `STEP_ERROR` | 某个 Step 抛出异常时 | `ISSTEPNAME`、`ISERROR` |
| `SUSPEND` | 实例被调度器挂起时 | `ISSUSPENDREASON`、`ISELAPSED` |
| `RESUME` | 实例从挂起恢复时 | `ISELAPSED` |
| `INTERRUPT` | 执行被外部中断时 | `ISINTERRUPTCODE`、`ISLINE` |

### 阶段 4：方法调用

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `CALL` | 方法被调用时（调用前） | `ISMETHOD`、`ISARGS`、`ISCALLER` |
| `CALL_AFTER` | 方法调用成功返回后 | `ISMETHOD`、`ISRETURN`、`ISELAPSED` |
| `CALL_ERROR` | 方法调用抛出异常时 | `ISMETHOD`、`ISARGS`、`ISERROR` |
| `CALL_RETURN` | return 语句执行时 | `ISMETHOD`、`ISRETURN` |
| `PUB_CALL` | 公开方法被外部调用时 | `ISMETHOD`、`ISARGS`、`ISCALLER` |
| `PRIV_CALL` | 私有方法被内部调用时 | `ISMETHOD`、`ISARGS` |

### 阶段 5：属性访问

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `GET` | 属性被读取前 | `ISPROPERTY`、`ISPROPTYPE` |
| `GET_AFTER` | 属性读取完成后 | `ISPROPERTY`、`ISPROPVALUE` |
| `GET_ERROR` | 属性读取出错时 | `ISPROPERTY`、`ISERROR` |
| `SET` | 属性被写入前 | `ISPROPERTY`、`ISPROPVALUE` |
| `SET_AFTER` | 属性写入完成后 | `ISPROPERTY` |
| `SET_ERROR` | 属性写入失败时 | `ISPROPERTY`、`ISERROR` |

### 阶段 6：子任务管理

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `FORK` | 派生子任务时 | `ISCHILDID`、`ISCHILDCOUNT` |
| `JOIN` | 等待子任务返回时 | `ISCHILDID`、`ISCHILDCOUNT` |
| `JOIN_AFTER` | 子任务完成、取回结果后 | `ISCHILDID`、`ISRETURN` |
| `CHILD_START` | 任意直接子任务开始执行时 | `ISCHILDID`、`ISCHILDNAME` |
| `CHILD_DONE` | 任意直接子任务完成时 | `ISCHILDID`、`ISCHILDRETURN` |
| `CHILD_ERROR` | 任意直接子任务出错时 | `ISCHILDID`、`ISERROR` |
| `CHILD_FORK` | 子任务又派生孙任务时 | `ISCHILDID`、`ISDEPTH` |
| `JOIN_ALL` | 等待所有子任务完成时 | `ISCHILDREN`、`ISCHILDCOUNT` |

### 阶段 7：管道/装饰器

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `PIPE_BEFORE` | 管道操作 `$` 数据流转前 | `ISPROPVALUE`、`ISPIPEINDEX` |
| `PIPE_AFTER` | 管道操作 `$` 数据流转后 | `ISPROPVALUE`、`ISPIPECOUNT` |
| `PIPE_ERROR` | 管道处理出错时 | `ISERROR`、`ISPIPEINDEX` |
| `PIPE_DONE` | 整条管道链完成时 | `ISPIPECOUNT`、`ISELAPSED` |
| `DECORATOR_BEFORE` | 装饰器处理数据前 | `ISDECORATORNAME`、`ISPROPVALUE` |
| `DECORATOR_AFTER` | 装饰器处理完成后 | `ISDECORATORNAME`、`ISELAPSED` |

### 阶段 8：完成

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `DONE` | 主代码体正常执行完成时 | `ISELAPSED`、`ISRETURN` |
| `ERROR` | 未捕获异常时 | `ISERROR`、`ISERRORTYPE`、`ISERRORSTACK` |
| `FINALLY` | 无论成功失败，DONE/ERROR 后触发 | `ISSTATUS`、`ISELAPSED` |
| `TIMEOUT` | 超过预设超时时 | `ISTIMEOUT`、`ISELAPSED` |
| `CANCEL` | 执行被外部取消时 | `ISCANCELREASON`、`ISELAPSED` |
| `HALT` | 遇到 TRAP 0 指令时 | `ISLINE` |
| `RETURN` | 通过 TASK_RET 返回结果时 | `ISRETURN`、`ISELAPSED` |

### 阶段 9：销毁/清理

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `DEL` | 实例即将销毁时 | `ISELAPSED` |
| `CLEANUP` | 资源释放过程中 | `ISCLEANUPTARGET` |
| `DISPOSE` | 内存即将回收时 | `ISELAPSED` |
| `UNLOAD` | 类定义被虚拟机卸载时（类级） | — |

### 特殊场景

| 钩子 | 触发时机 | 可用 IS\* |
|------|----------|-----------|
| `CONFIG` | WAIT 参数覆盖发生时 | `ISPROPERTY`、`ISPROPVALUE`、`ISDEFAULT` |
| `RETRY` | 失败重试逻辑触发时 | `ISRETRYCOUNT`、`ISRETRYLIMIT`、`ISERROR` |
| `VALIDATE` | 数据经过装饰器 `[validate]` 时 | `ISPROPVALUE`、`ISWARNING` |

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
