# Atomix TASK 语法

> 架构版本: v0.1 (设计阶段)
> 最后更新: 2026-07-15
> 所属体系: 语法设计
> 本层代号: Layer 3 — 编排逻辑层

---

## 组合约束总表（本层规则）

本层的灵活度最高，约束体现在**互斥规则**和**嵌套边界**上，而非书写顺序。

| 组合 | 合法？ | 说明 |
|------|--------|------|
| `CALL : func() => <变量>` | ✅ | 标准产出模式 |
| `CALL : func() = <变量>` | ✅ | 复制产出模式，与 `=>` 不冲突 |
| `CALL : func() \/` | ✅ | 管道模式，结果进 U |
| `CALL ... =>` + `CALL ... \/` 混用 | ❌ | `\/` 不能与 `=>`/`=`/`OUT :` 混用 |
| `CALLIF : func() => <变量> IF : <IS*> as <错误变量>` | ✅ | 调用 + 错误处理，条件**仅限** `IS*` 状态字 |
| `IF : (<普通布尔表达式>)` | ✅ | 独立 IF，条件**不可**使用 `IS*` 状态字 |
| CALL 参数混合使用 | ✅ | 位置参数在前，命名参数在后 |
| CALL 参数内嵌套 CALL 语句 | ❌ | 仅允许函数表达式 `func()`，不允许 `CALL : func()` |
| `IF` / `FOR` / `CALL` 嵌套 | ✅ | `{}` 体内嵌套深度不限 |
| FOR 体内声明 `OUT :` 变量 | ❌ | `OUT :` 只能在 TASK 顶层声明 |
| JOIN 使用 | ✅ | 多元聚合为一，目标变量必须已声明类型 |
| `WAIT <句柄> = <文件>() = <变量>` | ✅ | WAIT 可用（详见通用语法.md） |
| `WAIT <句柄> = <文件>() => <变量>` | ✅ | WAIT 可用（详见通用语法.md） |

---

## 1. 职责

编排核心。引用 INPUT 常量、调用函数、做条件/循环/聚合、声明产出。

一个文件有且仅有一个 TASK 区。

**限制：**
- 不定义函数、不声明入口
- 大小写不敏感
- 纯调用编排

---

## 2. CALL — 函数调用

### 2.1 语法

```
CALL : <函数>(<参数>) <产出模式>
CALL : <变量> = <函数>(<参数>) <产出模式>
```

### 2.2 参数传递

支持**位置参数**和**命名参数**，可混合使用。混合时位置参数在前，命名参数在后。

```
// 纯位置参数
CALL : fetch_data("https://api.com", 30, true) => raw

// 纯命名参数
CALL : fetch_data(url="https://api.com", timeout=30, ssl=true) => raw

// 混合（位置在前，命名在后）
CALL : fetch_data("https://api.com", timeout=30, ssl=true) => raw
```

### 2.3 函数嵌套

CALL 的参数中可以直接调用函数作为值表达式：

```
// 合法：func2() 是值表达式，直接作为参数
CALL : func1(func2(), func3(data)) => result
```

但不可在 CALL 的参数中嵌套另一个 CALL 语句：

```
// 不合法：CALL 语句不能出现在另一个 CALL 的参数中
CALL : func1(CALL : func2()) => result   // ❌
```

区别：`func2()` 是直接函数调用（值表达式），`CALL : func2()` 是一个完整的 CALL 语句。CALL 语句只在 TASK 区顶层出现。

### 2.4 四种产出模式

| 模式 | 写法 | 行为 |
|------|------|------|
| →变量（移动） | `=> <变量>` | CALL 结果所有权移动到变量，源不再持有 |
| →输出（移动） | `=> OUT : <变量>` | 移动至输出变量，对 OUT 区可见 |
| 变量=（复制） | `= <变量>` | CALL 结果复制到变量，双方均持有 |
| →管道（移动） | `\/` | 结果进隐式字典 `U`，自动传到下一条 CALL |

**注意：** `=>` 是移动语义，`=` 是复制语义，两者不同。

### 2.5 示例

```
CALL : parse_user(raw) => user_obj      // 移动
CALL : user_obj = parse_user(raw)        // 复制
CALL : transform(item) => OUT : result   // 移动至输出
CALL : fetch_profile(id) \/              // 管道
```

---

## 3. CALLIF — 调用 + 错误处理

CALL 调用失败时，用 CALLIF 捕获错误并处理。是 CALL + IF(IS*) 的组合关键字，替换了之前分离的 `CALL : func() IF : (...)` 写法。

### 3.1 语法

```
CALLIF : <函数>(<参数>) <产出模式> IF : <条件> as <错误变量> {
    <处理体>
}
```

| 段 | 必填 | 说明 |
|----|------|------|
| `CALLIF` | ✅ | 组合关键字，CALL + IF 结合 |
| `<产出模式>` | ✅ | 与 CALL 完全一致：`=>` `=` `\/` `=> OUT :` |
| `IF : <条件>` | ✅ | 条件**仅限** `IS*` 系列状态字 |
| `as <错误变量>` | ❌ | 将捕获的错误绑定到变量，可在处理体内引用 |
| `{ <处理体> }` | ✅ | 条件满足时的处理逻辑 |

IF 绑定到该次调用，检查其执行状态。条件基于本次 CALL 的结果判断，不依赖其他上下文。

### 3.2 条件语法

条件由三部分组成，均可选：

```
<状态字> [is <子类型>] [== <值>]
```

| 段 | 必填 | 示例 | 说明 |
|----|------|------|------|
| `<状态字>` | ✅ | `ISERROR` | 执行状态 |
| `is <子类型>` | ❌ | `is TypeError` | 错误/警告的具体类型 |
| `== <值>` | ❌ | `== 3ms` | 与阈值比较 |

### 3.3 条件形态

```
ISERROR                    仅判断是否出错
ISERROR is TypeError       是否出错且错误类型为 TypeError
ISTIMEOUT                  是否超时
ISTIMEOUT == 3ms           是否在 3ms 时超时
ISTIMEOUT == 30s           是否在 30s 时超时
ISBIGSIZE == 1024kb        数据大小是否达到阈值
ISBIGSIZE != 1mb           数据大小是否不等于阈值
```

### 3.4 完整示例

```
CALLIF : fetch_data(INPUT : URL) => raw IF : ISTIMEOUT == 30s as e {
    CALL : log("timeout after 30s: " + to_string(e))
    CALL : fetch_data(INPUT : BACKUP_URL) => raw
}

CALLIF : parse_user(raw) => user IF : ISERROR is TypeError as e {
    CALL : log("type mismatch: " + to_string(e))
    CALL : default_user() => user
}

CALLIF : process(data) => result IF : ISERROR as e {
    CALL : log("error: " + to_string(e))
    CALL : fallback(INPUT : DEFAULT) => OUT : result
}

CALLIF : fetch_profile(id) \/ IF : ISTIMEOUT {
    CALL : log("timeout, skip this profile")
}
```

---

## 4. IF — 条件分支（独立）

纯粹的布尔条件分支，与 CALL 无绑定关系。不可在此使用 `IS*` 状态字（状态字仅用于 CALLIF 的错误处理）。

### 4.1 语法

```
IF : (<条件>) { ... } ELSE : { ... }
```

### 4.2 条件

支持通用布尔表达式：

```
IF : (validated.count > 0) { ... }
IF : (status_code == 200) { ... } ELSE : { ... }
IF : (name != "") { ... }
```

### 4.3 示例

```
CALL : fetch_data(INPUT : URL) => raw

IF : (raw == null) {
    CALL : log("no data received")
    CALL : fetch_data(INPUT : BACKUP_URL) => raw
}

IF : (len(raw) > 1048576) {
    CALL : compress(raw) => OUT : result
} ELSE : {
    CALL : raw => OUT : result
}
```

---

## 5. FOR — 循环

### 5.1 语法

```
FOR : (<元素> in <集合变量>) {
    <语句体>
}
```

### 5.2 约束

- 迭代变量只在 `{}` 体内可见
- 体内可嵌 `CALL`/`IF`/`JOIN`
- 体内不可声明 `OUT :` 变量

---

## 6. JOIN — 数据聚合（TASK 区）

与 INPUT 区 JOIN 是同一语义：**多元聚合为一**。允许在 TASK 区使用，通常在循环 `FOR` 中逐次聚合。

### 6.1 语法

```
JOIN : <值表达式> => <变量> : <类型>
```

每次执行将值聚合到目标变量（追加到列表/合并到字典等）。目标变量必须已声明类型。

---

## 7. 隐式临时字典 U

由 `CALL ... \/` 管道模式自动填充，仅在下一条 CALL 前有效，之后覆盖。

```
CALL : fetch_profile(id) \/
// U = { profile: {...}, raw: bytes }

CALL : enrich(U[profile], INPUT : HOST) \/
// U = { enriched: {...} }
```

### 7.1 生命周期

1. `CALL ... \/` 执行后，返回值被拆解后填充 U
2. 下一条 CALL 可引用 `U[<键>]`
3. 再下一条 `CALL ... \/` 执行后 U 被覆盖

---

## 8. 执行日志

每个 `CALL` 被记录为一个执行 Step，包含函数名、入参、返回值、耗时、产出模式。管道模式可串联追踪。

---

## 9. 异常体系（全部阻断）

| 类型 | 说明 |
|------|------|
| `TaskCallError` | CALL 执行失败 |
| `TaskTypeError` | 类型不匹配 |
| `TaskConditionError` | 条件表达式异常 |
| `TaskUndefinedVar` | 未定义变量 |
| `TaskUndefinedFunc` | 未定义函数 |
| `TaskPipeBreak` | 管道链断裂（U 引用无效） |

---

## 10. 设计边界

### 10.1 为什么不在 TASK 区定义函数？

TASK 区的定位是"编排"而非"定义"。函数定义由宿主语言或 Atomix 标准库提供，TASK 区只做调用。

### 10.2 OUT : 为什么不能在 {} 内声明？

OUT 区需要静态知道有哪些产出变量。动态嵌套内的声明使编译器无法确定变量集合。
