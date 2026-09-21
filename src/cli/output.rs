use crate::error::Result;
use std::sync::atomic::{AtomicBool, Ordering};

static QUIET: AtomicBool = AtomicBool::new(false);
static JSON_MODE: AtomicBool = AtomicBool::new(false);

/// 初始化全局输出模式配置
pub fn init_output(json: bool, quiet: bool) {
    JSON_MODE.store(json, Ordering::Relaxed);
    QUIET.store(quiet, Ordering::Relaxed);
}

/// 是否开启了 --json 输出模式
pub fn is_json() -> bool {
    JSON_MODE.load(Ordering::Relaxed)
}

/// 是否开启了 --quiet / -q 静默模式
pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// 以标准格式向 stdout 打印 JSON 结果
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    println!("{}", s);
    Ok(())
}
