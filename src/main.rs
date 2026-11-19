mod api;
mod auth;
mod cli;
mod error;
mod live;
mod logger;
mod ui;
mod utils;

use clap::{CommandFactory, FromArgMatches};
use clap::error::ErrorKind;
use crossterm::style::Stylize;
use logger::init_logger;

#[derive(Debug, Default, PartialEq, Eq)]
struct RawArgsSummary {
    json: bool,
    quiet: bool,
    no_color: bool,
    is_top_level_help: bool,
}

fn scan_raw_args(args: &[String]) -> RawArgsSummary {
    let mut summary = RawArgsSummary::default();
    let mut remaining: Vec<&str> = Vec::new();
    let mut after_double_dash = false;
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        if after_double_dash {
            remaining.push(arg);
            i += 1;
            continue;
        }
        if arg == "--" {
            after_double_dash = true;
            i += 1;
            continue;
        }

        match arg {
            "--json" => {
                summary.json = true;
            }
            "-q" | "--quiet" => {
                summary.quiet = true;
            }
            "-y" | "--yes" | "-v" | "--verbose" | "-D" | "--direct" => {}
            "--no-color" => {
                summary.no_color = true;
            }
            "-u" | "--cookie-file" => {
                if let Some(next) = args.get(i + 1)
                    && !matches!(next.as_str(), "-h" | "--help" | "-V" | "--version")
                {
                    i += 1;
                }
            }
            _ if arg.starts_with("--cookie-file=") => {}
            _ if arg.starts_with('-') && !arg.starts_with("--") && arg.len() > 2 => {
                let mut chars = arg[1..].chars();
                while let Some(c) = chars.next() {
                    match c {
                        'q' => summary.quiet = true,
                        'y' | 'v' | 'D' => {}
                        'h' => remaining.push("-h"),
                        'V' => remaining.push("-V"),
                        'u' => {
                            if chars.as_str().is_empty()
                                && let Some(next) = args.get(i + 1)
                                && !matches!(
                                    next.as_str(),
                                    "-h" | "--help" | "-V" | "--version"
                                )
                            {
                                i += 1;
                            }
                            break;
                        }
                        _ => {
                            remaining.push(arg);
                            break;
                        }
                    }
                }
            }
            _ => {
                remaining.push(arg);
            }
        }
        i += 1;
    }

    summary.is_top_level_help = matches!(remaining.as_slice(), [] | ["-h" | "--help" | "help"]);
    summary
}

fn print_styled_version() {
    use std::io::Write;
    let ver = env!("CARGO_PKG_VERSION");
    let mut stdout = std::io::stdout().lock();
    if ui::theme::color_enabled() {
        let _ = writeln!(
            stdout,
            "{} {} {}",
            "bit".with(ui::theme::BILI_PINK).bold(),
            format!("v{ver}").with(ui::theme::BILI_CYAN).bold(),
            "(bili-tools · Bilibili CLI Toolbox)".dark_grey()
        );
    } else {
        let _ = writeln!(stdout, "bit {ver}");
    }
}

fn main() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let raw = scan_raw_args(&raw_args);

    if raw.no_color {
        cli::output::set_no_color(true);
    }

    let mut cmd = cli::Cli::command();
    if raw.no_color || raw.json {
        cmd = cmd.color(clap::ColorChoice::Never);
    }

    let cli = match cmd.try_get_matches().and_then(|m| cli::Cli::from_arg_matches(&m)) {
        Ok(cli) => cli,
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp => {
                if raw.is_top_level_help {
                    if raw.json {
                        if let Err(e) = cli::print_metadata_json() {
                            cli::output::print_json_error(e.error_kind(), &e.to_string());
                            std::process::exit(1);
                        }
                    } else {
                        ui::banner::print_welcome_help();
                    }
                } else {
                    let _ = err.print();
                }
                std::process::exit(0);
            }
            ErrorKind::DisplayVersion => {
                if raw.json {
                    let _ = cli::output::print_json(&serde_json::json!({
                        "name": "bit",
                        "package": env!("CARGO_PKG_NAME"),
                        "version": env!("CARGO_PKG_VERSION")
                    }));
                } else {
                    print_styled_version();
                }
                std::process::exit(0);
            }
            _ => {
                if raw.json {
                    cli::output::print_json_error("Input", err.to_string().trim());
                    std::process::exit(1);
                } else {
                    err.exit();
                }
            }
        },
    };

    init_logger(cli.verbose, cli.quiet);
    cli::output::init_output(
        cli.json,
        cli.quiet,
        cli.yes,
        cli.no_color,
        cli.cookie_file.clone(),
    );
    cli::output::set_direct(cli.direct);

    // 监听 Ctrl+C 信号：优先通知业务循环优雅退栈并清理临时文件，同时安全恢复终端状态
    utils::interrupt::install_ctrlc_handler();

    match cli::run(cli) {
        Ok(()) => {
            utils::interrupt::restore_terminal_state();
            if utils::interrupt::is_interrupted() {
                std::process::exit(130);
            }
        }
        Err(error::BiliLiveError::Interrupted) => {
            utils::interrupt::restore_terminal_state();
            std::process::exit(130);
        }
        Err(e) => {
            utils::interrupt::restore_terminal_state();
            if utils::interrupt::is_interrupted() {
                std::process::exit(130);
            }
            if cli::output::is_json() {
                cli::output::print_json_error(e.error_kind(), &e.to_string());
            } else {
                user_error!("{}", e);
            }
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn scan_raw_args_recognizes_top_level_help_and_global_flags() {
        assert_eq!(
            scan_raw_args(&s(&[])),
            RawArgsSummary {
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["--json", "--help"])),
            RawArgsSummary {
                json: true,
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["--no-color", "-y", "-u", "/tmp/c.json", "--help"])),
            RawArgsSummary {
                no_color: true,
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["--cookie-file=/tmp/c.json", "-qh"])),
            RawArgsSummary {
                quiet: true,
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["-u/tmp/c.json", "help"])),
            RawArgsSummary {
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["--json", "live", "--help"])),
            RawArgsSummary {
                json: true,
                is_top_level_help: false,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["help", "live"])),
            RawArgsSummary {
                is_top_level_help: false,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["--json", "badcmd"])),
            RawArgsSummary {
                json: true,
                is_top_level_help: false,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["--", "--json", "--no-color"])),
            RawArgsSummary {
                json: false,
                no_color: false,
                is_top_level_help: false,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["--json", "-qu", "/tmp/c.json", "--help"])),
            RawArgsSummary {
                json: true,
                quiet: true,
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["-qyu", "/tmp/c.json", "--help"])),
            RawArgsSummary {
                quiet: true,
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["--json", "-qu/tmp/c.json", "--help"])),
            RawArgsSummary {
                json: true,
                quiet: true,
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["-qu", "--help"])),
            RawArgsSummary {
                quiet: true,
                is_top_level_help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            scan_raw_args(&s(&["-qx", "--help"])),
            RawArgsSummary {
                quiet: true,
                is_top_level_help: false,
                ..Default::default()
            }
        );
    }
}
