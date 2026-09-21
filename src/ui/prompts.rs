#[macro_export]
macro_rules! user_info {
    ($($arg:tt)*) => {{
        if !$crate::cli::output::is_quiet() && !$crate::cli::output::is_json() {
            use crossterm::style::Stylize;
            println!("{} {}", "·".dark_grey(), format!($($arg)*));
        }
    }};
}

#[macro_export]
macro_rules! user_success {
    ($($arg:tt)*) => {{
        if !$crate::cli::output::is_quiet() && !$crate::cli::output::is_json() {
            use crossterm::style::Stylize;
            println!("{}", format!("✅ {}", format!($($arg)*)).green());
        }
    }};
}

#[macro_export]
macro_rules! user_warning {
    ($($arg:tt)*) => {{
        if !$crate::cli::output::is_quiet() && !$crate::cli::output::is_json() {
            use crossterm::style::Stylize;
            println!("{}", format!("⚠️  {}", format!($($arg)*)).yellow());
        }
    }};
}

#[macro_export]
macro_rules! user_error {
    ($($arg:tt)*) => {{
        use crossterm::style::Stylize;
        eprintln!("{}", format!("❌ {}", format!($($arg)*)).red());
    }};
}
