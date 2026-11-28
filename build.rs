fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Windows 的 PE 默认只给主线程保留 1MB 栈。调试构建下 clap 构建本工具的超大命令树
    // 会逼近该上限，新增少量子命令后就可能在 `bit --help` 阶段直接栈溢出。
    // 这里只对 Windows 调试构建放宽到 8MB，release 产物与其他平台不受影响。
    // MSVC 工具链用 link.exe 的 /STACK，MinGW (windows-gnu) 需要透传给 ld 的 -Wl,--stack。
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let debug_build = std::env::var("PROFILE").as_deref() == Ok("debug");
    if target_os != "windows" || !debug_build {
        return;
    }

    match target_env.as_str() {
        "msvc" => println!("cargo:rustc-link-arg-bins=/STACK:8388608"),
        "gnu" => println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608"),
        _ => {}
    }
}
