#[macro_export]
macro_rules! user_info {
    ($($arg:tt)*) => {{
        if !$crate::cli::output::is_quiet() && !$crate::cli::output::is_json() {
            let msg = format!($($arg)*);
            if $crate::ui::theme::stderr_color_enabled() {
                use crossterm::style::Stylize;
                eprintln!("  {} {}", "ℹ".with($crate::ui::theme::BILI_CYAN).bold(), msg);
            } else {
                eprintln!("  ℹ {msg}");
            }
        }
    }};
}

#[macro_export]
macro_rules! user_success {
    ($($arg:tt)*) => {{
        if !$crate::cli::output::is_quiet() && !$crate::cli::output::is_json() {
            let msg = format!($($arg)*);
            if $crate::ui::theme::stderr_color_enabled() {
                use crossterm::style::Stylize;
                eprintln!("  {} {}", "✔".green().bold(), msg);
            } else {
                eprintln!("  ✔ {msg}");
            }
        }
    }};
}

#[macro_export]
macro_rules! user_warning {
    ($($arg:tt)*) => {{
        if !$crate::cli::output::is_quiet() && !$crate::cli::output::is_json() {
            let msg = format!($($arg)*);
            if $crate::ui::theme::stderr_color_enabled() {
                use crossterm::style::Stylize;
                eprintln!("  {} {}", "⚠".yellow().bold(), msg.yellow());
            } else {
                eprintln!("  ⚠ {msg}");
            }
        }
    }};
}

#[macro_export]
macro_rules! user_error {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        if $crate::ui::theme::stderr_color_enabled() {
            use crossterm::style::Stylize;
            eprintln!("  {} {}", "✖".red().bold(), msg.red());
        } else {
            eprintln!("  ✖ {msg}");
        }
    }};
}
