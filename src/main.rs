mod api;
mod auth;
mod error;
mod live;
mod logger;
mod ui;
mod utils;

use crate::logger::init_logger;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use crossterm::style::Stylize;
use dialoguer::{Confirm, Input, theme::ColorfulTheme};
use error::{BiliLiveError, Result};
use std::io::Write;

#[derive(Parser)]
#[command(
    name = "bt",
    author,
    version,
    about = "B站工具箱 — 命令行一键开播/下播",
    long_about = None,
    override_usage = "bt <COMMAND> [OPTION]",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true,
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

// shell 补全支持的类型
#[derive(ValueEnum, Clone)]
enum Shell {
    /// Bash 补全
    Bash,
    /// Zsh 补全
    Zsh,
    /// Fish 补全
    Fish,
}

#[derive(Subcommand)]
enum Commands {
    /// 开始直播 (默认支持交互式选择)
    #[command(override_usage = "bt start [OPTION]")]
    Start {
        /// 指定直播分区 ID，跳过交互式选择
        #[arg(short = 'a', long)]
        area: Option<u32>,

        /// 指定直播标题，跳过输入
        #[arg(short = 't', long)]
        title: Option<String>,

        /// 显示完整推流码（不打码）
        #[arg(short = 's', long)]
        show: bool,

        /// 重新登录（清除 Cookie）
        #[arg(short = 'r', long)]
        relogin: bool,

        /// 所有确认默认选择 yes
        #[arg(short = 'y', long)]
        yes: bool,

        /// 显示帮助信息
        #[arg(short = 'h', long, action = clap::ArgAction::Help)]
        help: Option<bool>,
    },

    /// 停止直播
    #[command(override_usage = "bt stop [OPTION]")]
    Stop {
        /// 延迟下播 (如 30m, 1h30m, "18:30")
        #[arg(short = 'd', long)]
        delay: Option<String>,

        /// 显示帮助信息
        #[arg(short = 'h', long, action = clap::ArgAction::Help)]
        help: Option<bool>,
    },

    /// 查看当前直播状态
    Status,

    /// 显示帮助信息
    Help,

    /// 打印版本号
    Version,

    /// 生成 shell 补全脚本
    /// 用法: bt completions zsh --install
    Completions {
        /// 目标 shell (bash/zsh/fish)
        #[arg(value_enum)]
        shell: Shell,

        /// 自动安装到 ~/.zsh/completions/ 等系统目录
        #[arg(long, help = "自动安装到系统补全目录")]
        install: bool,
    },
}

// 程序入口：解析命令行参数并分发到对应子命令
fn main() {
    let args = Args::parse();
    init_logger();

    // 监听 Ctrl+C 信号，确保程序被中断时能恢复终端光标和原始模式
    let _ = ctrlc::set_handler(move || {
        let mut stdout = std::io::stdout();
        let _ = crossterm::execute!(stdout, crossterm::cursor::Show);
        let _ = crossterm::terminal::disable_raw_mode();
        std::process::exit(130);
    });

    if let Err(e) = run(args) {
        user_error!("{}", e);
        std::process::exit(1);
    }
}

// 根据子命令执行对应逻辑
fn run(args: Args) -> Result<()> {
    match args.command {
        Commands::Help => {
            use clap::CommandFactory;
            Args::command()
                .print_help()
                .map_err(|e| BiliLiveError::Input(format!("显示帮助失败: {}", e)))?;
            Ok(())
        }
        Commands::Version => {
            println!("{}", Args::command().render_version());
            Ok(())
        }
        // 生成 shell 补全脚本，支持 bash/zsh/fish
        Commands::Completions { shell, install } => {
            use clap::CommandFactory;
            use clap_complete::{generate, shells};
            let mut cmd = Args::command();
            let mut buf = Vec::new();
            match shell {
                Shell::Bash => generate(shells::Bash, &mut cmd, "bt", &mut buf),
                Shell::Zsh => generate(shells::Zsh, &mut cmd, "bt", &mut buf),
                Shell::Fish => generate(shells::Fish, &mut cmd, "bt", &mut buf),
            }
            if install {
                // 写到各 shell 的标准补全目录
                let path = match shell {
                    // bash: ~/.local/share/bash-completion/completions/
                    Shell::Bash => {
                        let home = std::env::var("HOME").unwrap_or_default();
                        format!("{home}/.local/share/bash-completion/completions")
                    }
                    // zsh: ~/.zsh/completions/
                    Shell::Zsh => {
                        let home = std::env::var("HOME").unwrap_or_default();
                        format!("{home}/.zsh/completions")
                    }
                    // fish: ~/.config/fish/completions/
                    Shell::Fish => {
                        let home = std::env::var("HOME").unwrap_or_default();
                        format!("{home}/.config/fish/completions")
                    }
                };
                std::fs::create_dir_all(&path)?;
                let file = match shell {
                    Shell::Bash => format!("{path}/bt"),
                    Shell::Zsh => format!("{path}/_bt"),
                    Shell::Fish => format!("{path}/bt.fish"),
                };
                std::fs::write(&file, &buf)?;
                user_success!("补全脚本已安装到 {}", file);
                user_info!("请执行 source ~/.zshrc 或重新打开终端即可生效");
            } else {
                // 未指定 --install，直接输出到 stdout（可 source）
                std::io::stdout().write_all(&buf)?;
            }
            Ok(())
        }
        // ── stop 子命令：下播（支持 -d 倒计时）──
        Commands::Stop { delay, .. } => {
            if let Some(d) = delay {
                // 支持 30m / 1h30m / 90s 格式
                let secs = parse_delay(&d)?;
                user_info!("将在 {} 后自动下播，按 Ctrl+C 取消", d);
                let total = secs;
                let start = std::time::Instant::now();
                while start.elapsed().as_secs() < total {
                    let remaining = total - start.elapsed().as_secs();
                    let h = remaining / 3600;
                    let m = (remaining % 3600) / 60;
                    let s = remaining % 60;
                    print!("\r⏳ 倒计时 {:02}:{:02}:{:02}", h, m, s);
                    std::io::stdout().flush().ok();
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                print!("\r\x1b[K");
                std::io::stdout().flush().ok();
                user_info!("倒计时结束，准备关闭直播...");
                ensure_login()?;
                let cookies = auth::cookies::read_cookies()?;
                if !api::live::check_live_status(cookies.room_id)? {
                    user_info!("当前未在直播中");
                    return Ok(());
                }
                live::stop_live()?;
                return Ok(());
            }
            ensure_login()?;
            let cookies = auth::cookies::read_cookies()?;
            if !api::live::check_live_status(cookies.room_id)? {
                user_info!("当前未在直播中");
                return Ok(());
            }
            user_info!("准备关闭直播...");
            live::stop_live()?;
            Ok(())
        }
        Commands::Status => {
            ensure_login()?;
            // 按终端宽度截断每行，避免换行
            if let Ok((cols, _)) = crossterm::terminal::size() {
                let w = cols as usize;
                let pink = crossterm::style::Color::Rgb {
                    r: 251,
                    g: 114,
                    b: 153,
                };
                for line in include_str!("logo.txt").lines() {
                    let end = line
                        .char_indices()
                        .take_while(|(i, _)| *i < w)
                        .map(|(i, c)| i + c.len_utf8())
                        .last()
                        .unwrap_or(0);
                    println!("{}", &line[..end.min(line.len())].with(pink));
                }
            }
            let cookies = auth::cookies::read_cookies()?;
            let is_live = api::live::check_live_status(cookies.room_id)?;
            if is_live {
                user_info!("当前正在直播中");
                if let Some(live_key) = cookies.live_key {
                    live::stats::get_live_info(live_key)?;
                }
            } else {
                user_info!("当前未在直播中");
            }
            Ok(())
        }
        Commands::Start {
            area,
            title,
            show,
            relogin,
            yes,
            ..
        } => {
            let auto_yes = yes;
            // 有 --relogin 参数时先清除登录信息
            if relogin {
                auth::cookies::delete_cookies()?;
                user_success!("已清除登录信息，重新登录");
                user_info!("需要登录，开始登录流程...");
                auth::start_login()?;
                user_success!("登录成功！");
            } else {
                ensure_login()?;
            }
            let cookies = auth::cookies::read_cookies()?;

            if api::live::check_live_status(cookies.room_id)? {
                user_info!("检测到当前正在直播中");
                // -y 模式下自动关闭，否则弹出确认对话框
                let close_live = if auto_yes {
                    user_info!("已开启自动确认，准备关闭直播...");
                    true
                } else {
                    Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt("检测到当前正在直播中，是否关闭？")
                        .default(true)
                        .interact()
                        .map_err(|e| BiliLiveError::Input(format!("读取输入失败: {}", e)))?
                };

                if close_live {
                    user_info!("准备关闭直播...");
                    live::stop_live()?;
                } else {
                    user_info!("直播继续运行中，程序退出");
                }
                return Ok(());
            }

            // 分区选择：--area 指定 > -y 自动 > 交互确认 > 手动选择
            let area_id = if let Some(id) = area {
                id
            } else if auto_yes {
                let (id, name) = api::live::get_recent_live()?;
                user_success!("使用上次的分区: {} - {}", name, id);
                id.parse()
                    .map_err(|e| BiliLiveError::Parse(format!("分区ID转换失败: {}", e)))?
            } else {
                let use_last = Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt("是否使用上次直播的分区？")
                    .default(true)
                    .interact()
                    .map_err(|e| BiliLiveError::Input(format!("读取输入失败: {}", e)))?;

                if use_last {
                    let (id, name) = api::live::get_recent_live()?;
                    user_success!("使用上次的分区: {} - {}", name, id);
                    id.parse()
                        .map_err(|e| BiliLiveError::Parse(format!("分区ID转换失败: {}", e)))?
                } else {
                    ui::get_area_choice()?
                }
            };

            // 标题输入：--title 指定 > -y 跳过 > 交互输入
            if let Some(ref t) = title {
                if !t.is_empty() {
                    api::live::update_title(&cookies, t)?;
                    user_success!("标题已更新为: {}", t);
                }
            } else if !auto_yes {
                let title: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("请输入新标题（回车保留原标题）")
                    .allow_empty(true)
                    .interact_text()
                    .map_err(|e| BiliLiveError::Input(format!("读取标题失败: {}", e)))?;
                let title = title.trim().to_string();
                if !title.is_empty() {
                    api::live::update_title(&cookies, &title)?;
                    user_success!("标题已更新为: {}", title);
                }
            }

            live::start_live(&area_id.to_string(), show)?;
            Ok(())
        }
    }
}

// 解析延迟时间字符串，返回秒数。支持 h(时) m(分) s(秒) 单位
fn parse_delay(input: &str) -> Result<u64> {
    let mut total = 0u64;
    let mut num = String::new();
    for c in input.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let val: u64 = num
                .parse()
                .map_err(|_| BiliLiveError::Parse(format!("无效的时间数值: {}", input)))?;
            num.clear();
            match c {
                'h' | 'H' => total += val * 3600,
                'm' | 'M' => total += val * 60,
                's' | 'S' => total += val,
                _ => return Err(BiliLiveError::Parse(format!("无效的时间单位: {}", c))),
            }
        }
    }
    if total == 0 {
        return Err(BiliLiveError::Parse(format!("无效的延迟时间: {}", input)));
    }
    Ok(total)
}

// 检查并确保登录状态，如未登录则引导登录
fn ensure_login() -> Result<()> {
    if auth::check_status()? {
        user_success!("登录状态正常");
        return Ok(());
    }
    user_info!("需要登录，开始登录流程...");
    auth::start_login()?;
    user_success!("登录成功！");
    Ok(())
}
