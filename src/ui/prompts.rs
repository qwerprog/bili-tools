#[macro_export]
macro_rules! user_info {
    ($($arg:tt)*) => {{
        use crossterm::style::Stylize;
        println!("{} {}", "·".dark_grey(), format!($($arg)*))
    }};
}

#[macro_export]
macro_rules! user_success {
    ($($arg:tt)*) => {{
        use crossterm::style::Stylize;
        println!("{}", format!("✅ {}", format!($($arg)*)).green())
    }};
}

#[macro_export]
macro_rules! user_warning {
    ($($arg:tt)*) => {{
        use crossterm::style::Stylize;
        println!("{}", format!("⚠️  {}", format!($($arg)*)).yellow())
    }};
}

#[macro_export]
macro_rules! user_error {
    ($($arg:tt)*) => {{
        use crossterm::style::Stylize;
        eprintln!("{}", format!("❌ {}", format!($($arg)*)).red())
        }};
    }
