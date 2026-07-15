# Atomix IO 语法

> 架构版本: v0.1 (设计阶段)
> 最后更新: 2026-07-15
> 所属体系: 语法设计
> 本层代号: Layer 2 — 数据流动层

---

## 组合约束总表（本层规则）

本层各段**顺序固定**，不接受变体排列。唯一的灵活点是箭头方向可逆。

```
<关键字> : <地址> <{嵌套块}> (配置) [装饰器] <箭头> <变量> : <类型>
<变量> : <类型> <箭头> <关键字> : <地址> <{嵌套块}> (配置) [装饰器]
```

| 组合 | 合法？ | 约束说明 |
|------|--------|----------|
| `关键字 : 地址 (配置)` | ✅ | 标准形式 |
| `关键字 : 地址 { 嵌套块 } (配置)` | ✅ | 嵌套块在配置前，顺序固定 |
| `关键字 : 地址 (配置) { 嵌套块 }` | ❌ | 不允许交换顺序 |
| `关键字 : 地址 => 变量 : 类型` | ✅ | 标准右流 |
| `变量 : 类型 <= 关键字 : 地址` | ✅ | 反向左流，完全等价 |
| `关键字 : 地址 => 变量 => 其他` | ❌ | 禁止链式箭头，每个声明一个箭头 |
| `关键字 : 地址 (配置) [装饰器]` | ✅ | 装饰器在配置后 |
| `{ 嵌套块 }` 独立出现 | ❌ | 嵌套块必须绑定到关键字声明 |

---

## 1. 职责

数据流动声明。IO 语法同时覆盖 INPUT（数据来源）和 OUT（数据交付）两个方向。

一个文件有且仅有一个 INPUT 区和一个 OUT 区。

- **INPUT 区**：声明数据来源，产出的变量是文件级常量，不可变
- **OUT 区**：引用 TASK 中 `OUT :` 声明的变量，执行数据交付，不创建新变量

---

---

## 1.5 三元区与编译期重排

Atomix 源码由 **INPUT / TASK / OUT** 三个逻辑区构成，详见 [`docs/编译行为.md`](../编译行为.md)。

> 本层只关心 INPUT 和 OUT 区的语法细节，三区的整体结构、数据流向、编译期重排规则均由编译器统一处理，参见《编译行为.md》。

---

## 2. 语法模板

### 2.1 标准形式

```
<关键字> : <地址> <{嵌套块}> (配置) [装饰器] <箭头> <变量> : <类型>
```

### 2.2 反向形式（完全等价）

```
<变量> : <类型> <箭头> <关键字> : <地址> <{嵌套块}> (配置) [装饰器]
```

### 2.3 段说明

| 段 | 必填 | 说明 |
|----|------|------|
| `<关键字>` | ✅ | 数据源 / 目标类型，见关键字总表 |
| `:<地址>` | ⚠️ | URL、文件路径、内存地址等，部分关键字不需要 |
| `<{嵌套块}>` | ❌ | 局部作用域，INPUT 和 OUT 均支持。块内为同层 IO 语法，变量块内有效，闭合后销毁 |
| `(配置)` | ❌ | 键值对参数，如 `method=POST` |
| `[装饰器]` | ❌ | 用户自定义函数。数据在源与目标之间流转时先经装饰器处理。INPUT 方向：源→[装饰器]→变量；OUT 方向：变量→[装饰器]→目标。不提供内置装饰器 |
| `<箭头>` | ✅ | `=>` 向右流 或 `<=` 向左流 |
| `<变量>` | ✅ | 变量名 |
| `:<类型>` | ❌ | 数据类型，省略则由编译器推导 |

### 2.4 示例

```
// INPUT: 数据流入
HTTP : "https://api.com/data" (method=GET) => RAW : bytes
JSON : "./config.json" { schema } => CFG : dict
RAW : bytes <= HTTP : "https://api.com/data"   // 等价反向
HTTP : "https://api.com/data" [decrypt] => DATA : bytes     // 带装饰器

// INPUT: 嵌套块
HTTP : "https://api.com/data" (method=GET) {
    JSON : "./header.json" => HEADER : dict
    JSON : "./body.json" => BODY : dict
    JOIN : HEADER, BODY => payload : dict
} [validate] => PACKET : bytes

// OUT: 数据流出
RAW => HTTP : "https://api.com/upload" (method=POST)
CFG => JSON : "/tmp/backup.json"
HTTP : "https://api.com/upload" (method=POST) <= RAW   // 等价反向
HEADER => MEM : 0x7ffd1000 (len=256)
DATA => HTTP : "https://api.com/submit" [encrypt]       // 带装饰器

// OUT: 嵌套块
PAYLOAD => HTTP : "https://api.com/upload" (method=POST) {
    JOIN : meta, body => packet : dict
} [compress]
```

---

## 3. JOIN — 数据聚合（INPUT 区）

```
JOIN : <来源1>, <来源2>, ... <箭头> <变量> : <类型>
```

默认输出类型为 **列表（list）**。可以通过类型标注尝试转换，不可转换则非法。

| 类型 | 输出行为 |
|------|----------|
| 不指定 / `: list` | `[val1, val2, ...]` 列表（默认） |
| `: dict` | `{ "var1": val1, "var2": val2 }`，K 为变量名（字符串），V 为数据 |
| `: string` | toString 拼接 |

**dict 特殊行为：**

- 来源有变量名 → K = 变量名（字符串），V = 对应数据
- 来源无变量名（裸地址/路径）→ K = 地址/路径自身

---

## 4. 嵌套块

### 4.1 递归模板

`{}` 内的内容与外部是同一模板，支持无限嵌套：

```
<关键字> : <地址> <{
    <关键字> : <地址> <{  ...  }> (配置) [装饰器] <箭头> <变量> : <类型>
    <关键字> : <地址> <{  ...  }> (配置) [装饰器] <箭头> <变量> : <类型>
}> (配置) [装饰器] <箭头> <变量> : <类型>
```

没有"第 N 层的特殊规则"，每层都是同一模板匹配。

### 4.2 数据规则

**① 顶层变量 = 文件级常量**

INPUT 区顶层声明的变量全局可见，全文件可引用。

```
HTTP : "url" => RAW : bytes
# RAW 是文件级常量，TASK 区和 OUT 区都能用
```

**② 进入 `{}` = 临时数据**

块内声明的变量只在本块内有效，闭合后全部销毁。必须通过 `JOIN` 显式聚合才能将数据传递到外层。

```
HTTP : "url" {
    JSON : "./h.json" => HEADER   # 临时的，出块销毁
    TXT : "./b.txt" => BODY       # 临时的，出块销毁
    JOIN : HEADER, BODY => payload : dict   # JOIN 传出
} => PACKET : bytes
# HEADER、BODY、payload 已销毁
# PACKET 持有 JOIN 的导出结果
```

**③ 嵌套层不继承外层块内变量**

每层 `{}` 是独立作用域，内层看不到外层块内变量，只看得到自己的声明和外层箭头传进来的数据。

```
HTTP : "url" {
    JSON : "./a.json" => A        # 当前层变量
    HTTP : "url2" {               # 内层嵌套
        JSON : "./b.json" => B    # 内层变量，A 不可见
        JOIN : B => inner : dict
    } => INNER                    # 内层导出
    JOIN : A, INNER => payload : dict   # 用 INNER，不是 B
} => PACKET
```

### 4.3 装饰器

```
INPUT:  源数据 → [装饰器] → => 变量
OUT:    变量 → [装饰器] → => 目标地址
```

多个装饰器可链式使用，执行顺序从左到右：

```
// INPUT：先解密再解压
HTTP : "url" [decrypt] [decompress] => DATA : bytes

// OUT：先压缩再加密  
DATA => HTTP : "url" [compress] [encrypt]
```

---

## 5. 异常体系（全部阻断）

| 类型 | 说明 |
|------|------|
| `IOError` | 基类 |
| `NetworkError` / `ConnectionError` / `DNSResolveError` / `TimeoutError` / `TLSHandshakeError` | 网络异常 |
| `FileError` / `FileNotFoundError` / `PermissionError` / `DiskFullError` | 文件异常 |
| `ParseError` | 格式解析失败 |
| `SerializeError` | 序列化失败 |
| `MemReadError` / `MemWriteError` | 内存异常 |
| `TaskRefError` / `TaskDeliverError` | TASK 引用 / 交付异常 |

> INPUT 区和 OUT 区共用同一套异常体系，方向仅影响异常触发场景。

---

## 6. 设计边界

### 6.1 为什么 ORDER 不在语法层约束？

三区（INPUT → TASK → OUT）的执行顺序由编译器重排保证，IO 语法层不关心文件中区的出现顺序。

### 6.2 嵌套块 vs 配置的顺序为什么锁死？

`<关键字> : <地址> <{嵌套块}> (配置)` 锁死的原因：
- 嵌套块是数据的作用域处理（变量声明、聚合转换）
- 配置是操作参数（如 method、timeout）
- 数据处理先于操作参数，符合直觉

### 6.3 箭头为什么不能链式？

每个声明只描述一次数据流动。链式箭头 `变量 A => 变量 B => 变量 C` 模糊了数据流的边界——每一段流动都应该是一个独立的声明。
