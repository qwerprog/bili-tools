use crate::cli::args::UserCommands;
use crate::cli::output::{is_json, print_json, print_spacer};
use crate::error::Result;
use crate::ui::components::{Cell, Column, InfoCard, print_section_header, print_table};
use crate::ui::theme::BILI_CYAN;
use crate::utils::string::{format_num, format_timestamp, truncate_to_width};
use crate::{user_info, user_success};
use crossterm::style::Color;
use serde_json::{Value, json};

fn extract_single_dynamic_summary(item: &Value) -> String {
    let md = &item["modules"]["module_dynamic"];
    let desc_text = md["desc"]["text"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let major = &md["major"];
    let major_type = major["type"].as_str().unwrap_or("");

    let major_text = match major_type {
        "MAJOR_TYPE_ARCHIVE" => major["archive"]["title"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|t| {
                let bvid = major["archive"]["bvid"].as_str().unwrap_or("");
                if bvid.is_empty() {
                    t.to_string()
                } else {
                    format!("{t} ({bvid})")
                }
            }),
        "MAJOR_TYPE_OPUS" => {
            let title = major["opus"]["title"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let summary = major["opus"]["summary"]["text"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            match (title, summary) {
                (Some(t), Some(s)) if t != s => Some(format!("{t}: {s}")),
                (Some(t), _) => Some(t.to_string()),
                (None, Some(s)) => Some(s.to_string()),
                _ => None,
            }
        }
        "MAJOR_TYPE_ARTICLE" => major["article"]["title"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "MAJOR_TYPE_PGC" => major["pgc"]["title"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "MAJOR_TYPE_UGC_SEASON" => major["ugc_season"]["title"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "MAJOR_TYPE_LIVE_RCMD" => major["live_rcmd"]["content"]
            .as_str()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|v| {
                let info = &v["live_play_info"];
                let title = info["title"].as_str()?.trim();
                if title.is_empty() {
                    None
                } else {
                    let room_id = info["room_id"].as_i64().unwrap_or(0);
                    if room_id > 0 {
                        Some(format!("[直播 {room_id}] {title}"))
                    } else {
                        Some(title.to_string())
                    }
                }
            }),
        "MAJOR_TYPE_LIVE" => major["live"]["title"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "MAJOR_TYPE_COMMON" => major["common"]["title"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "MAJOR_TYPE_MUSIC" => major["music"]["title"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "MAJOR_TYPE_COURSES" => major["courses"]["title"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "MAJOR_TYPE_NONE" => major["none"]["tips"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
    .or_else(|| {
        major["archive"]["title"]
            .as_str()
            .or_else(|| major["opus"]["summary"]["text"].as_str())
            .or_else(|| major["article"]["title"].as_str())
            .or_else(|| major["pgc"]["title"].as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });

    match (desc_text, major_text) {
        (Some(d), Some(m)) if d != m => format!("{d} · {m}"),
        (Some(d), _) => d.to_string(),
        (None, Some(m)) => m,
        (None, None) => "-".to_string(),
    }
}

pub(crate) fn extract_dynamic_summary(item: &Value) -> String {
    let dtype = item["type"].as_str().unwrap_or("");
    if dtype == "DYNAMIC_TYPE_FORWARD" {
        let comment = item["modules"]["module_dynamic"]["desc"]["text"]
            .as_str()
            .map(str::trim)
            .unwrap_or("");
        let orig = &item["orig"];
        let orig_summary = if orig.is_object() {
            let orig_author = orig["modules"]["module_author"]["name"]
                .as_str()
                .unwrap_or("");
            let inner = extract_single_dynamic_summary(orig);
            if orig_author.is_empty() {
                inner
            } else {
                format!("@{orig_author}: {inner}")
            }
        } else {
            String::new()
        };
        let combined = match (comment.is_empty(), orig_summary.is_empty()) {
            (false, false) => format!("{comment} // {orig_summary}"),
            (false, true) => comment.to_string(),
            (true, false) => orig_summary,
            (true, true) => "-".to_string(),
        };
        return combined.replace('\n', " ");
    }

    extract_single_dynamic_summary(item).replace('\n', " ")
}

fn dynamic_type_label(dtype: &str) -> &'static str {
    match dtype {
        "DYNAMIC_TYPE_AV" => "视频",
        "DYNAMIC_TYPE_WORD" => "文字",
        "DYNAMIC_TYPE_DRAW" => "图文",
        "DYNAMIC_TYPE_FORWARD" => "转发",
        "DYNAMIC_TYPE_ARTICLE" => "专栏",
        "DYNAMIC_TYPE_PGC" => "番剧",
        "DYNAMIC_TYPE_LIVE_RCMD" | "DYNAMIC_TYPE_LIVE" => "直播",
        "DYNAMIC_TYPE_UGC_SEASON" => "合集",
        "DYNAMIC_TYPE_MUSIC" => "音频",
        "DYNAMIC_TYPE_COURSES" | "DYNAMIC_TYPE_COURSES_SEASON" => "课程",
        "DYNAMIC_TYPE_NONE" => "失效",
        _ => "动态",
    }
}

pub(crate) fn render_dynamics_table(title: &str, data: &Value) {
    let items = data["items"].as_array();
    let has_more = data["has_more"].as_bool().unwrap_or(false);
    let next_offset = data["next_offset"]
        .as_str()
        .or_else(|| data["offset"].as_str())
        .unwrap_or("");

    let subtitle = if has_more && !next_offset.is_empty() {
        format!("更多游标: {next_offset}")
    } else {
        format!("共 {} 条", items.map(|a| a.len()).unwrap_or(0))
    };

    print_spacer();
    print_section_header(title, Some(&subtitle));
    print_spacer();

    match items {
        Some(list) if !list.is_empty() => {
            let cols = [
                Column::left("动态ID", 20),
                Column::left("UP主", 14),
                Column::left("类型", 6),
                Column::left("内容摘要", 36),
                Column::right("时间", 16),
            ];
            let rows: Vec<Vec<Cell>> = list
                .iter()
                .map(|item| {
                    let id_str = item["id_str"].as_str().unwrap_or("-");
                    let author = item["modules"]["module_author"]["name"]
                        .as_str()
                        .unwrap_or("-");
                    let dtype = item["type"].as_str().unwrap_or("");
                    let label = dynamic_type_label(dtype);
                    let summary = extract_dynamic_summary(item);
                    let pub_time = item["modules"]["module_author"]["pub_time"]
                        .as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            format_timestamp(
                                item["modules"]["module_author"]["pub_ts"]
                                    .as_i64()
                                    .unwrap_or(0),
                            )
                        });

                    vec![
                        Cell::cyan(truncate_to_width(id_str, 20)),
                        Cell::plain(truncate_to_width(author, 14)),
                        Cell::pink(label),
                        Cell::plain(truncate_to_width(&summary, 36)),
                        Cell::muted(truncate_to_width(&pub_time, 16)),
                    ]
                })
                .collect();
            print_table(&cols, &rows);
            print_spacer();
        }
        _ => {
            user_info!("暂无动态内容");
        }
    }
}

fn resolve_user_or_self(user: Option<String>) -> Result<i64> {
    match user {
        Some(raw) => crate::utils::id::resolve_user(&raw, false),
        None => {
            let mut cookies = crate::auth::ensure_login()?;
            if let Ok(mid) = cookies.dede_user_id.trim().parse::<i64>()
                && mid > 0
            {
                return Ok(mid);
            }
            let nav = crate::api::passport::get_user_info(&cookies)?;
            if let Some(mid) = nav["data"]["mid"].as_i64().filter(|&m| m > 0) {
                cookies.dede_user_id = mid.to_string();
                let _ = cookies.save();
                return Ok(mid);
            }
            Err(crate::error::BiliLiveError::Auth(
                "未能从当前凭证解析出用户 UID，请指定 <USER> 或重新执行 `bit auth login`"
                    .to_string(),
            ))
        }
    }
}

pub fn handle_user(cmd: UserCommands) -> Result<()> {
    match cmd {
        UserCommands::Info { user } => {
            let mid = resolve_user_or_self(user)?;
            let cookies = crate::auth::cookies::read_cookies().ok();
            let data = crate::api::user::get_user_info(mid, cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let uid = data["mid"].as_i64().unwrap_or(mid);
            let name = data["name"].as_str().unwrap_or("未知用户");
            let level = data["level"].as_i64().unwrap_or(0);
            let follower = format_num(&data["follower"]);
            let following = format_num(&data["following"]);
            let is_followed = data["is_followed"].as_bool().unwrap_or(false);
            let sign = data["sign"].as_str().unwrap_or("").replace('\n', " ");
            let sign = sign.trim();

            let live_desc = if let Some(lr) = data["live_room"].as_object() {
                let room_id = lr.get("roomid").and_then(|v| v.as_i64()).unwrap_or(0);
                let live_status = lr.get("liveStatus").and_then(|v| v.as_i64()).unwrap_or(0);
                let live_title = lr.get("title").and_then(|v| v.as_str()).unwrap_or("");
                if room_id > 0 {
                    let state = if live_status == 1 {
                        "直播中"
                    } else {
                        "未开播"
                    };
                    if live_title.is_empty() {
                        format!("房间 {room_id} ({state})")
                    } else {
                        format!("房间 {room_id} ({state}) · {live_title}")
                    }
                } else {
                    "未开通".to_string()
                }
            } else {
                "无直播间".to_string()
            };

            let mut card = InfoCard::new("👤 用户主页")
                .with_badge(format!("Lv.{level}"), BILI_CYAN)
                .add_row("昵称", Cell::bold_colored(name, Color::White))
                .add_plain_row("UID", uid.to_string())
                .add_row(
                    "粉丝 / 关注",
                    Cell::pink(format!("{follower} 粉丝  ·  {following} 关注")),
                )
                .add_row(
                    "关注状态",
                    if is_followed {
                        Cell::colored("已关注", Color::Green)
                    } else {
                        Cell::muted("未关注")
                    },
                )
                .add_plain_row("直播间", live_desc);

            if !sign.is_empty() {
                card = card.add_row("签名", Cell::muted(sign));
            }

            print_spacer();
            card.print();
            print_spacer();
        }

        UserCommands::Videos {
            user,
            page,
            page_size,
            order,
            keyword,
        } => {
            let mid = resolve_user_or_self(user)?;
            let cookies = crate::auth::cookies::read_cookies().ok();
            let data = crate::api::user::get_user_videos(
                mid,
                page,
                page_size,
                order.as_api_str(),
                keyword.as_deref(),
                cookies.as_ref(),
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let total = data["page"]["count"].as_i64().unwrap_or(0);
            let vlist = data["list"]["vlist"].as_array();

            print_spacer();
            print_section_header(
                &format!("用户投稿 · UID {mid}"),
                Some(&format!("共 {total} 条 · 第 {page} 页")),
            );
            print_spacer();

            match vlist {
                Some(list) if !list.is_empty() => {
                    let cols = [
                        Column::left("BV号", 14),
                        Column::left("标题", 36),
                        Column::right("播放", 9),
                        Column::right("时长", 8),
                        Column::right("投稿时间", 16),
                    ];
                    let rows: Vec<Vec<Cell>> = list
                        .iter()
                        .map(|item| {
                            let bvid = item["bvid"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                                .unwrap_or_else(|| {
                                    format!("av{}", item["aid"].as_i64().unwrap_or(0))
                                });
                            let title = item["title"].as_str().unwrap_or("-");
                            let play = format_num(&item["play"]);
                            let length = item["length"].as_str().unwrap_or("-");
                            let created = format_timestamp(item["created"].as_i64().unwrap_or(0));

                            vec![
                                Cell::cyan(truncate_to_width(&bvid, 14)),
                                Cell::plain(truncate_to_width(title, 36)),
                                Cell::pink(play),
                                Cell::muted(length),
                                Cell::muted(created),
                            ]
                        })
                        .collect();
                    print_table(&cols, &rows);
                    print_spacer();
                }
                _ => {
                    user_info!("该用户暂无投稿视频");
                }
            }
        }

        UserCommands::Dynamics { user, offset } => {
            let mid = resolve_user_or_self(user)?;
            let cookies = crate::auth::cookies::read_cookies().ok();
            let data =
                crate::api::user::get_user_dynamics(mid, offset.as_deref(), cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            render_dynamics_table(&format!("用户动态 · UID {mid}"), &data);
        }

        UserCommands::Follow { user } => {
            let mid = crate::utils::id::resolve_user(&user, false)?;
            let cookies = crate::auth::ensure_csrf()?;
            crate::api::user::modify_relation(&cookies, mid, true)?;

            if is_json() {
                print_json(&json!({
                    "success": true,
                    "mid": mid,
                    "followed": true,
                    "message": format!("已关注用户 {mid}")
                }))?;
            } else {
                user_success!("已关注用户 {mid}");
            }
        }

        UserCommands::Unfollow { user } => {
            let mid = crate::utils::id::resolve_user(&user, false)?;
            let cookies = crate::auth::ensure_csrf()?;
            crate::api::user::modify_relation(&cookies, mid, false)?;

            if is_json() {
                print_json(&json!({
                    "success": true,
                    "mid": mid,
                    "followed": false,
                    "message": format!("已取消关注用户 {mid}")
                }))?;
            } else {
                user_success!("已取消关注用户 {mid}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_dynamic_summary_handles_opus_archive_forward_and_live_rcmd() {
        let opus_item = json!({
            "type": "DYNAMIC_TYPE_WORD",
            "modules": {
                "module_dynamic": {
                    "desc": null,
                    "major": {
                        "type": "MAJOR_TYPE_OPUS",
                        "opus": {
                            "title": "标题",
                            "summary": { "text": "正文摘要" }
                        }
                    }
                }
            }
        });
        assert_eq!(extract_dynamic_summary(&opus_item), "标题: 正文摘要");

        let archive_item = json!({
            "type": "DYNAMIC_TYPE_AV",
            "modules": {
                "module_dynamic": {
                    "desc": null,
                    "major": {
                        "type": "MAJOR_TYPE_ARCHIVE",
                        "archive": { "bvid": "BV1uJ411r7hL", "title": "Mock Video" }
                    }
                }
            }
        });
        assert_eq!(
            extract_dynamic_summary(&archive_item),
            "Mock Video (BV1uJ411r7hL)"
        );

        let live_item = json!({
            "type": "DYNAMIC_TYPE_LIVE_RCMD",
            "modules": {
                "module_dynamic": {
                    "major": {
                        "type": "MAJOR_TYPE_LIVE_RCMD",
                        "live_rcmd": {
                            "content": r#"{"live_play_info":{"room_id":76,"title":"来聊天"}}"#
                        }
                    }
                }
            }
        });
        assert_eq!(extract_dynamic_summary(&live_item), "[直播 76] 来聊天");

        let fwd_item = json!({
            "type": "DYNAMIC_TYPE_FORWARD",
            "modules": {
                "module_dynamic": {
                    "desc": { "text": "转发评语" },
                    "major": null
                }
            },
            "orig": {
                "type": "DYNAMIC_TYPE_ARTICLE",
                "modules": {
                    "module_author": { "name": "原作者" },
                    "module_dynamic": {
                        "major": {
                            "type": "MAJOR_TYPE_ARTICLE",
                            "article": { "title": "专栏标题" }
                        }
                    }
                }
            }
        });
        assert_eq!(
            extract_dynamic_summary(&fwd_item),
            "转发评语 // @原作者: 专栏标题"
        );
    }
}
