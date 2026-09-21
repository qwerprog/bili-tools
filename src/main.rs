mod api;
mod auth;
mod cli;
mod error;
mod live;
mod logger;
mod ui;
mod utils;

use clap::Parser;
use logger::init_logger;

fn main() {
    let cli = cli::Cli::parse();
    init_logger();
    cli::output::init_output(cli.json, cli.quiet);

    // 监听 Ctrl+C 信号，确保程序被中断时能恢复终端光标和原始模式
    let _ = ctrlc::set_handler(move || {
        let mut stdout = std::io::stdout();
        let _ = crossterm::execute!(stdout, crossterm::cursor::Show);
        let _ = crossterm::terminal::disable_raw_mode();
        std::process::exit(130);
    });

    if let Err(e) = cli::run(cli) {
        user_error!("{}", e);
        std::process::exit(1);
    }
}
