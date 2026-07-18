//! atomix-build — 独立编译器二进制。
//!
//! 最小部署包可只包含此二进制 + 执行器，不含完整 CLI。
//! 用法: atomix-build <源文件> [--opt <级别>]

use clap::Parser;

#[derive(Parser)]
#[command(name = "atomix-build", about = "Atomix 编译器 — .atx → .atxe")]
struct Args {
    /// 源文件路径
    source: String,

    /// 优化级别: 0, 1, 2, s (默认 0)
    #[arg(long = "opt", default_value = "0")]
    opt_level: String,

    /// 输出路径（默认自动推断）
    #[arg(short = 'o', long = "output")]
    output: Option<String>,
}

fn main() {
    let args = Args::parse();
    println!(
        "编译: {} → {}.atxe (优化: {})",
        args.source,
        args.output
            .unwrap_or_else(|| format!("{}", args.source.replace(".atx", ""))),
        args.opt_level
    );
    // TODO: Phase 1 完成后调用完整编译管线
}
