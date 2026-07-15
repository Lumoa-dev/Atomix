# Atomix WORKS 语法

> 架构版本: v0.1 (设计阶段)
> 适用范围: WORKS 语法域
> 配套文档: [01-总纲与哲学.md](../01-总纲与哲学.md)、[通用语法.md](通用语法.md)

---

## 1. 语法域概述

WORKS 定义可复用的任务模板，结构与 Python 的 class 八九分像。实例化后由 TASK 区通过 `WAIT` 派发执行。

引用方向：WORKS 可引用 TOOLS 区函数。不可引用 INPUT、TASK、OUT。

---

## 2. WORKS 定义模板

```
<WORKS定义> : WORKS <名称> [ ( <继承> ) ] {
                 [ <属性声明>* ]
                 [ <代码体> ]
                 [ <钩子系统>* ]
                 [ <方法定义>* ]
             }

<继承>  : <WORKS名称> [ , <WORKS名称> ]*
```

各部分含义：

| 部分 | 说明 |
|------|------|
| `<名称>` | WORKS 名。建议驼峰命名（如 `DataProcessor`），不强制 |
| `<继承>` | 可选，逗号分隔的父 WORKS 名 |
| `<属性声明>` | 类属性/参数，声明词大写 |
| `<代码体>` | 实例化时默认执行，可写 IF/FOR/CALL 等 |
| `<钩子系统>` | 生命周期钩子链 |
| `<方法定义>` | 函数方法 |

---

## 3. 属性声明

属性既是类属性也是参数，通过 `self.<名称>` 引用：

```
WORKS DataProcessor {
    RAW : bytes
    TIMEOUT : i32 = 30
    RETRY : i32 = 3
    
    # 实例化后默认执行
    CALL self.process()
    
    fn process() {
        CALL transform(self.RAW)
    }
}
```

- 声明词**大写**（遵循关键词惯例）
- 代码体内建议常规大小写
- `self` 引用当前实例的属性

---

## 4. 钩子系统

### 4.1 钩子链模板

核心语义只有一句：**当触发了什么钩子的时候，什么条件执行什么东西，然后什么条件再触发什么钩子。**

```
<钩子链> : <触发钩子> [ :: <条件> ] [ :: <调用> ] [ :: <条件> ] [ :: <触发钩子> ]
```

- `<触发钩子>`：**必须有**。只能被动触发，不可主动调用
- `:: <条件>`：值表达式。不写就是无条件
- `:: <调用>`：CALL 或其他执行语句。不写就是纯信号

**合法组合（只要语义通顺就合情合法）：**

```
# 五段全齐
START :: self.RAW != "" :: CALL validate() :: true :: PROCESS

# 左三段：钩子→条件→调用（无下链）
START :: self.RAW != "" :: CALL validate()

# 右三段：调用→条件→钩子（无上链）
CALL validate() :: true :: PROCESS

# 中三段：钩子→调用→钩子（无条件）
START :: CALL validate() :: PROCESS

# 钩子→调用（无条件、无下链）
START :: CALL validate()

# 调用→钩子（无条件、无上链）
CALL validate() :: PROCESS

# 钩子→条件（只判断，不调用，不下链）
ERROR :: ISERRORTYPE is IOError

# 钩子→钩子（有意义还是没意义，系统不判断）
INIT :: START
START :: DONE
```

**不合法——裸钩子：**

```
START              # ❌ 裸钩子，没有一个 ::
```

裸钩子不构成链。其余任何排列，只要 `::` 把东西串起来了，语法上就合法。

> **系统不做安全检查。** 工具给你了。用不明白不关我事。`INIT :: INIT` 语法合法——循环也好空转也好，语义通不通你自己把握。

### 4.2 生命周期钩子

钩子名采用 Atomix 风格——全大写，无前缀，简短直接：

| 钩子 | 触发时机 |
|------|----------|
| `INIT` | 实例属性初始化完毕时 |
| `START` | 实例开始执行主代码体时 |
| `STEP` | 遇到 CALL 形成的每个 Step 前 |
| `STEP_AFTER` | 每个 Step 执行完毕后 |
| `CALL` | 实例的方法被调用时 |
| `GET` | 实例的属性被读取时 |
| `SET` | 实例的属性被写入时 |
| `FORK` | 实例派生子任务时 |
| `JOIN` | 子任务完成、结果返回时 |
| `DONE` | 执行正常完成时 |
| `ERROR` | 执行抛出异常时 |
| `FINALLY` | 无论成功失败，在 DONE/ERROR 后触发 |
| `DEL` | 实例即将销毁时 |

> 完整钩子列表详见附录。以上为核心钩子，覆盖绝大多数场景。

### 4.3 空钩子

```
VOID_0(<NAME>)  ~  VOID_9(<NAME>)
```

10 个预留空钩子，括号内指定扩展名，默认无行为，作为扩展点。

### 4.4 示例

```
WORKS DataPipeline {
    RAW : bytes
    
    START :: self.RAW != "" :: CALL validate() :: true :: PROCESS
    PROCESS :: CALL transform() :: self.valid :: DONE
    ERROR :: CALL recover() :: VOID_0
    FINALLY :: CALL cleanup()
    
    fn validate() {
        IF self.RAW == "" { raise("empty data") }
    }
    fn transform() { process(self.RAW) }
    fn recover() { log("recovering...") }
    fn cleanup() { log("cleanup done") }
}
```

---

## 5. 可见性

| 成员 | 默认可见性 | 说明 |
|------|-----------|------|
| **属性（变量）** | **公开** | 所有属性默认对外可见，外部可直接读写 |
| **方法（fn）** | **私有** | 所有方法默认私有，外部不可调用 |

要公开方法，使用 `PUB` 关键词声明。

---

## 6. 方法定义

### 6.1 模板

```
<方法定义>       : fn <名称> ( <参数列表> ) { <代码体> }
<公开方法声明>   : PUB fn <名称> ( <参数列表> )
<公开方法定义>   : PUB fn <名称> ( <参数列表> ) { <代码体> }

<参数列表> : <标识符> : <类型> [ , <标识符> : <类型> ]*
```

### 6.2 私有方法（默认）

```
WORKS Calculator {
    fn calc() {          # 私有，外部不可调
        CALL compute()
    }
}
```

### 6.3 公开方法

两种写法：

**分离式（先声明后定义）：**

```
WORKS DataProcessor {
    PUB fn get_name()    # 先声明——外部可见此接口
    PUB fn set_name()
    
    fn get_name() {      # 后定义
        self.name
    }
    
    fn set_name() {      # 后定义
        ...
    }
}
```

**内联式：**

```
WORKS DataProcessor {
    PUB fn get_name() {
        self.name
    }
}
```

分离式的好处是：看一眼 WORKS 头部就能知道对外提供了哪些接口，类似 C 的头文件。

### 6.4 `self` 引用

方法内外一致，通过 `self.<变量>` 访问实例属性：

```
WORKS Calculator {
    VALUE : i32
    
    PUB fn add(x : i32) {
        self.VALUE = self.VALUE + x
    }
    
    PUB fn get_value() : i32 {
        self.VALUE
    }
}
```

---

## 7. 实例化与派发

WORKS 由 TASK 区的 `WAIT` 派发执行（详见 TASK语法.md §6）：

```
TASK :
    WAIT DataProcessor (RAW = input_data) => result
```

---

## 8. 非法情况

| 情况 | 说明 |
|------|------|
| WORKS 中定义 INPUT/OUT 区 | WORKS 是纯计算模板，不处理 I/O |
| 钩子链循环引用 | 钩子链不可形成死循环 |
| `self` 引用未声明的属性 | `self.UNDEFINED` |
| 方法名与内置钩子重名 | 不可定义名为 `ON_START` 的方法 |
