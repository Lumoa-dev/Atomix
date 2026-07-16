"""
Build a unified PDF from all Atomix documentation markdown files.

Pipeline:
  1. Concatenate all .md files in logical reading order
  2. Convert to a single HTML file via Python markdown library
  3. Inject mermaid.js for diagram rendering
  4. Apply print-optimized CSS with CJK font support
  5. Use Playwright to render the final PDF (vector output)
"""

import os
import re
import markdown
from pathlib import Path

ROOT = Path(__file__).parent
DOCS_DIR = ROOT / "docs"
ASSETS_DIR = ROOT / ".assets"
OUTPUT_DIR = ROOT / "output"
OUTPUT_DIR.mkdir(exist_ok=True)

# ── File order: logical reading sequence ──
FILE_ORDER = [
    "01-总纲与哲学.md",
    "02-指令集规范.md",
    "编译行为.md",
    "编译管线.md",
    "运行时架构.md",
    "策略模块.md",
    "执行器设计.md",
    "配置设计.md",
    "外围工具.md",
    "语法设计/通用语法.md",
    "语法设计/类型系统.md",
    "语法设计/区外语法.md",
    "语法设计/INPUT语法.md",
    "语法设计/OUT语法.md",
    "语法设计/TASK语法.md",
    "语法设计/WORKS语法.md",
    "语法设计/TOOLS语法.md",
    "语法设计/内置函数.md",
    "语法设计/标准库.md",
    "语法设计/包管理.md",
    "语法设计/关键字参考.md",
    "语法设计/附录/钩子与IS值参考.md",
    "语法设计/附录/默认装饰器参考.md",
    "语法设计/附录/数据源地址与参数速查.md",
]


def read_all_markdown() -> str:
    """Concatenate all markdown files in order."""
    parts = []
    for fname in FILE_ORDER:
        fpath = DOCS_DIR / fname
        if not fpath.exists():
            print(f"  ⚠ 跳过（不存在）: {fname}")
            continue
        content = fpath.read_text(encoding="utf-8")
        # Remove YAML front matter if present (--- ... ---)
        content = re.sub(r'^---\n.*?\n---\n', '', content, flags=re.DOTALL)
        parts.append(content)
        print(f"  ✓ {fname}")
    return "\n\n" + "\n\n".join(parts)


def fix_image_paths(html: str) -> str:
    """Image paths in markdown are ../.assets/xxx.png (relative to docs/).
    HTML is at output/atomix_full.html, so ../.assets/ from output/ points to root .assets/.
    These paths are already correct — no transformation needed."""
    return html


def fix_mermaid_blocks(html: str) -> str:
    """Convert ```mermaid code blocks to <div class='mermaid'> for mermaid.js rendering."""
    # The markdown library converts ```mermaid to <code class="language-mermaid">
    # We need to: find all <pre><code class="language-mermaid">...</code></pre>
    # and replace them with <div class="mermaid">...</div>
    pattern = re.compile(
        r'<pre><code class="language-mermaid">(.*?)</code></pre>',
        re.DOTALL
    )
    def replacer(m):
        code = m.group(1)
        # Unescape HTML entities inside the mermaid code
        code = code.replace('&lt;', '<').replace('&gt;', '>').replace('&amp;', '&').replace('&quot;', '"')
        return f'<div class="mermaid">\n{code}\n</div>'
    return pattern.sub(replacer, html)


CSS = """
:root {
    --page-width: 210mm;
    --page-margin: 20mm 22mm 20mm 22mm;
    --font-body: 'Microsoft YaHei', 'Noto Sans SC', 'SimHei', 'Segoe UI', sans-serif;
    --font-mono: 'Cascadia Code', 'Fira Code', 'Consolas', 'Courier New', monospace;
    --c-text: #1a1a1a;
    --c-muted: #666666;
    --c-accent: #2c5f8a;
    --c-border: #dddddd;
    --c-code-bg: #f4f4f4;
}

@page {
    size: A4;
    margin: 20mm 22mm 20mm 22mm;
    @bottom-center {
        content: counter(page);
        font-family: var(--font-body);
        font-size: 9pt;
        color: var(--c-muted);
    }
}

@page :first {
    @bottom-center {
        content: none;
    }
}

html, body {
    margin: 0;
    padding: 0;
    font-family: var(--font-body);
    font-size: 11pt;
    line-height: 1.75;
    color: var(--c-text);
    background: white;
}

/* ── Cover page ── */
.cover {
    page-break-after: always;
    text-align: center;
    padding-top: 120px;
    padding-bottom: 80px;
}
.cover h1 {
    font-size: 32pt;
    font-weight: 800;
    margin-bottom: 12pt;
    color: var(--c-accent);
    letter-spacing: 2pt;
}
.cover .subtitle {
    font-size: 16pt;
    color: var(--c-muted);
    margin-bottom: 40pt;
}
.cover .meta {
    font-size: 10pt;
    color: var(--c-muted);
    margin-top: 60pt;
}
.cover .meta p { margin: 4pt 0; }

/* ── TOC ── */
.toc {
    page-break-after: always;
}
.toc h2 {
    font-size: 18pt;
    margin-bottom: 16pt;
    color: var(--c-accent);
}
.toc ul {
    list-style: none;
    padding-left: 0;
}
.toc li {
    padding: 3pt 0;
    border-bottom: 1px dotted var(--c-border);
}
.toc li a {
    text-decoration: none;
    color: var(--c-text);
}
.toc li a::after {
    content: leader('.') target-counter(attr(href), page);
    float: right;
}

/* ── Content ── */
h1 {
    font-size: 22pt;
    font-weight: 700;
    color: var(--c-accent);
    margin-top: 36pt;
    margin-bottom: 16pt;
    page-break-before: always;
    page-break-after: avoid;
    border-bottom: 2px solid var(--c-accent);
    padding-bottom: 6pt;
}
h1:first-of-type { page-break-before: avoid; }

h2 {
    font-size: 15pt;
    font-weight: 700;
    color: var(--c-accent);
    margin-top: 28pt;
    margin-bottom: 10pt;
    page-break-after: avoid;
}

h3 {
    font-size: 12.5pt;
    font-weight: 700;
    color: #333;
    margin-top: 20pt;
    margin-bottom: 8pt;
    page-break-after: avoid;
}

h4, h5, h6 {
    font-size: 11pt;
    font-weight: 700;
    margin-top: 16pt;
    margin-bottom: 6pt;
    page-break-after: avoid;
}

p {
    margin: 6pt 0 8pt 0;
    text-align: justify;
}

blockquote {
    margin: 12pt 0;
    padding: 6pt 16pt;
    border-left: 4px solid var(--c-accent);
    background: #f0f5fa;
    font-size: 10pt;
    color: #555;
}

/* ── Tables ── */
table {
    width: 100%;
    border-collapse: collapse;
    margin: 14pt 0;
    font-size: 9.5pt;
    page-break-inside: avoid;
}
thead { display: table-header-group; }
th {
    background: var(--c-accent);
    color: white;
    padding: 6pt 8pt;
    font-weight: 700;
    text-align: left;
}
td {
    padding: 5pt 8pt;
    border-bottom: 1px solid var(--c-border);
}
tr:nth-child(even) td { background: #fafafa; }

/* ── Code blocks ── */
pre {
    background: var(--c-code-bg);
    border: 1px solid var(--c-border);
    border-radius: 3px;
    padding: 10pt 14pt;
    font-family: var(--font-mono);
    font-size: 8.5pt;
    line-height: 1.5;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-all;
    page-break-inside: avoid;
}
code {
    font-family: var(--font-mono);
    font-size: 9pt;
    background: var(--c-code-bg);
    padding: 1pt 3pt;
    border-radius: 2px;
}
pre code {
    background: none;
    padding: 0;
}

/* ── Mermaid diagrams ── */
.mermaid {
    margin: 16pt auto;
    padding: 12pt;
    text-align: center;
    page-break-inside: avoid;
}
.mermaid svg {
    max-width: 100%;
    height: auto;
}

/* ── Images ── */
img {
    max-width: 100%;
    height: auto;
    display: block;
    margin: 14pt auto;
    page-break-inside: avoid;
}

/* ── Lists ── */
ul, ol {
    margin: 6pt 0;
    padding-left: 24pt;
}
li { margin: 3pt 0; }

/* ── Horizontal rules ── */
hr {
    border: none;
    border-top: 1px solid var(--c-border);
    margin: 24pt 0;
}

/* ── Print helpers ── */
@media print {
    body { print-color-adjust: exact; -webkit-print-color-adjust: exact; }
}
"""


def build_html(md_content: str) -> str:
    """Convert markdown to HTML with all processing."""
    # Convert markdown to HTML
    extensions = [
        'markdown.extensions.tables',
        'markdown.extensions.fenced_code',
        'markdown.extensions.codehilite',
        'markdown.extensions.toc',
        'markdown.extensions.sane_lists',
        'markdown.extensions.smarty',
    ]
    html_body = markdown.markdown(md_content, extensions=extensions)

    # Fix mermaid blocks
    html_body = fix_mermaid_blocks(html_body)

    # Fix image paths
    html_body = fix_image_paths(html_body)

    # Extract the first h1 as title for cover
    title_match = re.search(r'<h1>(.*?)</h1>', html_body)
    doc_title = title_match.group(1) if title_match else "Atomix 技术文档"

    # Build full HTML document
    full_html = f"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>{doc_title}</title>
<script src="https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js"></script>
<script>
mermaid.initialize({{
    startOnLoad: true,
    theme: 'neutral',
    securityLevel: 'loose',
    flowchart: {{ useMaxWidth: true, htmlLabels: true, curve: 'basis' }},
    sequence: {{ useMaxWidth: true, diagramMarginX: 30, diagramMarginY: 20 }},
    state: {{ useMaxWidth: true }},
}});
</script>
<style>
{CSS}
</style>
</head>
<body>

<!-- Cover Page -->
<div class="cover">
    <h1>Atomix</h1>
    <div class="subtitle">任务执行 DSL — 完整技术规格书</div>
    <div class="meta">
        <p>架构版本: v0.2 (设计阶段)</p>
        <p>文档生成日期: 2026-07-16</p>
        <p>包含 24 篇设计文档 · 30+ 流程图 · 14 张数据图表</p>
    </div>
</div>

<!-- Table of Contents -->
<div class="toc">
    <h2>目 录</h2>
    <nav id="toc"></nav>
</div>

<!-- Main Content -->
<div class="content">
{html_body}
</div>

<!-- Auto-generate TOC on load -->
<script>
(function() {{
    const toc = document.getElementById('toc');
    if (!toc) return;
    const content = document.querySelector('.content');
    const headings = content.querySelectorAll('h1, h2');
    const ul = document.createElement('ul');
    headings.forEach((h, i) => {{
        const id = 'section-' + i;
        h.id = id;
        const li = document.createElement('li');
        const a = document.createElement('a');
        a.href = '#' + id;
        a.textContent = h.textContent;
        if (h.tagName === 'H2') li.style.paddingLeft = '18pt';
        li.appendChild(a);
        ul.appendChild(li);
    }});
    toc.appendChild(ul);
}})();
</script>

</body>
</html>"""
    return full_html


def main():
    print("=" * 60)
    print("Atomix Docs → Unified PDF Builder")
    print("=" * 60)

    # Step 1: Read all markdown
    print("\n[1/4] 读取 Markdown 文件...")
    md_content = read_all_markdown()
    print(f"  总字符数: {len(md_content):,}")

    # Step 2: Convert to HTML
    print("\n[2/4] 转换为 HTML...")
    html = build_html(md_content)
    html_path = OUTPUT_DIR / "atomix_full.html"
    html_path.write_text(html, encoding="utf-8")
    print(f"  已保存: {html_path} ({html_path.stat().st_size:,} bytes)")

    # Step 3: Open in browser (manual step hint)
    print(f"\n[3/4] HTML 已就绪，请用浏览器打开后等待 Mermaid 图渲染完成")
    print(f"  文件: file:///{html_path.as_posix()}")

    # Step 4: Provide Playwright rendering command
    print(f"\n[4/4] 使用 Playwright 渲染 PDF（需先确保 mermaid 图在浏览器中渲染完成）")
    pdf_path = (OUTPUT_DIR / "atomix_full.pdf").as_posix()
    html_url = f"file:///{html_path.as_posix()}"

    print(f"""
    # 方法 1: 用 Chrome 打开 → Ctrl+P → 另存为 PDF
    # 方法 2: 用以下 Playwright 脚本:

    # 保存以下内容到 render_pdf.js，然后 node render_pdf.js
""")

    # Write the Playwright render script
    render_js = f"""// render_pdf.js — Render the HTML to PDF via Playwright
const {{ chromium }} = require('playwright');

(async () => {{
    const browser = await chromium.launch();
    const page = await browser.newPage();

    // Load the HTML
    await page.goto('{html_url}', {{ waitUntil: 'networkidle' }});

    // Wait for Mermaid diagrams to render
    await page.waitForFunction(() => {{
        const mermaids = document.querySelectorAll('.mermaid');
        if (mermaids.length === 0) return true; // no mermaid diagrams
        return Array.from(mermaids).every(el => el.querySelector('svg'));
    }}, {{ timeout: 30000 }});

    // Extra wait for any remaining rendering
    await page.waitForTimeout(2000);

    // Generate PDF
    await page.pdf({{
        path: '{pdf_path}',
        format: 'A4',
        margin: {{ top: '20mm', bottom: '20mm', left: '22mm', right: '22mm' }},
        printBackground: true,
        displayHeaderFooter: true,
        headerTemplate: '<span></span>',
        footerTemplate: '<div style="font-size:9px;text-align:center;width:100%%;color:#999;"><span class="pageNumber"></span> / <span class="totalPages"></span></div>',
    }});

    console.log('PDF saved: {pdf_path}');
    await browser.close();
}})();
"""
    render_path = OUTPUT_DIR / "render_pdf.js"
    render_path.write_text(render_js, encoding="utf-8")
    print(f"  Playwright 脚本已保存: {render_path}")
    print(f"\n  运行: cd output && node render_pdf.js")

    return html_path


if __name__ == "__main__":
    main()
