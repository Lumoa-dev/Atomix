//! Atomix 编译器。
//!
//! 完成 .atx 源码到 .atxe 二进制产物的完整编译管线：
//!
//! 词法分析 → 语法分析 → 语义分析 → IR 生成 → 优化 → 链接
//!
//! 详见 docs/04-编译管线.md。

pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod symbol;
pub mod type_checker;
pub mod semantic;
pub mod codegen;

use crate::compiler::ast::*;
use crate::compiler::codegen::assembly;
use crate::compiler::codegen::expr::{reset_vreg, ConstPool};
use crate::compiler::codegen::instr::InstrEmitter;
use crate::compiler::codegen::stmt;
use crate::compiler::lexer::Lexer;
use crate::compiler::parser::Parser;
use crate::compiler::semantic::SemanticAnalyzer;

/// 完整编译管线：.atx 源码 → .atxe 二进制字节。
///
/// 返回 (atxe_bytes, 错误列表)。
pub fn compile(source: &str) -> (Vec<u8>, Vec<String>) {
    let mut errors = Vec::new();

    // 1. 词法分析
    let (tokens, lex_errors) = Lexer::new(source).tokenize();
    for e in &lex_errors {
        errors.push(format!("词法错误: {}", e.message));
    }
    if !lex_errors.is_empty() {
        return (Vec::new(), errors);
    }

    // 2. 语法分析
    let (ast, parse_errors) = Parser::new(tokens).parse();
    for e in &parse_errors {
        errors.push(format!("语法错误: {}", e.message));
    }
    if !parse_errors.is_empty() {
        return (Vec::new(), errors);
    }

    // 3. 语义分析
    let mut analyzer = SemanticAnalyzer::new();
    analyzer.analyze(ast);
    for e in &analyzer.errors {
        errors.push(format!("语义错误: {}", e.message));
    }
    if !analyzer.errors.is_empty() {
        return (Vec::new(), errors);
    }

    // 4. 代码生成
    reset_vreg();
    let mut emit = InstrEmitter::new();
    let mut pool = ConstPool::new();

    // 编译所有 zone 体
    for zone_info in &analyzer.zones {
        stmt::compile_stmts(&mut emit, &mut pool, &zone_info.body);
    }

    emit.resolve_all();

    // 5. 寄存器分配
    let mut reg_alloc = crate::compiler::codegen::reg_alloc::RegAllocator::new();
    reg_alloc.allocate(&emit.text);
    let text = reg_alloc.insert_spill_code(&emit.text);

    // 6. 汇编为 .atxe
    let zones_meta: Vec<(ZoneKind, String)> = analyzer.zones.iter()
        .map(|z| (z.kind, z.name.clone().unwrap_or_default()))
        .collect();

    let mut final_emit = emit.clone();
    final_emit.text = text;
    let result = assembly::assemble(&final_emit, &pool.data, 0, &zones_meta);

    (result, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_simple_program() {
        let source = "TASK : { x : int = 42 }";
        let (bytes, errors) = compile(source);
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
        let (bytes, errors) = compile(source);
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
        let (bytes, errors) = compile(source);
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn compile_error_propagates() {
        let source = "TASK : { x : str = 42 }"; // type mismatch
        let (_, errors) = compile(source);
        assert!(!errors.is_empty());
    }

    #[test]
    fn end_to_end_with_functions() {
        let source = r#"TOOLS : { fn add(a : int, b : int) : int { a + b } }
        TASK : {
            CALL add(1, 2) => result
        }"#;
        let (bytes, errors) = compile(source);
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn empty_program_does_not_crash() {
        let source = "";
        let (_, _) = compile(source);
        // 空程序不会崩溃即可
    }
}
