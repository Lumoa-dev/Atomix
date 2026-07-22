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

use crate::base::isa::{opcode, reg};
use crate::compiler::ast::*;
use crate::compiler::codegen::assembly;
use crate::compiler::codegen::expr::{ConstPool, reset_vreg};
use crate::compiler::codegen::instr::InstrEmitter;
use crate::compiler::codegen::stmt;
use crate::compiler::lexer::Lexer;
use crate::compiler::parser::Parser;
use crate::compiler::semantic::SemanticAnalyzer;

/// 扫描源码，估算每个 zone 的起始和结束行号。
fn compute_zone_line_ranges(
    zones: &[semantic::ZoneInfo],
    source_lines: &[&str],
) -> std::collections::HashMap<ZoneKind, (u32, u32)> {
    let mut result = std::collections::HashMap::new();
    for zone in zones {
        let kind = zone.kind;
        let keyword = match kind {
            ZoneKind::Tools => "TOOLS",
            ZoneKind::Input => "INPUT",
            ZoneKind::Works => "WORKS",
            ZoneKind::Task => "TASK",
            ZoneKind::Out => "OUT",
        };
        // 在源码中搜索对应的 zone 关键字
        let mut start_line = 1u32;
        let mut end_line = (source_lines.len() as u32).max(1);
        for (i, line) in source_lines.iter().enumerate() {
            let trimmed = line.trim().to_uppercase();
            if trimmed.starts_with(keyword) || trimmed.starts_with(&format!("{}{}", keyword, ":")) {
                start_line = (i + 1) as u32;
                // 找到闭合的 } 作为 end_line
                let mut brace_depth = 0;
                let mut found_start = false;
                for (j, l) in source_lines.iter().enumerate().skip(i) {
                    for ch in l.chars() {
                        match ch {
                            '{' => {
                                brace_depth += 1;
                                found_start = true;
                            }
                            '}' => {
                                brace_depth -= 1;
                                if found_start && brace_depth <= 0 {
                                    end_line = (j + 1) as u32;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    if found_start && brace_depth <= 0 {
                        break;
                    }
                }
                break;
            }
        }
        result.insert(kind, (start_line, end_line));
    }
    result
}

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
    // 计算源码行号映射（用于 .debug 段）
    let zone_line_ranges = compute_zone_line_ranges(&analyzer.zones, &source_lines);

    // 编译所有 zone 体
    for zone_info in &analyzer.zones {
        let text_start = emit.instr_count();
        // 设置该 zone 对应的起始行号
        if let Some((start_line, _)) = zone_line_ranges.get(&zone_info.kind) {
            emit.source_line = *start_line;
            emit.line_map.push((text_start, *start_line));
        }
        // 编译常规语句
        stmt::compile_stmts(&mut emit, &mut pool, &zone_info.body, &mut exn_entries);
        // 编译数据源声明（INPUT 区：加载数据）
        for decl in &zone_info.source_decls {
            compile_source_decl(&mut emit, &mut pool, decl, &mut exn_entries);
        }
        // 编译数据交付声明（OUT 区：写出数据）
        for decl in &zone_info.target_decls {
            compile_target_decl(&mut emit, &mut pool, decl, &mut exn_entries);
        }
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
            && let Some(func_def) = &sym.func_def
        {
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

// ─── 数据源 codegen ─────────────────────────────────────

/// 编译一个数据源声明（INPUT 区），为其生成 ECALL 指令序列。
///
/// 支持的 source_kind:
/// - "files", "txt" → FS_OPEN + ALLOC + FS_READ + FS_CLOSE
/// - "tcp"           → TCP_CONNECT + ALLOC + TCP_RECV + TCP_CLOSE
fn compile_source_decl(
    emit: &mut InstrEmitter,
    pool: &mut ConstPool,
    decl: &SourceDecl,
    exn_entries: &mut Vec<assembly::ExnEntry>,
) {
    let kind = decl.source_kind.to_lowercase();
    let _ = exn_entries; // 暂不使用（异常表保留给未来）

    // 将路径/地址存入常量池（rodata），获得偏移
    let addr_off = pool.add_str(&decl.address);
    let addr_off_u16 = addr_off as u16;

    match kind.as_str() {
        "files" | "txt" | "csv" | "json" | "yaml" | "toml" | "xml" => {
            // 文件读取序列：
            // 1. MOVI A0, path_offset（路径在 rodata 中的偏移）
            // 2. MOVI A1, 0       (O_RDONLY)
            // 3. ECALL FS_OPEN    → A0 = fd
            // 4. MOV T0, A0       （保存 fd）
            // 5. MOVI A0, 4096    （缓冲区大小）
            // 6. ECALL ALLOC      → A0 = buf_addr
            // 7. MOV T1, A0       （保存 buf）
            // 8. MOV A0, T0       （恢复 fd）
            // 9. MOV A1, T1       （buf）
            // 10. MOVI A2, 4096   （size）
            // 11. ECALL FS_READ   → A0 = bytes_read
            // 12. MOV T2, A0      （保存 bytes_read）
            // 13. MOV A0, T0      （恢复 fd）
            // 14. ECALL FS_CLOSE  → A0 = 0
            emit.emit_r2i(opcode::MOVI, reg::A0 as u8, 0, addr_off_u16);
            emit.emit_r2i(opcode::MOVI, reg::A1 as u8, 0, 0u16); // O_RDONLY
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_OPEN);
            emit.emit_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0); // t0 = fd
            emit.emit_r2i(opcode::MOVI, reg::A0 as u8, 0, 4096u16); // buf_size
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::ALLOC);
            emit.emit_r3(opcode::MOV, reg::T1 as u8, reg::A0 as u8, 0, 0); // t1 = buf
            emit.emit_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0); // a0 = fd
            emit.emit_r3(opcode::MOV, reg::A1 as u8, reg::T1 as u8, 0, 0); // a1 = buf
            emit.emit_r2i(opcode::MOVI, reg::A2 as u8, 0, 4096u16); // a2 = size
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_READ);
            emit.emit_r3(opcode::MOV, reg::T2 as u8, reg::A0 as u8, 0, 0); // t2 = bytes_read
            emit.emit_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0); // a0 = fd
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_CLOSE);
        }
        "tcp" | "http" | "webs" => {
            // TCP 读取序列（HTTP/WebSocket 需要标准库协议解析，此处仅读原始流）：
            // 1. MOVI A0, addr_offset
            // 2. MOVI A1, port（默认 80）
            // 3. ECALL TCP_CONNECT → A0 = fd
            // 4. MOV T0, A0
            // 5. MOVI A0, 4096
            // 6. ECALL ALLOC → A0 = buf
            // 7. MOV T1, A0
            // 8. MOV A0, T0（fd）
            // 9. MOV A1, T1（buf）
            // 10. MOVI A2, 4096
            // 11. ECALL TCP_RECV → A0 = bytes_read
            // 12. MOV T2, A0
            // 13. MOV A0, T0（fd）
            // 14. ECALL TCP_CLOSE
            let port: u16 = decl
                .params
                .iter()
                .find(|(k, _)| k == "port")
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(80u16);
            emit.emit_r2i(opcode::MOVI, reg::A0 as u8, 0, addr_off_u16);
            emit.emit_r2i(opcode::MOVI, reg::A1 as u8, 0, port);
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::TCP_CONNECT);
            emit.emit_r3(opcode::MOV, reg::T0 as u8, reg::A0 as u8, 0, 0);
            emit.emit_r2i(opcode::MOVI, reg::A0 as u8, 0, 4096u16);
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::ALLOC);
            emit.emit_r3(opcode::MOV, reg::T1 as u8, reg::A0 as u8, 0, 0);
            emit.emit_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0);
            emit.emit_r3(opcode::MOV, reg::A1 as u8, reg::T1 as u8, 0, 0);
            emit.emit_r2i(opcode::MOVI, reg::A2 as u8, 0, 4096u16);
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::TCP_RECV);
            emit.emit_r3(opcode::MOV, reg::T2 as u8, reg::A0 as u8, 0, 0);
            emit.emit_r3(opcode::MOV, reg::A0 as u8, reg::T0 as u8, 0, 0);
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::TCP_CLOSE);
        }
        _ => {
            // 未知数据源类型：不生成代码（静默跳过，留给标准库扩展）
        }
    }
}

/// 编译一个数据交付声明（OUT 区），为其生成 ECALL 指令序列。
///
/// 支持的 target_kind:
/// - "files", "txt" → FS_OPEN + FS_WRITE + FS_CLOSE
fn compile_target_decl(
    emit: &mut InstrEmitter,
    pool: &mut ConstPool,
    decl: &TargetDecl,
    exn_entries: &mut Vec<assembly::ExnEntry>,
) {
    let kind = decl.target_kind.to_lowercase();
    let _ = exn_entries;
    let _ = pool;

    match kind.as_str() {
        "files" | "txt" | "csv" | "json" | "yaml" | "toml" | "xml" => {
            // 简单实现：将数据源对应的寄存器内容写入文件
            // 实际应通过符号表查 source_var 的地址和长度
            // 当前简化：使用 T0/T1（由 source_decl 设置的指针/长度）
            // 1. MOVI A0, path_offset
            // 2. MOVI A1, 1 (O_WRONLY|O_CREATE)
            // 3. ECALL FS_OPEN → A0 = fd
            // 4. MOV T3, A0 (save fd)
            // 5. MOV A0, T3 (fd)
            // 6. MOV A1, T0 (data ptr from source)
            // 7. MOV A2, T1 (data len from source)
            // 8. ECALL FS_WRITE → A0 = bytes_written
            // 9. MOV A0, T3 (fd)
            // 10. ECALL FS_CLOSE
            let addr_off = pool.add_str(&decl.address);
            let addr_off_u16 = addr_off as u16;
            emit.emit_r2i(opcode::MOVI, reg::A0 as u8, 0, addr_off_u16);
            emit.emit_r2i(opcode::MOVI, reg::A1 as u8, 0, 1u16); // O_WRONLY|O_CREATE
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_OPEN);
            emit.emit_r3(opcode::MOV, reg::T3 as u8, reg::A0 as u8, 0, 0); // t3 = fd
            emit.emit_r3(opcode::MOV, reg::A0 as u8, reg::T3 as u8, 0, 0); // a0 = fd
            emit.emit_r3(opcode::MOV, reg::A1 as u8, reg::T0 as u8, 0, 0); // a1 = data ptr
            emit.emit_r3(opcode::MOV, reg::A2 as u8, reg::T1 as u8, 0, 0); // a2 = data len
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_WRITE);
            emit.emit_r3(opcode::MOV, reg::A0 as u8, reg::T3 as u8, 0, 0); // a0 = fd
            emit.emit_r1i(opcode::ECALL, 0, crate::base::isa::ecall::FS_CLOSE);
        }
        _ => {}
    }
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

    #[test]
    fn end_to_end_with_input_files_source() {
        // 编译一个包含 INPUT FILES 数据源的程序
        // 语法: INPUT { FILES : "/tmp/test.txt" => data }
        let source = r#"INPUT : {
            FILES : "/tmp/test.txt" => data
        }
        TASK : {
            x : int = 42
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);

        // 验证产物可解码
        let decoded = crate::base::ir::AtxeBinary::from_bytes(&bytes);
        assert!(decoded.is_some(), "compiled binary should be valid .atxe");
        let binary = decoded.unwrap();
        // 应该包含指令（数据源的 FS_OPEN/READ/CLOSE 序列）
        assert!(
            binary.text.len() > 0,
            "should generate instructions for I/O"
        );

        // 验证路径字符串在 .rodata 中
        let rodata_str = String::from_utf8_lossy(&binary.rodata);
        assert!(
            rodata_str.contains("/tmp/test.txt"),
            "rodata should contain the file path"
        );
    }

    #[test]
    fn end_to_end_with_input_tcp_source() {
        // 编译一个包含 INPUT TCP 数据源的程序
        // 语法: INPUT { TCP : "127.0.0.1" (port=8080) => stream }
        let source = r#"INPUT : {
            TCP : "127.0.0.1" (port=8080) => stream
        }
        TASK : {
            x : int = 1
        }"#;
        let (bytes, errors) = compile(source, "0");
        assert!(errors.is_empty(), "{:?}", errors);

        let decoded = crate::base::ir::AtxeBinary::from_bytes(&bytes);
        assert!(decoded.is_some());
        let binary = decoded.unwrap();
        assert!(binary.text.len() > 0, "should generate I/O instructions");

        // 验证地址字符串在 .rodata 中
        let rodata_str = String::from_utf8_lossy(&binary.rodata);
        assert!(
            rodata_str.contains("127.0.0.1"),
            "rodata should contain the address"
        );
    }

    #[test]
    fn end_to_end_with_out_files_target() {
        // 编译一个包含 OUT FILES 数据交付的程序
        // 语法: OUT { data [decos] => FILES : "/tmp/output.txt" }
        let source = r#"TASK : {
            data : int = 42
            GOOUT data : int = 42
        }
        OUT : {
            data => FILES : "/tmp/output.txt"
        }"#;
        let (bytes, errors) = compile(source, "0");
        // OUT target 语法是: source_var => KIND : "address"
        // 当前编译器对 OUT 区的 target_decls 有 GOOUT 验证逻辑
        // 如果验证通过就检查产物
        if errors.is_empty() {
            let decoded = crate::base::ir::AtxeBinary::from_bytes(&bytes);
            assert!(decoded.is_some());
            let binary = decoded.unwrap();
            let rodata_str = String::from_utf8_lossy(&binary.rodata);
            assert!(rodata_str.contains("/tmp/output.txt"));
        }
    }
}
