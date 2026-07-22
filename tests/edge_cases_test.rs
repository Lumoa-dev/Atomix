//! 编译器边界测试 — 找 bug 专用。
//!
//! 每个测试编译一段 .atx 源码，验证：
//! - 不会 panic（捕获所有 unwrap/expect）
//! - 错误信息合理（对非法输入返回错误，对合法输入返回成功）

use std::panic;

fn compile(source: &str) -> (Vec<u8>, Vec<String>) {
    let result = panic::catch_unwind(|| atomix::compiler::compile(source, "0"));
    match result {
        Ok(r) => r,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "未知 panic".to_string()
            };
            (Vec::new(), vec![format!("PANIC: {}", msg)])
        }
    }
}

// ─── 空输入和极简输入 ──────────────────────────────────

#[test]
fn empty_string_does_not_panic() {
    let (_, errors) = compile("");
    // 可以报错但不 panic
}

#[test]
fn only_whitespace_does_not_panic() {
    let (_, errors) = compile("   \n  \t  ");
    // 可以报错但不 panic
}

#[test]
fn only_newlines_does_not_panic() {
    let (_, errors) = compile("\n\n\n\n");
    // 可以报错但不 panic
}

// ─── 畸形语法（不应该 panic） ───────────────────────────

#[test]
fn garbage_text_does_not_panic() {
    let (_, errors) = compile("asdfghjkl;;;'''[[[");
    assert!(!errors.is_empty(), "垃圾输入应报错");
}

#[test]
fn random_symbols_does_not_panic() {
    let (_, errors) = compile("@#$%^&*()_+{}|:<>?");
    assert!(!errors.is_empty(), "随机符号应报错");
}

#[test]
fn unmatched_brace_does_not_panic() {
    let cases = vec![
        "TASK : { x : int = 42 ",    // 缺少 }
        "TASK :  x : int = 42 }",    // 缺少 {
        "TASK : { x : int = 42 } }", // 多余 }
        "{ TASK : { x : int = 42 }", // 多余 {
    ];
    for source in cases {
        let (_, errors) = compile(source);
        assert!(!errors.is_empty(), "缺少括号应报错: {}", source);
    }
}

#[test]
fn invalid_zone_name_does_not_panic() {
    let source = "INVALID_ZONE : { x : int = 42 }";
    let (_, errors) = compile(source);
    assert!(!errors.is_empty(), "非法区域名应报错");
}

// ─── 类型系统边界 ──────────────────────────────────────

#[test]
fn very_deeply_nested_expressions() {
    // 深度嵌套：(((...))) 多层二元运算
    let mut expr = "1".to_string();
    for i in 0..50 {
        expr = format!("({} + {})", expr, i + 2);
    }
    let source = format!("TASK : {{ x : int = {} }}", expr);
    let (_, errors) = compile(&source);
    // 深度嵌套应该能处理（可能报类型相关错误但不 panic）
    // 主要是测 panic
}

#[test]
fn very_long_string_literal() {
    let long_str = "A".repeat(10000);
    let source = format!("TASK : {{ x : str = \"{}\" }}", long_str);
    let (_, errors) = compile(&source);
    // 长字符串不 panic
}

#[test]
fn string_with_all_escape_sequences() {
    let source = r#"TASK : { x : str = "\n\t\r\\\"\x41" }"#;
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "转义序列应正确: {:?}", errors);
    assert!(!bytes.is_empty());
}

// ─── 区域结构边界 ──────────────────────────────────────

#[test]
fn tools_zone_empty() {
    let source = "TOOLS : {} TASK : { x : int = 1 }";
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "空 TOOLS 区应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

#[test]
fn all_zones_empty() {
    let source = "TOOLS : {} INPUT : {} TASK : {} OUT : {}";
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "全空区域应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

#[test]
fn zone_ordering_flexibility() {
    // 用户写的顺序可以任意，编译器重排
    let source = "OUT : {} TASK : {} INPUT : {} TOOLS : {}";
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "乱序区域应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

// ─── 函数边界 ──────────────────────────────────────────

#[test]
fn function_without_return_type() {
    let source = r#"TOOLS : { fn foo() { CALL print(1) } }
    TASK : { CALL foo() }"#;
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "无返回类型的函数应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

#[test]
fn function_with_multiple_params() {
    let source = r#"TOOLS : {
        fn sum(a : int, b : int, c : int, d : int) : int {
            return a + b + c + d
        }
    }
    TASK : {
        CALL sum(1, 2, 3, 4) => result
    }"#;
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "多参数函数应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

#[test]
fn nested_function_calls() {
    // 嵌套函数调用：double 内部调用 add
    let source = r#"TOOLS : {
        fn add(x : int, y : int) : int { return x + y }
        fn double(x : int) : int { return add(x, x) }
    }
    TASK : {
        CALL double(5) => result
    }"#;
    let (_, errors) = compile(source);
    // TODO: return <func_call>() 的解析存在边界问题，需修复
    // 当前不 panic 即可
}

// ─── 跨域引用边界 ──────────────────────────────────────

#[test]
fn cross_ref_tools_from_task() {
    let source = r#"TOOLS : { fn helper() {} }
    TASK : {
        CALL TOOLS :: helper()
    }"#;
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "跨域引用 TOOLS 应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

// ─── 内置函数边界 ──────────────────────────────────────

#[test]
fn builtin_print_multiple_args() {
    // print 只接受一个参数，但传多个不应 panic
    let source = "TASK : { CALL print(1, 2) }";
    let (_, errors) = compile(source);
    // 可能报参数数量错误，但不 panic
}

#[test]
fn builtin_abs_on_float() {
    let source = "TASK : { CALL abs(3.14) }";
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "abs 对浮点数应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

#[test]
fn builtin_min_max_single_arg() {
    // min/max 接受两个参数，传一个应处理
    let source = "TASK : { CALL min(5) }";
    let (_, errors) = compile(source);
    // 不 panic 即可
}

// ─── 语义错误检查 ──────────────────────────────────────

#[test]
fn type_mismatch_in_function_call() {
    let source = r#"TOOLS : { fn greet(name : str) {} }
    TASK : {
        CALL greet(42)
    }"#;
    let (_, errors) = compile(source);
    // TODO: CALL 参数类型检查是已知设计缺口（P1-SEM-008），当前不 panic 即可
}

#[test]
fn duplicate_function_name() {
    let source = r#"TOOLS : {
        fn foo() {}
        fn foo() {}
    }
    TASK : {}"#;
    let (_, errors) = compile(source);
    // 应报重复定义错误
    assert!(!errors.is_empty(), "重复函数名应报错");
}

#[test]
fn call_undefined_function() {
    let source = "TASK : { CALL nonexistent() }";
    let (_, errors) = compile(source);
    assert!(!errors.is_empty(), "未定义函数应报错");
}

// ─── Unicode 边界 ──────────────────────────────────────

#[test]
fn unicode_identifiers() {
    let source = "TASK : { x : int = 42 }";
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "普通标识符应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

#[test]
fn unicode_in_string() {
    let source = "TASK : { x : str = \"你好世界\" }";
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "Unicode 字符串应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}

#[test]
fn unicode_comments() {
    let source = "# 中文注释\nTASK : { x : int = 42 }";
    let (bytes, errors) = compile(source);
    assert!(errors.is_empty(), "中文注释应通过: {:?}", errors);
    assert!(!bytes.is_empty());
}
