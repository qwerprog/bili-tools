use crate::cli::args::SearchCommands;
use crate::cli::output::{is_json, print_json};
use crate::error::Result;
use crate::utils::string::{format_num, pad_to_width, strip_html_tags, truncate_to_width};
use crate::{user_info, user_warning};
use crossterm::style::Stylize;

pub fn handle_search(cmd: SearchCommands) -> Result<()> {
    let cookies = crate::auth::cookies::read_cookies().ok();

    match cmd {
        SearchCommands::Video {
            keyword,
            page,
            order,
        } => {
            let data = crate::api::search::search_videos(
                &keyword,
                page,
                order.as_deref(),
                cookies.as_ref(),
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let results = data["result"].as_array();
            let count = data["numResults"].as_i64().unwrap_or(0);
            println!(
                "搜索视频: \"{}\" (共找到约 {} 条结果，当前第 {} 页):",
                keyword, count, page
            );
            println!(
                "  {}  {}  {}  {}  {}",
                pad_to_width("BV号", 14).dark_grey(),
                pad_to_width("标题", 36).dark_grey(),
                pad_to_width("UP主", 16).dark_grey(),
                pad_to_width("播放", 10).dark_grey(),
                "时长".dark_grey()
            );
            println!("  {}", "-".repeat(88).dark_grey());

            match results {
                Some(list) if !list.is_empty() => {
                    for item in list {
                        let raw_bvid = item["bvid"].as_str().unwrap_or("");
                        let bvid = if !raw_bvid.is_empty() {
                            raw_bvid.to_string()
                        } else if let Some(aid) = item["aid"].as_i64() {
                            format!("av{aid}")
                        } else {
                            "-".to_string()
                        };

                        let title = strip_html_tags(item["title"].as_str().unwrap_or(""));
                        let author = strip_html_tags(item["author"].as_str().unwrap_or(""));
                        let play = format_num(&item["play"]);

                        let raw_duration = item["duration"].as_str().unwrap_or("");
                        let duration = if !raw_duration.is_empty() {
                            raw_duration
                        } else if let Some(ep) = item["episode_count_text"].as_str() {
                            if !ep.is_empty() { ep } else { "-" }
                        } else {
                            "-"
                        };

                        let title_formatted = truncate_to_width(&title, 36);
                        let author_formatted = truncate_to_width(&author, 16);

                        println!(
                            "  {}  {}  {}  {}  {}",
                            pad_to_width(&bvid, 14).cyan(),
                            pad_to_width(&title_formatted, 36),
                            pad_to_width(&author_formatted, 16),
                            pad_to_width(&play, 10),
                            duration
                        );
                    }
                }
                _ => {
                    user_warning!("未找到相关视频");
                }
            }
        }

        SearchCommands::User {
            keyword,
            page,
            order,
        } => {
            let data = crate::api::search::search_users(
                &keyword,
                page,
                order.as_deref(),
                cookies.as_ref(),
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let results = data["result"].as_array();
            let count = data["numResults"].as_i64().unwrap_or(0);
            println!(
                "搜索用户: \"{}\" (共找到约 {} 个用户，当前第 {} 页):",
                keyword, count, page
            );
            println!(
                "  {}  {}  {}  {}  {}",
                pad_to_width("UID", 12).dark_grey(),
                pad_to_width("昵称", 18).dark_grey(),
                pad_to_width("粉丝数", 10).dark_grey(),
                pad_to_width("视频数", 8).dark_grey(),
                "个人简介".dark_grey()
            );
            println!("  {}", "-".repeat(84).dark_grey());

            match results {
                Some(list) if !list.is_empty() => {
                    for item in list {
                        let mid = item["mid"].as_i64().unwrap_or(0);
                        let uname = strip_html_tags(item["uname"].as_str().unwrap_or(""));
                        let fans = format_num(&item["fans"]);
                        let videos = format_num(&item["videos"]);
                        let usign = strip_html_tags(
                            item["usign"]
                                .as_str()
                                .unwrap_or("")
                                .replace('\n', " ")
                                .trim(),
                        );

                        let uname_formatted = truncate_to_width(&uname, 18);
                        let usign_formatted = truncate_to_width(&usign, 30);
                        println!(
                            "  {}  {}  {}  {}  {}",
                            pad_to_width(&mid.to_string(), 12).cyan(),
                            pad_to_width(&uname_formatted, 18),
                            pad_to_width(&fans, 10),
                            pad_to_width(&videos, 8),
                            usign_formatted.dark_grey()
                        );
                    }
                }
                _ => {
                    user_warning!("未找到相关用户");
                }
            }
        }

        SearchCommands::Live { keyword, page } => {
            let data = crate::api::search::search_live(&keyword, page, cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let live_rooms = data["result"]["live_room"]
                .as_array()
                .or_else(|| data["result"].as_array());

            println!("搜索直播间: \"{}\" (当前第 {} 页):", keyword, page);
            println!(
                "  {}  {}  {}  {}  {}",
                pad_to_width("房间号", 10).dark_grey(),
                pad_to_width("主播", 16).dark_grey(),
                pad_to_width("分区", 14).dark_grey(),
                pad_to_width("在线/人气", 10).dark_grey(),
                "直播标题".dark_grey()
            );
            println!("  {}", "-".repeat(80).dark_grey());

            match live_rooms {
                Some(list) if !list.is_empty() => {
                    for item in list {
                        let roomid = item["roomid"].as_i64().unwrap_or(0);
                        let uname = strip_html_tags(item["uname"].as_str().unwrap_or(""));
                        let cate_name = strip_html_tags(item["cate_name"].as_str().unwrap_or("-"));
                        let online = format_num(&item["online"]);
                        let title = strip_html_tags(item["title"].as_str().unwrap_or(""));

                        let uname_formatted = truncate_to_width(&uname, 16);
                        let cate_formatted = truncate_to_width(&cate_name, 14);
                        let title_formatted = truncate_to_width(&title, 28);
                        println!(
                            "  {}  {}  {}  {}  {}",
                            pad_to_width(&roomid.to_string(), 10).cyan(),
                            pad_to_width(&uname_formatted, 16),
                            pad_to_width(&cate_formatted, 14),
                            pad_to_width(&online, 10),
                            title_formatted
                        );
                    }
                }
                _ => {
                    user_warning!("未找到相关直播间");
                }
            }
        }

        SearchCommands::Hot { limit } => {
            let data = crate::api::search::get_hot_search(Some(limit))?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            println!("🔥 B站实时热搜榜单 (Top {}):", limit);
            println!(
                "  {}  {}  {}",
                pad_to_width("排名", 4).dark_grey(),
                pad_to_width("搜索关键词", 26).dark_grey(),
                "标签".dark_grey()
            );
            println!("  {}", "-".repeat(44).dark_grey());

            if let Some(list) = data.as_array() {
                for (idx, item) in list.iter().take(limit as usize).enumerate() {
                    let rank = idx + 1;
                    let kw = item["keyword"]
                        .as_str()
                        .or_else(|| item["show_name"].as_str())
                        .unwrap_or("");
                    let icon_name = item["icon_name"]
                        .as_str()
                        .or_else(|| item["label_name"].as_str())
                        .unwrap_or("");

                    let rank_str = match rank {
                        1 => format!("{:>2}", rank).red().bold(),
                        2 => format!("{:>2}", rank).yellow().bold(),
                        3 => format!("{:>2}", rank).yellow(),
                        _ => format!("{:>2}", rank).dark_grey(),
                    };

                    let tag_str = if !icon_name.is_empty() {
                        format!("[{}]", icon_name).red().to_string()
                    } else {
                        String::new()
                    };

                    println!(
                        "  {}    {}  {}",
                        rank_str,
                        pad_to_width(&truncate_to_width(kw, 26), 26),
                        tag_str
                    );
                }
            } else {
                user_info!("暂无热搜数据");
            }
        }
    }

    Ok(())
}
