//! atomix-build — 独立编译器二进制。
//!
//! 最小部署包可只包含此二进制 + 执行器，不含完整 CLI。
//! 用法: atomix-build <源文件> [--opt <级别>]

use clap::Parser;
use std::fs;

#[derive(Parser)]
#[command(name = "atomix-build", about = "Atomix 编译器（standalone）")]
struct Args {
    /// 源文件路径
    source: String,

    /// 优化级别: 0, 1, 2, s
    #[arg(long = "opt", default_value = "0")]
    opt_level: String,

    /// 输出文件路径
    #[arg(short = 'o')]
    output: Option<String>,
}

fn main() {
    let args = Args::parse();

    let source_content = match fs::read_to_string(&args.source) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("错误: 无法读取源文件 `{}`: {e}", args.source);
            std::process::exit(1);
        }
    };

    let (binary, errors) = atomix::compiler::compile(&source_content, &args.opt_level);

    if !errors.is_empty() {
        for err in &errors {
            eprintln!("{}", err);
        }
        if binary.is_empty() {
            std::process::exit(1);
        }
    }

    let output_path = args.output.unwrap_or_else(|| {
        let p = std::path::Path::new(&args.source);
        if p.extension().is_some_and(|e| e == "atx") {
            p.with_extension("atxe").to_string_lossy().to_string()
        } else {
            format!("{}.atxe", args.source)
        }
    });

    if let Err(e) = fs::write(&output_path, &binary) {
        eprintln!("错误: 无法写入 `{output_path}`: {e}");
        std::process::exit(1);
    }

    println!(
        "编译成功: {} → {} ({} 字节, 优化级别: {})",
        args.source,
        output_path,
        binary.len(),
        args.opt_level
    );
}
