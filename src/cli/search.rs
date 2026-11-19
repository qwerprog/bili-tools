use crate::cli::args::{SearchCommands, SearchUserOrder, SearchVideoOrder};
use crate::cli::output::{is_json, print_json, print_spacer};
use crate::error::Result;
use crate::ui::components::{Cell, Column, print_section_header, print_table};
use crate::ui::theme::BILI_PINK;
use crate::utils::string::{format_num, strip_html_tags};
use crate::{user_info, user_warning};
use crossterm::style::Color;

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
                order.map(SearchVideoOrder::as_api_str),
                cookies.as_ref(),
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let results = data["result"].as_array();
            let count = data["numResults"].as_i64().unwrap_or(0);
            print_spacer();
            print_section_header(
                &format!("搜索视频 · \"{keyword}\""),
                Some(&format!("共约 {count} 条 · 第 {page} 页")),
            );
            print_spacer();

            match results {
                Some(list) if !list.is_empty() => {
                    let cols = [
                        Column::left("BV号", 14),
                        Column::left("标题", 36),
                        Column::left("UP主", 16),
                        Column::right("播放", 9),
                        Column::right("时长", 8),
                    ];
                    let rows: Vec<Vec<Cell>> = list
                        .iter()
                        .map(|item| {
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
                                raw_duration.to_string()
                            } else if let Some(ep) = item["episode_count_text"].as_str()
                                && !ep.is_empty()
                            {
                                ep.to_string()
                            } else {
                                "-".to_string()
                            };

                            vec![
                                Cell::cyan(bvid),
                                Cell::plain(title),
                                Cell::plain(author),
                                Cell::pink(play),
                                Cell::muted(duration),
                            ]
                        })
                        .collect();

                    print_table(&cols, &rows);
                    print_spacer();
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
                order.map(SearchUserOrder::as_api_str),
                cookies.as_ref(),
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let results = data["result"].as_array();
            let count = data["numResults"].as_i64().unwrap_or(0);
            print_spacer();
            print_section_header(
                &format!("搜索用户 · \"{keyword}\""),
                Some(&format!("共约 {count} 位 · 第 {page} 页")),
            );
            print_spacer();

            match results {
                Some(list) if !list.is_empty() => {
                    let cols = [
                        Column::left("UID", 12),
                        Column::left("昵称", 18),
                        Column::right("粉丝数", 10),
                        Column::right("投稿", 8),
                        Column::left("个人简介", 32),
                    ];
                    let rows: Vec<Vec<Cell>> = list
                        .iter()
                        .map(|item| {
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

                            vec![
                                Cell::cyan(mid.to_string()),
                                Cell::plain(uname),
                                Cell::pink(fans),
                                Cell::plain(videos),
                                Cell::muted(if usign.is_empty() {
                                    "-".to_string()
                                } else {
                                    usign
                                }),
                            ]
                        })
                        .collect();

                    print_table(&cols, &rows);
                    print_spacer();
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

            print_spacer();
            print_section_header(
                &format!("搜索直播间 · \"{keyword}\""),
                Some(&format!("第 {page} 页")),
            );
            print_spacer();

            match live_rooms {
                Some(list) if !list.is_empty() => {
                    let cols = [
                        Column::left("房间号", 10),
                        Column::left("主播", 16),
                        Column::left("分区", 14),
                        Column::right("人气", 10),
                        Column::left("直播标题", 30),
                    ];
                    let rows: Vec<Vec<Cell>> = list
                        .iter()
                        .map(|item| {
                            let roomid = item["roomid"]
                                .as_i64()
                                .or_else(|| item["room_id"].as_i64())
                                .unwrap_or(0);
                            let uname = strip_html_tags(item["uname"].as_str().unwrap_or(""));
                            let cate_name =
                                strip_html_tags(item["cate_name"].as_str().unwrap_or("-"));
                            let online = format_num(&item["online"]);
                            let title = strip_html_tags(item["title"].as_str().unwrap_or(""));

                            vec![
                                Cell::cyan(roomid.to_string()),
                                Cell::plain(uname),
                                Cell::muted(cate_name),
                                Cell::pink(online),
                                Cell::plain(title),
                            ]
                        })
                        .collect();

                    print_table(&cols, &rows);
                    print_spacer();
                }
                _ => {
                    user_warning!("未找到相关直播间");
                }
            }
        }

        SearchCommands::Hot { limit } => {
            let data = crate::api::search::get_hot_search(Some(limit), cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            print_spacer();
            print_section_header("🔥 B站实时热搜榜单", Some(&format!("Top {limit}")));
            print_spacer();

            match data.as_array() {
                Some(list) if !list.is_empty() => {
                    let cols = [
                        Column::right("#", 4),
                        Column::left("热搜关键词", 30),
                        Column::left("标签", 8),
                    ];
                    let rows: Vec<Vec<Cell>> = list
                        .iter()
                        .take(limit as usize)
                        .enumerate()
                        .map(|(idx, item)| {
                            let rank = idx + 1;
                            let kw = item["keyword"]
                                .as_str()
                                .or_else(|| item["show_name"].as_str())
                                .unwrap_or("");
                            let icon_name = item["icon_name"]
                                .as_str()
                                .or_else(|| item["label_name"].as_str())
                                .unwrap_or("");

                            let rank_cell = match rank {
                                1 => Cell::bold_colored(rank.to_string(), BILI_PINK),
                                2 => Cell::bold_colored(rank.to_string(), Color::Yellow),
                                3 => Cell::colored(rank.to_string(), Color::Yellow),
                                _ => Cell::muted(rank.to_string()),
                            };
                            let tag_cell = if !icon_name.is_empty() {
                                Cell::bold_colored(format!("[{icon_name}]"), BILI_PINK)
                            } else {
                                Cell::plain("")
                            };

                            vec![rank_cell, Cell::plain(kw), tag_cell]
                        })
                        .collect();

                    print_table(&cols, &rows);
                    print_spacer();
                }
                _ => {
                    user_info!("暂无热搜数据");
                }
            }
        }
    }

    Ok(())
}
