use crate::cli::args::LaterCommands;
use crate::cli::output::{confirm_action, is_json, print_json, print_spacer};
use crate::error::Result;
use crate::ui::components::{Cell, Column, print_section_header, print_table};
use crate::utils::string::format_duration;
use crate::{user_info, user_success};
use crossterm::style::Color;

pub fn handle_later(cmd: LaterCommands) -> Result<()> {
    match cmd {
        LaterCommands::List => {
            let cookies = crate::auth::ensure_login()?;
            let data = crate::api::history::get_toview_list(&cookies)?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let count = data["count"].as_i64().unwrap_or(0);
            print_spacer();
            print_section_header("稍后再看", Some(&format!("共 {count} 条视频")));
            print_spacer();

            match data["list"].as_array() {
                Some(list) if !list.is_empty() => {
                    let cols = [
                        Column::left("BV号", 14),
                        Column::left("标题", 36),
                        Column::left("UP主", 16),
                        Column::left("观看进度", 10),
                        Column::right("时长", 8),
                    ];
                    let rows: Vec<Vec<Cell>> = list
                        .iter()
                        .map(|item| {
                            let bvid = item["bvid"].as_str().unwrap_or("-");
                            let title = item["title"].as_str().unwrap_or("-");
                            let owner = item["owner"]["name"].as_str().unwrap_or("-");
                            let duration = item["duration"].as_i64().unwrap_or(0);
                            let progress = item["progress"].as_i64().unwrap_or(0);

                            let progress_cell = if progress < 0 {
                                Cell::colored("✔ 已看完", Color::Green)
                            } else if progress == 0 {
                                Cell::muted("○ 未观看")
                            } else if duration > 0 {
                                let pct =
                                    ((progress as f64 / duration as f64) * 100.0).round() as u32;
                                Cell::colored(format!("▶ {pct}%"), Color::Yellow)
                            } else {
                                Cell::colored(format!("▶ {progress}s"), Color::Yellow)
                            };

                            vec![
                                Cell::cyan(bvid),
                                Cell::plain(title),
                                Cell::plain(owner),
                                progress_cell,
                                Cell::muted(format_duration(duration)),
                            ]
                        })
                        .collect();

                    print_table(&cols, &rows);
                    print_spacer();
                }
                _ => {
                    user_info!("稍后再看列表为空");
                }
            }
        }

        LaterCommands::Add { video } => {
            let cookies = crate::auth::ensure_csrf()?;
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

        LaterCommands::Remove { video } => {
            let cookies = crate::auth::ensure_csrf()?;
            let aid = crate::api::history::remove_toview(&cookies, &video)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "video": video,
                    "aid": aid,
                    "success": true,
                    "message": "已从稍后再看移除"
                }))?;
            } else {
                user_success!("已从稍后再看移除 {} (av{})", video, aid);
            }
        }

        LaterCommands::Clear { viewed_only } => {
            let prompt = if viewed_only {
                "确定要清理稍后再看中所有已观看的视频吗？"
            } else {
                "确定要清空全部稍后再看列表吗？此操作不可撤销"
            };
            if !confirm_action(prompt)? {
                user_info!("已取消操作");
                return Ok(());
            }

            let cookies = crate::auth::ensure_csrf()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BiliLiveError;

    #[test]
    fn later_clear_requires_confirmation_in_json_mode_without_yes() {
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::cli::output::init_output(true, true, false, true, None);
        let err = handle_later(LaterCommands::Clear { viewed_only: false }).unwrap_err();
        crate::cli::output::init_output(false, false, false, false, None);
        assert!(matches!(err, BiliLiveError::Input(_)));
        assert!(
            err.to_string()
                .contains("Confirmation required in non-interactive mode; pass --yes")
        );
    }
}
