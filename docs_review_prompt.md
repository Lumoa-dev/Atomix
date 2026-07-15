# Atomix Documentation Review — Actionable Summary

## Critical Contradictions (Fix Immediately)

1. **ISA §1.2 opcode table vs §3.8**: `0x80-0xEF` claimed as "reserved, 0 allocated" but MCPY(0x80) and MSET(0x81) already allocated. Also `0x70-0x7F` says "3 allocated" but only ECALL(0x70) documented — 2 phantom instructions.

2. **Syntax §5.2 (integer type suffixes u8/u16/u32/u64/i8/…/i64) vs Type System §2 (explicitly states "no narrow types, VM only handles 64-bit")**: Contradiction. Either remove suffixes from syntax or add narrow type support to type system.

3. **Compilation Behavior §1.1 uses `OUT : batch_data : list` in TASK region vs Common Syntax §10.2 uses `GOOUT result : i32 = CALL : compute()`**: Same mechanism (output variable annotation) uses two different keywords. Unify to one.

## Design Issues

- **Hook system (50+ hooks, 70+ IS* values) contradicts "lightweight" principle** — each hook requires VM runtime support. Justify overhead or cut drastically.
- **`$` pipe variable is syntactically orphaned**: Not an identifier (`$` prefix violates identifier rules), not a standard operator. `$[key]` reuses `[]` access syntax but `$` itself is a special form. Needs proper grammar classification.
- **`FROM "x" USE "y" :: Target` and cross-domain reference `TOOLS :: func` both use `::` separator** — lexer ambiguity risk.
- **NOT, NEG use R3 template** (3-reg + funct) but are unary ops — wastes encoding bits. Use R1I or R2I.
- **MCPY/MSET use R3 template** with `memcpy(rd_dst, rs1_src, rs2_len)` but `rd` = dest breaks memcpy convention order — document clearly or swap.
- **DB data source** parameters are "TBD" — too vague for implementer.
- **`WEBS` naming misleading**: suggests web/WebSocket, actually raw socket.

## Consistency Issues

- **Index §7 lists `语法设计/关键字参考.md`** — file does not exist. Also lists `IO语法.md` but actual files are `INPUT语法.md` and `OUT语法.md`.
- **Convention §1: "declaration/structure keywords = UPPERCASE"** contradicted by `fn` and `return` listed as lowercase in §4.2.
- **Illegal-cases table placement inconsistent**: Common Syntax embeds after each section + final summary; others only at end.
- **Cross-reference format inconsistent**: mix of `详见 doc.md §X` and `[link](path)` styles.
- **Code blocks mixed**: indented (4-space) vs fenced (```) without standard.

## Language/Tone Issues

- Heavy colloquialism throughout: `干完就撤`, `不搞`, `用不明白不关我事`, `语法不拦你`, `肝火比较旺`, `傻问题蠢问题`, `八九分像`, `连踹`
- Tone varies between sarcastic (WORKS), confrontational (TASK), neutral (Type System), placeholder (Runtime)
- All first-person commentary, dismissive phrasing, and meta-instructions to AI should be removed — rewrite as factual specifications.

## Missing Documentation

- **`语法设计/关键字参考.md`**: indexed in §7, file does not exist
- **Dynamic concurrency algorithm**: placeholder only — no actual formula, quota calculation, or scheduling policy
- **Runtime VM design**: execution loop, context switching, memory management — absent
- **Standard library**: referenced in package management but no design
- **Multi-file project model**: compilation units, project structure — undefined
- **Error propagation**: how syscall errors map to DSL-level exceptions — unspecified
- **Closure capture**: `do` anonymous function — no specification of variable capture semantics

## Quick Actionable Items (sorted by impact)

1. Fix ISA opcode space table → reconcile with actual allocation
2. Resolve type-suffix contradiction → delete suffixes or expand type system
3. Unify GOOUT / OUT : into one keyword
4. Audit and rewrite all unprofessional phrasing to neutral technical language
5. Delete or substantially justify hook system vs lightweight constraint
6. Create missing `关键字参考.md` or remove from index
7. Formalize `$` pipe variable grammar position
8. Standardize cross-reference format across all documents
9. Write actual runtime architecture specification
10. Ensure `::` disambiguation between import syntax and cross-domain reference
