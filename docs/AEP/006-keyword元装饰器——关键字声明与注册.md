# AEP-006: `#[keyword]` 元装饰器——关键字声明与注册
Atomix Enhancement Proposal

| 字段 | 内容 |
|------|------|
| **状态** | Draft |
| **优先级** | P1 |
| **关联文档** | AEP-005 装饰器系统增强, 通用语法.md, INPUT语法.md, OUT语法.md, 关键字参考.md, token.rs |
| **提出日期** | 2026-07-20 |

## 1. 动机

当前 ATX 的源数据关键字（即 INPUT/OUT 区使用的 `JSON`、`CSV`、`HTTP` 等）是**硬编码在编译器中的**——`token.rs` 的 `match_keyword()` 和 `is_source_target_keyword()` 维护一个固定集合，语法设计文档明确声明"语法核心不再新增数据源关键词"。

这个封闭设计在实际使用中带来三个问题：

**问题一：标准库的格式解析无法获得关键字体验。**

标准库实现了 `json_parse`、`msgpack_decode`、`csv_parse` 等函数，用户只能在 `FILES : "f" [json_parse]` 的装饰器形式中使用它们，无法获得 `JSON : "f"` 这种关键字级别的语法体验。

**问题二：装饰器叠加的认知负担。**

```
FILES : "data.bin" [gunzip] [msgpack_decode] [validate_schema] => data
```

每多一个装饰器就是一层黑盒。对于不熟悉管道细节的读者，这条声明难以快速理解。

**问题三：封闭关键字集与标准库发展之间的矛盾。**

随着标准库增长（protobuf、avro、各类数据库驱动），要么不断修改编译器内核来注册新关键字，要么一直忍受"标准库有函数但用不了关键字语法"的落差。

本提案通过引入 `#[keyword]` 元装饰器，将关键字声明权从编译器下放到标准库和用户代码，同时保持语言核心的稳定。

## 2. 设计

### 2.0 元装饰器与装饰器的区分

| 语法 | 名称 | 生效时机 | 职责 |
|------|------|---------|------|
| `[name]` | 装饰器 | 运行时 | 数据变换、包装行为 |
| `#[name]` | 元装饰器 | 编译期 | 注册信息、生成代码、修改 AST |

元装饰器一律以 `#` 开头，出现在声明顶部，作用于紧跟其后的声明。

### 2.1 `#[keyword]` 元装饰器语法

```
#[keyword(
    kind  = "source" | "decl" | ...,  // 关键字种类（必填）
    scope = [INPUT] | [OUT] | [INPUT, OUT],  // 适用范围（必填）
    base  = FILES | WEBS | MEMS       // 传输层（仅 kind="source" 时需要）
)]
```

**参数详解：**

| 参数 | 必填 | 可选值 | 说明 |
|------|------|--------|------|
| `kind` | 是 | `"source"`（当前），未来可扩展 `"decl"` 等 | 关键字的语法角色 |
| `scope` | 是 | 区域名列表 | 该关键字可以在哪些区域中出现 |
| `base` | kind="source" 时必填 | `FILES` `WEBS` `MEMS` | 指定该关键字的底层传输协议 |

### 2.2 示例：声明一个源关键字

```
TOOLS : {
    #[keyword(kind="source", scope=[INPUT, OUT], base=FILES)]
    fn JSON(args, kwargs) : dict {
        CALL json_parse(args[0])
    }
}
```

`#[keyword]` 元装饰器在编译期执行：
1. 将 `JSON` 注册到关键字表中，关联传输层 `FILES`
2. 记录 `scope = [INPUT, OUT]`——编译器在 OUT 区见到 `JSON` 不会报错，在 TOOLS 区见到则报错
3. 函数 `JSON` 本身保留为一个普通 TOOLS 函数，可被其他代码直接调用

### 2.3 关键字展开规则

当用户在 INPUT/OUT 中使用已注册的关键字时，编译器将其展开为传输层 + 装饰器链：

```
INPUT : {
    JSON : "config.json" => config : dict
}

// 展开为：
// FILES : "config.json" [JSON] => config : dict
// 其中 [JSON] 调用上面声明的 JSON 函数
```

如果用户追加额外装饰器：

```
INPUT : {
    JSON : "config.json" [decrypt] => config : dict
}

// 展开为：
// FILES : "config.json" [JSON] [decrypt] => config : dict
// 关键字内部装饰器（这里没有）在左，外部追加的装饰器在右
```

数据流顺序：`FILES → [JSON] → [decrypt] → config`

### 2.4 通过 USE 模块注册关键字

标准库模块可以携带 `#[keyword]` 声明：

```
// std/json.atx —— 标准库模块
TOOLS : {
    #[keyword(kind="source", scope=[INPUT, OUT], base=FILES)]
    fn JSON(args, kwargs) : dict {
        CALL json_parse(args[0])
    }
}
```

用户使用时：

```
USE : "json"    // 加载 json 模块，自动注册 JSON 关键字

INPUT : {
    JSON : "config.json" => config : dict    // 直接用
}
```

`USE : "json"` 除了导入函数，还将模块内所有 `#[keyword]` 声明注册到编译器的关键字表。

**命名冲突规则：**
- 如果用户代码中已有同名关键字，`USE` 导入的关键字**隐藏**（不覆盖，但用户代码需通过模块前缀 `USE json::JSON` 区分）
- 同一模块内的多个 `#[keyword]` 声明之间不允许同名

### 2.5 关键字表（编译器内部变更）

当前 `is_source_target_keyword()` 是硬编码匹配：

```rust
pub fn is_source_target_keyword(&self) -> bool {
    matches!(
        self,
        TokenKind::Webs | TokenKind::Files | TokenKind::Mems
            | TokenKind::Http | TokenKind::Tcp | TokenKind::Db | TokenKind::Oss
            | TokenKind::Txt | TokenKind::Csv | TokenKind::Json | ...
    )
}
```

变更为动态查询：

```rust
pub fn is_source_target_keyword(tok: &TokenKind, symbol_table: &SymbolTable) -> bool {
    // 1. 检查硬编码的三个基础关键字（FILES、WEBS、MEMS）
    if matches!(tok, TokenKind::Files | TokenKind::Webs | TokenKind::Mems) {
        return true;
    }
    // 2. 检查注册表中的关键字
    if let Some(name) = tok.as_ident() {
        if symbol_table.is_keyword_registered(name, "source") {
            return true;
        }
    }
    false
}
```

三个基础关键字（`FILES` `WEBS` `MEMS`）始终硬编码，不参与注册机制。

### 2.6 基础关键字与注册关键字的关系

```
// 三个基础关键字——硬编码，不可覆盖
FILES : "f" => data      // 裸文件读取
WEBS : "addr" => data    // 裸网络读取
MEMS : "0x..." => data   // 裸内存读取

// 注册关键字——到基础关键字的映射
JSON : "f" => data       // 等价于 FILES : "f" [JSON] => data
MSG : "addr" => data     // 等价于 WEBS : "addr" [MSG] => data
```

注册关键字的 `base` 参数决定了它映射到哪个基础关键字。

### 2.7 作用域验证

编译器在语义分析阶段检查关键字使用位置是否在 `scope` 允许范围内：

```
#[keyword(kind="source", scope=[INPUT], base=FILES)]
fn SCANNER(args, kwargs) : image { ... }

OUT : {
    // ❌ 编译错误：SCANNER 不能在 OUT 区使用（scope 仅为 [INPUT]）
    result => SCANNER : "/dev/out"
}
```

### 2.8 关键字与装饰器叠加的完整模型

整个系统的统一的视图：

```
INPUT : {
    // 以下写法都是合法的，并且表达同一个逻辑模型
    //
    // 形式 A：纯关键字（最简洁）
    MSGPACK : "data.bin" => data
    //
    // 形式 B：关键字 + 装饰器（关键字展开后再追加装饰器）
    MSGPACK : "data.bin" [decrypt] => data
    //
    // 形式 C：完全展开（极客模式）
    FILES : "data.bin" [gunzip] [msgpack_decode] [decrypt] => data
}

// 如果 MSGPACK 定义为：
// #[keyword(kind="source", scope=[INPUT, OUT], base=FILES)]
// fn MSGPACK(args, kwargs) : dict {
//     CALL gunzip(args[0]) => raw
//     CALL msgpack_decode(raw) => result
// }
//
// 那么：形式 B 展开为 FILES : "f" [gunzip] [msgpack_decode] [decrypt] => data
//       形式 A 展开为 FILES : "f" [gunzip] [msgpack_decode] => data
```

## 3. 示例

### 3.1 标准库模块声明关键字

```atx
// std/msgpack.atx
TOOLS : {
    #[keyword(kind="source", scope=[INPUT, OUT], base=FILES)]
    fn MSGPACK(args, kwargs) : dict {
        CALL uncompress(args[0]) => raw
        CALL unpack_msgpack(raw) => result
    }
}
```

### 3.2 用户使用

```atx
USE : "msgpack"

INPUT : {
    // 简洁形式
    MSGPACK : "data.msgpack" => raw_data : dict

    // 带额外处理
    MSGPACK : "data.msgpack" [decrypt] => secure_data : dict
}

TASK : {
    // 函数名也可直接用
    CALL MSGPACK(raw_bytes) => result
}
```

### 3.3 声明专用于 OUT 的关键字

```atx
TOOLS : {
    #[keyword(kind="source", scope=[OUT], base=WEBS)]
    fn REMOTE_LOG(args, kwargs) : status {
        CALL send_log(args[0], kwargs.endpoint)
    }
}

OUT : {
    log_data => REMOTE_LOG(endpoint="https://log.example.com") : "https://log.example.com/api"
}
```

### 3.4 用户自定义关键字

```atx
// 用户自己的 atx 文件里也可以声明关键字
TOOLS : {
    #[keyword(kind="source", scope=[INPUT], base=FILES)]
    fn MY_CONFIG(args, kwargs) : dict {
        CALL parse_toml(args[0])
    }
}

INPUT : {
    MY_CONFIG : "~/.app/config.toml" => cfg : dict
}
```

## 4. 影响范围

| 组件 | 影响 |
|------|------|
| **词法分析** | 无影响。关键字仍作为标识符进入，查询阶段由语法/语义层判断 |
| **语法分析** | `parse_source_decl` 和 `parse_target_decl` 的 `is_source_target_keyword()` 检查改为查询注册表 |
| **语义分析** | 新增 `#[keyword]` 元装饰器的解析和注册逻辑；新增关键字作用域验证；关键字注册表维护 |
| **符号表** | 新增 `KeywordRegistry`：维护 `{kind, scope, base, func_name}` 映射 |
| **代码生成** | `compile_source_decl()` 中已有 `_ => { /* 留给标准库 */ }` 分支，直接利用 |
| **VM** | 无影响 |
| **标准库** | 标准库模块可自由声明关键字，推动核心关键字从编译器移入标准库 |

## 5. 向后兼容性

- 三个基础关键字（`FILES` `WEBS` `MEMS`）不变，不受影响
- 当前硬编码的衍生关键字（`JSON` `CSV` `HTTP` `TCP` `DB` `OSS` `TXT` `YAML` `TOML` `XML` `JSONS`）在过渡期采用**兼容策略**：编译器内置注册表同时包含硬编码列表，用户声明的关键字优先级高于内置列表
- 不会破坏任何现有代码

**未来迁移路径：**
在标准库稳定后，逐步将编译器内置的衍生关键字迁移到标准库模块中，由 `#[keyword]` 声明接管。最终编译器只硬编码三个基础关键字。

## 6. 未解决的问题

1. **关键字命名**：关键字是全大写惯例（如 `JSON`），而 TOOLS 函数是驼峰或小写（如 `json_parse`）。`#[keyword]` 声明的函数名是否要求全大写？如果用户用小写名称注册关键字，是否自动转为大写？

2. **关键字优先级**：当 USE 导入的关键字与当前作用域已有的函数/关键字重名时，详细的遮蔽规则需要定义。

3. **模块内关键字可见性**：`#[keyword]` 是否支持 `PUB` 控制？不写 PUB 是否意味着仅在当前文件可见？

4. **跨模块关键字引用**：模块 A 中声明的关键字，能否在模块 B 的 `#[keyword(base=A::SOME_KEYWORD)]` 中被引用？

5. **测试与 mock**：当关键字从标准库注册时，测试环境如何 mock 关键字行为？
