"""Generate project-level README/index for Atomix.

Scans the entire project tree and produces a structured markdown index
covering all major components: design docs, Rust source, sim, tests, etc.

Usage:  python scripts/gen-index.py
Output: docs/index.md (auto-generated, do not edit manually)
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "docs" / "index.md"

# Directories / files to skip entirely
SKIP_DIRS = {".git", ".github", ".assets", ".zcode", "target", "__pycache__"}
SKIP_FILES = {"index.md", "nul", ".gitattributes", ".gitignore", "Cargo.lock"}
SKIP_EXT = {".png", ".jpg", ".jpeg", ".gif", ".ico", ".svg", ".woff2", ".ttf",
            ".otf", ".eot", ".lock", ".exe", ".dll", ".pdb", ".pyc"}


def extract_title(filepath: Path) -> str:
    """Read the first #-level heading from a markdown file."""
    try:
        with open(filepath, "r", encoding="utf-8") as f:
            for line in f:
                stripped = line.strip()
                if stripped.startswith("# ") and not stripped.startswith("## "):
                    return stripped[2:].strip()
        return filepath.stem
    except Exception:
        return filepath.stem


def describe_rust_file(filepath: Path) -> str:
    """Guess a Rust file's purpose from its name and a brief peek."""
    stem = filepath.stem
    module_map = {
        "lib": "Crate root / re-exports",
        "mod": "Module root",
        "isa": "Instruction Set Architecture (54 opcodes)",
        "ir": "Intermediate Representation binary format",
        "error": "Error types",
        "ast": "Abstract Syntax Tree",
        "token": "Token definitions",
        "lexer": "Lexical analysis",
        "parser": "Syntactic analysis",
        "semantic": "Semantic analysis",
        "type_checker": "Type checking",
        "symbol": "Symbol table",
        "linker": "Linking",
        "builtins": "Built-in functions",
        "assembly": "Assembly generation",
        "instr": "Instruction selection",
        "expr": "Expression codegen",
        "stmt": "Statement codegen",
        "optimizer": "Optimization passes",
        "reg_alloc": "Register allocation",
        "config": "Runner configuration",
        "decode": "Bytecode decoding",
        "execute": "Instruction execution (VM)",
        "memory": "Memory management / memory wall",
        "loader": "Binary loader (.atxe)",
        "hwinfo": "Hardware info interrogation",
        "pool": "Task pool",
        "sched": "Adaptive scheduler",
        "slot": "Slot management",
        "task": "Task representation",
        "batch": "Batch management",
    }
    return module_map.get(stem, f"Module: {stem}")


def describe_python_file(filepath: Path) -> str:
    """Guess a Python file's purpose from its name."""
    stem = filepath.stem
    desc_map = {
        "__init__": "Package init",
        "main": "Entry point / CLI",
        "config": "Configuration types",
        "simulation": "Discrete-time simulation engine",
        "adaptive_controller": "Adaptive resource controller (strategy)",
        "slot_manager": "Slot (memory) manager",
        "executor": "Task executor model",
        "task_generator": "Task arrival generator",
        "hardware_model": "Hardware resource model",
        "metrics": "Metrics collector",
        "visualizer": "Chart generation (matplotlib)",
        "report_generator": "Report generation",
        "scenarios": "Predefined test scenarios",
        "load_balancer": "Load balancer + prefetch + defrag",
        "regression_model": "Linear regression memory model",
        "generate_charts": "Standalone chart generator",
        "build_pdf": "PDF builder",
        "run_all": "Run all scenarios",
    }
    return desc_map.get(stem, f"Script: {stem}")


def build_src_index() -> list[tuple[str, str, str]]:
    """Index Rust source: group by subdirectory."""
    src = ROOT / "src"
    if not src.is_dir():
        return []
    entries = []
    for fp in sorted(src.rglob("*.rs")):
        rel = fp.relative_to(ROOT)
        desc = describe_rust_file(fp)
        entries.append((str(rel), str(rel).replace("\\", "/"), desc))
    return entries


def build_docs_index() -> list[tuple[str, str, str, str]]:
    """Index design docs: returns (group, name, rel, title)."""
    docs = ROOT / "docs"
    if not docs.is_dir():
        return []
    entries = []
    for fp in sorted(docs.rglob("*.md")):
        if fp.name in SKIP_FILES or fp.name.startswith("."):
            continue
        rel = fp.relative_to(ROOT)
        title = extract_title(fp)
        group = str(rel.parent).replace("\\", "/")
        # Clean up group labels
        group = group.replace("docs/", "").replace("语法设计/附录", "语法设计 · 附录")
        if group == "." or group == "docs":
            group = "设计文档"
        entries.append((group, fp.name, str(rel).replace("\\", "/"), title))
    return entries


def build_sim_index() -> list[tuple[str, str, str]]:
    """Index Python simulation files."""
    sim = ROOT / "sim"
    if not sim.is_dir():
        return []
    entries = []
    for fp in sorted(sim.rglob("*.py")):
        if fp.name.startswith("."):
            continue
        rel = fp.relative_to(ROOT)
        desc = describe_python_file(fp)
        entries.append((str(rel), str(rel).replace("\\", "/"), desc))
    return entries


def build_tests_index() -> list[tuple[str, str, str]]:
    """Index test files."""
    tests = ROOT / "tests"
    if not tests.is_dir():
        return []
    entries = []
    for fp in sorted(tests.rglob("*")):
        if fp.is_dir() or fp.name in SKIP_FILES:
            continue
        if any(fp.name.endswith(ext) for ext in [".rs", ".atx"]):
            rel = fp.relative_to(ROOT)
            desc = f"Test fixture: {fp.stem}" if fp.suffix == ".atx" else f"Test: {fp.stem}"
            entries.append((str(rel), str(rel).replace("\\", "/"), desc))
    return entries


def build_scripts_index() -> list[tuple[str, str, str]]:
    """Index utility scripts."""
    scripts = ROOT / "scripts"
    if not scripts.is_dir():
        return []
    entries = []
    for fp in sorted(scripts.rglob("*.py")):
        if fp.name.startswith("."):
            continue
        rel = fp.relative_to(ROOT)
        desc = describe_python_file(fp)
        entries.append((str(rel), str(rel).replace("\\", "/"), desc))
    return entries


def build_syntax_index() -> list[tuple[str, str, str]]:
    """Index TextMate grammar files."""
    syn = ROOT / "syntaxes"
    if not syn.is_dir():
        return []
    entries = []
    for fp in sorted(syn.rglob("*")):
        if fp.is_dir() or fp.name.startswith("."):
            continue
        if any(fp.name.endswith(ext) for ext in [".json", ".plist", ".yaml", ".yml"]):
            rel = fp.relative_to(ROOT)
            entries.append((str(rel), str(rel).replace("\\", "/"), fp.name))
    return entries


def section(title: str, rows: list[tuple]) -> list[str]:
    """Render a markdown table section."""
    if not rows:
        return []
    lines = [f"## {title}", "", "| 路径 | 说明 |", "|------|------|"]
    for row in rows:
        path = row[1] if len(row) == 3 else row[2]
        desc = row[-1]
        lines.append(f"| [{row[0]}]({path}) | {desc} |")
    lines.append("")
    return lines


def main():
    lines = [
        "# Atomix 项目索引",
        "",
        f"> 📝 此文件由 `scripts/gen-index.py` 自动生成。",
        f"> 增删文件后执行 `python scripts/gen-index.py` 更新。",
        "",
        "## 项目概览",
        "",
        "| 文件 | 说明 |",
        "|------|------|",
        "| [Cargo.toml](Cargo.toml) | Rust 项目清单（3 个 binary） |",
        "| [README.md](README.md) | 项目自述 |",
        "",
    ]

    # ── 1. 设计文档 ──
    doc_entries = build_docs_index()
    if doc_entries:
        # Group docs by subdirectory
        groups: dict[str, list] = {}
        for grp, name, rel, title in doc_entries:
            groups.setdefault(grp, []).append((name, rel, title))
        ordered = ["设计文档"]
        for g in groups:
            if g != "设计文档":
                ordered.append(g)
        for grp in ordered:
            rows = groups.get(grp, [])
            lines.extend(section(grp, rows))

    # ── 2. Rust 源码 ──
    src_entries = build_src_index()
    if src_entries:
        # Group by module directory
        groups: dict[str, list] = {}
        for path, rel, desc in src_entries:
            parts = Path(path).parent.parts
            if len(parts) >= 2 and parts[0] == "src":
                grp = "/".join(parts) if len(parts) >= 2 else "src"
            else:
                grp = "src"
            groups.setdefault(grp, []).append((path, rel, desc))
        ordered = sorted(groups.keys())
        for grp in ordered:
            rows = groups[grp]
            label = {"src/base": "基础层", "src/compiler": "编译器",
                     "src/compiler/codegen": "编译器/代码生成", "src/runner": "运行时",
                     "src/bin": "二进制入口"}.get(grp, grp)
            lines.extend(section(f"Rust 源码 — {label}", rows))

    # ── 3. 仿真 ──
    sim_entries = build_sim_index()
    lines.extend(section("仿真 (sim/)", sim_entries))

    # ── 4. 测试 ──
    test_entries = build_tests_index()
    lines.extend(section("测试 (tests/)", test_entries))

    # ── 5. 脚本工具 ──
    script_entries = build_scripts_index()
    lines.extend(section("脚本工具 (scripts/)", script_entries))

    # ── 6. 语法高亮 ──
    syntax_entries = build_syntax_index()
    lines.extend(section("语法高亮 (syntaxes/)", syntax_entries))

    total = sum(len(v) for v in [doc_entries, src_entries, sim_entries,
                                  test_entries, script_entries, syntax_entries])
    lines.append(f"---")
    lines.append(f"共 {total} 项。自动生成于 `scripts/gen-index.py`。")

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"Generated {OUTPUT} with {total} entries.")


if __name__ == "__main__":
    main()
