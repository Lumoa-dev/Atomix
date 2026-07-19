//! Atomix 编译器。
//!
//! 完成 .atx 源码到 .atxe 二进制产物的完整编译管线：
//!
//! 词法分析 → 语法分析 → 语义分析 → IR 生成 → 优化 → 链接
//!
//! 详见 docs/04-编译管线.md。

pub mod ast;
pub mod builtins;
pub mod codegen;
pub mod lexer;
pub mod linker;
pub mod parser;
pub mod semantic;
pub mod symbol;
pub mod token;
pub mod type_checker;

use crate::compiler::ast::*;
use crate::compiler::codegen::assembly;
use crate::compiler::codegen::expr::{ConstPool, reset_vreg};
use crate::compiler::codegen::instr::InstrEmitter;
use crate::compiler::codegen::stmt;
use crate::compiler::lexer::Lexer;
use crate::compiler::parser::Parser;
use crate::compiler::semantic::SemanticAnalyzer;

/// 完整编译管线：.atx 源码 → .atxe 二进制字节。
///
/// `opt_level`: 0=O0(无), 1=O1, 2=O2, s=Os
/// 返回 (atxe_bytes, 错误列表)。
pub fn compile(source: &str, opt_level: &str) -> (Vec<u8>, Vec<String>) {
    let mut errors: Vec<String> = Vec::new();
    let source_lines: Vec<&str> = source.lines().collect();

    /// 格式化带源码行的错误信息。
    fn fmt_error(msg: &str, line: usize, col: usize, source_lines: &[&str]) -> String {
        let mut out = String::new();
        out.push_str(msg);
        out.push_str(&format!("\n  ┌─ 第 {} 行 第 {} 列", line, col));
        if line > 0 && line <= source_lines.len() {
            out.push_str("\n  │\n");
            out.push_str(&format!("{:>4}│ {}", line, source_lines[line - 1]));
            out.push_str("\n  │ ");
            for _ in 1..col {
                out.push(' ');
            }
            out.push('^');
        }
        out
    }

    // 1. 词法分析
    let (tokens, lex_errors) = Lexer::new(source).tokenize();
    for e in &lex_errors {
        errors.push(fmt_error(
            &e.to_string(),
            e.span.start.line,
            e.span.start.col,
            &source_lines,
        ));
    }
    if !lex_errors.is_empty() {
        return (Vec::new(), errors);
    }

    // 2. 语法分析
    let (ast, parse_errors) = Parser::new(tokens).parse();
    for e in &parse_errors {
        errors.push(fmt_error(
            &e.to_string(),
            e.span.start.line,
            e.span.start.col,
            &source_lines,
        ));
    }
    if !parse_errors.is_empty() {
        return (Vec::new(), errors);
    }

    // 3. 语义分析
    let mut analyzer = SemanticAnalyzer::new();
    analyzer.analyze(ast);
    for e in &analyzer.errors {
        let (line, col) = match &e.span {
            Some(span) => (span.start.line, span.start.col),
            None => (0, 0),
        };
        errors.push(fmt_error(&e.to_string(), line, col, &source_lines));
    }
    for w in &analyzer.warnings {
        errors.push(w.to_string());
    }
    if !analyzer.errors.is_empty() {
        return (Vec::new(), errors);
    }

    // 4. 代码生成
    reset_vreg();
    let mut emit = InstrEmitter::new();
    let mut pool = ConstPool::new();
    let mut exn_entries: Vec<assembly::ExnEntry> = Vec::new();
    // 记录每个 zone 的指令区间 (kind, name, text_start, text_end)
    let mut zone_ranges: Vec<(ZoneKind, String, usize, usize)> = Vec::new();

    // 编译所有 zone 体
    for zone_info in &analyzer.zones {
        let text_start = emit.instr_count();
        stmt::compile_stmts(&mut emit, &mut pool, &zone_info.body, &mut exn_entries);
        let text_end = emit.instr_count();
        zone_ranges.push((
            zone_info.kind,
            zone_info.name.clone().unwrap_or_default(),
            text_start,
            text_end,
        ));
    }

    // 编译单态化函数体（在符号表中以 "func::type" 命名）
    for sym in analyzer.symbols.functions() {
        if sym.name.contains("::")
            && let Some(func_def) = &sym.func_def {
                // 为单态化函数生成标签和指令
                emit.emit_label(&sym.name);
                // 为参数分配虚拟寄存器（简化实现）
                for param in &func_def.params {
                    let _ = crate::compiler::codegen::expr::alloc_vreg();
                    let _ = param;
                }
                stmt::compile_stmts(&mut emit, &mut pool, &func_def.body, &mut exn_entries);
                // 函数末尾隐式 return
                emit.emit_r1i(
                    crate::base::isa::opcode::JMPR,
                    crate::base::isa::reg::RA as u8,
                    0,
                );
            }
    }

    emit.resolve_all();

    // 5. 优化（根据参数选择优化级别）
    let opt: crate::compiler::codegen::optimizer::OptLevel = opt_level
        .parse()
        .unwrap_or(crate::compiler::codegen::optimizer::OptLevel::O1);
    let mut optimizer = crate::compiler::codegen::optimizer::Optimizer::new(opt);
    let optimized_text = optimizer.optimize(&emit.text);

    // 6. 寄存器分配
    let mut reg_alloc = crate::compiler::codegen::reg_alloc::RegAllocator::new();
    reg_alloc.allocate(&optimized_text);
    let text = reg_alloc.insert_spill_code(&optimized_text);

    // 7. 汇编为 .atxe
    let mut final_emit = emit.clone();
    final_emit.text = text;
    let result = assembly::assemble(&final_emit, &pool.data, 0, &zone_ranges, &exn_entries);

    (result, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_simple_program() {
        let source = "TASK : { x : int = 42 }";
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());

        // 验证 .atxe 可解码
        let decoded = crate::base::ir::AtxeBinary::from_bytes(&bytes);
        assert!(decoded.is_some());
        let binary = decoded.unwrap();
        assert_eq!(binary.header.total_instrs, binary.text.len() as u32);
        assert!(binary.text.len() > 0);
    }

    #[test]
    fn end_to_end_with_expressions() {
        let source = r#"TASK : {
            x : int = 2 + 3 * 4
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn end_to_end_with_control_flow() {
        let source = r#"TASK : {
            IF true {
                x : int = 1
            } ELSE {
                x : int = 2
            }
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn compile_error_propagates() {
        let source = "TASK : { x : str = 42 }"; // type mismatch
        let (_, errors) = compile(source, "0");
        assert!(!errors.is_empty());
    }

    #[test]
    fn end_to_end_with_functions() {
        let source = r#"TOOLS : { fn add(a : int, b : int) : int { a + b } }
        TASK : {
            CALL add(1, 2) => result
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn empty_program_does_not_crash() {
        let source = "";
        let (_, _) = compile(source, "0");
        // 空程序不会崩溃即可
    }

    #[test]
    fn end_to_end_with_generic_function() {
        let source = r#"TOOLS : {
            fn identity<T>(x : T) : T { x }
        }
        TASK : {
            CALL identity(42) => result
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
        // 验证 .atxe 可解码
        let decoded = crate::base::ir::AtxeBinary::from_bytes(&bytes);
        assert!(decoded.is_some());
        let binary = decoded.unwrap();
        assert!(binary.text.len() > 0);
    }

    #[test]
    fn end_to_end_generic_multiple_types() {
        let source = r#"TOOLS : {
            fn identity<T>(x : T) : T { x }
        }
        TASK : {
            CALL identity(42) => a
            CALL identity("hello") => b
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn end_to_end_with_print_builtin() {
        // print 是最简单的 ECALL 类内置函数
        let source = r#"TASK : {
            CALL print(42)
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn end_to_end_with_bool_int_float_builtins() {
        // 类型转换类内置函数（通过 IR 指令展开）
        let source = r#"TASK : {
            CALL int(3)
            CALL float(42)
            CALL bool(1)
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn end_to_end_with_abs_min_max_builtins() {
        // 数学类内置函数（通过 IR 序列展开）
        let source = r#"TASK : {
            CALL abs(-5)
            CALL min(3, 7)
            CALL max(10, 2)
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }
}
