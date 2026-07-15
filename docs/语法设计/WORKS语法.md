# Atomix WORKS 语法

> 架构版本: v0.1 (设计阶段)
> 最后更新: 2026-07-15
> 所属体系: 语法设计
> 本层代号: Layer 5 — 可复用任务单元层

---

## 组合约束总表（本层规则）

| 组合 | 合法？ | 约束说明 |
|------|--------|----------|
| `WORKS : <区名> { ... }` | ✅ | 标准定义 |
| `WORKS : <区名> (<继承>) { ... }` | ✅ | 带继承 |
| `<类型> <变量>` 在 WORKS 顶层 | ✅ | 参数变量声明，默认公开 |
| `<类型> : fn <名>(<参数>) { ... }` | ✅ | 方法定义，默认私有 |
| `PUB : <方法名>, <方法名>` | ✅ | 批量暴露方法为公开 |
| `PUB : <变量> : <类型>` | ✅ | 暴露数据结构（字典/列表）内含方法引用 |
| `VOID_0` ~ `VOID_9` | ✅ | 无意义钩子基座 |
| `VOID_N*(NAME)` | ✅ | 无意义钩子命名派生 |
| `this.<变量>` | ✅ | 引用实例变量 |
| `return <变量>` | ✅ | 返回值 |
| `func()` 直接调 | ✅ | 纯函数调用 |
| `CALLIF func() IF : ISERROR as e {}` | ✅ | 调用错误捕获 |
| `=>` / `<=` | ✅ | 数据移动 |
| `if` / `for`（建议小写） | ✅ | 控制流 |
| `+` `-` `*` `/` `%` | ✅ | 算术运算 |
| `JOIN` | ✅ | 数据聚合 |
| `HTTP : "..."` | ❌ | I/O 关键字不可用 |
| `CALL : func()` | ❌ | CALL 语句不可用 |
| `WAIT` | ❌ | 任务派发不可用 |

---

## 1. 职责

WORKS 是 Atomix 的**可复用任务单元模板**。它定义了一个可实例化、可派发的计算单元。

**核心特性：**
- 一个文件可定义**多个** WORKS（区别于 INPUT/TASK/OUT 的单一性）
- 参数变量 = 实例变量，实例化时传入，**默认公开**
- 方法**默认私有**，通过 `PUB` 集中暴露
- 纯计算——无 I/O、无 CALL、无 WAIT
- 非对象——无 GC，编译期确定生命周期

**设计哲学：**

> 强迫你想明白数据的流向。I/O 必须在 INPUT 区预先加载完毕，WORKS 只做计算。你无法在 WORKS 里临时去拉个文件、发个请求——数据没准备齐，是你没想清楚，不是语言的错。

---

## 2. 区定义

### 2.1 语法模板

```
WORKS : <区名> (<继承>) {
    <参数变量>
    <方法>
}
```

| 段 | 必填 | 说明 |
|----|------|------|
| `WORKS` | ✅ | 区关键字 |
| `<区名>` | ✅ | WORKS 名称，标识符 |
| `(<继承>)` | ❌ | 继承的父 WORKS 名称 |
| `{ ... }` | ✅ | 体，包含参数变量和方法 |

### 2.2 示例

```
// 基础定义
WORKS : DataFetcher {
    string url
    dict headers
    int timeout
}

// 带继承
WORKS : ImageDownloader (DataFetcher) {
    string format
    int quality
}
```

---

## 3. 参数变量

C 风格声明。**默认公开**——你定义了参数，就已经做好了被人看/写的心理准备。

### 3.1 语法

```
<类型> <变量名>
```

### 3.2 公开语义

参数公开的含义：当外部通过 `WAIT` 实例化该 WORKS 后，调用者可以访问/写入这些参数。这不是"封装泄露"，而是 WORKS 的设计初衷——参数就是实例化时的输入接口，天然应该可见。

### 3.3 示例

```
WORKS : Processor {
    string input_path       // 公开参数，外部可读写
    dict config             // 公开参数
    int timeout             // 公开参数
}
```

### 3.4 实例化对应关系

参数按位置绑定到 `WAIT` 传入的参数：

```
// WORKS 定义
WORKS : Fetcher {
    string url
    string method
}

// 使用时
WAIT h = Fetcher("https://api.com", "GET") => result
//         url ↑                  method ↑
```

---

## 4. 方法

**默认私有**——方法是对外隐藏的实现细节，只有通过 `PUB` 声明的才对外可见。

### 4.1 语法模板

```
<返回类型> : fn <方法名> (<参数>) {
    <语句体>
}
```

| 段 | 必填 | 说明 |
|----|------|------|
| `<返回类型>` | ✅ | 返回值类型，`void` 表示无返回 |
| `fn` | ✅ | 方法关键字 |
| `<方法名>` | ✅ | 方法名 |
| `(<参数>)` | ❌ | 参数列表，每个参数为 `<类型> <变量名>` |
| `{ <语句体> }` | ✅ | 方法体 |

### 4.2 示例

```
WORKS : Calculator {
    int a
    int b

    // 私有方法
    int : fn add() {
        return this.a + this.b
    }

    // 私有方法
    void : fn validate() {
        if (this.a < 0 || this.b < 0) {
            // 校验逻辑
        }
    }
}
```

### 4.3 方法内可以使用的语法

| 语法 | 可用？ | 说明 |
|------|--------|------|
| `func()` 直接调 | ✅ | 纯函数调用 |
| `this.method()` | ✅ | 调本 WORKS 的其他方法 |
| `this.变量` | ✅ | 引用实例变量 |
| `return <变量>` | ✅ | 返回值 |
| `=>` / `<=` | ✅ | 数据移动 |
| `if` / `for`（建议小写） | ✅ | 控制流 |
| `+` `-` `*` `/` `%` | ✅ | 算术运算 |
| `JOIN` | ✅ | 数据聚合 |
| `int result = ...` | ✅ | 局部变量声明 |
| `CALLIF func() IF : ISERROR as e {}` | ✅ | 调用错误捕获 |
| `HTTP : "..."` | ❌ | I/O 关键字 |
| `CALL : func()` | ❌ | CALL 语句 |
| `WAIT` | ❌ | 任务派发 |
| `\/` 管道 | ❌ | TASK 专属 |

---

## 5. PUB — 公开声明

方法默认私有，外部不可见。通过 `PUB` 声明将方法暴露为公开接口。

### 5.1 直接暴露

```
PUB : <方法名>, <方法名>, ...
```

逐个列出要暴露的方法名：

```
WORKS : DataProcessor {
    string url
    dict config

    // 私有方法
    void : fn validate() {
        // 内部校验
    }

    dict : fn process() {
        this.validate()
        // 处理逻辑
        return result
    }

    void : fn cleanup() {
        // 清理资源
    }

    // 只暴露 process，validate 和 cleanup 对外不可见
    PUB : process
}
```

### 5.2 数据结构暴露

通过字典、列表等数据结构聚合方法引用，再将整个结构暴露：

```
PUB : <变量> : <类型>
```

```
WORKS : Router {
    void : fn route_a(string path) {
        // 路由 A
    }

    void : fn route_b(string path) {
        // 路由 B
    }

    void : fn route_c(string path) {
        // 路由 C
    }

    // 构建一个路由表（方法引用放进字典）
    dict routes = {
        "a": this.route_a,
        "b": this.route_b,
        "c": this.route_c
    }

    // 将整个路由表暴露
    PUB : routes : dict
}
```

### 5.3 两种形式的对比

| 形式 | 语法 | 适用场景 |
|------|------|----------|
| 直接暴露 | `PUB : func1, func2` | 简单、少量、固定的公开接口 |
| 数据结构暴露 | `PUB : dispatch_table : dict` | 动态路由、策略模式、批量派发 |

### 5.4 规则

- `PUB` 声明放在 WORKS 体末尾（参数和方法定义之后）
- 直接暴露的方法名必须在当前 WORKS 中已定义
- 数据结构暴露的变量必须在当前 WORKS 中已声明，且内容必须包含方法引用
- 未通过 `PUB` 声明的方法和变量，外部不可见

---

## 6. `this` 引用

`this` 显式引用当前实例的变量和方法。

| 写法 | 含义 |
|------|------|
| `this.xxx` | 引用参数变量 `xxx` |
| `this.method()` | 调用本 WORKS 的方法 `method` |

**规则：**
- `this` 不可省略
- `this` 只能在本 WORKS 的方法体内使用

```
WORKS : Counter {
    int count

    void : fn increment() {
        this.count = this.count + 1
    }

    int : fn get() {
        return this.count
    }
}
```

---

## 7. `return`

### 7.1 语法

```
return <变量>
```

### 7.2 行为

| 方法返回类型 | `return` 写法 | 说明 |
|-------------|--------------|------|
| `void` | 无 `return` 或 `return` | 无返回值 |
| 其他类型 | `return <变量>` | 返回变量值 |

### 7.3 示例

```
WORKS : MathOp {
    int a
    int b

    int : fn sum() {
        return this.a + this.b
    }

    void : fn log_sum() {
        int total = this.sum()
        // 不返回
    }
}
```

---

## 8. 钩子系统 — 声明钩子

声明钩子是 WORKS 的**通用扩展机制**。`::` 是钩子的核心运算符，连接"触发源"、"限定条件"、"执行代码"三部分。

### 8.1 五元模板

```
<触发钩子> :: <左限定> :: <代码> :: <右限定> :: <目标钩子>
```

| 段 | 必填 | 说明 |
|----|------|------|
| `<触发钩子>` | ✅ | 什么事件触发这个钩子（生命周期、错误、属性变化等） |
| `<条件判断>` | ❌ | 准入条件。通用条件表达式 + 钩子上下文谓词，真则执行中间代码 |
| `<代码>` | ✅ | 要执行的代码（可以为空） |
| `<条件判断>` | ❌ | 转向条件。同上，真则切换到目标钩子 |
| `<目标钩子>` | ✅ | 代码执行完毕后切换到哪个钩子/阶段 |

**核心规则：** 缺省的段视为"无限制/无动作"，语法依然合法。左右条件判断共用同一套语法体系。

**核心规则：** 缺省的段视为"无限制/无动作"，语法依然合法。

### 8.2 五种形态

缺省段从两端开始收缩。所有形态都是同一条五元模板的缺省简写：

#### 五元（完整）

```
ERROR :: is TypeError :: log() :: retry < 3 :: RETRY
```

#### 四元（缺一个端限定）

```
INIT :: validation_ok :: process() :: RUN
// 缺：右限定（无限制）
```

```
process() :: count < 10 :: loop() :: RUN
// 缺：左限定（无限制）
```

#### 三元（缺左右限定 = 夹心饼干）

```
<触发> :: <代码> :: <目标>
```

```
INIT :: if (ready) {} :: READY
```

> 这就是你最早看到的那个形式。夹心饼干合法，但小心自循环：`INIT :: if (条件) {} :: INIT`，从 INIT 进去执行完又切回 INIT，无限循环。工具给你了，组合出问题不归我管。

#### 二元（最常见）

```
<触发> :: <代码>
```

```
INIT :: load_config()
// 进入 INIT 阶段时执行 load_config()
// 目标钩子缺省：执行完后自然结束
```

```
<代码> :: <目标>
```

```
load_config() :: READY
// 执行 load_config() 后切换到 READY 阶段
```

#### 一元（不理解但尊重）

```
<钩子> :: <钩子>
```

```
INIT :: INIT
// 进入 INIT → 执行空代码 → 切换到 INIT → 又进入 INIT → 死循环
// 语法合法，语义是你的问题
```

#### 形态速查

| 元数 | 示例 | 说明 |
|------|------|------|
| 五元 | `A :: cond1 :: code :: cond2 :: B` | 完整规则 |
| 四元 | `A :: cond :: code :: B` / `A :: code :: cond :: B` | 缺一个限定 |
| 三元 | `A :: code :: B` | 夹心饼干，缺两个限定 |
| 二元 | `A :: code` / `code :: B` | 最常见形式 |
| 一元 | `A :: A` | 不理解但尊重 |

### 8.3 限定条件系统

限定条件就是**完整的条件判断表达式**——把 `if` 条件判断那一套全搬进来，再加上几个专门给钩子上下文用的特殊谓词。

**左右限定语义完全对称：** 左边是"准不准入"，右边是"转不转向"。两者共用同一套语法。

#### 通用条件表达式

| 类型 | 语法 | 示例 |
|------|------|------|
| 关系比较 | `<左值> <op> <右值>` | `retry < 3`、`count == 0`、`this.timeout > 30` |
| 相等/不等 | `==` / `!=` | `method == "process"`、`status != "done"` |
| 类型判断 | `is <Type>` | `is TypeError`、`is IOError` |
| 逻辑与 | `&&` | `retry < 3 && this.ready` |
| 逻辑或 | `\|\|` | `count == 0 \|\| this.force` |
| 逻辑非 | `!` | `!this.ready` |
| 分组 | `(...)` | `(retry < 3 && this.ready) \|\| force` |
| 字面量 | 直接值 | `42`、`"hello"`、`true` |
| 缺省 | 不写 | 无条件，直接通过 |

#### 钩子上下文特殊谓词 / 值

钩子触发时，上下文会携带额外信息。这些信息通过两类方式暴露：

**谓词（布尔值，可直接做条件）：**

| 谓词 | 说明 | 示例 |
|------|------|------|
| `ISCALL` | 当前是否由方法调用触发 | `ISCALL` |
| `ISNEVERCALL` | 该方法是否从未被调用过 | `ISNEVERCALL` |
| `ISSET` | 属性是否正在被写入 | `ISSET` |
| `ISGET` | 属性是否正在被读取 | `ISGET` |

**值（参与通用条件表达式比较）：**

| 值 | 类型 | 说明 | 示例 |
|----|------|------|------|
| `ISCALLNAME` | `string` | 被调用方法名 | `ISCALLNAME == "process"` |
| `ISCALLNUM` | `int` | 调用计数器 | `ISCALLNUM < 10`、`ISCALLNUM == 0` |
| `ISCALLER` | `string` | 调用者方法名（从哪里调过来的） | `ISCALLER != "dispatch"` |

**使用方式：**

```
// 谓词直接用在条件位置
ISCALL && ISNEVERCALL        // 是方法调用，且从未被调过
ISCALL && ISCALLNUM == 0     // 等价写法

// 值参与通用表达式比较
ISCALLNAME == "process"      // 被调方法名为 process
ISCALLNUM < 10               // 调用次数未达上限
ISCALLNUM > 0 && ISCALLNAME is name  // 至少调过一次，且方法名匹配变量 name

// 谓词 + 值 自由组合
ISCALL && ISCALLNUM < 3 && ISCALLNAME != "init"
```

> 特殊谓词/值与通用条件表达式完全兼容，可以任意组合使用 `&&` `||` `!`。

#### 五元语义流（完整）

```
<触发钩子> :: <条件判断> :: <代码> :: <条件判断> :: <目标钩子>
```

完整读法：

> **当 `<触发钩子>` 发生时，如果 `<条件判断>`（左）为真，则执行 `<代码>`；代码执行完后，如果 `<条件判断>`（右）为真，则切换到 `<目标钩子>`。**

```
INIT :: retry < 3 :: retry_connect() :: retry > 0 :: RETRY
```

"当 INIT 发生时，如果重试次数小于 3 则执行 retry_connect；执行完后如果重试次数仍大于 0，则切换到 RETRY 阶段"

```
ERROR :: is IOError && retry < 3 :: retry_connect() :: !this.ready :: RETRY
```

"当 ERROR 发生时，如果是 IOError 且重试未耗尽则执行重连；重连后如果实例还没就绪，则再次进入 RETRY"

#### 各场景示例

```
// ① 纯通用条件
BEFORE :: method == "save" && this.valid :: log_start()
SET :: this.timeout > 30 && this.timeout < 120 :: clamp_timeout()

// ② 特殊谓词
BEFORE :: ISFIRSTCALL :: init_buffer()
BEFORE :: ISNEVERCALL :: warmup_cache()

// ③ 谓词 + 条件组合
ERROR :: is IOError && ISFIRSTCALL :: retry_once() :: ISLASTCALL :: DEL

// ④ 左右限定都用
ERROR :: is AuthError :: refresh_token() :: ISFIRSTCALL :: RETRY
// 出错→如果是认证错误→刷新令牌→如果这是首次尝试→重试
```

### 8.4 钩子类型总表

#### 生命周期类

| 钩子 | 方向 | 说明 |
|------|------|------|
| `NEW` | 生命周期 | 实例上下文分配时触发 |
| `INIT` | 生命周期 | 初始化阶段，参数绑定 + 顶层代码执行 |
| `READY` | 生命周期 | 初始化完毕，实例就绪，可接受方法调用 |
| `RUN` | 生命周期 | 方法正在执行中 |
| `RET` | 生命周期 | 方法返回 |
| `WAIT` | 生命周期 | 等待子任务完成 |
| `SUSPEND` | 生命周期 | 实例被挂起/暂停 |
| `ERROR` | 生命周期 | 错误/异常触发 |
| `DEL` | 生命周期 | 实例析构/清理 |

> 完整生命周期的阶段图待补充（§8.6）。

#### 属性拦截类

| 钩子 | 方向 | 说明 |
|------|------|------|
| `SET` | 属性 | 实例参数/属性被赋值时触发 |
| `GET` | 属性 | 实例参数/属性被读取时触发 |

#### 方法拦截类

| 钩子 | 方向 | 说明 |
|------|------|------|
| `BEFORE` | 方法 | 方法被调用前触发 |
| `AFTER` | 方法 | 方法调用结束后触发 |

#### 继承类

| 钩子 | 方向 | 说明 |
|------|------|------|
| `ON_INHERIT` | 继承 | 继承关系建立时触发 |
| `ON_OVERRIDE` | 继承 | 方法被覆写时触发 |
| `BEFORE_PARENT` | 继承 | 父类方法执行前触发（子类中生效） |
| `AFTER_PARENT` | 继承 | 父类方法执行后触发（子类中生效） |

#### 数据流类

| 钩子 | 方向 | 说明 |
|------|------|------|
| `FLOW_IN` | 数据 | 数据流入实例时触发 |
| `FLOW_OUT` | 数据 | 数据流出实例时触发 |

#### 无意义钩子类

| 钩子 | 方向 | 说明 |
|------|------|------|
| `VOID_0` ~ `VOID_9` | 无意义 | 10 个无内置语义的钩子基座，供用户自定义使用 |
| `VOID_N*(NAME)` | 无意义 | 在基座上命名派生，无限扩展 |

> 无意义钩子没有任何内置行为。但它们拥有钩子的一切能力（触发、限定、代码、切换），语法行为与生命周期钩子完全一致。详见 §8.5。

### 8.5 有意义钩子示例

```
// 生命周期：初始化完成后切入 READY
INIT :: load_config() :: READY

// 生命周期：出错时记录日志再进入 DEL
ERROR :: is ConnectionError :: log_error() :: DEL

// 属性拦截：timeout 被赋值时校验合法性
SET :: this_key == "timeout" :: validate_timeout()

// 方法拦截：每次调用 process 前打日志
BEFORE :: method == "process" :: log("process starting")

// 数据流：数据流出时自动压缩
FLOW_OUT :: compress()

// 二元常见组合
INIT :: func()
func() :: READY
RUN :: process()
process() :: RET
ERROR :: cleanup() :: DEL
```

### 8.6 无意义钩子

无意义钩子没有任何内置行为——它不和生命周期、属性、方法、数据流中的任何一个绑定。但它拥有声明钩子的一切能力：可以触发、限定条件、执行代码、切换目标。

它的意义就是**给你用的，不是给我（语言设计者）用的**。

#### 基座系统

```
VOID_0    VOID_1    VOID_2    VOID_3    VOID_4
VOID_5    VOID_6    VOID_7    VOID_8    VOID_9
```

10 个基座，每个独立可随意使用。

#### 命名派生

```
VOID_N*(NAME)
```

在基座上通过 `*(NAME)` 派生命名实例。`NAME` 由用户自定义，任意合法标识符即可。

```
// 派生命名钩子
VOID_0*(beforeCompute)
VOID_0*(afterCompute)
VOID_1*(onChunkStart)
VOID_1*(onChunkEnd)

// 派生数量不限
VOID_0*(a)   VOID_0*(b)   VOID_0*(c)   ...  // 随你
```

组合方式：
- 10 个基座 × 无限命名 = 无限的无意义钩子
- 每个行为完全一致：空、无绑定、等你去填

#### 真实场景

用户完全可以用无意义钩子搭出自己的插件/事件体系：

```
WORKS : PluginEngine {
    VOID_0*(beforePlugin) :: validate(plugin) :: VOID_0*(pluginReady)
    VOID_0*(pluginReady)  :: plugin.exec()     :: VOID_0*(pluginDone)
    VOID_0*(pluginDone)   :: collect()          :: VOID_1*(allDone)
}
```

语法层面：全是无意义钩子。用户层面：这是一套完整的插件生命周期。

#### 与内置钩子的关系

| | 内置钩子（INIT / ERROR / SET …） | 无意义钩子（VOID_0 …） |
|--|-----------------------------------|------------------------|
| 内置行为 | ✅ 有特定触发条件 | ❌ 没有任何绑定 |
| 可被触发 | ✅ 引擎自动触发 | ✅ 可以手动触发 |
| 可被切换 | ✅ 可作为 `::` 目标 | ✅ 可作为 `::` 目标 |
| 可加限定 | ✅ | ✅ |
| 可执行代码 | ✅ | ✅ |

#### 小心自循环

五元模板中，`A :: code :: A` 会导致自循环触发（进 A → 执行 → 又进 A → 又执行……）。无意义钩子也不例外：

```
VOID_0 :: do_something() :: VOID_0   // 死循环
```

工具给你了，组合出问题不归我管。

### 8.7 生命周期阶段图

#### 设计定位

WORKS 的无 GC 实现借鉴了 Rust 的确定性销毁思想，但场景更受限因此方案更轻：

| 对比 | Rust | WORKS |
|------|------|-------|
| 生命周期标注 | 泛型 `'a`，程序员书写 | 编译器从阶段图推导 |
| 所有权转移 | `&` / `&mut` / move | `=>` 移动 / `=` 复制（语法级区分） |
| 销毁时机 | `Drop` trait + 作用域结束 | 编译器按阶段图调度 |
| 约束强度 | 全场景通用（指针、线程、容器） | 固定阶段图（无非循环引用） |

Rust 解决的是通用内存安全，WORKS 只需要管好"实例从生到死走一条固定的阶段图"——编译器在编译期就能完整分析。

#### 完整阶段图

```
                ┌──────────────────────────────────┐
                │  NEW                              │  实例上下文分配
                │    ↓                              │
                │  INIT                             │  参数绑定 + 顶层代码执行
                │    ↓                              │
                │  READY  ◄── ─── ─── ── ─┐        │  就绪，等待方法调用
                │   ↕                      │        │
                │  RUN → WAIT → RET ───────┘        │  方法执行子循环
                │    ↓                              │
                │  SUSPEND ◄── ── ─┐                │  暂停/挂起
                │    ↓            │                 │
                │  ERROR ─────────┤                 │  异常
                │    ↓            │                 │
                │  RETRY ─────────┤                 │  重试
                │    ↓            │                 │
                │  DEL                              │  析构回收
                └──────────────────────────────────┘
```

#### 合法阶段转换

| 当前阶段 | 可转换到 | 说明 |
|----------|----------|------|
| `NEW` | `INIT` | 分配完成，进入初始化 |
| `INIT` | `READY`, `ERROR` | 初始化后可就绪或出错 |
| `READY` | `RUN`, `SUSPEND`, `ERROR`, `DEL` | 就绪后可执行、挂起、出错或直接回收 |
| `RUN` | `RET`, `ERROR` | 方法执行中 |
| `RET` | `READY`, `ERROR` | 方法返回后回到就绪 |
| `WAIT` | `RUN`, `ERROR` | 等待结束后恢复执行 |
| `SUSPEND` | `READY`, `ERROR`, `DEL` | 挂起后可恢复、出错或回收 |
| `ERROR` | `RETRY`, `DEL` | 错误后可重试或析构 |
| `RETRY` | `INIT`, `READY` | 重试一般重走初始化或直接就绪 |
| `DEL` | （终点） | 回收完成，实例销毁 |

> 任何阶段图上不存在的转换均为非法，编译器会拒绝编译。

#### 确定性回收的实现

编译器在编译期遍历阶段图，按以下规则计算回收时机：

1. **实例进入 `DEL` 时** → 所有资源立即回收（包括参数变量、方法上下文）
2. **`SUSPEND` 状态** → 资源保留，编译器标记"可被回收，但暂不动"
3. **`ERROR` 状态** → 编译器检查是否有 `RETRY` 路径，有则保留、无则回收
4. **无引用实例** → 编译器静态分析出不再被引用的实例，直接注入 `→ DEL` 转换

```
// 编译器在 IR 层面看到的：
// ── 相当于每个 WORKS 实例的 .task 条目里附带一个"当前阶段"字段
// ── 调度器按阶段图推进，到 DEL 就回收
// ── 不需要 GC、不需要引用计数、不需要 tracing
```

#### Rust 借鉴摘要

| 从 Rust 借来的 | 怎么改的 | 为什么能更轻 |
|---------------|----------|-------------|
| 所有权移动（`move`） | `=>` 箭头运算符 | WORKS 内部无非循环引用 |
| Drop 的确定性 | `DEL` 阶段 + 阶段图 | 实例生命周期是固定轨道 |
| 借用检查 | 不需要 | WORKS 纯计算，无并发别名引用 |
| 生命周期标注 | 不需要 | 阶段图推导生命周期，无需程序员手动写 `'a` |

### 8.8 继承的实现

#### 基础规则

| 规则 | 说明 |
|------|------|
| **单继承** | 一个 WORKS 只能继承一个父 WORKS，不搞多继承 |
| **参数继承** | 子类继承父类的全部参数变量 |
| **方法继承** | 子类继承父类的所有方法（包括父类 `PUB` 暴露的方法）|
| **覆写** | 子类可覆写父类方法，同名方法覆盖父类 |
| **super** | 子类方法内可通过 `super.method()` 调用父类版本 |
| **钩子继承** | 父类的钩子声明全部继承到子类，子类可覆写 |

#### 钩子继承机制

父类的钩子在子类中自动生效。子类可覆写父类的钩子，覆写规则与普通方法相同：

```
// 父类
WORKS : BaseProcessor {
    string url

    INIT :: load_config() :: READY
    PUB : process
}

// 子类 —— 继承父类的钩子，可覆写
WORKS : CustomProcessor (BaseProcessor) {
    // 覆写父类的 INIT 行为：先调父类，再走自己的
    INIT :: super.load_config() :: load_custom_config() :: READY

    // 新增子类专属钩子
    AFTER_PARENT :: cleanup_temp()
}
```

#### 父子钩子的交互

用 `PARENT` / `CHILD` 关键字在钩子中引用父/子的生命周期阶段：

```
// 在子类中编排父子生命周期顺序
PARENT :: INIT :: CHILD :: INIT           // 父初始化完，子再初始化
CHILD :: READY :: PARENT :: READY         // 子就绪了，父才能就绪
CHILD :: ERROR :: PARENT :: ERROR         // 子出错，父也进错误处理
```

#### 继承类的专属钩子

| 钩子 | 触发时机 | 使用场景 |
|------|----------|----------|
| `ON_INHERIT` | 继承关系建立时 | 校验子类是否满足父类契约 |
| `ON_OVERRIDE` | 方法被覆写时 | 日志、权限检查 |
| `BEFORE_PARENT` | 父类方法执行前（子类上下文中） | 预处理、参数校验 |
| `AFTER_PARENT` | 父类方法执行后（子类上下文中） | 后处理、结果缓存 |

```
WORKS : Plugin (BasePlugin) {
    ON_INHERIT :: validate_interface()
    ON_OVERRIDE :: log_override()

    BEFORE_PARENT :: method == "run" :: prepare_env()
    AFTER_PARENT :: method == "run" :: collect_result()
}
```

#### 钩子继承的组合潜力

继承 + 钩子 + 无意义钩子，三者组合起来后，子类可以：

- 继承父类的完整生命周期
- 覆写其中任何一段行为
- 在父子之间插入自定义逻辑
- 通过 `VOID_N*(NAME)` 添加父类没想过的扩展点

```
// 父类框架 —— 框架作者定义
WORKS : FrameworkBase {
    INIT :: register_services() :: READY
    VOID_0*(onRequest) :: handle() :: VOID_0*(onResponse)
    PUB : run
}

// 插件作者 —— 继承框架，插自己的钩子
WORKS : MyPlugin (FrameworkBase) {
    ON_INHERIT :: check_version()

    // 在父类的请求/响应之间加一道自己的逻辑
    VOID_0*(onRequest) :: validate_token() :: VOID_1*(preprocess)
    VOID_1*(preprocess) :: business_logic() :: VOID_0*(onResponse)
}
```

框架作者不需要知道插件作者打算怎么用——父类的生命周期骨架固定，子类在骨架上自由搭钩子。

---

## 9. 异常体系

| 异常 | 触发场景 |
|------|----------|
| `WorksParamTypeError` | 参数类型不匹配 |
| `WorksMethodNotFound` | 调用的方法不存在 |
| `WorksReturnTypeError` | 返回类型与方法签名不匹配 |
| `WorksVisibilityError` | 外部访问私有成员 |

## 10. 异常传播

### 10.1 传播规则

WORKS 方法中的异常传播规则与 TOOLS 一致：未捕获的异常沿调用链向上传播，直到被 CALLIF 捕获或到达 TASK 调用边界。

```
TASK 区 WAIT 或 CALL
    └── WORKS 实例
            └── 方法 process()
                    └── 内部 helper() ← 此处 RAISE
                              │
                              ▼
                    未捕获 → 传播到 process()
                              │
                              ▼
                    未捕获 → 传播到 TASK
                              │
                              ▼
                    无 CALLIF → TASK 步骤失败
```

### 10.2 WORKS 方法中的 CALLIF

```
WORKS : Fetcher {
    string url

    dict : fn fetch_data() {
        // 在 WORKS 方法内捕获错误
        CALLIF : http_get(this.url) => data IF : ISERROR as e {
            CALL : log(f"fetch failed: {to_string(e)}")
            return default_data()
        }
        return data
    }
}
```

> 未在 WORKS 方法内捕获的异常，会传播到调用方（TASK 区的 `WAIT` 或 `CALL`）。

---

## 11. 设计边界

### 10.1 为什么 WORKS 里不能用 I/O 关键字？

I/O 是边界操作，只允许在 INPUT（数据进入）和 OUT（数据离开）发生。WORKS 处于数据流的中间层，职责是计算而非传输。强迫用户在 INPUT 区预先声明所有数据来源，使数据流在编译期完全可见。

### 10.2 为什么 WORKS 里不能用 CALL？

CALL 是 TASK 区的编排语句，引入的是宿主函数调用语义。WORKS 内部使用直接函数调用 `func()`，不需要 CASE 语句的产出模式（`=>`/`=`/`\/`/`OUT :`）。

### 10.3 为什么 WORKS 可以定义多个而 INPUT/TASK/OUT 不行？

INPUT/TASK/OUT 是文件的三元区结构，每个文件各一个，构成完整的数据流管道。WORKS 是可复用的任务模板，类似"类库"，需要支持多个定义以满足不同计算场景。

### 10.4 参数默认公开 vs 方法默认私有

参数是 WORKS 的输入接口，天然需要对外可见——调用者必须知道要传什么参数进来。方法是内部实现逻辑，默认隐藏，只通过 `PUB` 暴露必要的接口。这种设计鼓励清晰的接口边界：调用者只看到 `PUB` 声明的东西，内部实现可以自由修改不影响外部。
