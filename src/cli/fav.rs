use crate::cli::args::FavCommands;
use crate::cli::output::{is_json, print_json};
use crate::error::{BiliLiveError, Result};
use crate::utils::string::{format_num, pad_to_width, truncate_to_width};
use crate::{user_info, user_success, user_warning};
use crossterm::style::Stylize;
use std::io::Write;

pub fn handle_fav(cmd: FavCommands) -> Result<()> {
    let cookies = crate::auth::cookies::read_cookies().ok();

    match cmd {
        FavCommands::List { mid } => {
            let target_mid = match mid {
                Some(m) => m,
                None => match &cookies {
                    Some(c) if !c.dede_user_id.is_empty() => c
                        .dede_user_id
                        .parse::<i64>()
                        .map_err(|e| BiliLiveError::Parse(format!("UID解析失败: {e}")))?,
                    _ => {
                        return Err(BiliLiveError::Input(
                            "未指定 --mid 且当前未登录，请指定用户 mid 或先执行 `bt auth login`".to_string(),
                        ));
                    }
                },
            };

            let data = crate::api::fav::get_user_fav_folders(target_mid, cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            println!("用户 UID: {} 的收藏夹列表:", target_mid);
            println!(
                "  {}  {}  {}  {}",
                pad_to_width("收藏夹ID", 12).dark_grey(),
                pad_to_width("标题", 26).dark_grey(),
                pad_to_width("数量", 8).dark_grey(),
                "属性".dark_grey()
            );
            println!("  {}", "-".repeat(58).dark_grey());

            if let Some(list) = data["list"].as_array() {
                if list.is_empty() {
                    user_info!("暂无收藏夹");
                } else {
                    for folder in list {
                        let id = folder["id"].as_i64().unwrap_or(0);
                        let title = folder["title"].as_str().unwrap_or("-");
                        let media_count = folder["media_count"].as_i64().unwrap_or(0);
                        let attr = folder["attr"].as_i64().unwrap_or(0);
                        let is_public = (attr & 1) == 0;
                        let attr_str = if is_public { "公开" } else { "私密" };

                        let title_formatted = truncate_to_width(title, 26);
                        println!(
                            "  {}  {}  {}  {}",
                            pad_to_width(&id.to_string(), 12).cyan(),
                            pad_to_width(&title_formatted, 26),
                            pad_to_width(&media_count.to_string(), 8),
                            attr_str.dark_grey()
                        );
                    }
                }
            }
        }

        FavCommands::Show {
            media_id,
            page,
            page_size,
        } => {
            let data = crate::api::fav::get_fav_resources(
                media_id,
                page,
                page_size,
                cookies.as_ref(),
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let folder_title = data["info"]["title"].as_str().unwrap_or("收藏夹");
            let total_count = data["info"]["media_count"].as_i64().unwrap_or(0);

            println!(
                "收藏夹: \"{}\" (共 {} 条内容，当前第 {} 页):",
                folder_title, total_count, page
            );
            println!(
                "  {}  {}  {}  {}",
                pad_to_width("BV号", 14).dark_grey(),
                pad_to_width("标题", 36).dark_grey(),
                pad_to_width("UP主", 16).dark_grey(),
                "播放".dark_grey()
            );
            println!("  {}", "-".repeat(78).dark_grey());

            if let Some(medias) = data["medias"].as_array() {
                if medias.is_empty() {
                    user_warning!("此收藏夹暂无内容");
                } else {
                    for item in medias {
                        let bvid = item["bvid"].as_str().unwrap_or("-");
                        let title = item["title"].as_str().unwrap_or("-");
                        let upper = item["upper"]["name"].as_str().unwrap_or("-");
                        let play = format_num(&item["cnt_info"]["play"]);

                        let title_formatted = truncate_to_width(title, 36);
                        let upper_formatted = truncate_to_width(upper, 16);
                        println!(
                            "  {}  {}  {}  {}",
                            pad_to_width(bvid, 14).cyan(),
                            pad_to_width(&title_formatted, 36),
                            pad_to_width(&upper_formatted, 16),
                            play
                        );
                    }
                }
            }
        }

        FavCommands::Export {
            media_id,
            output,
            format,
        } => {
            if !is_json() && output.is_some() {
                user_info!("正在拉取收藏夹 (ID: {}) 全部数据...", media_id);
            }

            let (folder_info, medias) =
                crate::api::fav::get_all_fav_resources(media_id, cookies.as_ref())?;

            let content = match format.to_lowercase().as_str() {
                "csv" => {
                    let mut csv = String::from("bvid,aid,title,upper_name,play,duration,link\n");
                    for item in &medias {
                        let bvid = item["bvid"].as_str().unwrap_or("");
                        let aid = item["id"].as_i64().unwrap_or(0);
                        let title = item["title"].as_str().unwrap_or("");
                        let upper = item["upper"]["name"].as_str().unwrap_or("");
                        let play = item["cnt_info"]["play"].as_i64().unwrap_or(0);
                        let duration = item["duration"].as_i64().unwrap_or(0);
                        let link = format!("https://www.bilibili.com/video/{}", bvid);

                        csv.push_str(&format!(
                            "{},{},{},{},{},{},{}\n",
                            escape_csv(bvid),
                            aid,
                            escape_csv(title),
                            escape_csv(upper),
                            play,
                            duration,
                            escape_csv(&link)
                        ));
                    }
                    csv
                }
                "text" | "txt" => {
                    let mut txt = String::new();
                    let folder_title = folder_info["title"].as_str().unwrap_or("收藏夹");
                    txt.push_str(&format!("# 收藏夹: {}\n\n", folder_title));
                    for item in &medias {
                        let bvid = item["bvid"].as_str().unwrap_or("");
                        let title = item["title"].as_str().unwrap_or("");
                        let upper = item["upper"]["name"].as_str().unwrap_or("");
                        let link = format!("https://www.bilibili.com/video/{}", bvid);
                        txt.push_str(&format!("- {} | {} ({}) : {}\n", title, upper, bvid, link));
                    }
                    txt
                }
                _ => {
                    // 默认 json 格式
                    serde_json::to_string_pretty(&serde_json::json!({
                        "info": folder_info,
                        "count": medias.len(),
                        "medias": medias
                    }))?
                }
            };

            if let Some(path) = output {
                std::fs::write(&path, content.as_bytes())?;
                if is_json() {
                    print_json(&serde_json::json!({
                        "media_id": media_id,
                        "count": medias.len(),
                        "file": path,
                        "format": format
                    }))?;
                } else {
                    user_success!(
                        "成功导出 {} 条收藏视频到: {}",
                        medias.len(),
                        path
                    );
                }
            } else {
                std::io::stdout().write_all(content.as_bytes())?;
                if !content.ends_with('\n') {
                    println!();
                }
            }
        }
    }

    Ok(())
}

fn escape_csv(val: &str) -> String {
    format!("\"{}\"", val.replace('"', "\"\""))
}
