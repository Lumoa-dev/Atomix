fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 使用 protoc-bin-vendored 提供的 protoc
    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    // SAFETY: build script, single-threaded, setting env var for prost-build
    unsafe { std::env::set_var("PROTOC", protoc_path); }

    // 编译 atxp.proto
    prost_build::compile_protos(
        &["docs/atxp.proto"],
        &["docs/"],
    )?;
    Ok(())
}
