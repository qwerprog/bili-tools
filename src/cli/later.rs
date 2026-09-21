use crate::cli::args::LaterCommands;
use crate::cli::output::{is_json, print_json};
use crate::error::Result;
use crate::utils::string::{format_duration, pad_to_width, truncate_to_width};
use crate::{user_info, user_success};
use crossterm::style::Stylize;

pub fn handle_later(cmd: LaterCommands) -> Result<()> {
    let cookies = crate::auth::ensure_login()?;

    match cmd {
        LaterCommands::List => {
            let data = crate::api::history::get_toview_list(&cookies)?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let count = data["count"].as_i64().unwrap_or(0);
            println!("稍后再看列表 (共 {} 条视频):", count);
            println!(
                "  {}  {}  {}  {}  {}",
                pad_to_width("BV号", 14).dark_grey(),
                pad_to_width("标题", 36).dark_grey(),
                pad_to_width("UP主", 16).dark_grey(),
                pad_to_width("进度", 10).dark_grey(),
                "时长".dark_grey()
            );
            println!("  {}", "-".repeat(88).dark_grey());

            if let Some(list) = data["list"].as_array() {
                if list.is_empty() {
                    user_info!("稍后再看列表为空");
                } else {
                    for item in list {
                        let bvid = item["bvid"].as_str().unwrap_or("-");
                        let title = item["title"].as_str().unwrap_or("-");
                        let owner = item["owner"]["name"].as_str().unwrap_or("-");
                        let duration = item["duration"].as_i64().unwrap_or(0);
                        let progress = item["progress"].as_i64().unwrap_or(0);

                        let progress_str = if progress < 0 {
                            "已看完".green().to_string()
                        } else if progress == 0 {
                            "未观看".dark_grey().to_string()
                        } else if duration > 0 {
                            let pct = ((progress as f64 / duration as f64) * 100.0).round() as u32;
                            format!("看到 {pct}%").yellow().to_string()
                        } else {
                            format!("{progress}秒").yellow().to_string()
                        };

                        let title_formatted = truncate_to_width(title, 36);
                        let owner_formatted = truncate_to_width(owner, 16);
                        let duration_str = format_duration(duration);

                        println!(
                            "  {}  {}  {}  {}  {}",
                            pad_to_width(bvid, 14).cyan(),
                            pad_to_width(&title_formatted, 36),
                            pad_to_width(&owner_formatted, 16),
                            pad_to_width(&progress_str, 10),
                            duration_str
                        );
                    }
                }
            }
        }

        LaterCommands::Add { video } => {
            crate::api::history::add_toview(&cookies, &video)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "video": video,
                    "success": true,
                    "message": "已添加到稍后再看"
                }))?;
            } else {
                user_success!("已添加 {} 到稍后再看", video);
            }
        }

        LaterCommands::Clear { viewed_only } => {
            crate::api::history::clear_toview(&cookies, viewed_only)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "viewed_only": viewed_only,
                    "message": if viewed_only { "已清理已观看的稍后再看视频" } else { "已清空稍后再看列表" }
                }))?;
            } else {
                user_success!(
                    "已成功{}",
                    if viewed_only {
                        "清理已观看视频"
                    } else {
                        "清空稍后再看列表"
                    }
                );
            }
        }
    }

    Ok(())
}
