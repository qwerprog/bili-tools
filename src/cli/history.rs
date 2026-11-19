use crate::cli::args::{HistoryCategory, HistoryCommands};
use crate::cli::output::{confirm_action, is_json, print_json, print_spacer};
use crate::error::Result;
use crate::ui::components::{Cell, Column, print_section_header, print_table};
use crate::ui::theme::BILI_PINK;
use crate::utils::string::format_timestamp;
use crate::{user_info, user_success, user_warning};
use crossterm::style::Color;

pub fn handle_history(cmd: HistoryCommands) -> Result<()> {
    match cmd {
        HistoryCommands::List {
            limit,
            category,
            max,
            view_at,
        } => {
            let cookies = crate::auth::ensure_login()?;
            let (data, next_cursor) = crate::api::history::get_history_page(
                &cookies,
                limit,
                category.map(HistoryCategory::as_api_str),
                max,
                view_at,
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let sub = match next_cursor {
                Some((m, va)) => format!("本页上限 {limit} 条 · 下一页: --max {m} --view-at {va}"),
                None => format!("最近 {limit} 条"),
            };
            print_spacer();
            print_section_header("观看历史记录", Some(&sub));
            print_spacer();

            match data.as_array() {
                Some(list) if !list.is_empty() => {
                    let cols = [
                        Column::left("类型", 6),
                        Column::left("标识", 14),
                        Column::left("标题", 34),
                        Column::left("UP主/主播", 16),
                        Column::left("进度", 10),
                        Column::left("观看时间", 16),
                    ];
                    let rows: Vec<Vec<Cell>> = list
                        .iter()
                        .map(|item| {
                            let business = item["history"]["business"].as_str().unwrap_or("-");
                            let type_cell = match business {
                                "archive" => Cell::muted("视频"),
                                "live" => Cell::pink("直播"),
                                "article" => Cell::colored("专栏", BILI_PINK),
                                _ => Cell::muted(business),
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

                            let progress_cell = if business == "live" {
                                Cell::muted("-")
                            } else if progress < 0 {
                                Cell::colored("✔ 已看完", Color::Green)
                            } else if progress == 0 {
                                Cell::muted("○ 刚开始")
                            } else if duration > 0 {
                                let pct =
                                    ((progress as f64 / duration as f64) * 100.0).round() as u32;
                                Cell::colored(format!("▶ {pct}%"), Color::Yellow)
                            } else {
                                Cell::colored(format!("▶ {progress}s"), Color::Yellow)
                            };

                            vec![
                                type_cell,
                                Cell::cyan(bvid),
                                Cell::plain(title),
                                Cell::plain(author),
                                progress_cell,
                                Cell::muted(format_timestamp(view_at)),
                            ]
                        })
                        .collect();

                    print_table(&cols, &rows);
                    print_spacer();
                }
                Some(_) => {
                    user_info!("暂无观看历史记录");
                }
                None if data.is_null() => {
                    user_info!("暂无观看历史记录");
                }
                None => {
                    user_warning!("无法解析历史记录响应");
                }
            }
        }

        HistoryCommands::Delete { video } => {
            let cookies = crate::auth::ensure_csrf()?;
            let deleted = crate::api::history::delete_history_item(&cookies, &video)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "video": video,
                    "aid": deleted.oid,
                    "oid": deleted.oid,
                    "business": deleted.business,
                    "kid": deleted.kid,
                    "success": true,
                    "message": "已删除历史记录"
                }))?;
            } else {
                let kind_label = match deleted.business.as_str() {
                    "live" => "直播",
                    "article" => "专栏",
                    "pgc" => "番剧",
                    _ => "视频",
                };
                user_success!(
                    "已删除{}观看历史记录: {} ({})",
                    kind_label,
                    video,
                    deleted.kid
                );
            }
        }

        HistoryCommands::Clear => {
            if !confirm_action("确定要清空全部观看历史记录吗？此操作不可撤销")? {
                user_info!("已取消操作");
                return Ok(());
            }

            let cookies = crate::auth::ensure_csrf()?;
            crate::api::history::clear_history(&cookies)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "message": "已清空观看历史记录"
                }))?;
            } else {
                user_success!("已清空全部观看历史记录");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BiliLiveError;

    #[test]
    fn history_clear_requires_confirmation_in_json_mode_without_yes() {
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::cli::output::init_output(true, true, false, true, None);
        let err = handle_history(HistoryCommands::Clear).unwrap_err();
        crate::cli::output::init_output(false, false, false, false, None);
        assert!(matches!(err, BiliLiveError::Input(_)));
        assert!(
            err.to_string()
                .contains("Confirmation required in non-interactive mode; pass --yes")
        );
    }
}
