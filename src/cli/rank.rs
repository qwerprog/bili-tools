use crate::cli::args::RankCommands;
use crate::cli::output::{is_json, print_json, print_spacer};
use crate::error::Result;
use crate::ui::components::{Cell, Column, print_section_header, print_table};
use crate::ui::theme::BILI_PINK;
use crate::user_info;
use crate::utils::string::{format_duration, format_num, truncate_to_width};
use crossterm::style::Color;
use serde_json::Value;

fn render_rank_table(title: &str, subtitle: &str, list: &[Value]) {
    print_spacer();
    print_section_header(title, Some(subtitle));
    print_spacer();

    if list.is_empty() {
        user_info!("暂无排行榜视频数据");
        return;
    }

    let cols = [
        Column::right("#", 4),
        Column::left("BV号", 14),
        Column::left("标题", 36),
        Column::left("UP主", 16),
        Column::right("播放", 9),
        Column::right("时长", 8),
    ];

    let rows: Vec<Vec<Cell>> = list
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let pos = idx + 1;
            let rank_cell = match pos {
                1 => Cell::bold_colored(pos.to_string(), BILI_PINK),
                2 => Cell::bold_colored(pos.to_string(), Color::Yellow),
                3 => Cell::colored(pos.to_string(), Color::Yellow),
                _ => Cell::muted(pos.to_string()),
            };
            let bvid = item["bvid"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("av{}", item["aid"].as_i64().unwrap_or(0)));
            let title = item["title"].as_str().unwrap_or("-");
            let owner = item["owner"]["name"].as_str().unwrap_or("-");
            let views = format_num(&item["stat"]["view"]);
            let dur = format_duration(item["duration"].as_i64().unwrap_or(0));

            vec![
                rank_cell,
                Cell::cyan(truncate_to_width(&bvid, 14)),
                Cell::plain(truncate_to_width(title, 36)),
                Cell::plain(truncate_to_width(owner, 16)),
                Cell::pink(views),
                Cell::muted(dur),
            ]
        })
        .collect();

    print_table(&cols, &rows);
    print_spacer();
}

pub fn handle_rank(cmd: RankCommands) -> Result<()> {
    let cookies = crate::auth::cookies::read_cookies().ok();

    match cmd {
        RankCommands::Video { rid, rank_type } => {
            let data =
                crate::api::rank::get_video_ranking(rid, rank_type.as_api_str(), cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let list = data["list"].as_array().map(|v| v.as_slice()).unwrap_or(&[]);
            render_rank_table(
                &format!("🏆 视频排行榜 · 分区 {rid}"),
                &format!("类型: {} · 共 {} 条", rank_type.as_api_str(), list.len()),
                list,
            );
        }

        RankCommands::Popular { page, page_size } => {
            let data = crate::api::rank::get_popular_videos(page, page_size, cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let list = data["list"].as_array().map(|v| v.as_slice()).unwrap_or(&[]);
            render_rank_table(
                "🔥 综合热门视频",
                &format!("第 {page} 页 · 共 {} 条", list.len()),
                list,
            );
        }
    }

    Ok(())
}
