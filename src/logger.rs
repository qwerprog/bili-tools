use env_logger::{Builder, Env, Target};
use log::LevelFilter;
use std::io::Write;

pub fn init_logger(verbose: u8, quiet: bool) {
    let has_rust_log = std::env::var_os("RUST_LOG").is_some_and(|v| !v.is_empty());
    let default_level = if quiet && !has_rust_log {
        "off"
    } else {
        match verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };

    let mut builder = Builder::from_env(Env::default().default_filter_or(default_level));

    builder.format(|buf, record| {
        let level_emoji = match record.level() {
            log::Level::Error => "❌",
            log::Level::Warn => "⚠️",
            log::Level::Info => "ℹ️",
            log::Level::Debug => "🔍",
            log::Level::Trace => "🔬",
        };

        writeln!(buf, "{} {}", level_emoji, record.args())
    });

    if quiet && !has_rust_log {
        builder.filter_level(LevelFilter::Off);
    }

    builder.target(Target::Stderr);

    let _ = builder.try_init();
}
