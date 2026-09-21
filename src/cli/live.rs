use crate::cli::args::LiveCommands;
use crate::cli::output::{is_json, is_quiet, print_json};
use crate::error::{BiliLiveError, Result};
use crate::{user_info, user_success, user_warning};
use crossterm::style::Stylize;
use dialoguer::{Confirm, Input, theme::ColorfulTheme};
use std::io::Write;

pub fn handle_live(cmd: LiveCommands) -> Result<()> {
    match cmd {
        LiveCommands::Start {
            area,
            title,
            show,
            relogin,
            yes,
        } => {
            let auto_yes = yes;
            if relogin {
                crate::auth::cookies::delete_cookies()?;
                user_success!("已清除登录信息，重新登录");
                user_info!("需要登录，开始登录流程...");
                crate::auth::start_login()?;
                user_success!("登录成功！");
            } else {
                crate::auth::ensure_login()?;
            }

            let cookies = crate::auth::cookies::read_cookies()?;
            if cookies.room_id <= 0 {
                return Err(BiliLiveError::Api(
                    "未检测到绑定的直播间（房间号为0），请确认当前账号已开通直播间".to_string(),
                ));
            }

            if crate::api::live::check_live_status(cookies.room_id)? {
                user_info!("检测到当前正在直播中");
                let close_live = if auto_yes {
                    user_info!("已开启自动确认，准备关闭直播...");
                    true
                } else if is_json() {
                    return Err(BiliLiveError::Api("当前正在直播中，请使用 bt live stop 关闭".to_string()));
                } else {
                    Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt("检测到当前正在直播中，是否关闭？")
                        .default(true)
                        .interact()
                        .map_err(|e| BiliLiveError::Input(format!("读取输入失败: {}", e)))?
                };

                if close_live {
                    user_info!("准备关闭直播...");
                    let res = crate::live::stop_live()?;
                    if is_json() {
                        print_json(&res)?;
                    }
                } else {
                    user_info!("直播继续运行中，程序退出");
                }
                return Ok(());
            }

            // 分区选择：--area 指定 > -y 自动 > 交互确认 > 手动选择
            let area_id = if let Some(id) = area {
                id
            } else if auto_yes || is_json() {
                let (id, name) = crate::api::live::get_recent_live()?;
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
                    let (id, name) = crate::api::live::get_recent_live()?;
                    user_success!("使用上次的分区: {} - {}", name, id);
                    id.parse()
                        .map_err(|e| BiliLiveError::Parse(format!("分区ID转换失败: {}", e)))?
                } else {
                    crate::ui::get_area_choice()?
                }
            };

            // 标题输入：--title 指定 > -y 跳过 > 交互输入
            if let Some(ref t) = title {
                if !t.is_empty() {
                    crate::api::live::update_title(&cookies, t)?;
                    user_success!("标题已更新为: {}", t);
                }
            } else if !auto_yes && !is_json() {
                let title: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("请输入新标题（回车保留原标题）")
                    .allow_empty(true)
                    .interact_text()
                    .map_err(|e| BiliLiveError::Input(format!("读取标题失败: {}", e)))?;
                let title = title.trim().to_string();
                if !title.is_empty() {
                    crate::api::live::update_title(&cookies, &title)?;
                    user_success!("标题已更新为: {}", title);
                }
            }

            let res = crate::live::start_live(&area_id.to_string(), show)?;
            if is_json() {
                print_json(&res)?;
            }
        }

        LiveCommands::Stop { delay } => {
            if let Some(d) = delay {
                let secs = parse_delay(&d)?;
                user_info!("将在 {} 后自动下播，按 Ctrl+C 取消", d);
                let total = secs;
                let start = std::time::Instant::now();
                if !is_json() && !is_quiet() {
                    let mut stdout = std::io::stdout();
                    let _ = crossterm::execute!(stdout, crossterm::cursor::Hide);
                    while start.elapsed().as_secs() < total {
                        let remaining = total.saturating_sub(start.elapsed().as_secs());
                        let h = remaining / 3600;
                        let m = (remaining % 3600) / 60;
                        let s = remaining % 60;
                        print!("\r⏳ 倒计时 {:02}:{:02}:{:02}", h, m, s);
                        std::io::stdout().flush().ok();
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    let _ = crossterm::execute!(stdout, crossterm::cursor::Show);
                    print!("\r\x1b[K");
                    std::io::stdout().flush().ok();
                } else {
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                }

                user_info!("倒计时结束，准备关闭直播...");
                crate::auth::ensure_login()?;
                let cookies = crate::auth::cookies::read_cookies()?;
                if cookies.room_id <= 0 {
                    return Err(BiliLiveError::Api(
                        "未检测到绑定的直播间（房间号为0），请确认当前账号已开通直播间".to_string(),
                    ));
                }
                if !crate::api::live::check_live_status(cookies.room_id)? {
                    if is_json() {
                        print_json(&serde_json::json!({ "stopped": false, "message": "当前未在直播中" }))?;
                    } else {
                        user_info!("当前未在直播中");
                    }
                    return Ok(());
                }
                let res = crate::live::stop_live()?;
                if is_json() {
                    print_json(&res)?;
                }
                return Ok(());
            }

            crate::auth::ensure_login()?;
            let cookies = crate::auth::cookies::read_cookies()?;
            if cookies.room_id <= 0 {
                return Err(BiliLiveError::Api(
                    "未检测到绑定的直播间（房间号为0），请确认当前账号已开通直播间".to_string(),
                ));
            }
            if !crate::api::live::check_live_status(cookies.room_id)? {
                if is_json() {
                    print_json(&serde_json::json!({ "stopped": false, "message": "当前未在直播中" }))?;
                } else {
                    user_info!("当前未在直播中");
                }
                return Ok(());
            }
            user_info!("准备关闭直播...");
            let res = crate::live::stop_live()?;
            if is_json() {
                print_json(&res)?;
            }
        }

        LiveCommands::Status => {
            crate::auth::ensure_login()?;
            let cookies = crate::auth::cookies::read_cookies()?;
            if cookies.room_id <= 0 {
                return Err(BiliLiveError::Api(
                    "未检测到绑定的直播间（房间号为0），请确认当前账号已开通直播间".to_string(),
                ));
            }
            let room_info = crate::api::live::get_room_info(cookies.room_id)?;
            let is_live = room_info["live_status"].as_i64() == Some(1);

            if is_json() {
                print_json(&serde_json::json!({
                    "is_live": is_live,
                    "room_id": cookies.room_id,
                    "room_info": room_info
                }))?;
                return Ok(());
            }

            if !is_quiet()
                && let Ok((cols, _)) = crossterm::terminal::size()
            {
                let w = cols as usize;
                let pink = crossterm::style::Color::Rgb {
                    r: 251,
                    g: 114,
                    b: 153,
                };
                for line in include_str!("../logo.txt").lines() {
                    let end = line
                        .char_indices()
                        .take_while(|(i, _)| *i < w)
                        .map(|(i, c)| i + c.len_utf8())
                        .last()
                        .unwrap_or(0);
                    println!("{}", line[..end.min(line.len())].with(pink));
                }
            }

            if is_live {
                user_info!("当前正在直播中 (房间号: {})", cookies.room_id);
                println!("  {:>6}  {}", "标题".dark_grey(), room_info["title"].as_str().unwrap_or(""));
                println!("  {:>6}  {}", "分区".dark_grey(), room_info["area_name"].as_str().unwrap_or(""));
                println!("  {:>6}  {}", "人气".dark_grey(), room_info["online"].as_i64().unwrap_or(0));
                if let Some(live_key) = cookies.live_key {
                    let _ = crate::live::stats::get_live_info(live_key);
                }
            } else {
                user_info!("当前未在直播中 (房间号: {})", cookies.room_id);
                println!("  {:>6}  {}", "标题".dark_grey(), room_info["title"].as_str().unwrap_or(""));
                println!("  {:>6}  {}", "分区".dark_grey(), room_info["area_name"].as_str().unwrap_or(""));
            }
        }

        LiveCommands::Title { title, title_pos } => {
            crate::auth::ensure_login()?;
            let cookies = crate::auth::cookies::read_cookies()?;
            if cookies.room_id <= 0 {
                return Err(BiliLiveError::Api(
                    "未检测到绑定的直播间（房间号为0），请确认当前账号已开通直播间".to_string(),
                ));
            }
            let target_title = title.or(title_pos);

            if let Some(ref new_title) = target_title {
                crate::api::live::update_title(&cookies, new_title)?;
                if is_json() {
                    print_json(&serde_json::json!({
                        "success": true,
                        "room_id": cookies.room_id,
                        "title": new_title
                    }))?;
                } else {
                    user_success!("直播间标题已更新为: {}", new_title);
                }
            } else {
                let room_info = crate::api::live::get_room_info(cookies.room_id)?;
                let current_title = room_info["title"].as_str().unwrap_or("");
                if is_json() {
                    print_json(&serde_json::json!({
                        "room_id": cookies.room_id,
                        "title": current_title
                    }))?;
                } else {
                    println!("当前直播间标题: {}", current_title);
                }
            }
        }

        LiveCommands::Area { keyword, keyword_pos } => {
            let area_data = crate::api::area::fetch_area_list()?;
            let mut matches = Vec::new();
            let target_keyword = keyword.or(keyword_pos);

            if let Some(data_array) = area_data["data"].as_array() {
                for parent in data_array {
                    let parent_name = parent["name"].as_str().unwrap_or("");
                    let parent_id = parent["id"].as_str().unwrap_or("");

                    if let Some(child_list) = parent["list"].as_array() {
                        for child in child_list {
                            let child_id = child["id"].as_str().unwrap_or("");
                            let child_name = child["name"].as_str().unwrap_or("");

                            let matched = match &target_keyword {
                                Some(kw) => {
                                    let kw_lower = kw.to_lowercase();
                                    child_name.to_lowercase().contains(&kw_lower)
                                        || parent_name.to_lowercase().contains(&kw_lower)
                                        || child_id == kw.as_str()
                                }
                                None => true,
                            };

                            if matched {
                                matches.push(serde_json::json!({
                                    "id": child_id,
                                    "name": child_name,
                                    "parent_id": parent_id,
                                    "parent_name": parent_name
                                }));
                            }
                        }
                    }
                }
            }

            if is_json() {
                print_json(&matches)?;
            } else {
                if matches.is_empty() {
                    user_warning!("未找到匹配的分区");
                    return Ok(());
                }
                println!(
                    "  {}  {}  {}",
                    crate::utils::string::pad_to_width("分区ID", 8).dark_grey(),
                    crate::utils::string::pad_to_width("分区名称", 18).dark_grey(),
                    "所属大区".dark_grey()
                );
                println!("  {}", "-".repeat(48).dark_grey());
                for item in &matches {
                    let id = item["id"].as_str().unwrap_or("");
                    let name = item["name"].as_str().unwrap_or("");
                    let parent = item["parent_name"].as_str().unwrap_or("");
                    println!(
                        "  {}  {}  {}",
                        crate::utils::string::pad_to_width(id, 8),
                        crate::utils::string::pad_to_width(name, 18),
                        parent
                    );
                }
                user_info!("共计 {} 个分区", matches.len());
            }
        }

        LiveCommands::Following { page, page_size } => {
            crate::auth::ensure_login()?;
            let cookies = crate::auth::cookies::read_cookies()?;
            let data = crate::api::live::get_following_live(&cookies, page, page_size)?;

            if is_json() {
                print_json(&data)?;
            } else {
                let total = data["count"].as_i64().unwrap_or(0);
                let list = data["list"].as_array();

                println!("关注的主播开播状态 (共 {} 位，当前第 {} 页):", total, page);
                println!(
                    "  {}  {}  {}  {}",
                    crate::utils::string::pad_to_width("状态", 10).dark_grey(),
                    crate::utils::string::pad_to_width("主播", 16).dark_grey(),
                    crate::utils::string::pad_to_width("房间号", 10).dark_grey(),
                    "直播标题 / 分区".dark_grey()
                );
                println!("  {}", "-".repeat(60).dark_grey());

                if let Some(items) = list {
                    if items.is_empty() {
                        user_info!("暂无关注主播开播");
                    } else {
                        for item in items {
                            let live_status = item["live_status"].as_i64().unwrap_or(0);
                            let status_str = if live_status == 1 {
                                "🔴 直播中".red()
                            } else {
                                "⚪ 未开播".dark_grey()
                            };
                            let uname = item["uname"].as_str().unwrap_or("未知");
                            let room_id = item["roomid"].as_i64().unwrap_or(0);
                            let title = item["title"].as_str().unwrap_or("");
                            let area_name = item["area_name"].as_str().unwrap_or("");
                            let extra = if !area_name.is_empty() {
                                format!("[{}] {}", area_name, title)
                            } else {
                                title.to_string()
                            };
                            println!(
                                "  {}  {}  {}  {}",
                                status_str,
                                crate::utils::string::pad_to_width(
                                    &crate::utils::string::truncate_to_width(uname, 16),
                                    16
                                ),
                                crate::utils::string::pad_to_width(&room_id.to_string(), 10),
                                extra
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// 解析延迟时间字符串，返回秒数。支持 h(时) m(分) s(秒) 单位
pub fn parse_delay(input: &str) -> Result<u64> {
    let mut total = 0u64;
    let mut num = String::new();
    for c in input.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if num.is_empty() {
                return Err(BiliLiveError::Parse(format!(
                    "无效的时间格式: 连续的单位或单位出现在数值之前 ({})",
                    input
                )));
            }
            let val: u64 = num
                .parse()
                .map_err(|_| BiliLiveError::Parse(format!("无效的时间数值: {}", input)))?;
            num.clear();
            let multiplier = match c {
                'h' | 'H' => 3600,
                'm' | 'M' => 60,
                's' | 'S' => 1,
                _ => {
                    return Err(BiliLiveError::Parse(format!(
                        "无效的时间单位 '{}' (仅支持 h, m, s)",
                        c
                    )));
                }
            };
            total = val
                .checked_mul(multiplier)
                .and_then(|seconds| total.checked_add(seconds))
                .ok_or_else(|| BiliLiveError::Parse("延迟时间超过支持范围".to_string()))?;
        }
    }
    if !num.is_empty() {
        return Err(BiliLiveError::Parse(format!(
            "数值 '{}' 缺少时间单位 (例如 h, m, s)",
            num
        )));
    }
    if total == 0 {
        return Err(BiliLiveError::Parse(format!("无效的延迟时间: {}", input)));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_delay() {
        assert_eq!(parse_delay("30s").unwrap(), 30);
        assert_eq!(parse_delay("5m").unwrap(), 300);
        assert_eq!(parse_delay("2h").unwrap(), 7200);
        assert_eq!(parse_delay("1h30m10s").unwrap(), 5410);

        // 验证错误格式的处理
        assert!(parse_delay("30").is_err());
        assert!(parse_delay("30m45").is_err());
        assert!(parse_delay("abc").is_err());
        assert!(parse_delay("m30").is_err());
        assert!(parse_delay("18446744073709551615h").is_err());
        assert!(parse_delay("18446744073709551615m").is_err());
        assert!(parse_delay("18446744073709551615s2s").is_err());
        assert_eq!(parse_delay("18446744073709551615s").unwrap(), u64::MAX);
        assert!(parse_delay("").is_err());
        assert!(parse_delay("0s").is_err());
    }
}
