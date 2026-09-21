use crate::cli::args::VideoCommands;
use crate::cli::output::{is_json, print_json};
use crate::error::Result;
use crate::utils::string::{format_duration, format_num, format_timestamp};
use crate::{user_info, user_success};
use crossterm::style::Stylize;

pub fn handle_video(cmd: VideoCommands) -> Result<()> {
    match cmd {
        VideoCommands::Info { video } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let data = crate::api::video::get_video_info(&video, cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let title = data["title"].as_str().unwrap_or("未知标题");
            let bvid = data["bvid"].as_str().unwrap_or("-");
            let aid = data["aid"].as_i64().unwrap_or(0);
            let owner = data["owner"]["name"].as_str().unwrap_or("未知");
            let mid = data["owner"]["mid"].as_i64().unwrap_or(0);
            let duration = data["duration"].as_i64().unwrap_or(0);
            let pubdate = data["pubdate"].as_i64().unwrap_or(0);

            let view = format_num(&data["stat"]["view"]);
            let danmaku = format_num(&data["stat"]["danmaku"]);
            let like = format_num(&data["stat"]["like"]);
            let coin = format_num(&data["stat"]["coin"]);
            let fav = format_num(&data["stat"]["favorite"]);
            let share = format_num(&data["stat"]["share"]);

            let desc = data["desc"].as_str().unwrap_or("").trim();

            println!();
            println!("{}", title.bold());
            println!("  {:>6}  {} (AV{})", "标识".dark_grey(), bvid.cyan(), aid);
            println!("  {:>6}  {} (UID: {})", "UP主".dark_grey(), owner, mid);
            println!(
                "  {:>6}  {} | 发布: {}",
                "时长".dark_grey(),
                format_duration(duration),
                format_timestamp(pubdate)
            );
            println!(
                "  {:>6}  播放: {} | 弹幕: {} | 点赞: {} | 投币: {} | 收藏: {} | 分享: {}",
                "数据".dark_grey(),
                view,
                danmaku,
                like,
                coin,
                fav,
                share
            );

            if !desc.is_empty() {
                let desc_preview = if desc.chars().count() > 200 {
                    let mut s: String = desc.chars().take(198).collect();
                    s.push_str("...");
                    s
                } else {
                    desc.to_string()
                };
                println!("  {:>6}  {}", "简介".dark_grey(), desc_preview.dark_grey());
            }
        }

        VideoCommands::Like { video, cancel } => {
            let cookies = crate::auth::ensure_login()?;
            crate::api::video::like_video(&cookies, &video, cancel)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "video": video,
                    "liked": !cancel,
                    "success": true
                }))?;
            } else {
                user_success!("{}成功: {}", if cancel { "取消点赞" } else { "点赞" }, video);
            }
        }

        VideoCommands::Coin {
            video,
            num,
            also_like,
        } => {
            let cookies = crate::auth::ensure_login()?;
            let liked = crate::api::video::coin_video(&cookies, &video, num, also_like)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "video": video,
                    "coins": num,
                    "also_like": also_like,
                    "success": true,
                    "liked": liked
                }))?;
            } else {
                user_success!(
                    "成功为 {} 投币 {} 枚{}",
                    video,
                    num,
                    if also_like { "，并已同步点赞" } else { "" }
                );
            }
        }

        VideoCommands::Triple { video } => {
            let cookies = crate::auth::ensure_login()?;
            let res = crate::api::video::triple_video(&cookies, &video)?;

            if is_json() {
                print_json(&res)?;
            } else {
                let like = res["like"].as_bool().unwrap_or(false);
                let coin = res["coin"].as_bool().unwrap_or(false);
                let fav = res["fav"].as_bool().unwrap_or(false);

                user_success!("视频一键三连完成: {}", video);
                user_info!(
                    "点赞: {} | 投币: {} | 收藏: {}",
                    if like { "成功".green() } else { "跳过/已点赞".dark_grey() },
                    if coin { "成功".green() } else { "跳过/已投币".dark_grey() },
                    if fav { "成功".green() } else { "跳过/已收藏".dark_grey() },
                );
            }
        }
    }

    Ok(())
}

