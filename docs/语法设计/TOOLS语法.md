# Atomix TOOLS 语法

> 架构版本: v0.1 (设计阶段)
> 适用范围: TOOLS 语法域
> 配套文档: 详见 通用语法.md

---

## 1. 语法域概述

TOOLS 区是定义工具函数、泛型、异常的场所——**纯定义区，没有执行逻辑**。

TOOLS 是全场语法最少的区：它里面只有函数。函数语法（含泛型）已由通用语法统一定义（详见 通用语法.md §16），TOOLS 仅做承接。

## 2. 区声明

```
<TOOLS区>  : TOOLS : { <函数定义>+ }
```

## 3. 函数定义

函数语法参见通用语法 §16：

```
fn add(x : int, y : int) : int {
    CALL x + y
}

fn process(data : bytes) : dict[str, any] {
    CALL transform(data)
}
```

支持先声明后定义（分离式）：

```
PUB fn process(data : bytes) : dict[str, any]    # 声明

fn process(data : bytes) : dict[str, any] {       # 定义
    CALL transform(data)
}
```

## 4. 内置装饰器

TOOLS 区默认提供一批内置装饰器（如 `[gzip]`、`[encrypt]`、`[validate]` 等），用户无需声明即可直接使用。详见 附录/默认装饰器参考.md。

## 5. 非法情况

| 情况 | 说明 |
|------|------|
| 在 TOOLS 中写 INPUT/TASK/OUT/WORKS 声明 | TOOLS 不做编排、不处理 I/O |
| 函数名与内置装饰器重名 | 不可定义名为 `gzip`、`encrypt` 等与内置装饰器同名的函数 |
