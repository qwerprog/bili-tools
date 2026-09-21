use crate::cli::args::HistoryCommands;
use crate::cli::output::{is_json, print_json};
use crate::error::Result;
use crate::utils::string::{format_timestamp, pad_to_width, truncate_to_width};
use crate::{user_info, user_warning};
use crossterm::style::Stylize;

pub fn handle_history(cmd: HistoryCommands) -> Result<()> {
    let cookies = crate::auth::ensure_login()?;

    match cmd {
        HistoryCommands::List { limit, category } => {
            let data = crate::api::history::get_history(&cookies, limit, category.as_deref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            println!("观看历史记录 (最近最多 {} 条):", limit);
            println!(
                "  {}  {}  {}  {}  {}  {}",
                pad_to_width("类型", 6).dark_grey(),
                pad_to_width("标识", 14).dark_grey(),
                pad_to_width("标题", 36).dark_grey(),
                pad_to_width("UP主/主播", 16).dark_grey(),
                pad_to_width("进度", 10).dark_grey(),
                "观看时间".dark_grey()
            );
            println!("  {}", "-".repeat(98).dark_grey());

            if let Some(list) = data.as_array() {
                if list.is_empty() {
                    user_info!("暂无观看历史记录");
                } else {
                    for item in list {
                        let business = item["history"]["business"].as_str().unwrap_or("-");
                        let type_str = match business {
                            "archive" => "视频",
                            "live" => "直播",
                            "article" => "专栏",
                            _ => business,
                        };

                        let raw_bvid = item["history"]["bvid"]
                            .as_str()
                            .or_else(|| item["bvid"].as_str())
                            .unwrap_or("");
                        let oid = item["history"]["oid"].as_i64().unwrap_or(0);

                        let bvid = if !raw_bvid.is_empty() {
                            raw_bvid.to_string()
                        } else if business == "live" && oid > 0 {
                            format!("room:{oid}")
                        } else if business == "article" && oid > 0 {
                            format!("cv{oid}")
                        } else if oid > 0 {
                            oid.to_string()
                        } else {
                            "-".to_string()
                        };

                        let title = item["title"].as_str().unwrap_or("-");
                        let author = item["author_name"].as_str().unwrap_or("-");
                        let progress = item["progress"].as_i64().unwrap_or(0);
                        let duration = item["duration"].as_i64().unwrap_or(0);
                        let view_at = item["view_at"].as_i64().unwrap_or(0);

                        let progress_str = if business == "live" {
                            "-".dark_grey().to_string()
                        } else if progress < 0 {
                            "已看完".green().to_string()
                        } else if progress == 0 {
                            "刚开始".dark_grey().to_string()
                        } else if duration > 0 {
                            let pct = ((progress as f64 / duration as f64) * 100.0).round() as u32;
                            format!("看到 {pct}%").yellow().to_string()
                        } else {
                            format!("{progress}秒").yellow().to_string()
                        };

                        let title_formatted = truncate_to_width(title, 36);
                        let author_formatted = truncate_to_width(author, 16);
                        let time_str = format_timestamp(view_at);

                        println!(
                            "  {}  {}  {}  {}  {}  {}",
                            pad_to_width(type_str, 6).dark_grey(),
                            pad_to_width(&bvid, 14).cyan(),
                            pad_to_width(&title_formatted, 36),
                            pad_to_width(&author_formatted, 16),
                            pad_to_width(&progress_str, 10),
                            time_str.dark_grey()
                        );
                    }
                }
            } else {
                user_warning!("无法解析历史记录响应");
            }
        }
    }

    Ok(())
}
