"""Generate docs/index.md from all .md files under docs/.

Usage:  python scripts/gen-index.py
Output: docs/index.md (auto-generated, do not edit manually)
"""

from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent / "docs"
OUTPUT = DOCS / "index.md"
EXCLUDE = {"index.md", "nul"}


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


def build_tree() -> dict[str, list[tuple[str, str, str]]]:
    """Walk docs/, group .md files by directory."""
    groups: dict[str, list[tuple[str, str, str]]] = {}
    for fp in sorted(DOCS.rglob("*.md")):
        if fp.name in EXCLUDE or fp.name.startswith("."):
            continue
        rel = fp.relative_to(DOCS)
        dir_name = str(rel.parent) if str(rel.parent) != "." else "根目录"
        title = extract_title(fp)
        groups.setdefault(dir_name, []).append((fp.name, str(rel).replace("\\", "/"), title))
    return groups


def main():
    groups = build_tree()
    lines = [
        "# Atomix 文档索引",
        "",
        "> ⚠️ 此文件由 `scripts/gen-index.py` 自动生成，请勿手动编辑。",
        f"> 最后生成: 见 git log",
        "",
    ]

    # Order top-level groups sensibly
    order = ["根目录"]
    for k in groups:
        if k != "根目录":
            order.append(k)

    for group in order:
        files = groups.get(group, [])
        if not files:
            continue
        lines.append(f"## {group}")
        lines.append("")
        # Build table
        lines.append("| 文件 | 标题 |")
        lines.append("|------|------|")
        for name, rel, title in files:
            lines.append(f"| [{name}]({rel}) | {title} |")
        lines.append("")

    lines.append(f"---")
    lines.append(f"共 {sum(len(v) for v in groups.values())} 份文档。")

    OUTPUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"Generated {OUTPUT} with {sum(len(v) for v in groups.values())} entries.")


if __name__ == "__main__":
    main()
