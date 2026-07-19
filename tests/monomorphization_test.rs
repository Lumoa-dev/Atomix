//! 验证泛型单态化正确工作
use atomix::compiler::semantic::SemanticAnalyzer;
use atomix::compiler::lexer::Lexer;
use atomix::compiler::parser::Parser;

fn analyze(source: &str) -> (SemanticAnalyzer, Vec<String>) {
    let (tokens, lex_errors) = Lexer::new(source).tokenize();
    assert!(lex_errors.is_empty(), "lex errors: {:?}", lex_errors);
    let (ast, parse_errors) = Parser::new(tokens).parse();
    assert!(parse_errors.is_empty(), "parse errors: {:?}", parse_errors);

    let mut analyzer = SemanticAnalyzer::new();
    analyzer.analyze(ast);
    let errors: Vec<String> = analyzer.errors.iter().map(|e| e.message.clone()).collect();
    (analyzer, errors)
}

#[test]
fn generic_function_is_found_in_symbols() {
    let src = r#"TOOLS : {
        fn identity<T>(x : T) : T { return x }
    }
    TASK : {
        CALL identity(42) => result
    }"#;
    let (analyzer, errors) = analyze(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    assert!(analyzer.symbols.contains("identity"), "identity should be in symbols");
}

#[test]
fn generic_function_has_type_params() {
    let src = r#"TOOLS : {
        fn identity<T>(x : T) : T { return x }
    }
    TASK : { CALL identity(42) => result }"#;
    let (analyzer, _) = analyze(src);
    let sym = analyzer.symbols.lookup("identity").unwrap();
    let fd = sym.func_def.as_ref().unwrap();
    assert!(!fd.type_params.is_empty(), "should have type params");
    assert_eq!(fd.type_params[0], "T");
}

#[test]
fn generic_call_gets_monomorphized() {
    let src = r#"TOOLS : {
        fn identity<T>(x : T) : T { return x }
    }
    TASK : {
        CALL identity(42) => result
    }"#;
    let (analyzer, errors) = analyze(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    assert!(analyzer.symbols.contains("identity::int"),
        "identity::int should be registered after monomorphization, symbols: {:?}",
        analyzer.symbols.functions().iter().map(|s| &s.name).collect::<Vec<_>>());
}
