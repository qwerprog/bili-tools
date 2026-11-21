use crate::auth::cookies::Cookies;
use crate::cli::args::LiveCommands;
use crate::cli::output::{is_json, is_quiet, is_yes, print_json, print_spacer};
use crate::error::{BiliLiveError, Result};
use crate::ui::components::{
    Cell, Column, InfoCard, print_section_header, print_table, render_countdown_bar,
};
use crate::ui::theme::{bili_dialoguer_theme, stderr_color_enabled};
use crate::utils::string::format_num;
use crate::{user_info, user_success, user_warning};
use crossterm::style::Color;
use dialoguer::{Confirm, Input};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

pub(crate) fn resolve_own_room_id(cookies: &mut Cookies) -> Result<i64> {
    if cookies.room_id > 0 {
        return Ok(cookies.room_id as i64);
    }

    let mid = cookies
        .dede_user_id
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|&m| m > 0)
        .or_else(|| {
            crate::api::passport::get_user_info(cookies)
                .ok()
                .and_then(|info| info["data"]["mid"].as_i64())
                .filter(|&m| m > 0)
        });

    if let Some(m) = mid {
        if cookies.dede_user_id.trim().is_empty() {
            cookies.dede_user_id = m.to_string();
        }
        if let Ok(rid) = crate::api::passport::get_roomid_by_mid(m)
            && rid > 0
        {
            cookies.room_id = rid;
            let _ = cookies.save();
            return Ok(rid as i64);
        }
    }

    Err(BiliLiveError::Api(
        "未检测到绑定的直播间（房间号为0），请确认当前账号已开通直播间".to_string(),
    ))
}

pub fn handle_live(cmd: LiveCommands) -> Result<()> {
    match cmd {
        LiveCommands::Start {
            area,
            title,
            show,
            relogin,
        } => handle_live_start(area, title, show, relogin),
        LiveCommands::Stop { delay } => handle_live_stop(delay),
        LiveCommands::Status => handle_live_status(),
        LiveCommands::Info { room } => handle_live_info(room),
        LiveCommands::Title { title } => handle_live_title(title),
        LiveCommands::Area { keyword } => handle_live_area(keyword),
        LiveCommands::Following { page, page_size } => handle_live_following(page, page_size),
        LiveCommands::Stream { room, quality } => handle_live_stream(room, quality),
        LiveCommands::Danmaku {
            room,
            limit,
            follow,
            interactive,
            no_enter,
        } => {
            if interactive {
                handle_live_chat(Some(room), no_enter, limit.unwrap_or(15))
            } else if follow {
                handle_live_danmaku_follow(room, !no_enter, limit)
            } else {
                if no_enter && !is_json() && !is_quiet() {
                    user_warning!(
                        "历史弹幕快照不包含观众进房消息，--no-enter 仅在 -f 实时流或 -i 聊天室模式下生效"
                    );
                }
                handle_live_danmaku(room, limit.unwrap_or(20))
            }
        }
        LiveCommands::Chat {
            room,
            no_enter,
            history,
        } => handle_live_chat(room, no_enter, history),
        LiveCommands::Send { room, message } => handle_live_send(room, message),
        LiveCommands::Record {
            room,
            output,
            quality,
            duration,
            danmaku,
            danmaku_format,
            watch,
            interval,
            once,
        } => handle_live_record(LiveRecordOptions {
            room,
            output,
            quality,
            duration,
            danmaku,
            danmaku_format,
            watch,
            interval,
            once,
        }),
    }
}

pub(crate) fn handle_live_start(
    area: Option<u32>,
    title: Option<String>,
    show: bool,
    relogin: bool,
) -> Result<()> {
    if let Some(ref t) = title
        && t.trim().is_empty()
    {
        return Err(BiliLiveError::Input("直播间标题不能为空".to_string()));
    }
    let auto_yes = is_yes();
    if relogin {
        if is_json() || !std::io::stdin().is_terminal() {
            return Err(BiliLiveError::Auth(
                "JSON 或非交互模式下不支持 --relogin 交互登录，请先使用 `bit auth login --method cookie`"
                    .to_string(),
            ));
        }
        crate::auth::cookies::delete_cookies()?;
        user_success!("已清除登录信息，重新登录");
        user_info!("需要登录，开始登录流程...");
        crate::auth::start_login()?;
        user_success!("登录成功！");
    }

    let mut cookies = crate::auth::ensure_csrf_interactive()?;
    let room_id = resolve_own_room_id(&mut cookies)?;
    let room_info = crate::api::live::get_room_info(room_id)?;

    if room_info["live_status"].as_i64() == Some(1) {
        if is_json() {
            print_json(&serde_json::json!({
                "success": true,
                "already_live": true,
                "is_live": true,
                "room_id": room_id,
                "title": room_info["title"],
                "area_id": room_info["area_id"],
                "area_name": room_info["area_name"],
                "live_key": cookies.live_key,
                "room_info": room_info,
                "message": "当前正在直播中，未执行重复开播或关播操作"
            }))?;
        } else {
            user_warning!(
                "直播间 {} 当前已在直播中，未执行重复开播或关闭操作；如需下播请使用 `bit live stop`",
                room_id
            );
        }
        return Ok(());
    }

    // 分区选择：--area 指定 > -y/--json 自动 > 交互确认 > 手动选择
    let area_id = if let Some(id) = area {
        id
    } else if auto_yes || is_json() {
        let (id, name) = crate::api::live::get_recent_live()?;
        user_success!("使用上次的分区: {} - {}", name, id);
        id.parse()
            .map_err(|e| BiliLiveError::Parse(format!("分区ID转换失败: {}", e)))?
    } else {
        let use_last = Confirm::with_theme(&bili_dialoguer_theme())
            .with_prompt("是否使用上次直播的分区？")
            .default(true)
            .interact()
            .map_err(|e| BiliLiveError::Input(format!("读取输入失败: {}", e)))?;

        if use_last {
            let (id, name) = crate::api::live::get_recent_live()?;
            user_success!("使用上次的分区: {} - {}", name, id);
            id.parse()
                .map_err(|e| BiliLiveError::Parse(format!("分区ID转换失败: {}", e)))?
        } else {
            crate::ui::get_area_choice()?
        }
    };

    // 标题输入：--title 指定 > 已传 -a 且已有原标题跳过 > -y/--json 跳过 > 纯向导模式交互输入
    let existing_title = room_info["title"].as_str().unwrap_or("").trim();
    if let Some(ref t) = title {
        let trimmed = t.trim();
        if !trimmed.is_empty() {
            crate::api::live::update_title(&cookies, trimmed)?;
            user_success!("标题已更新为: {}", trimmed);
        }
    } else if !auto_yes
        && !is_json()
        && std::io::stdin().is_terminal()
        && (area.is_none() || existing_title.is_empty())
    {
        let title: String = Input::with_theme(&bili_dialoguer_theme())
            .with_prompt("请输入新标题（回车保留原标题）")
            .allow_empty(true)
            .interact_text()
            .map_err(|e| BiliLiveError::Input(format!("读取标题失败: {}", e)))?;
        let title = title.trim().to_string();
        if !title.is_empty() {
            crate::api::live::update_title(&cookies, &title)?;
            user_success!("标题已更新为: {}", title);
        }
    }

    let res = crate::live::start_live(&area_id.to_string(), show)?;
    if is_json() {
        print_json(&res)?;
    }
    Ok(())
}

pub(crate) fn handle_live_stop(delay: Option<String>) -> Result<()> {
    let delay_secs = delay.as_deref().map(parse_delay).transpose()?;
    let mut cookies = crate::auth::ensure_csrf()?;
    let room_id = resolve_own_room_id(&mut cookies)?;

    if !crate::api::live::check_live_status(room_id)? {
        if is_json() {
            print_json(&serde_json::json!({
                "stopped": false,
                "room_id": room_id,
                "message": "当前未在直播中"
            }))?;
        } else {
            user_info!("当前未在直播中");
        }
        return Ok(());
    }

    if let Some(secs) = delay_secs {
        user_info!(
            "将在 {} 后自动下播，按 Ctrl+C 取消",
            delay.as_deref().unwrap_or("")
        );
        let start = std::time::Instant::now();
        let use_bar = !is_json() && !is_quiet() && std::io::stderr().is_terminal();
        let use_ansi = use_bar && stderr_color_enabled();

        if use_bar {
            let mut stderr = std::io::stderr();
            if use_ansi {
                let _ = crossterm::execute!(stderr, crossterm::cursor::Hide);
                crate::utils::interrupt::set_stderr_cursor_hidden(true);
            }
            let wait_res: Result<()> = (|| {
                while start.elapsed().as_secs() < secs {
                    crate::utils::interrupt::check_interrupted()?;
                    let elapsed = start.elapsed().as_secs();
                    let bar = render_countdown_bar(elapsed, secs, use_ansi);
                    if use_ansi {
                        eprint!("\r\x1b[K{bar}");
                    } else {
                        eprint!("\r{bar}");
                    }
                    let _ = std::io::stderr().flush();
                    let remaining_ms = secs
                        .saturating_mul(1000)
                        .saturating_sub(start.elapsed().as_millis() as u64);
                    let step_ms = remaining_ms.clamp(1, 1000);
                    crate::utils::interrupt::sleep_interruptible(
                        std::time::Duration::from_millis(step_ms),
                    )?;
                }
                Ok(())
            })();
            if use_ansi {
                let _ = crossterm::execute!(stderr, crossterm::cursor::Show);
                crate::utils::interrupt::set_stderr_cursor_hidden(false);
                eprint!("\r\x1b[K");
            } else {
                eprintln!();
            }
            let _ = std::io::stderr().flush();
            wait_res?;
        } else {
            crate::utils::interrupt::sleep_interruptible(std::time::Duration::from_secs(secs))?;
        }

        user_info!("倒计时结束，准备关闭直播...");
        if !crate::api::live::check_live_status(room_id)? {
            if is_json() {
                print_json(&serde_json::json!({
                    "stopped": false,
                    "room_id": room_id,
                    "message": "当前未在直播中"
                }))?;
            } else {
                user_info!("当前未在直播中");
            }
            return Ok(());
        }
    } else {
        user_info!("准备关闭直播...");
    }

    let res = crate::live::stop_live()?;
    if is_json() {
        print_json(&res)?;
    }
    Ok(())
}

fn handle_live_status() -> Result<()> {
    let mut cookies = crate::auth::ensure_login()?;
    let room_id = resolve_own_room_id(&mut cookies)?;
    let room_info = crate::api::live::get_room_info(room_id)?;
    let is_live = room_info["live_status"].as_i64() == Some(1);

    if is_json() {
        let mut payload = serde_json::json!({
            "is_live": is_live,
            "room_id": room_id,
            "room_info": room_info
        });
        if is_live
            && let Some(live_key) = cookies.live_key
            && let Ok(Some(stats)) = crate::live::stats::fetch_live_stats_data(live_key)
        {
            payload["stats"] = stats;
        }
        print_json(&payload)?;
        return Ok(());
    }

    if !is_quiet() {
        crate::ui::banner::print_banner();
    }

    let title_str = room_info["title"].as_str().unwrap_or("-");
    let area_str = room_info["area_name"].as_str().unwrap_or("-");

    if is_live {
        let online = room_info["online"].as_i64().unwrap_or(0);
        let card = InfoCard::new("📺 直播间状态")
            .with_badge("● 直播中 LIVE", Color::Green)
            .add_row("房间号", Cell::cyan(room_id.to_string()))
            .add_row("标题", Cell::bold_colored(title_str, Color::White))
            .add_plain_row("分区", area_str)
            .add_row("在线人气", Cell::pink(online.to_string()));
        card.print();

        if let Some(live_key) = cookies.live_key {
            print_spacer();
            let _ = crate::live::stats::get_live_info(live_key);
        }
        print_spacer();
    } else {
        let card = InfoCard::new("📺 直播间状态")
            .with_badge("○ 未开播 OFFLINE", Color::DarkGrey)
            .add_row("房间号", Cell::cyan(room_id.to_string()))
            .add_plain_row("标题", title_str)
            .add_plain_row("分区", area_str);
        card.print();
        print_spacer();
    }
    Ok(())
}

fn resolve_target_room_canonical(room: &str) -> Result<i64> {
    let input_id = crate::utils::id::resolve_room(room, false)?;
    let init = crate::api::live::room_init(input_id)?;
    Ok(init["room_id"]
        .as_i64()
        .filter(|&id| id > 0)
        .unwrap_or(input_id))
}

pub(crate) fn handle_live_info(room: Option<String>) -> Result<()> {
    let canonical_id = match room {
        Some(ref raw) => resolve_target_room_canonical(raw)?,
        None => {
            let mut cookies = crate::auth::ensure_login()?;
            resolve_own_room_id(&mut cookies)?
        }
    };

    let room_info = crate::api::live::get_room_info(canonical_id)?;
    let anchor_uname = crate::api::live::get_anchor_in_room(canonical_id)
        .ok()
        .and_then(|d| d["info"]["uname"].as_str().map(str::to_string))
        .filter(|s| !s.is_empty());

    let live_status = room_info["live_status"].as_i64().unwrap_or(0);
    let is_live = live_status == 1;
    let short_id = room_info["short_id"].as_i64().unwrap_or(0);
    let uid = room_info["uid"].as_i64().unwrap_or(0);
    let title = room_info["title"].as_str().unwrap_or("-");
    let area_id = room_info["area_id"].as_i64().unwrap_or(0);
    let area_name = room_info["area_name"].as_str().unwrap_or("-");
    let parent_area_name = room_info["parent_area_name"].as_str().unwrap_or("");
    let online = room_info["online"].as_i64().unwrap_or(0);
    let attention = room_info["attention"].as_i64().unwrap_or(0);
    let live_time = room_info["live_time"].as_str().unwrap_or("");

    if is_json() {
        print_json(&serde_json::json!({
            "room_id": canonical_id,
            "short_id": short_id,
            "uid": uid,
            "uname": anchor_uname,
            "is_live": is_live,
            "live_status": live_status,
            "title": title,
            "area_id": area_id,
            "area_name": area_name,
            "parent_area_name": parent_area_name,
            "online": online,
            "attention": attention,
            "live_time": live_time,
            "room_info": room_info
        }))?;
        return Ok(());
    }

    let (badge_text, badge_color) = match live_status {
        1 => ("● 直播中 LIVE", Color::Green),
        2 => ("◐ 轮播中 LOOP", Color::Yellow),
        _ => ("○ 未开播 OFFLINE", Color::DarkGrey),
    };

    let area_display = if parent_area_name.is_empty() {
        area_name.to_string()
    } else {
        format!("{parent_area_name} / {area_name}")
    };

    let mut card = InfoCard::new("📺 直播间详情")
        .with_badge(badge_text, badge_color)
        .add_row("房间号", Cell::cyan(canonical_id.to_string()));
    if short_id > 0 {
        card = card.add_plain_row("短号", short_id.to_string());
    }
    if let Some(ref uname) = anchor_uname {
        card = card.add_plain_row("主播", uname.clone());
    }
    card = card
        .add_plain_row("UID", uid.to_string())
        .add_row("标题", Cell::bold_colored(title, Color::White))
        .add_plain_row("分区", area_display)
        .add_row(
            "在线人气",
            Cell::pink(format_num(&serde_json::json!(online))),
        )
        .add_plain_row("关注数", format_num(&serde_json::json!(attention)));
    if !live_time.is_empty() && !live_time.starts_with("0000-") {
        card = card.add_plain_row("开播时间", live_time);
    }

    print_spacer();
    card.print();
    print_spacer();
    Ok(())
}

fn handle_live_title(title: String) -> Result<()> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(BiliLiveError::Input("直播间标题不能为空".to_string()));
    }
    let mut cookies = crate::auth::ensure_csrf()?;
    let room_id = resolve_own_room_id(&mut cookies)?;
    crate::api::live::update_title(&cookies, trimmed)?;
    if is_json() {
        print_json(&serde_json::json!({
            "success": true,
            "room_id": room_id,
            "title": trimmed
        }))?;
    } else {
        user_success!("直播间标题已更新为: {}", trimmed);
    }
    Ok(())
}

fn handle_live_area(keyword: Option<String>) -> Result<()> {
    let area_data = crate::api::area::fetch_area_list()?;
    if let Some(code) = area_data["code"].as_i64()
        && code != 0
    {
        return Err(BiliLiveError::Api(format!(
            "获取分区列表失败: {}",
            area_data["message"].as_str().unwrap_or("未知错误")
        )));
    }
    let mut matches = Vec::new();

    if let Some(data_array) = area_data["data"].as_array() {
        for parent in data_array {
            let parent_name = parent["name"].as_str().unwrap_or("");
            let parent_id = parent["id"].as_str().unwrap_or("");

            if let Some(child_list) = parent["list"].as_array() {
                for child in child_list {
                    let child_id = child["id"].as_str().unwrap_or("");
                    let child_name = child["name"].as_str().unwrap_or("");

                    let matched = match &keyword {
                        Some(kw) => {
                            let kw_lower = kw.to_lowercase();
                            child_name.to_lowercase().contains(&kw_lower)
                                || parent_name.to_lowercase().contains(&kw_lower)
                                || child_id == kw.as_str()
                        }
                        None => true,
                    };

                    if matched {
                        matches.push(serde_json::json!({
                            "id": child_id,
                            "name": child_name,
                            "parent_id": parent_id,
                            "parent_name": parent_name
                        }));
                    }
                }
            }
        }
    }

    if is_json() {
        print_json(&matches)?;
    } else {
        if matches.is_empty() {
            user_warning!("未找到匹配的分区");
            return Ok(());
        }
        print_spacer();
        print_section_header(
            "直播分区列表",
            Some(&format!("共 {} 个匹配分区", matches.len())),
        );
        print_spacer();

        let cols = [
            Column::left("分区 ID", 10),
            Column::left("子分区名称", 22),
            Column::left("所属大区", 18),
        ];
        let rows: Vec<Vec<Cell>> = matches
            .iter()
            .map(|item| {
                let id = item["id"].as_str().unwrap_or("");
                let name = item["name"].as_str().unwrap_or("");
                let parent = item["parent_name"].as_str().unwrap_or("");
                vec![Cell::cyan(id), Cell::plain(name), Cell::muted(parent)]
            })
            .collect();

        print_table(&cols, &rows);
        print_spacer();
    }
    Ok(())
}

fn handle_live_following(page: u32, page_size: u32) -> Result<()> {
    let cookies = crate::auth::ensure_login()?;
    let data = crate::api::live::get_following_live(&cookies, page, page_size)?;

    if is_json() {
        print_json(&data)?;
        return Ok(());
    }

    let total = data["count"].as_i64().unwrap_or(0);
    print_spacer();
    print_section_header(
        "关注的主播开播状态",
        Some(&format!("共 {total} 位 · 第 {page} 页")),
    );
    print_spacer();

    match data["list"].as_array() {
        Some(items) if !items.is_empty() => {
            let cols = [
                Column::left("状态", 10),
                Column::left("主播", 18),
                Column::left("房间号", 12),
                Column::left("分区 / 标题", 36),
            ];
            let rows: Vec<Vec<Cell>> = items
                .iter()
                .map(|item| {
                    let live_status = item["live_status"].as_i64().unwrap_or(0);
                    let status_cell = if live_status == 1 {
                        Cell::bold_colored("● 直播中", Color::Green)
                    } else {
                        Cell::muted("○ 未开播")
                    };
                    let uname = item["uname"].as_str().unwrap_or("未知");
                    let room_id = item["roomid"].as_i64().unwrap_or(0);
                    let title = item["title"].as_str().unwrap_or("");
                    let area_name = item["area_name"].as_str().unwrap_or("");
                    let extra = if !area_name.is_empty() {
                        format!("[{area_name}] {title}")
                    } else {
                        title.to_string()
                    };
                    vec![
                        status_cell,
                        Cell::plain(uname),
                        Cell::cyan(room_id.to_string()),
                        Cell::plain(extra),
                    ]
                })
                .collect();

            print_table(&cols, &rows);
            print_spacer();
        }
        _ => {
            user_info!("暂无关注主播开播");
        }
    }
    Ok(())
}

pub(crate) fn handle_live_stream(room: String, quality: u32) -> Result<()> {
    let cookies = crate::auth::cookies::read_cookies().ok();
    let canonical_id = resolve_target_room_canonical(&room)?;
    let play_info = crate::api::live::get_room_play_info(canonical_id, quality, cookies.as_ref())?;
    let live_status = play_info["live_status"].as_i64().unwrap_or(0);
    let is_live = live_status == 1;
    let streams = crate::api::live::extract_live_streams(&play_info);

    if is_json() {
        print_json(&serde_json::json!({
            "room_id": canonical_id,
            "live_status": live_status,
            "is_live": is_live,
            "requested_qn": quality,
            "streams": streams
        }))?;
        return Ok(());
    }

    print_spacer();
    print_section_header(
        &format!("直播拉流地址 · 房间 {canonical_id}"),
        Some(&format!("请求画质 qn={quality} · 共 {} 条", streams.len())),
    );
    print_spacer();

    if streams.is_empty() {
        user_warning!(
            "直播间 {} 当前未开播或暂无可用拉流地址 (live_status={})",
            canonical_id,
            live_status
        );
        return Ok(());
    }

    let cols = [
        Column::right("#", 3),
        Column::left("协议", 12),
        Column::left("格式", 8),
        Column::left("编码", 8),
        Column::left("清晰度", 16),
    ];
    let rows: Vec<Vec<Cell>> = streams
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            vec![
                Cell::muted((idx + 1).to_string()),
                Cell::cyan(s.protocol.clone()),
                Cell::plain(s.format.clone()),
                Cell::muted(s.codec.clone()),
                Cell::pink(format!("{} ({})", s.quality_desc, s.current_qn)),
            ]
        })
        .collect();
    print_table(&cols, &rows);

    if !is_quiet() {
        print_spacer();
        let use_color = crate::ui::theme::color_enabled();
        let mut stdout = std::io::stdout().lock();
        for (idx, s) in streams.iter().enumerate() {
            let tag = format!(
                "[{}] {} · {} · {} · {} ({})",
                idx + 1,
                s.protocol,
                s.format,
                s.codec,
                s.quality_desc,
                s.current_qn
            );
            let header_ok = if use_color {
                use crossterm::style::Stylize;
                writeln!(stdout, "  {}", tag.with(crate::ui::theme::BILI_CYAN).bold())
            } else {
                writeln!(stdout, "  {tag}")
            };
            if header_ok.is_err() || writeln!(stdout, "  {}", s.url).is_err() {
                break;
            }
        }
    }
    print_spacer();
    Ok(())
}

pub(crate) fn handle_live_danmaku(room: String, limit: u32) -> Result<()> {
    let canonical_id = resolve_target_room_canonical(&room)?;
    let data = crate::api::live::get_live_danmaku_history(canonical_id)?;

    let mut merged: Vec<serde_json::Value> = Vec::new();
    for key in ["admin", "room"] {
        if let Some(arr) = data[key].as_array() {
            merged.extend(arr.iter().cloned());
        }
    }
    merged.sort_by(|a, b| {
        let ta = a["timeline"].as_str().unwrap_or("");
        let tb = b["timeline"].as_str().unwrap_or("");
        ta.cmp(tb)
    });
    let max_items = limit as usize;
    if merged.len() > max_items {
        merged = merged.split_off(merged.len() - max_items);
    }

    if is_json() {
        print_json(&serde_json::json!({
            "room_id": canonical_id,
            "limit": limit,
            "count": merged.len(),
            "danmaku": merged
        }))?;
        return Ok(());
    }

    print_spacer();
    print_section_header(
        &format!("直播间近期弹幕 · 房间 {canonical_id}"),
        Some(&format!("共 {} 条 (上限 {limit})", merged.len())),
    );
    print_spacer();

    if merged.is_empty() {
        user_info!("暂无历史弹幕");
        return Ok(());
    }

    let cols = [
        Column::left("时间", 20),
        Column::left("发送者", 18),
        Column::left("弹幕内容", 44),
    ];
    let rows: Vec<Vec<Cell>> = merged
        .iter()
        .map(|item| {
            let timeline = item["timeline"].as_str().unwrap_or("-");
            let nickname = item["nickname"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| item["user"]["base"]["name"].as_str())
                .unwrap_or("匿名用户");
            let text = item["text"].as_str().unwrap_or("");
            let is_admin = item["isadmin"].as_i64().unwrap_or(0) == 1;
            let name_cell = if is_admin {
                Cell::pink(format!("[房管] {nickname}"))
            } else {
                Cell::cyan(nickname)
            };
            vec![Cell::muted(timeline), name_cell, Cell::plain(text)]
        })
        .collect();
    print_table(&cols, &rows);
    print_spacer();
    Ok(())
}

fn build_room_initial_context(
    canonical_id: i64,
    history_limit: u32,
) -> Result<crate::ui::live_chat::RoomInitialContext> {
    let room_info = crate::api::live::get_room_info(canonical_id)?;
    let anchor_name = crate::api::live::get_anchor_in_room(canonical_id)
        .ok()
        .and_then(|d| d["info"]["uname"].as_str().map(str::to_string))
        .unwrap_or_default();

    let title = room_info["title"].as_str().unwrap_or("-").to_string();
    let area_name = room_info["area_name"].as_str().unwrap_or("").to_string();
    let is_live = room_info["live_status"].as_i64() == Some(1);
    let online = room_info["online"].as_u64().unwrap_or(0);

    let mut history_items = Vec::new();
    if history_limit > 0
        && let Ok(hist) = crate::api::live::get_live_danmaku_history(canonical_id)
    {
        let mut merged: Vec<serde_json::Value> = Vec::new();
        for key in ["admin", "room"] {
            if let Some(arr) = hist[key].as_array() {
                merged.extend(arr.iter().cloned());
            }
        }
        merged.sort_by(|a, b| {
            let ta = a["timeline"].as_str().unwrap_or("");
            let tb = b["timeline"].as_str().unwrap_or("");
            ta.cmp(tb)
        });
        let max_n = history_limit as usize;
        if merged.len() > max_n {
            merged = merged.split_off(merged.len() - max_n);
        }
        for item in merged {
            let raw_tl = item["timeline"].as_str().unwrap_or("");
            let hhmmss = raw_tl
                .split_whitespace()
                .nth(1)
                .unwrap_or(raw_tl)
                .to_string();
            let nick = item["nickname"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| item["user"]["base"]["name"].as_str())
                .unwrap_or("观众")
                .to_string();
            let text = item["text"].as_str().unwrap_or("").to_string();
            if !text.is_empty() {
                history_items.push((hhmmss, nick, text));
            }
        }
    }

    Ok(crate::ui::live_chat::RoomInitialContext {
        room_id: canonical_id,
        title,
        area_name,
        anchor_name,
        is_live,
        online,
        history_items,
    })
}

pub(crate) fn handle_live_danmaku_follow(
    room: String,
    show_enter: bool,
    max_events: Option<u32>,
) -> Result<()> {
    let canonical_id = resolve_target_room_canonical(&room)?;
    let cookies = crate::auth::cookies::read_cookies().ok();
    let ctx = build_room_initial_context(canonical_id, 0)?;
    crate::ui::live_chat::run_follow_stream(ctx, cookies, show_enter, max_events)
}

pub(crate) fn handle_live_chat(room: Option<String>, no_enter: bool, history: u32) -> Result<()> {
    let cookies = crate::auth::cookies::read_cookies().ok();
    let canonical_id = match room {
        Some(ref raw) => resolve_target_room_canonical(raw)?,
        None => {
            let mut ck = crate::auth::ensure_login()?;
            resolve_own_room_id(&mut ck)?
        }
    };
    let ctx = build_room_initial_context(canonical_id, history)?;
    crate::ui::live_chat::run_live_chat_tui(ctx, cookies, !no_enter)
}

pub(crate) fn handle_live_send(room: String, message: String) -> Result<()> {
    if message.trim().is_empty() {
        return Err(BiliLiveError::Input("弹幕内容不能为空".to_string()));
    }
    let cookies = crate::auth::ensure_csrf()?;
    let canonical_id = resolve_target_room_canonical(&room)?;
    let data = crate::api::live::send_live_danmaku(&cookies, canonical_id, &message)?;

    if is_json() {
        print_json(&serde_json::json!({
            "success": true,
            "room_id": canonical_id,
            "message": message,
            "data": data
        }))?;
    } else {
        user_success!("弹幕已发送至直播间 {}: {}", canonical_id, message);
    }
    Ok(())
}

pub(crate) struct LiveRecordOptions {
    pub room: String,
    pub output: Option<PathBuf>,
    pub quality: u32,
    pub duration: Option<String>,
    pub danmaku: bool,
    pub danmaku_format: crate::cli::args::DownloadDanmakuFormat,
    pub watch: bool,
    pub interval: u64,
    pub once: bool,
}

struct LiveDanmakuRecorder {
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    collector: std::thread::JoinHandle<Vec<crate::api::danmaku::DanmakuItem>>,
}

fn start_live_danmaku_recorder(room_id: i64, cookies: Option<Cookies>) -> LiveDanmakuRecorder {
    use crate::api::danmaku::DanmakuItem;
    use crate::live::ws::{LiveEvent, spawn_live_ws_listener};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let stop_flag = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<LiveEvent>();
    let _ws_handle = spawn_live_ws_listener(room_id, cookies, stop_flag.clone(), tx);

    let stop_clone = stop_flag.clone();
    let collector = std::thread::spawn(move || {
        let seg_start = Instant::now();
        let mut items: Vec<DanmakuItem> = Vec::new();
        while !stop_clone.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(120)) {
                Ok(LiveEvent::Danmaku {
                    uid,
                    text,
                    color_rgb,
                    ..
                }) => {
                    let color = color_rgb & 0x00FF_FFFF;
                    items.push(DanmakuItem {
                        time: seg_start.elapsed().as_secs_f64(),
                        mode: 1,
                        font_size: 25,
                        color,
                        color_hex: format!("#{:06X}", color),
                        send_time: chrono::Utc::now().timestamp(),
                        pool: 0,
                        sender_hash: uid.to_string(),
                        dmid: (items.len() + 1).to_string(),
                        weight: None,
                        text,
                    });
                }
                Ok(LiveEvent::SuperChat {
                    uname, price, text, ..
                }) => {
                    items.push(DanmakuItem {
                        time: seg_start.elapsed().as_secs_f64(),
                        mode: 5,
                        font_size: 25,
                        color: 0xFFD700,
                        color_hex: "#FFD700".to_string(),
                        send_time: chrono::Utc::now().timestamp(),
                        pool: 0,
                        sender_hash: "0".to_string(),
                        dmid: (items.len() + 1).to_string(),
                        weight: None,
                        text: format!("[SC ¥{price}] {uname}: {text}"),
                    });
                }
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        items
    });

    LiveDanmakuRecorder {
        stop_flag,
        collector,
    }
}

impl LiveDanmakuRecorder {
    fn finish_and_save(
        self,
        video_path: &std::path::Path,
        room_id: i64,
        fmt: crate::cli::args::DownloadDanmakuFormat,
    ) -> Result<(PathBuf, usize)> {
        self.stop_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let items = self.collector.join().unwrap_or_default();
        let count = items.len();

        let ext = match fmt {
            crate::cli::args::DownloadDanmakuFormat::Ass => "ass",
            crate::cli::args::DownloadDanmakuFormat::Xml => "xml",
        };
        let dm_path = video_path.with_extension(ext);
        let content = match fmt {
            crate::cli::args::DownloadDanmakuFormat::Ass => {
                crate::api::danmaku::to_ass(&items, &format!("Live Room {room_id}"))
            }
            crate::cli::args::DownloadDanmakuFormat::Xml => {
                crate::api::danmaku::to_xml(&items, room_id)
            }
        };
        std::fs::write(&dm_path, content.as_bytes())?;
        Ok((dm_path, count))
    }
}

fn resolve_segment_output_path(
    base_output: Option<&std::path::Path>,
    canonical_id: i64,
    stream_format: &str,
    segment_idx: usize,
    watch_mode: bool,
) -> Result<PathBuf> {
    let default_ext = if stream_format == "flv" { "flv" } else { "ts" };
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");

    match base_output {
        Some(p) => {
            if p.is_dir() || p.to_string_lossy().ends_with('/') {
                std::fs::create_dir_all(p)?;
                Ok(p.join(format!("live_{canonical_id}_{ts}.{default_ext}")))
            } else {
                if let Some(parent) = p.parent()
                    && !parent.as_os_str().is_empty()
                {
                    std::fs::create_dir_all(parent)?;
                }
                if !watch_mode && segment_idx == 0 {
                    return Ok(p.to_path_buf());
                }
                if segment_idx == 0 && !p.exists() {
                    return Ok(p.to_path_buf());
                }
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("live");
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or(default_ext);
                let parent = p.parent().unwrap_or_else(|| std::path::Path::new("."));
                Ok(parent.join(format!("{stem}_part{}_{ts}.{ext}", segment_idx + 1)))
            }
        }
        None => Ok(PathBuf::from(format!(
            "live_{canonical_id}_{ts}.{default_ext}"
        ))),
    }
}

fn run_ffmpeg_segment(
    ffmpeg_bin: &std::path::Path,
    stream_url: &str,
    duration_secs: Option<u64>,
    output_path: &std::path::Path,
) -> Result<bool> {
    use std::process::Stdio;
    use std::time::Duration;

    let mut cmd = std::process::Command::new(ffmpeg_bin);
    cmd.args([
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-user_agent",
        crate::api::client::DEFAULT_USER_AGENT,
        "-headers",
        "Referer: https://live.bilibili.com/\r\n",
        "-i",
        stream_url,
    ]);
    if let Some(secs) = duration_secs {
        cmd.args(["-t", &secs.to_string()]);
    }
    cmd.args(["-c", "copy"]);
    cmd.arg(output_path);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let mut stdin_pipe = child.stdin.take();
    let stderr_pipe = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || -> String {
        use std::io::Read;
        let mut s = String::new();
        if let Some(mut r) = stderr_pipe {
            let _ = r.read_to_string(&mut s);
        }
        s
    });

    let mut interrupted = false;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if crate::utils::interrupt::is_interrupted() {
            interrupted = true;
            // 向 ffmpeg 发送 'q' 让其正常封装文件尾后退出
            if let Some(mut sin) = stdin_pipe.take() {
                let _ = sin.write_all(b"q\n");
                let _ = sin.flush();
            }
            let deadline = std::time::Instant::now() + Duration::from_millis(1200);
            let mut exited = None;
            while std::time::Instant::now() < deadline {
                if let Some(st) = child.try_wait()? {
                    exited = Some(st);
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            match exited {
                Some(st) => break st,
                None => {
                    let _ = child.kill();
                    break child.wait()?;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(60));
    };

    let stderr_tail = stderr_handle.join().unwrap_or_default();
    if interrupted || crate::utils::interrupt::is_interrupted() {
        return Ok(true);
    }

    if !status.success() {
        let code_str = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        return Err(BiliLiveError::ExternalTool(format!(
            "ffmpeg 录制失败 (exit {code_str}): {}",
            stderr_tail.trim()
        )));
    }
    Ok(false)
}

pub(crate) fn handle_live_record(opts: LiveRecordOptions) -> Result<()> {
    if let Some(ref p) = opts.output
        && p.as_os_str().is_empty()
    {
        return Err(BiliLiveError::Input("输出文件路径不能为空".to_string()));
    }
    let ffmpeg_bin = crate::utils::ffmpeg::ensure_ffmpeg()?;
    let duration_secs = opts.duration.as_deref().map(parse_delay).transpose()?;

    let cookies = crate::auth::cookies::read_cookies().ok();
    let canonical_id = resolve_target_room_canonical(&opts.room)?;

    if !opts.watch {
        let play_info =
            crate::api::live::get_room_play_info(canonical_id, opts.quality, cookies.as_ref())?;
        let live_status = play_info["live_status"].as_i64().unwrap_or(0);
        let streams = crate::api::live::extract_live_streams(&play_info);

        let selected = streams
            .iter()
            .find(|s| s.protocol == "http_stream" && s.format == "flv")
            .or_else(|| streams.iter().find(|s| s.protocol == "http_stream"))
            .or_else(|| streams.iter().find(|s| s.format == "ts"))
            .or_else(|| streams.first())
            .ok_or_else(|| {
                BiliLiveError::Api(format!(
                    "直播间 {canonical_id} 未开播或暂无可录制的流地址 (live_status={live_status})"
                ))
            })?;

        let output_path = resolve_segment_output_path(
            opts.output.as_deref(),
            canonical_id,
            &selected.format,
            0,
            false,
        )?;

        user_info!(
            "开始录制直播间 {} ({}{}) -> {}",
            canonical_id,
            selected.quality_desc,
            if opts.danmaku { " + 实时弹幕" } else { "" },
            output_path.display()
        );

        let dm_rec = opts
            .danmaku
            .then(|| start_live_danmaku_recorder(canonical_id, cookies.clone()));
        let was_interrupted =
            run_ffmpeg_segment(&ffmpeg_bin, &selected.url, duration_secs, &output_path)?;

        let dm_saved = match dm_rec {
            Some(rec) => {
                Some(rec.finish_and_save(&output_path, canonical_id, opts.danmaku_format)?)
            }
            None => None,
        };

        let bytes = std::fs::metadata(&output_path)
            .map(|m| m.len())
            .unwrap_or(0);

        if is_json() {
            let mut payload = serde_json::json!({
                "success": true,
                "room_id": canonical_id,
                "stream_url": selected.url,
                "format": selected.format,
                "quality": selected.current_qn,
                "duration_secs": duration_secs,
                "output": output_path.display().to_string(),
                "bytes": bytes
            });
            if let Some((ref dm_path, dm_cnt)) = dm_saved {
                payload["danmaku_file"] = serde_json::json!(dm_path.display().to_string());
                payload["danmaku_count"] = serde_json::json!(dm_cnt);
            }
            print_json(&payload)?;
        } else if let Some((ref dm_path, dm_cnt)) = dm_saved {
            user_success!(
                "直播录制完成: {} ({} bytes) · 同步弹幕: {} ({} 条)",
                output_path.display(),
                bytes,
                dm_path.display(),
                dm_cnt
            );
        } else {
            user_success!("直播录制完成: {} ({} bytes)", output_path.display(), bytes);
        }

        if was_interrupted {
            return Err(BiliLiveError::Interrupted);
        }
        return Ok(());
    }

    // ── --watch 无人值守自动蹲守与断流重连分段录制模式 ──
    user_info!(
        "进入直播值守录制模式: 房间 #{} (轮询间隔={}s{}，按 Ctrl+C 停止)",
        canonical_id,
        opts.interval,
        if opts.danmaku {
            "，同步录制弹幕"
        } else {
            ""
        }
    );

    let mut segment_reports = Vec::new();
    let mut segment_idx: usize = 0;
    let mut had_live_session = false;
    let mut logged_waiting = false;

    loop {
        if crate::utils::interrupt::is_interrupted() {
            break;
        }

        let play_res =
            crate::api::live::get_room_play_info(canonical_id, opts.quality, cookies.as_ref());
        let play_info = match play_res {
            Ok(info) => info,
            Err(e) => {
                user_warning!("查询直播状态暂时失败，{}s 后重试: {}", opts.interval, e);
                if crate::utils::interrupt::sleep_interruptible(std::time::Duration::from_secs(
                    opts.interval,
                ))
                .is_err()
                {
                    break;
                }
                continue;
            }
        };

        let live_status = play_info["live_status"].as_i64().unwrap_or(0);
        let streams = crate::api::live::extract_live_streams(&play_info);
        let selected = if live_status == 1 {
            streams
                .iter()
                .find(|s| s.protocol == "http_stream" && s.format == "flv")
                .or_else(|| streams.iter().find(|s| s.protocol == "http_stream"))
                .or_else(|| streams.iter().find(|s| s.format == "ts"))
                .or_else(|| streams.first())
        } else {
            None
        };

        let Some(stream) = selected else {
            if had_live_session && opts.once {
                user_info!("主播已下播，--once 单场值守录制结束");
                break;
            }
            if !logged_waiting {
                user_info!(
                    "直播间 #{} 当前未开播 (status={})，蹲守等待开播中...",
                    canonical_id,
                    live_status
                );
                logged_waiting = true;
            }
            if crate::utils::interrupt::sleep_interruptible(std::time::Duration::from_secs(
                opts.interval,
            ))
            .is_err()
            {
                break;
            }
            continue;
        };

        had_live_session = true;
        logged_waiting = false;

        let seg_path = resolve_segment_output_path(
            opts.output.as_deref(),
            canonical_id,
            &stream.format,
            segment_idx,
            true,
        )?;
        segment_idx += 1;

        user_info!(
            "检测到房间 #{} 正在直播，开始录制第 {} 段 ({}) -> {}",
            canonical_id,
            segment_idx,
            stream.quality_desc,
            seg_path.display()
        );

        let dm_rec = opts
            .danmaku
            .then(|| start_live_danmaku_recorder(canonical_id, cookies.clone()));
        let seg_res = run_ffmpeg_segment(&ffmpeg_bin, &stream.url, duration_secs, &seg_path);

        let dm_saved = match dm_rec {
            Some(rec) => rec
                .finish_and_save(&seg_path, canonical_id, opts.danmaku_format)
                .ok(),
            None => None,
        };

        let bytes = std::fs::metadata(&seg_path).map(|m| m.len()).unwrap_or(0);
        let mut report = serde_json::json!({
            "segment": segment_idx,
            "output": seg_path.display().to_string(),
            "bytes": bytes,
            "quality": stream.current_qn,
        });
        if let Some((ref dm_p, dm_c)) = dm_saved {
            report["danmaku_file"] = serde_json::json!(dm_p.display().to_string());
            report["danmaku_count"] = serde_json::json!(dm_c);
        }
        segment_reports.push(report);

        match seg_res {
            Ok(true) => {
                user_info!("已收到停止信号，正在收尾当前分段: {}", seg_path.display());
                break;
            }
            Ok(false) => {
                user_success!(
                    "分段 #{} 录制结束: {} ({} bytes)",
                    segment_idx,
                    seg_path.display(),
                    bytes
                );
                if duration_secs.is_some() && opts.once {
                    break;
                }
            }
            Err(e) => {
                user_warning!("分段 #{} 流中断 ({})，3s 后自动检查重连...", segment_idx, e);
            }
        }

        if crate::utils::interrupt::sleep_interruptible(std::time::Duration::from_secs(3)).is_err()
        {
            break;
        }
    }

    if is_json() {
        print_json(&serde_json::json!({
            "success": true,
            "room_id": canonical_id,
            "watch": true,
            "segment_count": segment_reports.len(),
            "segments": segment_reports,
        }))?;
    } else {
        user_success!("值守录制结束，共保存 {} 个视频分段", segment_reports.len());
    }

    if crate::utils::interrupt::is_interrupted() {
        return Err(BiliLiveError::Interrupted);
    }
    Ok(())
}

// 解析延迟时间字符串，返回秒数。支持 h(时) m(分) s(秒) 单位
pub fn parse_delay(input: &str) -> Result<u64> {
    let mut total = 0u64;
    let mut num = String::new();
    for c in input.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if num.is_empty() {
                return Err(BiliLiveError::Parse(format!(
                    "无效的时间格式: 连续的单位或单位出现在数值之前 ({})",
                    input
                )));
            }
            let val: u64 = num
                .parse()
                .map_err(|_| BiliLiveError::Parse(format!("无效的时间数值: {}", input)))?;
            num.clear();
            let multiplier = match c {
                'h' | 'H' => 3600,
                'm' | 'M' => 60,
                's' | 'S' => 1,
                _ => {
                    return Err(BiliLiveError::Parse(format!(
                        "无效的时间单位 '{}' (仅支持 h, m, s)",
                        c
                    )));
                }
            };
            total = val
                .checked_mul(multiplier)
                .and_then(|seconds| total.checked_add(seconds))
                .ok_or_else(|| BiliLiveError::Parse("延迟时间超过支持范围".to_string()))?;
        }
    }
    if !num.is_empty() {
        return Err(BiliLiveError::Parse(format!(
            "数值 '{}' 缺少时间单位 (例如 h, m, s)",
            num
        )));
    }
    if total == 0 {
        return Err(BiliLiveError::Parse(format!("无效的延迟时间: {}", input)));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_delay() {
        assert_eq!(parse_delay("30s").unwrap(), 30);
        assert_eq!(parse_delay("5m").unwrap(), 300);
        assert_eq!(parse_delay("2h").unwrap(), 7200);
        assert_eq!(parse_delay("1h30m10s").unwrap(), 5410);

        // 验证错误格式的处理
        assert!(parse_delay("30").is_err());
        assert!(parse_delay("30m45").is_err());
        assert!(parse_delay("abc").is_err());
        assert!(parse_delay("m30").is_err());
        assert!(parse_delay("18446744073709551615h").is_err());
        assert!(parse_delay("18446744073709551615m").is_err());
        assert!(parse_delay("18446744073709551615s2s").is_err());
        assert_eq!(parse_delay("18446744073709551615s").unwrap(), u64::MAX);
        assert!(parse_delay("").is_err());
        assert!(parse_delay("0s").is_err());
    }

    fn serve_mock_http(
        responses: Vec<(&'static str, String)>,
    ) -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        std::thread::JoinHandle<()>,
    ) {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let rec_clone = Arc::clone(&recorded);
        let handle = std::thread::spawn(move || {
            for (expected_prefix, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                rec_clone.lock().unwrap().push(first.trim().to_string());
                assert!(
                    first.contains(expected_prefix),
                    "expected `{expected_prefix}` in `{first}`"
                );
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        (base, recorded, handle)
    }

    struct LiveTestGuard {
        live_url: Option<String>,
        api_url: Option<String>,
        ffmpeg: Option<String>,
        path: Option<String>,
    }
    impl LiveTestGuard {
        fn capture() -> Self {
            Self {
                live_url: std::env::var("BT_LIVE_API_BASE_URL").ok(),
                api_url: std::env::var("BT_API_BASE_URL").ok(),
                ffmpeg: std::env::var("BT_FFMPEG_PATH").ok(),
                path: std::env::var("PATH").ok(),
            }
        }
    }
    impl Drop for LiveTestGuard {
        fn drop(&mut self) {
            crate::utils::paths::set_cookie_file_override(None);
            crate::cli::output::init_output(false, false, false, false, None);
            unsafe {
                match &self.live_url {
                    Some(v) => std::env::set_var("BT_LIVE_API_BASE_URL", v),
                    None => std::env::remove_var("BT_LIVE_API_BASE_URL"),
                }
                match &self.api_url {
                    Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                    None => std::env::remove_var("BT_API_BASE_URL"),
                }
                match &self.ffmpeg {
                    Some(v) => std::env::set_var("BT_FFMPEG_PATH", v),
                    None => std::env::remove_var("BT_FFMPEG_PATH"),
                }
                match &self.path {
                    Some(v) => std::env::set_var("PATH", v),
                    None => std::env::remove_var("PATH"),
                }
            }
        }
    }

    #[test]
    fn live_start_when_already_live_with_yes_never_stops_and_relogin_is_guarded() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = LiveTestGuard::capture();

        let tmp = tempfile::TempDir::new().unwrap();
        let cookie_path = tmp.path().join("cookies.json");
        let cookies = Cookies {
            sessdata: "sess".into(),
            bili_jct: "jct".into(),
            dede_user_id: "10".into(),
            buvid3: "b3".into(),
            buvid4: "b4".into(),
            room_id: 14073662,
            ..Default::default()
        };
        cookies.save_to(&cookie_path).unwrap();
        crate::utils::paths::set_cookie_file_override(Some(cookie_path.clone()));

        let (base, recorded, handle) = serve_mock_http(vec![(
            "/room/v1/Room/get_info?room_id=14073662",
            r#"{"code":0,"data":{"room_id":14073662,"live_status":1,"title":"Live Now","area_id":371,"area_name":"网游"}}"#.into(),
        )]);

        crate::cli::output::init_output(true, true, true, true, Some(cookie_path.clone()));
        unsafe {
            std::env::set_var("BT_LIVE_API_BASE_URL", &base);
        }

        handle_live_start(None, None, false, false).unwrap();
        handle.join().unwrap();
        let reqs = recorded.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert!(!reqs.iter().any(|r| r.contains("stopLive")));

        // --relogin in --json mode must fail WITHOUT deleting cookie_path
        let err = handle_live_start(None, None, false, true).unwrap_err();
        assert!(matches!(err, BiliLiveError::Auth(_)));
        assert!(cookie_path.exists());
    }

    #[test]
    fn live_stop_delay_prevalidates_before_sleeping_when_offline() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = LiveTestGuard::capture();

        let tmp = tempfile::TempDir::new().unwrap();
        let cookie_path = tmp.path().join("cookies.json");
        let cookies = Cookies {
            sessdata: "sess".into(),
            bili_jct: "jct".into(),
            dede_user_id: "10".into(),
            buvid3: "b3".into(),
            buvid4: "b4".into(),
            room_id: 14073662,
            ..Default::default()
        };
        cookies.save_to(&cookie_path).unwrap();
        crate::utils::paths::set_cookie_file_override(Some(cookie_path.clone()));

        let (base, recorded, handle) = serve_mock_http(vec![(
            "/room/v1/Room/get_info?room_id=14073662",
            r#"{"code":0,"data":{"room_id":14073662,"live_status":0}}"#.into(),
        )]);

        crate::cli::output::init_output(true, true, false, true, Some(cookie_path));
        unsafe {
            std::env::set_var("BT_LIVE_API_BASE_URL", &base);
        }

        let start = std::time::Instant::now();
        handle_live_stop(Some("60s".into())).unwrap();
        let elapsed = start.elapsed();
        handle.join().unwrap();

        assert!(elapsed < std::time::Duration::from_secs(2));
        let reqs = recorded.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert!(!reqs.iter().any(|r| r.contains("stopLive")));
    }

    #[test]
    fn live_info_danmaku_and_send_validation() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = LiveTestGuard::capture();

        // Empty danmaku message fails before network
        let err = handle_live_send("76".into(), "   ".into()).unwrap_err();
        assert!(matches!(err, BiliLiveError::Input(_)));

        let (base, _rec, handle) = serve_mock_http(vec![
            (
                "/room/v1/Room/room_init?id=76",
                r#"{"code":0,"data":{"room_id":14073662,"short_id":76,"uid":99,"live_status":1}}"#.into(),
            ),
            (
                "/room/v1/Room/get_info?room_id=14073662",
                r#"{"code":0,"data":{"room_id":14073662,"short_id":76,"uid":99,"live_status":1,"title":"Test Room","area_id":371,"area_name":"网游","parent_area_name":"游戏","online":1234,"attention":56,"live_time":"2026-01-01 00:00:00"}}"#.into(),
            ),
            (
                "/live_user/v1/UserInfo/get_anchor_in_room?roomid=14073662",
                r#"{"code":0,"data":{"info":{"uid":99,"uname":"TestAnchor"}}}"#.into(),
            ),
            (
                "/room/v1/Room/room_init?id=76",
                r#"{"code":0,"data":{"room_id":14073662,"short_id":76,"uid":99,"live_status":1}}"#.into(),
            ),
            (
                "/xlive/web-room/v1/dM/gethistory?roomid=14073662",
                r#"{"code":0,"data":{"admin":null,"room":[{"text":"hi","nickname":"u1","timeline":"2026-01-01 00:00:01","isadmin":0}]}}"#.into(),
            ),
        ]);

        crate::cli::output::init_output(true, true, false, true, None);
        unsafe {
            std::env::set_var("BT_LIVE_API_BASE_URL", &base);
        }

        handle_live_info(Some("76".into())).unwrap();
        handle_live_danmaku("76".into(), 10).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn live_record_guards_missing_ffmpeg_before_network_and_runs_mock_ffmpeg() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = LiveTestGuard::capture();

        let tmp = tempfile::TempDir::new().unwrap();
        crate::cli::output::init_output(true, true, false, true, None);
        unsafe {
            std::env::remove_var("BT_FFMPEG_PATH");
            std::env::set_var("PATH", tmp.path());
        }

        // Listener proves 0 HTTP requests are sent when ffmpeg is missing
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        unsafe {
            std::env::set_var("BT_LIVE_API_BASE_URL", &base);
        }

        let err = handle_live_record(LiveRecordOptions {
            room: "76".into(),
            output: Some(tmp.path().join("out.flv")),
            quality: 10000,
            duration: Some("5s".into()),
            danmaku: false,
            danmaku_format: crate::cli::args::DownloadDanmakuFormat::Ass,
            watch: false,
            interval: 30,
            once: false,
        })
        .unwrap_err();
        assert!(matches!(err, BiliLiveError::ExternalTool(_)));
        assert!(
            listener.accept().is_err(),
            "no HTTP requests expected when ffmpeg is missing"
        );

        // Now test with a mock executable ffmpeg script via BT_FFMPEG_PATH
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script_path = tmp.path().join("mock_ffmpeg");
            let args_log = tmp.path().join("ffmpeg_args.txt");
            let script = format!(
                "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then echo 'ffmpeg version 6.0'; exit 0; fi\nprintf '%s\\n' \"$@\" > \"{}\"\nout=\"\"\nfor a in \"$@\"; do out=\"$a\"; done\nprintf 'FLVDATA' > \"$out\"\n",
                args_log.display()
            );
            std::fs::write(&script_path, script).unwrap();
            let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).unwrap();

            let (mock_base, _rec, handle) = serve_mock_http(vec![
                (
                    "/room/v1/Room/room_init?id=76",
                    r#"{"code":0,"data":{"room_id":14073662,"short_id":76,"uid":99,"live_status":1}}"#.into(),
                ),
                (
                    "/xlive/web-room/v2/index/getRoomPlayInfo?room_id=14073662",
                    r#"{"code":0,"data":{"room_id":14073662,"live_status":1,"playurl_info":{"playurl":{"g_qn_desc":[{"qn":10000,"desc":"原画"}],"stream":[{"protocol_name":"http_stream","format":[{"format_name":"flv","codec":[{"codec_name":"avc","current_qn":10000,"accept_qn":[10000],"base_url":"/live/14073662.flv?","url_info":[{"host":"http://127.0.0.1:9","extra":"k=1"}]}]}]}]}}}}"#.into(),
                ),
            ]);

            unsafe {
                std::env::set_var("BT_FFMPEG_PATH", &script_path);
                std::env::set_var("BT_LIVE_API_BASE_URL", &mock_base);
            }

            let out_file = tmp.path().join("recorded/test.flv");
            handle_live_record(LiveRecordOptions {
                room: "76".into(),
                output: Some(out_file.clone()),
                quality: 10000,
                duration: Some("5s".into()),
                danmaku: false,
                danmaku_format: crate::cli::args::DownloadDanmakuFormat::Ass,
                watch: false,
                interval: 30,
                once: false,
            })
            .unwrap();
            handle.join().unwrap();

            assert_eq!(std::fs::read(&out_file).unwrap(), b"FLVDATA");
            let logged = std::fs::read_to_string(&args_log).unwrap();
            assert!(logged.contains("-t\n5\n"));
            assert!(logged.contains("http://127.0.0.1:9/live/14073662.flv?k=1"));
        }
    }
}
