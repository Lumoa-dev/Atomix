//! 编译器集成测试 — 读取 tests/fixtures/ 中的 .atx 文件并验证编译结果。
//!
//! valid/*.atx     → 编译必须 0 错误，产生非空 .atxe
//! invalid/*.atx   → 编译必须产生至少一个错误

use std::fs;
use std::path::Path;

/// 编译一个 .atx 文件，返回 (atxe_bytes, errors)。
fn compile_file(path: &Path) -> (Vec<u8>, Vec<String>) {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));
    atomix::compiler::compile(&source, "0")
}

/// 递归收集目录下所有 .atx 文件。
fn collect_atx_files(dir: &str) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("无法读取目录 {}: {}", dir, e));
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "atx") {
            files.push(path);
        }
    }
    files.sort();
    files
}

// ─── valid/ 目录：所有文件必须编译成功 ───────────────────

#[test]
fn valid_hello_world() {
    let path = Path::new("tests/fixtures/valid/hello_world.atx");
    let (bytes, errors) = compile_file(path);
    assert!(errors.is_empty(), "{}\n{}", path.display(), errors.join("\n"));
    assert!(!bytes.is_empty(), "{} 产生空输出", path.display());
}

#[test]
fn valid_expressions() {
    let path = Path::new("tests/fixtures/valid/expressions.atx");
    let (bytes, errors) = compile_file(path);
    assert!(errors.is_empty(), "{}\n{}", path.display(), errors.join("\n"));
    assert!(!bytes.is_empty());
}

#[test]
fn valid_control_flow() {
    let path = Path::new("tests/fixtures/valid/control_flow.atx");
    let (bytes, errors) = compile_file(path);
    assert!(errors.is_empty(), "{}\n{}", path.display(), errors.join("\n"));
    assert!(!bytes.is_empty());
}

#[test]
fn valid_functions() {
    let path = Path::new("tests/fixtures/valid/functions.atx");
    let (bytes, errors) = compile_file(path);
    assert!(errors.is_empty(), "{}\n{}", path.display(), errors.join("\n"));
    assert!(!bytes.is_empty());
}

#[test]
fn valid_generics() {
    let path = Path::new("tests/fixtures/valid/generics.atx");
    let (bytes, errors) = compile_file(path);
    assert!(errors.is_empty(), "{}\n{}", path.display(), errors.join("\n"));
    assert!(!bytes.is_empty());
}

#[test]
fn valid_builtins() {
    let path = Path::new("tests/fixtures/valid/builtins.atx");
    let (bytes, errors) = compile_file(path);
    assert!(errors.is_empty(), "{}\n{}", path.display(), errors.join("\n"));
    assert!(!bytes.is_empty());
}

#[test]
fn valid_all_zones() {
    let path = Path::new("tests/fixtures/valid/all_zones.atx");
    let (bytes, errors) = compile_file(path);
    assert!(errors.is_empty(), "{}\n{}", path.display(), errors.join("\n"));
    assert!(!bytes.is_empty());
}

// ─── invalid/ 目录：所有文件必须编译报错 ────────────────

#[test]
fn invalid_type_mismatch() {
    let path = Path::new("tests/fixtures/invalid/type_mismatch.atx");
    let (_, errors) = compile_file(path);
    assert!(!errors.is_empty(), "{} 应产生编译错误", path.display());
}

#[test]
fn invalid_undefined_var() {
    let path = Path::new("tests/fixtures/invalid/undefined_var.atx");
    let (_, errors) = compile_file(path);
    assert!(!errors.is_empty(), "{} 应产生编译错误", path.display());
}

#[test]
fn invalid_bad_escape() {
    let path = Path::new("tests/fixtures/invalid/bad_escape.atx");
    let (_, errors) = compile_file(path);
    assert!(!errors.is_empty(), "{} 应产生编译错误", path.display());
}
