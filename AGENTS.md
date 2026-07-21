# Atomix AGENTS.md

## Build & Test Commands

```bash
cargo build --release          # full build (protoc vendored, no system dep)
cargo test                     # 365 tests, all pass
cargo test -p atomix           # same, single crate
cargo test --test compile_test   # fixture-based compiler tests only
cargo test --test edge_cases_test  # panic-catch & error-msg tests
cargo test --test monomorphization_test  # generics monomorphization
cargo test execute::             # run only execute.rs tests
cargo run -- build demo.atx     # compile .atx -> .atxe
cargo run -- runner run demo.atxe   # execute locally
cargo run -- --help
```

No Makefile, no CI workflows, no rust-toolchain, no rustfmt/clippy config in repo.
Edition: **Rust 2024** (`Cargo.toml`).

## Crate Dependencies

| Crate | Use | Notes |
|-------|-----|-------|
| `clap` 4 (derive) | CLI parsing | All 4 binaries use derive macros |
| `serde` / `serde_json` / `toml` | Serialization | Config, lock files |
| `rustyline` 14 | REPL | atomix-debug only |
| `prost` 0.13 / `tokio` 1 | ATXP remote protocol | Daemon mode only; tokio features: rt, net, io-util, sync, time, macros, signal |
| `prost-build` 0.13 + `protoc-bin-vendored` 3 | Proto codegen | Build-time only, vendored protoc (no system dep) |

## Code Generation (proto)

`build.rs` compiles `docs/atxp.proto` via `prost-build + protoc-bin-vendored`.
Generated code ends up in `OUT_DIR`; no committed proto stubs. First build may be slower.

## Architecture (single crate, multi-bin)

```
src/
├── lib.rs         # pub mod base, compiler, debug, runner, origin
├── origin.rs      # flat at src/ root (not a submodule)
├── base/          # isa.rs (54 opcodes), ir.rs (.atxe format), atxp.rs, error.rs
├── compiler/      # lexer -> parser -> semantic -> codegen -> linker
│   └── codegen/   # assembly, expr, instr, stmt, optimizer, reg_alloc
├── runner/        # VmState, execute.rs (54 instr dispatch + ECALL), runtime, pool, etc.
├── debug/         # repl.rs, disassemble.rs, eval.rs, debug_segment.rs
└── bin/
    ├── atomix.rs         # main CLI (build/check/runner/task/origin)
    ├── atomix-build.rs   # standalone compiler
    ├── atomix-runner.rs  # run (single) | daemon (ATXP server)
    └── atomix-debug.rs   # standalone debugger REPL
```

Key entrypoints:
- `atomix::compiler::compile(source, opt_level)` → `(Vec<u8>, Vec<String>)`
- `atomix::runner::VmState::load_atxe(bytes)` → `Result<VmState, String>`
- `atomix::debug::repl::DebugSession::new(vm)` + `run_repl(&mut session)`

## ISA & VM (non-obvious details)

- R0 = zero (hardwired 0, writes ignored)
- R14 = task_id (read-only, writes ignored)
- R15 = tmp
- `VmState::clone()` intentionally drops open files/sockets/listeners
- Quantum = 1000 instructions by default; suspension always at instruction boundary

## Test Patterns

- All tests use `#[test]` inline with `#[cfg(test)] mod tests` — plain `cargo test`
- Fixture-based integration: `tests/compile_test.rs` reads `.atx` files from:
  - `tests/fixtures/valid/*.atx` — must compile without errors
  - `tests/fixtures/invalid/*.atx` — must produce at least one error
- `tests/edge_cases_test.rs` — panic-catch tests that verify the compiler doesn't panic and produces reasonable error messages
- `tests/monomorphization_test.rs` — generics monomorphization correctness tests
- To run a single test: `cargo test test_name` (Cargo test filter)
- No snapshot testing, no external services required

## Documentation (read before modifying sensitive areas)

- `docs/01-总纲与哲学.md` — design philosophy and overall architecture
- `docs/02-指令集规范.md` — ISA reference (54 opcodes)
- `docs/04-编译管线.md` — compiler pipeline stages
- `docs/05-通信协议.md` — ATXP protocol (used by daemon mode)
- `docs/AEP/*.md` — Atomix Enhancement Proposals; read relevant AEP before implementing large features
- `docs/语法设计/` — language grammar, typesystem, builtins, keywords
- `docs/index.md` — auto-generated file index (regenerate via `python scripts/gen-index.py`)

## Simulation Tools (`sim/`)

Python-based discrete-time simulator for VM resource modeling:
- `sim/main.py` — CLI entry point
- `sim/simulation.py` — discrete-time simulation engine
- `sim/visualizer.py` — matplotlib chart generation
- Requires Python 3 + matplotlib + numpy

## Important Constraints

- **Linker is single-file only** — multi-file linking not yet implemented
- **Optimizer**: O0/O1 pass, O2 (inline/loop) missing
- **Standard library and package manager are not implemented**
- ATXP daemon uses Tokio async (`#[tokio::main]`); the rest is sync
- `Cargo.lock` is in `.gitignore` (app binary repo)
- No IDE config or VSCode settings in repo; syntax highlighting via `syntaxes/atomix.tmLanguage.json`
- `.gitattributes` uses `* text=auto eol=lf`; `.atxe` is marked as binary
