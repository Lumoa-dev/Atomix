# Atomix TOOLS 语法

> 架构版本: v0.1 (设计阶段)
> 最后更新: 2026-07-15
> 所属体系: 语法设计
> 本层代号: Layer 6 — 工具函数层

---

## 组合约束总表（本层规则）

| 组合 | 合法？ | 约束说明 |
|------|--------|----------|
| `TOOLS { ... }` | ✅ | 标准定义 |
| `<类型> : fn <名>(<参数>) { ... }` | ✅ | 函数定义 |
| `GENERIC <T> <类型> : fn <名>(<参数>) { ... }` | ✅ | 泛型函数定义 |
| `GENERIC <T : Constraint> <类型> : fn <名>(<参数>) { ... }` | ✅ | 带约束的泛型 |
| `EXCEPTION <名>` | ✅ | 异常定义 |
| `EXCEPTION <名> : <父>` | ✅ | 带继承的异常 |
| `PUB : <函数名>, <函数名>` | ✅ | 暴露函数 |
| `CALLIF func() IF : ISERROR as e {}` | ✅ | 调用错误捕获 |
| `func()` 直接调 | ✅ | 纯函数调用 |
| `if` / `for`（建议小写） | ✅ | 控制流 |
| `+` `-` `*` `/` `%` | ✅ | 算术运算 |
| `return <变量>` | ✅ | 返回值 |
| `=>` / `<=` | ✅ | 数据移动 |
| `JOIN` | ✅ | 数据聚合 |
| `HTTP : "..."` | ❌ | I/O 关键字不可用 |
| `CALL : func()` | ❌ | CALL 语句不可用 |
| `WAIT` | ❌ | 任务派发不可用 |
| 生命周期钩子 `::` | ❌ | 无生命周期 |
| `this` | ❌ | 无实例概念 |

---

## 1. 职责

TOOLS 是 Atomix 的**工具函数箱子**。它定义的是纯计算函数，不涉及任何生命周期、状态、实例化。

**核心特性：**
- 一个文件有且仅有一个 TOOLS 区（与 INPUT/TASK/OUT 一样是单例区）
- 只包含函数定义 + 类型定义（泛型、异常等"脏活累活"）
- 无生命周期、无钩子、无 `this`、无继承
- 不可 I/O、不可 CALL、不可 WAIT
- 位置在文件头，早于各区的顺序，属于全局共享的工具层

**设计哲学：**

> TOOLS 是存放"脏活累活"的地方。泛型、异常类型、通用工具函数——这些不归 WORKS 管、不归 TASK 管、不归 INPUT/OUT 管的东西，都放在 TOOLS 里。它是整个文件的底层工具箱，所有区都可以引用它。

---

## 2. 位置

TOOLS 区在文件中的位置：**文件头，USE 之后、各逻辑区之前。**

```
// 文件头
USE helper.atx :: TOOLS

// TOOLS 区
TOOLS {
    // 工具函数……
}

// 各区
INPUT : { ... }
TASK : { ... }
WORKS : { ... }
OUT : { ... }
```

同一文件中，TOOLS 定义的所有函数对所有区可见（INPUT / TASK / WORKS / OUT）。

---

## 3. 函数定义

### 3.1 语法模板

```
<返回类型> : fn <函数名>(<参数>) {
    <语句体>
}
```

| 段 | 必填 | 说明 |
|----|------|------|
| `<返回类型>` | ✅ | 返回值类型，`void` 表示无返回 |
| `fn` | ✅ | 函数关键字 |
| `<函数名>` | ✅ | 函数名 |
| `(<参数>)` | ❌ | 参数列表，每个参数为 `<类型> <变量名>` |
| `{ <语句体> }` | ✅ | 函数体 |

### 3.2 示例

```
TOOLS {
    int : fn clamp(int value, int min, int max) {
        if (value < min) {
            return min
        }
        if (value > max) {
            return max
        }
        return value
    }

    string : fn format_timeout(int seconds) {
        return seconds + "s"
    }

    void : fn validate_url(string url) {
        if (url == "" || url == null) {
            // 校验不通过
        }
    }

    dict : fn merge_config(dict base, dict override) {
        // 合并两个配置字典
        return result
    }
}
```

### 3.3 函数内可以使用的语法

| 语法 | 可用？ | 说明 |
|------|--------|------|
| `func()` 直接调 | ✅ | 调本 TOOLS 或其他 TOOLS 的函数 |
| `CALLIF func() IF : ISERROR as e {}` | ✅ | 调用错误捕获 |
| `return <变量>` | ✅ | 返回值 |
| `=>` / `<=` | ✅ | 数据移动 |
| `if` / `for`（建议小写） | ✅ | 控制流 |
| `+` `-` `*` `/` `%` | ✅ | 算术运算 |
| `JOIN` | ✅ | 数据聚合 |
| `int result = ...` | ✅ | 局部变量声明 |
| `HTTP : "..."` | ❌ | I/O 关键字 |
| `CALL : func()` | ❌ | CALL 语句 |
| `WAIT` | ❌ | 任务派发 |
| `this` | ❌ | 无实例概念 |
| 钩子 `::` | ❌ | 无生命周期 |

### 3.4 访问控制

TOOLS 的函数默认当前文件可见。通过 `PUB` 暴露后可被外部文件 USE：

```
TOOLS {
    // 私有函数，仅当前文件可用
    void : fn internal_check(string input) {
        // 内部校验
    }

    // 公开函数，可被 USE file.atx :: TOOLS :: parse_csv 导入
    PUB : parse_csv, format_output

    dict : fn parse_csv(string raw) { ... }
    string : fn format_output(dict data) { ... }
}
```

> `PUB` 的语法与 WORKS 一致：`PUB : <函数名>, <函数名>` 直接暴露。

### 3.5 默认参数

参数列表中的参数可通过 `= <默认值>` 赋予默认值。调用时若省略该参数，则使用默认值。

**语法：**

```
<返回类型> : fn <函数名>(<类型> <参数1> = <默认值>, <类型> <参数2> = <默认值>, ...)
```

**规则：**

| 规则 | 说明 |
|------|------|
| 带默认值的参数必须位于参数列表末尾 | 位置参数在前，默认参数在后 |
| 默认值必须是编译期常量 | 不支持运行时表达式作为默认值 |
| 调用时可省略部分或全部默认参数 | 省略从末尾开始，不可跳着省略 |

**示例：**

```
TOOLS {
    string : fn connect(string host, int port = 8080, int timeout = 30) {
        // ...
    }

    // 调用方式
    connect("localhost")                    // port=8080, timeout=30
    connect("localhost", 9090)              // port=9090, timeout=30
    connect("localhost", 9090, 60)          // port=9090, timeout=60
    connect("localhost", timeout=60)        // ❌ 不可跳着省略 port
}
```

---

## 4. 泛型定义

TOOLS 是定义泛型的唯一位置。语法继承 Rust 风格，与 Atomix 底层实现语言一脉相承。

### 4.1 关键字

```
GENERIC
```

全大写，与 `USE` `WAIT` `CALL` `PUB` `JOIN` 风格一致。

### 4.2 语法

```
GENERIC <T> <返回类型> : fn <函数名>(<参数>) {
    <体>
}
```

| 段 | 必填 | 说明 |
|----|------|------|
| `GENERIC <T>` | ✅ | 泛型声明，`T` 为类型参数名，可多个 `<K, V>` |
| `<T : Constraint>` | ❌ | 约束声明，限制 `T` 必须满足某接口 |

### 4.3 示例

```
TOOLS {
    // 基础泛型
    GENERIC <T> T : fn identity(T value) {
        return value
    }

    // 多泛型参数
    GENERIC <K, V> V : fn get_or_default(dict[K, V] map, K key, V default) {
        if (map has key) {
            return map[key]
        }
        return default
    }

    // 带约束
    GENERIC <T : Comparable> T : fn max(T a, T b) {
        if (a > b) return a
        return b
    }

    GENERIC <T : Numeric> T : fn sum(list[T] items) {
        T total = 0
        for (item in items) {
            total = total + item
        }
        return total
    }

    // 多条约束（用 + 连接）
    GENERIC <T : Comparable + Hashable> bool : fn equals_and_hash(T a, T b) {
        return a == b
    }
}
```

### 4.4 内置约束

| 约束 | 说明 |
|------|------|
| `Comparable` | 支持 `==` `!=` `<` `>` `<=` `>=` 比较 |
| `Numeric` | 支持 `+` `-` `*` `/` `%` 算术运算 |
| `Hashable` | 可用作字典键 |
| `Iterable` | 可用 `for` 遍历 |

> 约束系统目前仅包含上述四种内置约束。自定义约束待后续扩展。

---

## 5. 异常类型定义

自定义异常类型也在 TOOLS 中定义。异常按树结构组织，与 Python 的异常体系一致。

### 5.1 关键字

```
EXCEPTION
```

### 5.2 语法

```
EXCEPTION <异常名>              // 定义新异常，父级为默认基类
EXCEPTION <异常名> : <父异常>   // 定义异常并指定父级
```

### 5.3 异常树结构

```
EXCEPTION Error                              // 基类
EXCEPTION TypeError : Error
EXCEPTION ValueError : Error
EXCEPTION IOError : Error
EXCEPTION     NetworkError : IOError
EXCEPTION     FileError : IOError
EXCEPTION         FileNotFoundError : FileError
EXCEPTION         PermissionError : FileError
EXCEPTION ParseError : Error
EXCEPTION SerializeError : Error
EXCEPTION ConfigError : Error
EXCEPTION     MissingField : ConfigError
EXCEPTION     InvalidValue : ConfigError
```

### 5.4 示例

```
TOOLS {
    // 定义异常树
    EXCEPTION AppError
    EXCEPTION ValidationError : AppError
    EXCEPTION DBError : AppError
    EXCEPTION     ConnectionTimeout : DBError
    EXCEPTION     QueryFailed : DBError

    // 在函数中使用
    void : fn validate_input(dict data) {
        if (!data has "name") {
            // 触发 ValidationError
        }
    }

    GENERIC <T : Comparable> bool : fn assert_range(T value, T min, T max) {
        if (value < min || value > max) {
            // 触发 ValidationError
        }
        return true
    }
}
```

### 5.5 与内置异常的关系

Atomix 运行时内置了一套异常体系（见各语法文档的"异常体系"节）。用户自定义异常可以继承内置异常，也可以从自己的根异常开始：

```
// 继承内置异常
EXCEPTION MyNetworkError : NetworkError        // 复用内置网络异常体系
EXCEPTION MyParseError : ParseError            // 复用内置解析异常体系

// 独立体系（从 Error 开始）
EXCEPTION DomainError : Error
EXCEPTION     BusinessRuleError : DomainError
```

内置异常基类：

| 异常 | 所属 | 说明 |
|------|------|------|
| `Error` | 全局 | 所有异常的根基类 |
| `IOError` | I/O | IO 操作异常基类 |
| `ParseError` | I/O | 数据解析异常 |
| `SerializeError` | I/O | 序列化异常 |
| `TaskCallError` | TASK | CALL 执行异常 |
| `TaskTypeError` | TASK | 类型不匹配 |
| `WorksParamTypeError` | WORKS | 参数类型异常 |
| `WorksVisibilityError` | WORKS | 访问控制异常 |

---

## 6. 与 WORKS 的对比

| 维度 | WORKS | TOOLS |
|------|-------|-------|
| 定位 | 可实例化的任务模板 | 纯函数工具箱子 |
| 数量 | 多个 | 一个 |
| 生命周期 | ✅ 完整阶段图 | ❌ 无 |
| 钩子系统 | ✅ 五元模板 + 无限派生 | ❌ |
| 继承 | ✅ 单继承 | ❌ |
| `this` | ✅ | ❌ |
| `PUB` | ✅ 方法暴露 | ✅ 函数暴露 |
| 泛型定义 | ❌ | ✅ |
| 异常定义 | ❌ | ✅ |
| 函数 | 方法，绑定实例 | 纯函数，无状态 |

---

## 7. USE 与 TOOLS 的配合

外部文件通过 USE 引用 TOOLS：

```
// 文件头
USE util.atx :: TOOLS                          // 引入全部
USE util.atx :: TOOLS :: parse_csv             // 引入特定函数
USE util.atx :: TOOLS :: parse_csv, format     // 引入多个

TASK :
    CALL : parse_csv(INPUT : RAW) => data
    CALL : format(data) => output
```

详见《通用语法.md》第 6 节 USE。

---

## 8. 异常体系

| 异常 | 触发场景 |
|------|----------|
| `ToolsFuncNotFound` | 调用的 TOOLS 函数不存在 |
| `ToolsTypeError` | 函数参数类型不匹配 |

## 9. 异常传播

### 9.1 传播规则

TOOLS 函数中抛出的异常（通过 `RAISE` 或由被调用函数传播上来的），若不被 `CALLIF` 捕获，则**沿着调用链向上传播**：

```
TASK 区 CALL
    └── TOOLS 函数 foo()
            └── TOOLS 函数 bar()  ← 此处 RAISE
                      │
                      ▼
          未捕获 → 传播到 foo()
                      │
                      ▼
          未捕获 → 传播到 TASK CALL
                      │
                      ▼
          无 CALLIF → TASK 步骤失败 → 任务标记为失败
```

| 层级 | 可捕获 | 未捕获则 |
|------|--------|----------|
| TOOLS 函数内部 | `CALLIF func() IF : ISERROR as e {}` | 传播到调用者 |
| TOOLS 函数之间 | 同上（每个调用点独立决定） | 沿调用链向上传播 |
| TASK 区 CALL | `CALLIF : func() ... IF : ISERROR as e {}` | 该 TASK 步骤失败，任务进入 ERROR 状态 |

### 9.2 CALLIF 的链式传播

被 CALLIF 捕获的异常不会自动传播——CALLIF 是"阻断"的。CALLIF 体内的处理体执行完毕后，TASK 或函数继续执行后续逻辑。

```
CALLIF : fetch(INPUT : URL) => data IF : ISERROR as e {
    // 异常在此被捕获，不会继续传播
    CALL : log("fetch failed: " + to_string(e))
    CALL : fetch(INPUT : BACKUP_URL) => data    // 用后备数据
}
// 继续正常执行 —— 错误已被处理，不传播
```

### 9.3 RAISE 的语义

`RAISE` 在 TOOLS/WORKS 中是一个**终止当前执行流并向上传播错误**的操作：

| 方面 | 行为 |
|------|------|
| 执行流 | RAISE 之后的代码**不执行** |
| 返回值 | 函数不返回，返回值不存在 |
| 传播方向 | 沿调用链向上，直到被 CALLIF 捕获或到达任务边界 |
| 任务边界 | 传播到 TASK 区 CALL 时，若该 CALL 无 CALLIF 包裹，任务失败 |

```
void : fn validate(dict data) {
    if (!data has "name") {
        RAISE ValidationError("missing name")
        // 这行不会执行
    }
    // 如果 RAISE 了，这里也不会执行
}
```

---

## 10. 设计边界

### 9.1 为什么 TOOLS 只能有一个？

TOOLS 是文件的全局工具层——所有函数、泛型、异常定义集中管理。多个 TOOLS 区会造成定义分散，增加复杂度。需要分组的工具应放入不同文件中，通过 USE 按需引入。

### 9.2 为什么 TOOLS 没有生命周期？

TOOLS 的函数不绑定任何实例状态——调了就执行，执行完就结束。生命周期只对有状态的任务单元（WORKS）有意义。

### 9.3 为什么泛型和异常定义要放 TOOLS？

泛型和异常是全局性定义，需要被文件内所有区引用。TOOLS 作为文件头部的全局层，是存放这类跨区定义的自然位置。放在任何其他区（INPUT/TASK/WORKS/OUT）都会造成循环依赖或可见性问题。
