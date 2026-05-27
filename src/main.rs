mod api;
mod auth;
mod error;
mod live;
mod logger;
mod ui;
mod utils;

use crate::logger::init_logger;
use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input, theme::ColorfulTheme};
use error::{BiliLiveError, Result};
use std::io::Write;

#[derive(Parser)]
#[command(
    name = "bt",
    author,
    version,
    about = "B站直播开播工具 — 命令行一键开播/下播",
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
}

fn main() {
    let args = Args::parse();
    init_logger();

    if let Err(e) = run(args) {
        user_error!("{}", e);
        std::process::exit(1);
    }
}

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
            use clap::CommandFactory;
            println!("{}", Args::command().render_version());
            Ok(())
        }
        Commands::Stop { delay, .. } => {
            if let Some(d) = delay {
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
            println!("{}", include_str!("logo.txt"));
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
