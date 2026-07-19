# Atomix 项目索引

> 📝 此文件由 `scripts/gen-index.py` 自动生成。
> 增删文件后执行 `python scripts/gen-index.py` 更新。

## 项目概览

| 文件 | 说明 |
|------|------|
| [Cargo.toml](Cargo.toml) | Rust 项目清单（3 个 binary） |
| [README.md](README.md) | 项目自述 |

## 设计文档

| 路径 | 说明 |
|------|------|
| [01-总纲与哲学.md](docs/01-总纲与哲学.md) | Atomix 设计总纲与哲学 |
| [02-指令集规范.md](docs/02-指令集规范.md) | Atomix 指令集规范 (ISA) |
| [03-编译行为.md](docs/03-编译行为.md) | Atomix 编译行为 |
| [04-编译管线.md](docs/04-编译管线.md) | Atomix 编译管线 |
| [05-通信协议.md](docs/05-通信协议.md) | Atomix 通信协议 (ATXP) |
| [06-外围工具.md](docs/06-外围工具.md) | Atomix 外围工具 |
| [07-Runner完整架构设计.md](docs/07-Runner完整架构设计.md) | Atomix Runner 完整架构设计 |
| [09-配置设计.md](docs/09-配置设计.md) | Atomix 资源配置 |
| [10-命令行规范.md](docs/10-命令行规范.md) | Atomix 命令行规范 |
| [12-debugger-设计.md](docs/12-debugger-设计.md) | Atomix Debugger 设计文档 (atomix-debug) |
| [13-sdk-设计.md](docs/13-sdk-设计.md) | Atomix SDK 设计文档 |
| [14-Atomix-Rust-实现任务文档.md](docs/14-Atomix-Rust-实现任务文档.md) | Atomix Rust 实现任务文档 |

## AEP

| 路径 | 说明 |
|------|------|
| [000-模板.md](docs/AEP/000-模板.md) | AEP-{NNN}: {标题} |
| [001-装饰器统一规范.md](docs/AEP/001-装饰器统一规范.md) | AEP-001: 装饰器统一规范 |
| [002-增量编译.md](docs/AEP/002-增量编译.md) | AEP-002: 增量编译 |
| [003-指令优化策略.md](docs/AEP/003-指令优化策略.md) | AEP-003: 指令优化策略 |
| [004-LLVM后端兼容性-BETA.md](docs/AEP/004-LLVM后端兼容性-BETA.md) | AEP-004 (BETA): LLVM 后端兼容性 |
| [README.md](docs/AEP/README.md) | Atomix Enhancement Proposals (AEP) |

## 语法设计

| 路径 | 说明 |
|------|------|
| [INPUT语法.md](docs/语法设计/INPUT语法.md) | Atomix INPUT 语法 |
| [OUT语法.md](docs/语法设计/OUT语法.md) | Atomix OUT 语法 |
| [TASK语法.md](docs/语法设计/TASK语法.md) | Atomix TASK 语法 |
| [TOOLS语法.md](docs/语法设计/TOOLS语法.md) | Atomix TOOLS 语法 |
| [WORKS语法.md](docs/语法设计/WORKS语法.md) | Atomix WORKS 语法 |
| [关键字参考.md](docs/语法设计/关键字参考.md) | Atomix 关键字参考 |
| [内置函数.md](docs/语法设计/内置函数.md) | Atomix 内置函数 |
| [包管理.md](docs/语法设计/包管理.md) | Atomix 包管理 |
| [区外语法.md](docs/语法设计/区外语法.md) | Atomix 区外语法 |
| [标准库.md](docs/语法设计/标准库.md) | Atomix 标准库 |
| [类型系统.md](docs/语法设计/类型系统.md) | Atomix 类型系统 |
| [通用语法.md](docs/语法设计/通用语法.md) | Atomix 通用语法 |

## 语法设计 · 附录

| 路径 | 说明 |
|------|------|
| [数据源地址与参数速查.md](docs/语法设计/附录/数据源地址与参数速查.md) | 附录 A：数据源地址与参数速查 |
| [钩子与IS值参考.md](docs/语法设计/附录/钩子与IS值参考.md) | 附录 C：钩子与 IS\* 值参考 |
| [默认装饰器参考.md](docs/语法设计/附录/默认装饰器参考.md) | 附录 B：默认装饰器参考 |

## Rust 源码 — src

| 路径 | 说明 |
|------|------|
| [src\lib.rs](src/lib.rs) | Crate root / re-exports |

## Rust 源码 — 基础层

| 路径 | 说明 |
|------|------|
| [src\base\error.rs](src/base/error.rs) | Error types |
| [src\base\ir.rs](src/base/ir.rs) | Intermediate Representation binary format |
| [src\base\isa.rs](src/base/isa.rs) | Instruction Set Architecture (54 opcodes) |
| [src\base\mod.rs](src/base/mod.rs) | Module root |

## Rust 源码 — 二进制入口

| 路径 | 说明 |
|------|------|
| [src\bin\atomix-build.rs](src/bin/atomix-build.rs) | Module: atomix-build |
| [src\bin\atomix-runner.rs](src/bin/atomix-runner.rs) | Module: atomix-runner |
| [src\bin\atomix.rs](src/bin/atomix.rs) | Module: atomix |

## Rust 源码 — 编译器

| 路径 | 说明 |
|------|------|
| [src\compiler\ast.rs](src/compiler/ast.rs) | Abstract Syntax Tree |
| [src\compiler\builtins.rs](src/compiler/builtins.rs) | Built-in functions |
| [src\compiler\lexer.rs](src/compiler/lexer.rs) | Lexical analysis |
| [src\compiler\linker.rs](src/compiler/linker.rs) | Linking |
| [src\compiler\mod.rs](src/compiler/mod.rs) | Module root |
| [src\compiler\parser.rs](src/compiler/parser.rs) | Syntactic analysis |
| [src\compiler\semantic.rs](src/compiler/semantic.rs) | Semantic analysis |
| [src\compiler\symbol.rs](src/compiler/symbol.rs) | Symbol table |
| [src\compiler\token.rs](src/compiler/token.rs) | Token definitions |
| [src\compiler\type_checker.rs](src/compiler/type_checker.rs) | Type checking |

## Rust 源码 — 编译器/代码生成

| 路径 | 说明 |
|------|------|
| [src\compiler\codegen\assembly.rs](src/compiler/codegen/assembly.rs) | Assembly generation |
| [src\compiler\codegen\expr.rs](src/compiler/codegen/expr.rs) | Expression codegen |
| [src\compiler\codegen\instr.rs](src/compiler/codegen/instr.rs) | Instruction selection |
| [src\compiler\codegen\mod.rs](src/compiler/codegen/mod.rs) | Module root |
| [src\compiler\codegen\optimizer.rs](src/compiler/codegen/optimizer.rs) | Optimization passes |
| [src\compiler\codegen\reg_alloc.rs](src/compiler/codegen/reg_alloc.rs) | Register allocation |
| [src\compiler\codegen\stmt.rs](src/compiler/codegen/stmt.rs) | Statement codegen |

## Rust 源码 — 运行时

| 路径 | 说明 |
|------|------|
| [src\runner\batch.rs](src/runner/batch.rs) | Batch management |
| [src\runner\config.rs](src/runner/config.rs) | Runner configuration |
| [src\runner\decode.rs](src/runner/decode.rs) | Bytecode decoding |
| [src\runner\execute.rs](src/runner/execute.rs) | Instruction execution (VM) |
| [src\runner\hwinfo.rs](src/runner/hwinfo.rs) | Hardware info interrogation |
| [src\runner\loader.rs](src/runner/loader.rs) | Binary loader (.atxe) |
| [src\runner\memory.rs](src/runner/memory.rs) | Memory management / memory wall |
| [src\runner\mod.rs](src/runner/mod.rs) | Module root |
| [src\runner\pool.rs](src/runner/pool.rs) | Task pool |
| [src\runner\sched.rs](src/runner/sched.rs) | Adaptive scheduler |
| [src\runner\slot.rs](src/runner/slot.rs) | Slot management |
| [src\runner\task.rs](src/runner/task.rs) | Task representation |

## 仿真 (sim/)

| 路径 | 说明 |
|------|------|
| [sim\__init__.py](sim/__init__.py) | Package init |
| [sim\adaptive_controller.py](sim/adaptive_controller.py) | Adaptive resource controller (strategy) |
| [sim\config.py](sim/config.py) | Configuration types |
| [sim\executor.py](sim/executor.py) | Task executor model |
| [sim\generate_charts.py](sim/generate_charts.py) | Standalone chart generator |
| [sim\hardware_model.py](sim/hardware_model.py) | Hardware resource model |
| [sim\load_balancer.py](sim/load_balancer.py) | Load balancer + prefetch + defrag |
| [sim\main.py](sim/main.py) | Entry point / CLI |
| [sim\metrics.py](sim/metrics.py) | Metrics collector |
| [sim\regression_model.py](sim/regression_model.py) | Linear regression memory model |
| [sim\report_generator.py](sim/report_generator.py) | Report generation |
| [sim\scenarios.py](sim/scenarios.py) | Predefined test scenarios |
| [sim\simulation.py](sim/simulation.py) | Discrete-time simulation engine |
| [sim\slot_manager.py](sim/slot_manager.py) | Slot (memory) manager |
| [sim\task_generator.py](sim/task_generator.py) | Task arrival generator |
| [sim\visualizer.py](sim/visualizer.py) | Chart generation (matplotlib) |

## 测试 (tests/)

| 路径 | 说明 |
|------|------|
| [tests\compile_test.rs](tests/compile_test.rs) | Test: compile_test |
| [tests\edge_cases_test.rs](tests/edge_cases_test.rs) | Test: edge_cases_test |
| [tests\fixtures\invalid\bad_escape.atx](tests/fixtures/invalid/bad_escape.atx) | Test fixture: bad_escape |
| [tests\fixtures\invalid\type_mismatch.atx](tests/fixtures/invalid/type_mismatch.atx) | Test fixture: type_mismatch |
| [tests\fixtures\invalid\undefined_var.atx](tests/fixtures/invalid/undefined_var.atx) | Test fixture: undefined_var |
| [tests\fixtures\valid\all_zones.atx](tests/fixtures/valid/all_zones.atx) | Test fixture: all_zones |
| [tests\fixtures\valid\builtins.atx](tests/fixtures/valid/builtins.atx) | Test fixture: builtins |
| [tests\fixtures\valid\control_flow.atx](tests/fixtures/valid/control_flow.atx) | Test fixture: control_flow |
| [tests\fixtures\valid\expressions.atx](tests/fixtures/valid/expressions.atx) | Test fixture: expressions |
| [tests\fixtures\valid\functions.atx](tests/fixtures/valid/functions.atx) | Test fixture: functions |
| [tests\fixtures\valid\generics.atx](tests/fixtures/valid/generics.atx) | Test fixture: generics |
| [tests\fixtures\valid\hello_world.atx](tests/fixtures/valid/hello_world.atx) | Test fixture: hello_world |
| [tests\monomorphization_test.rs](tests/monomorphization_test.rs) | Test: monomorphization_test |

## 脚本工具 (scripts/)

| 路径 | 说明 |
|------|------|
| [scripts\gen-index.py](scripts/gen-index.py) | Script: gen-index |

## 语法高亮 (syntaxes/)

| 路径 | 说明 |
|------|------|
| [syntaxes\atomix.tmLanguage.json](syntaxes/atomix.tmLanguage.json) | atomix.tmLanguage.json |

---
共 101 项。自动生成于 `scripts/gen-index.py`。