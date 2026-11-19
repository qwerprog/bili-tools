use crate::auth::cookies::Cookies;
use crate::cli::output::{is_json, is_quiet};
use crate::error::{BiliLiveError, Result};
use crate::live::ws::{
    InteractKind, LiveEvent, now_hhmmss, spawn_live_ws_listener,
};
use crate::ui::components::wrap_to_width;
use crate::ui::theme::{BILI_CYAN, BILI_PINK, color_enabled};
use crate::utils::string::{display_width, pad_to_width, truncate_to_width};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Color, Stylize};
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ execute, queue };
use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MAX_CHAT_ITEMS: usize = 800;
const DANMAKU_CHAR_LIMIT: usize = 30;

/// RAII 守卫：无论正常退出还是异常中断，自动退出备用屏幕并恢复终端光标与 Cooked 模式
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode().map_err(BiliLiveError::Io)?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, Clear(ClearType::All), Show)
            .map_err(BiliLiveError::Io)?;
        crate::utils::interrupt::set_alt_screen_active(true);
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        crate::utils::interrupt::set_alt_screen_active(false);
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ChatSubmission {
    Empty,
    Quit,
    Clear,
    ToggleEnter,
    Help,
    UnknownCommand(String),
    SendDanmaku(String),
}

fn parse_chat_submission(trimmed: &str) -> ChatSubmission {
    if trimmed.is_empty() {
        return ChatSubmission::Empty;
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return ChatSubmission::SendDanmaku(format!("/{rest}"));
    }
    if trimmed.starts_with('/') {
        let cmd = trimmed
            .split_whitespace()
            .next()
            .unwrap_or(trimmed)
            .to_ascii_lowercase();
        return match cmd.as_str() {
            "/q" | "/quit" | "/exit" => ChatSubmission::Quit,
            "/clear" | "/cls" => ChatSubmission::Clear,
            "/enter" => ChatSubmission::ToggleEnter,
            "/help" | "/?" | "/h" => ChatSubmission::Help,
            _ => ChatSubmission::UnknownCommand(cmd),
        };
    }
    ChatSubmission::SendDanmaku(trimmed.to_string())
}

#[derive(Debug, Clone)]
pub struct RoomInitialContext {
    pub room_id: i64,
    pub title: String,
    pub area_name: String,
    pub anchor_name: String,
    pub is_live: bool,
    pub online: u64,
    pub history_items: Vec<(String, String, String)>, // (hh:mm:ss, nickname, text)
}

#[derive(Debug, Clone)]
enum FeedItemKind {
    Danmaku {
        uname: String,
        text: String,
        medal: Option<(String, u32)>,
        guard_level: u8,
        is_admin: bool,
        is_self: bool,
        color_rgb: u32,
    },
    SuperChat {
        uname: String,
        price: i64,
        text: String,
    },
    Gift {
        uname: String,
        action: String,
        gift_name: String,
        num: u64,
    },
    GuardBuy {
        uname: String,
        guard_name: String,
        num: u64,
    },
    Interact {
        uname: String,
        kind: InteractKind,
        medal: Option<(String, u32)>,
    },
    System {
        is_error: bool,
        message: String,
    },
}

#[derive(Debug, Clone)]
struct FeedItem {
    time: String,
    kind: FeedItemKind,
}

impl FeedItem {
    fn is_enter_event(&self) -> bool {
        matches!(
            self.kind,
            FeedItemKind::Interact {
                kind: InteractKind::Enter,
                ..
            }
        )
    }
}

enum UiMsg {
    Live(LiveEvent),
    SendDone { text: String },
    SendErr { err: String },
}

/// 流式监听模式 (`bt live danmaku [ROOM] -f`)：持续输出弹幕流或 NDJSON 事件流
pub fn run_follow_stream(
    ctx: RoomInitialContext,
    cookies: Option<Cookies>,
    show_enter: bool,
    max_events: Option<u32>,
) -> Result<()> {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<LiveEvent>();
    let _ws_handle = spawn_live_ws_listener(ctx.room_id, cookies, stop_flag.clone(), tx);

    if !is_json() && !is_quiet() {
        let use_color = color_enabled();
        let mut stderr = std::io::stderr();
        if use_color {
            let _ = writeln!(
                stderr,
                "  {} 正在实时监听直播间 {} ({}) · 按 Ctrl+C 停止",
                "●".with(BILI_PINK).bold(),
                format!("#{}", ctx.room_id).with(BILI_CYAN).bold(),
                ctx.title
            );
        } else {
            let _ = writeln!(
                stderr,
                "  ● 正在实时监听直播间 #{} ({}) · 按 Ctrl+C 停止",
                ctx.room_id, ctx.title
            );
        }
    }

    let use_color = color_enabled();
    let mut emitted: u32 = 0;
    loop {
        if crate::utils::interrupt::is_interrupted() {
            stop_flag.store(true, Ordering::Relaxed);
            return Err(BiliLiveError::Interrupted);
        }
        match rx.recv_timeout(Duration::from_millis(120)) {
            Ok(ev) => {
                if let LiveEvent::Interact {
                    kind: InteractKind::Enter,
                    ..
                } = &ev
                    && !show_enter
                {
                    continue;
                }

                if is_json() {
                    // 仅输出用户可见的业务消息事件，过滤高频人气心跳避免刷屏（仍保留开下播状态变更）
                    if matches!(
                        ev,
                        LiveEvent::Danmaku { .. }
                            | LiveEvent::SuperChat { .. }
                            | LiveEvent::Gift { .. }
                            | LiveEvent::GuardBuy { .. }
                            | LiveEvent::Interact { .. }
                            | LiveEvent::RoomMeta { .. }
                    ) && let Ok(line) = serde_json::to_string(&ev)
                    {
                        let mut stdout = std::io::stdout().lock();
                        if writeln!(stdout, "{line}").is_err() {
                            stop_flag.store(true, Ordering::Relaxed);
                            break;
                        }
                        emitted += 1;
                        if max_events.is_some_and(|lim| emitted >= lim) {
                            break;
                        }
                    }
                    continue;
                }

                if is_quiet() {
                    if let LiveEvent::Danmaku { uname, text, .. } = ev {
                        let mut stdout = std::io::stdout().lock();
                        if writeln!(stdout, "{uname}: {text}").is_err() {
                            stop_flag.store(true, Ordering::Relaxed);
                            break;
                        }
                        emitted += 1;
                        if max_events.is_some_and(|lim| emitted >= lim) {
                            break;
                        }
                    }
                    continue;
                }

                if let Some(formatted) = format_follow_line(&ev, use_color) {
                    let mut stdout = std::io::stdout().lock();
                    if writeln!(stdout, "{formatted}").is_err() {
                        stop_flag.store(true, Ordering::Relaxed);
                        break;
                    }
                    if !matches!(ev, LiveEvent::Disconnected { .. }) {
                        emitted += 1;
                        if max_events.is_some_and(|lim| emitted >= lim) {
                            break;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    stop_flag.store(true, Ordering::Relaxed);
    Ok(())
}

fn format_follow_line(ev: &LiveEvent, use_color: bool) -> Option<String> {
    match ev {
        LiveEvent::Danmaku {
            time,
            uname,
            text,
            medal,
            guard_level,
            is_admin,
            ..
        } => {
            let badge = build_plain_badge(*guard_level, *is_admin, medal.as_ref());
            if use_color {
                Some(format!(
                    "  {} {}{} {}",
                    time.as_str().dark_grey(),
                    if badge.is_empty() {
                        String::new()
                    } else {
                        format!("{} ", badge.with(BILI_CYAN))
                    },
                    format!("{uname}:").bold(),
                    text
                ))
            } else if badge.is_empty() {
                Some(format!("  {time} {uname}: {text}"))
            } else {
                Some(format!("  {time} {badge} {uname}: {text}"))
            }
        }
        LiveEvent::SuperChat {
            time,
            uname,
            price,
            text,
        } => {
            if use_color {
                Some(format!(
                    "  {} {} {} {}",
                    time.as_str().dark_grey(),
                    format!("[SC ¥{price}]").with(BILI_PINK).bold(),
                    format!("{uname}:").yellow().bold(),
                    text.as_str().yellow()
                ))
            } else {
                Some(format!("  {time} [SC ¥{price}] {uname}: {text}"))
            }
        }
        LiveEvent::Gift {
            time,
            uname,
            action,
            gift_name,
            num,
        } => {
            if use_color {
                Some(format!(
                    "  {} 🎁 {} {action} {} × {num}",
                    time.as_str().dark_grey(),
                    uname.as_str().with(BILI_PINK).bold(),
                    gift_name.as_str().yellow()
                ))
            } else {
                Some(format!("  {time} 🎁 {uname} {action} {gift_name} × {num}"))
            }
        }
        LiveEvent::GuardBuy {
            time,
            uname,
            guard_name,
            num,
        } => {
            if use_color {
                Some(format!(
                    "  {} ⚓ {} 开通了 {} × {num}",
                    time.as_str().dark_grey(),
                    uname.as_str().with(BILI_PINK).bold(),
                    guard_name.as_str().with(BILI_CYAN).bold()
                ))
            } else {
                Some(format!("  {time} ⚓ {uname} 开通了 {guard_name} × {num}"))
            }
        }
        LiveEvent::Interact {
            time, uname, kind, ..
        } => {
            let label = kind.label();
            if use_color {
                Some(format!(
                    "  {} · {} {label}",
                    time.as_str().dark_grey(),
                    uname.as_str().dark_grey()
                ))
            } else {
                Some(format!("  {time} · {uname} {label}"))
            }
        }
        LiveEvent::Disconnected { reason } => {
            if use_color {
                Some(format!("  {} {reason}", "⚠".yellow().bold()))
            } else {
                Some(format!("  ⚠ {reason}"))
            }
        }
        _ => None,
    }
}

/// 看发一体全屏终端聊天室 TUI (`bt live chat [ROOM]` / `bt live danmaku -i`)
pub fn run_live_chat_tui(
    ctx: RoomInitialContext,
    cookies: Option<Cookies>,
    mut show_enter: bool,
) -> Result<()> {
    if is_json() {
        return Err(BiliLiveError::Input(
            "交互式弹幕聊天室不支持 --json 模式；如需 JSON 弹幕流请使用 `bt --json live danmaku <ROOM> -f`"
                .to_string(),
        ));
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(BiliLiveError::Input(
            "交互式弹幕聊天室需要交互式终端 (TTY)；管道环境请使用 `bt live danmaku <ROOM> -f`"
                .to_string(),
        ));
    }

    let _guard = TerminalGuard::enter()?;
    let stop_flag = Arc::new(AtomicBool::new(false));
    let (ws_tx, ws_rx) = mpsc::channel::<LiveEvent>();
    let (ui_tx, ui_rx) = mpsc::channel::<UiMsg>();

    // 转发 WebSocket 事件到统一 UI 通道
    let bridge_tx = ui_tx.clone();
    std::thread::spawn(move || {
        while let Ok(ev) = ws_rx.recv() {
            if bridge_tx.send(UiMsg::Live(ev)).is_err() {
                break;
            }
        }
    });

    let _ws_handle = spawn_live_ws_listener(ctx.room_id, cookies.clone(), stop_flag.clone(), ws_tx);

    let mut feed: VecDeque<FeedItem> = VecDeque::with_capacity(MAX_CHAT_ITEMS);
    for (time, nick, text) in ctx.history_items {
        feed.push_back(FeedItem {
            time,
            kind: FeedItemKind::Danmaku {
                uname: nick,
                text,
                medal: None,
                guard_level: 0,
                is_admin: false,
                is_self: false,
                color_rgb: 0xFFFFFF,
            },
        });
    }
    feed.push_back(FeedItem {
        time: now_hhmmss(),
        kind: FeedItemKind::System {
            is_error: false,
            message: format!(
                "正在连接直播间 #{} 弹幕服务器... (Enter 发送弹幕，Ctrl+T 开关进房提示，Esc 退出)",
                ctx.room_id
            ),
        },
    });

    let mut room_title = ctx.title;
    let mut area_name = ctx.area_name;
    let anchor_name = ctx.anchor_name;
    let mut is_live = ctx.is_live;
    let mut online = ctx.online;
    let mut watched = String::from("-");
    let mut likes: u64 = 0;
    let mut popularity: u32 = 0;
    let mut ws_connected = false;

    let can_send = cookies
        .as_ref()
        .is_some_and(|c| !c.sessdata.is_empty() && !c.bili_jct.is_empty());

    let mut input_chars: Vec<char> = Vec::new();
    let mut cursor_idx: usize = 0;
    let mut scroll_offset: usize = 0; // 距底部的视觉行数偏移
    let mut unread_below: usize = 0;
    let mut pending_self_echo: Option<(String, Instant)> = None;
    let mut dirty = true;

    loop {
        if crate::utils::interrupt::is_interrupted() {
            stop_flag.store(true, Ordering::Relaxed);
            return Err(BiliLiveError::Interrupted);
        }

        // 1. 消费所有待处理的后台消息
        while let Ok(msg) = ui_rx.try_recv() {
            dirty = true;
            match msg {
                UiMsg::Live(ev) => match ev {
                    LiveEvent::Connected { .. } => {
                        ws_connected = true;
                        push_feed(
                            &mut feed,
                            FeedItem {
                                time: now_hhmmss(),
                                kind: FeedItemKind::System {
                                    is_error: false,
                                    message: "实时弹幕流已连接".to_string(),
                                },
                            },
                            scroll_offset > 0,
                            &mut unread_below,
                        );
                    }
                    LiveEvent::Disconnected { reason } => {
                        ws_connected = false;
                        push_feed(
                            &mut feed,
                            FeedItem {
                                time: now_hhmmss(),
                                kind: FeedItemKind::System {
                                    is_error: true,
                                    message: reason,
                                },
                            },
                            scroll_offset > 0,
                            &mut unread_below,
                        );
                    }
                    LiveEvent::Danmaku {
                        time,
                        uname,
                        text,
                        medal,
                        guard_level,
                        is_admin,
                        color_rgb,
                        is_self,
                        ..
                    } => {
                        // 若刚刚本地回显过自己发的同内容弹幕（3 秒内），跳过重复服务端回显
                        if is_self
                            && let Some((ref last_txt, sent_at)) = pending_self_echo
                            && last_txt == &text
                            && sent_at.elapsed() < Duration::from_secs(4)
                        {
                            pending_self_echo = None;
                            continue;
                        }
                        push_feed(
                            &mut feed,
                            FeedItem {
                                time,
                                kind: FeedItemKind::Danmaku {
                                    uname,
                                    text,
                                    medal,
                                    guard_level,
                                    is_admin,
                                    is_self,
                                    color_rgb,
                                },
                            },
                            scroll_offset > 0,
                            &mut unread_below,
                        );
                    }
                    LiveEvent::SuperChat {
                        time,
                        uname,
                        price,
                        text,
                    } => {
                        push_feed(
                            &mut feed,
                            FeedItem {
                                time,
                                kind: FeedItemKind::SuperChat { uname, price, text },
                            },
                            scroll_offset > 0,
                            &mut unread_below,
                        );
                    }
                    LiveEvent::Gift {
                        time,
                        uname,
                        action,
                        gift_name,
                        num,
                    } => {
                        push_feed(
                            &mut feed,
                            FeedItem {
                                time,
                                kind: FeedItemKind::Gift {
                                    uname,
                                    action,
                                    gift_name,
                                    num,
                                },
                            },
                            scroll_offset > 0,
                            &mut unread_below,
                        );
                    }
                    LiveEvent::GuardBuy {
                        time,
                        uname,
                        guard_name,
                        num,
                    } => {
                        push_feed(
                            &mut feed,
                            FeedItem {
                                time,
                                kind: FeedItemKind::GuardBuy {
                                    uname,
                                    guard_name,
                                    num,
                                },
                            },
                            scroll_offset > 0,
                            &mut unread_below,
                        );
                    }
                    LiveEvent::Interact {
                        time,
                        uname,
                        kind,
                        medal,
                    } => {
                        let count_unread =
                            scroll_offset > 0 && (show_enter || kind != InteractKind::Enter);
                        push_feed(
                            &mut feed,
                            FeedItem {
                                time,
                                kind: FeedItemKind::Interact { uname, kind, medal },
                            },
                            count_unread,
                            &mut unread_below,
                        );
                    }
                    LiveEvent::RoomStats {
                        online: o,
                        watched: w,
                        likes: l,
                        popularity: p,
                    } => {
                        if let Some(v) = o {
                            online = v;
                        }
                        if let Some(v) = w {
                            watched = v;
                        }
                        if let Some(v) = l {
                            likes = v;
                        }
                        if let Some(v) = p {
                            popularity = v;
                        }
                    }
                    LiveEvent::RoomMeta {
                        title,
                        area_name: area,
                        is_live: live_flag,
                    } => {
                        if let Some(t) = title {
                            room_title = t;
                        }
                        if let Some(a) = area {
                            area_name = a;
                        }
                        if let Some(flag) = live_flag {
                            is_live = flag;
                        }
                    }
                },
                UiMsg::SendDone { text } => {
                    pending_self_echo = Some((text.clone(), Instant::now()));
                    push_feed(
                        &mut feed,
                        FeedItem {
                            time: now_hhmmss(),
                            kind: FeedItemKind::Danmaku {
                                uname: "我".to_string(),
                                text,
                                medal: None,
                                guard_level: 0,
                                is_admin: false,
                                is_self: true,
                                color_rgb: 0xFFFFFF,
                            },
                        },
                        false,
                        &mut unread_below,
                    );
                    scroll_offset = 0;
                    unread_below = 0;
                }
                UiMsg::SendErr { err } => {
                    push_feed(
                        &mut feed,
                        FeedItem {
                            time: now_hhmmss(),
                            kind: FeedItemKind::System {
                                is_error: true,
                                message: format!("弹幕发送失败: {err}"),
                            },
                        },
                        false,
                        &mut unread_below,
                    );
                }
            }
        }

        // 2. 按需重绘整个界面
        if dirty {
            let state = RenderState {
                room_id: ctx.room_id,
                room_title: &room_title,
                area_name: &area_name,
                anchor_name: &anchor_name,
                is_live,
                online,
                watched: &watched,
                likes,
                popularity,
                ws_connected,
                show_enter,
                can_send,
                feed: &feed,
                input_chars: &input_chars,
                cursor_idx,
                scroll_offset: &mut scroll_offset,
                unread_below,
            };
            render_tui_frame(state)?;
            dirty = false;
        }

        // 3. 非阻塞轮询键盘与窗口缩放事件 (40ms 响应周期)
        if event::poll(Duration::from_millis(40)).map_err(BiliLiveError::Io)? {
            match event::read().map_err(BiliLiveError::Io)? {
                Event::Resize(_, _) => {
                    dirty = true;
                }
                Event::Key(KeyEvent {
                    code,
                    modifiers,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    dirty = true;
                    match (code, modifiers) {
                        (KeyCode::Esc, _) => {
                            if !input_chars.is_empty() {
                                input_chars.clear();
                                cursor_idx = 0;
                            } else {
                                break;
                            }
                        }
                        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => break,
                        (KeyCode::Char('d'), m)
                            if m.contains(KeyModifiers::CONTROL) && input_chars.is_empty() =>
                        {
                            break;
                        }
                        (KeyCode::Char('t'), m) if m.contains(KeyModifiers::CONTROL) => {
                            show_enter = !show_enter;
                        }
                        (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                            feed.clear();
                            scroll_offset = 0;
                            unread_below = 0;
                        }
                        (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                            cursor_idx = input_chars.len();
                            if scroll_offset > 0 && input_chars.is_empty() {
                                scroll_offset = 0;
                                unread_below = 0;
                            }
                        }
                        (KeyCode::Up, _) => {
                            scroll_offset = scroll_offset.saturating_add(1);
                        }
                        (KeyCode::Down, _) => {
                            scroll_offset = scroll_offset.saturating_sub(1);
                            if scroll_offset == 0 {
                                unread_below = 0;
                            }
                        }
                        (KeyCode::PageUp, _) => {
                            scroll_offset = scroll_offset.saturating_add(10);
                        }
                        (KeyCode::PageDown, _) => {
                            scroll_offset = scroll_offset.saturating_sub(10);
                            if scroll_offset == 0 {
                                unread_below = 0;
                            }
                        }
                        (KeyCode::End, m) if m.contains(KeyModifiers::CONTROL) => {
                            scroll_offset = 0;
                            unread_below = 0;
                        }
                        (KeyCode::Left, _) => {
                            cursor_idx = cursor_idx.saturating_sub(1);
                        }
                        (KeyCode::Right, _) => {
                            if cursor_idx < input_chars.len() {
                                cursor_idx += 1;
                            }
                        }
                        (KeyCode::Home, _) => {
                            cursor_idx = 0;
                        }
                        (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
                            cursor_idx = 0;
                        }
                        (KeyCode::End, _) => {
                            cursor_idx = input_chars.len();
                            if scroll_offset > 0 && input_chars.is_empty() {
                                scroll_offset = 0;
                                unread_below = 0;
                            }
                        }
                        (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                            input_chars.drain(..cursor_idx);
                            cursor_idx = 0;
                        }
                        (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) => {
                            input_chars.truncate(cursor_idx);
                        }
                        (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
                            while cursor_idx > 0 && input_chars[cursor_idx - 1].is_whitespace() {
                                cursor_idx -= 1;
                                input_chars.remove(cursor_idx);
                            }
                            while cursor_idx > 0 && !input_chars[cursor_idx - 1].is_whitespace() {
                                cursor_idx -= 1;
                                input_chars.remove(cursor_idx);
                            }
                        }
                        (KeyCode::Backspace, _) => {
                            if cursor_idx > 0 {
                                cursor_idx -= 1;
                                input_chars.remove(cursor_idx);
                            }
                        }
                        (KeyCode::Delete, _) => {
                            if cursor_idx < input_chars.len() {
                                input_chars.remove(cursor_idx);
                            }
                        }
                        (KeyCode::Enter, _) => {
                            let raw: String = input_chars.iter().collect();
                            let trimmed = raw.trim().to_string();
                            input_chars.clear();
                            cursor_idx = 0;

                            let msg_to_send = match parse_chat_submission(&trimmed) {
                                ChatSubmission::Empty => {
                                    scroll_offset = 0;
                                    unread_below = 0;
                                    continue;
                                }
                                ChatSubmission::Quit => break,
                                ChatSubmission::Clear => {
                                    feed.clear();
                                    scroll_offset = 0;
                                    unread_below = 0;
                                    continue;
                                }
                                ChatSubmission::ToggleEnter => {
                                    show_enter = !show_enter;
                                    push_feed(
                                        &mut feed,
                                        FeedItem {
                                            time: now_hhmmss(),
                                            kind: FeedItemKind::System {
                                                is_error: false,
                                                message: format!(
                                                    "进房欢迎提示已{}",
                                                    if show_enter { "开启" } else { "隐藏" }
                                                ),
                                            },
                                        },
                                        false,
                                        &mut unread_below,
                                    );
                                    continue;
                                }
                                ChatSubmission::Help => {
                                    push_feed(
                                        &mut feed,
                                        FeedItem {
                                            time: now_hhmmss(),
                                            kind: FeedItemKind::System {
                                                is_error: false,
                                                message: "指令: /help 帮助 · /enter 进房提示开关 · /clear 清屏 · /quit 退出 · // 开头可发送斜杠弹幕 | 快捷键: Ctrl+T 进房提示 · Ctrl+L 清屏 · Ctrl+A/E/U/K/W 行编辑 · Esc 清空输入/退出".to_string(),
                                            },
                                        },
                                        false,
                                        &mut unread_below,
                                    );
                                    scroll_offset = 0;
                                    unread_below = 0;
                                    continue;
                                }
                                ChatSubmission::UnknownCommand(cmd) => {
                                    push_feed(
                                        &mut feed,
                                        FeedItem {
                                            time: now_hhmmss(),
                                            kind: FeedItemKind::System {
                                                is_error: true,
                                                message: format!(
                                                    "未知本地指令 `{cmd}`（已拦截，未作为弹幕发送）；输入 /help 查看可用指令，若需发送 `/` 开头弹幕请输入 `//`"
                                                ),
                                            },
                                        },
                                        false,
                                        &mut unread_below,
                                    );
                                    scroll_offset = 0;
                                    unread_below = 0;
                                    continue;
                                }
                                ChatSubmission::SendDanmaku(msg) => msg,
                            };

                            let char_cnt = msg_to_send.chars().count();
                            if char_cnt > DANMAKU_CHAR_LIMIT {
                                push_feed(
                                    &mut feed,
                                    FeedItem {
                                        time: now_hhmmss(),
                                        kind: FeedItemKind::System {
                                            is_error: true,
                                            message: format!(
                                                "弹幕字数超限 ({char_cnt}/{DANMAKU_CHAR_LIMIT} 字)，请精简后再发"
                                            ),
                                        },
                                    },
                                    false,
                                    &mut unread_below,
                                );
                                // 保留原输入供用户删减
                                input_chars = msg_to_send.chars().collect();
                                cursor_idx = input_chars.len();
                                continue;
                            }

                            let Some(ref ck) = cookies else {
                                push_feed(
                                    &mut feed,
                                    FeedItem {
                                        time: now_hhmmss(),
                                        kind: FeedItemKind::System {
                                            is_error: true,
                                            message: "当前未登录，无法发送弹幕；请退出后执行 `bt auth login`"
                                                .to_string(),
                                        },
                                    },
                                    false,
                                    &mut unread_below,
                                );
                                continue;
                            };
                            if ck.sessdata.is_empty() || ck.bili_jct.is_empty() {
                                push_feed(
                                    &mut feed,
                                    FeedItem {
                                        time: now_hhmmss(),
                                        kind: FeedItemKind::System {
                                            is_error: true,
                                            message: "当前凭证缺少 CSRF (bili_jct)，请执行 `bt auth login` 重新登录"
                                                .to_string(),
                                        },
                                    },
                                    false,
                                    &mut unread_below,
                                );
                                continue;
                            }

                            let ck_clone = ck.clone();
                            let tx_done = ui_tx.clone();
                            let target_room = ctx.room_id;
                            std::thread::spawn(move || {
                                match crate::api::live::send_live_danmaku(
                                    &ck_clone,
                                    target_room,
                                    &msg_to_send,
                                ) {
                                    Ok(_) => {
                                        let _ = tx_done.send(UiMsg::SendDone { text: msg_to_send });
                                    }
                                    Err(e) => {
                                        let _ = tx_done.send(UiMsg::SendErr { err: e.to_string() });
                                    }
                                }
                            });
                        }
                        (KeyCode::Char(c), m)
                            if !m.contains(KeyModifiers::CONTROL)
                                && !m.contains(KeyModifiers::ALT) =>
                        {
                            input_chars.insert(cursor_idx, c);
                            cursor_idx += 1;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    stop_flag.store(true, Ordering::Relaxed);
    Ok(())
}

fn push_feed(
    feed: &mut VecDeque<FeedItem>,
    item: FeedItem,
    is_scrolled: bool,
    unread_below: &mut usize,
) {
    if feed.len() >= MAX_CHAT_ITEMS {
        feed.pop_front();
    }
    feed.push_back(item);
    if is_scrolled {
        *unread_below = unread_below.saturating_add(1);
    }
}

struct RenderState<'a> {
    room_id: i64,
    room_title: &'a str,
    area_name: &'a str,
    anchor_name: &'a str,
    is_live: bool,
    online: u64,
    watched: &'a str,
    likes: u64,
    popularity: u32,
    ws_connected: bool,
    show_enter: bool,
    can_send: bool,
    feed: &'a VecDeque<FeedItem>,
    input_chars: &'a [char],
    cursor_idx: usize,
    scroll_offset: &'a mut usize,
    unread_below: usize,
}

fn render_tui_frame(state: RenderState<'_>) -> Result<()> {
    let (cols_u16, rows_u16) = crossterm::terminal::size().unwrap_or((88, 26));
    let cols = (cols_u16 as usize).max(40);
    let rows = (rows_u16 as usize).max(10);
    let use_color = color_enabled();

    let mut stdout = std::io::stdout();
    queue!(stdout, Hide, MoveTo(0, 0)).map_err(BiliLiveError::Io)?;

    // ── 顶部第 0 行：房间标题栏 ──
    let live_badge = if state.is_live {
        "● 直播中"
    } else {
        "○ 未开播"
    };
    let anchor_part = if state.anchor_name.is_empty() {
        String::new()
    } else {
        format!(" · 主播: {}", state.anchor_name)
    };
    let area_part = if state.area_name.is_empty() {
        String::new()
    } else {
        format!(" [{}]", state.area_name)
    };
    let header_plain = format!(
        " 📺 #{} {} · {}{}{}",
        state.room_id, live_badge, state.room_title, area_part, anchor_part
    );
    let header_trunc = truncate_to_width(&header_plain, cols.saturating_sub(1));
    if use_color {
        let live_styled = if state.is_live {
            live_badge.green().bold().to_string()
        } else {
            live_badge.dark_grey().to_string()
        };
        let rest_budget = cols
            .saturating_sub(display_width(&format!(" 📺 #{} {} · ", state.room_id, live_badge)) + 1);
        let title_and_meta = truncate_to_width(
            &format!("{}{}{}", state.room_title, area_part, anchor_part),
            rest_budget,
        );
        write!(
            stdout,
            " {} {} {} {} {}\r\n",
            "📺".with(BILI_PINK),
            format!("#{}", state.room_id).with(BILI_CYAN).bold(),
            live_styled,
            "·".dark_grey(),
            title_and_meta.bold()
        )
        .ok();
    } else {
        write!(stdout, "{}\x1b[K\r\n", header_trunc).ok();
    }

    // ── 顶部第 1 行：实时指标状态栏 ──
    let conn_txt = if state.ws_connected {
        "● 已连接"
    } else {
        "◌ 连接中"
    };
    let enter_txt = if state.show_enter { "开" } else { "关" };
    let stats_plain = format!(
        " 🔥 在线 {}  ·  👀 看过 {}  ·  👍 点赞 {}  ·  人气 {}  ·  {}  ·  进房提示: {}(Ctrl+T)",
        state.online, state.watched, state.likes, state.popularity, conn_txt, enter_txt
    );
    let stats_trunc = truncate_to_width(&stats_plain, cols.saturating_sub(1));
    if use_color {
        write!(stdout, "\x1b[K{}\r\n", stats_trunc.as_str().dark_grey()).ok();
    } else {
        write!(stdout, "\x1b[K{}\r\n", stats_trunc).ok();
    }

    // ── 顶部第 2 行：分隔线 ──
    let sep = "─".repeat(cols);
    if use_color {
        write!(stdout, "{}\r\n", sep.as_str().dark_grey()).ok();
    } else {
        write!(stdout, "{sep}\r\n").ok();
    }

    // 布局高度：顶部 3 行 + 底部 4 行（输入框顶边 + 输入行 + 输入框底边 + 快捷键提示）
    let footer_rows = 4usize;
    let header_rows = 3usize;
    let chat_height = rows.saturating_sub(header_rows + footer_rows).max(2);

    // ── 中部弹幕滚动视口 ──
    let mut visual_lines: Vec<String> = Vec::with_capacity(chat_height * 2);
    let content_width = cols.saturating_sub(2).max(20);
    for item in state.feed {
        if !state.show_enter && item.is_enter_event() {
            continue;
        }
        render_feed_item_lines(item, content_width, use_color, &mut visual_lines);
    }

    let max_scroll = visual_lines.len().saturating_sub(chat_height);
    if *state.scroll_offset > max_scroll {
        *state.scroll_offset = max_scroll;
    }

    let reserve_banner = if *state.scroll_offset > 0 { 1usize } else { 0usize };
    let visible_slots = chat_height.saturating_sub(reserve_banner);
    let end_idx = visual_lines.len().saturating_sub(*state.scroll_offset);
    let start_idx = end_idx.saturating_sub(visible_slots);
    let slice = &visual_lines[start_idx..end_idx];

    let empty_top = visible_slots.saturating_sub(slice.len());
    for _ in 0..empty_top {
        write!(stdout, "\x1b[K\r\n").ok();
    }
    for line in slice {
        write!(stdout, "\x1b[K {line}\r\n").ok();
    }
    if reserve_banner == 1 {
        let banner_txt = if state.unread_below > 0 {
            format!(
                " ↓ 历史回看中 (上移 {} 行 · 底部有 {} 条新消息，按 Ctrl+E 恢复实时滚动) ",
                *state.scroll_offset, state.unread_below
            )
        } else {
            format!(
                " ↓ 历史回看中 (上移 {} 行，按 Ctrl+E 回到底部) ",
                *state.scroll_offset
            )
        };
        let trunc = truncate_to_width(&banner_txt, cols.saturating_sub(2));
        if use_color {
            write!(stdout, "\x1b[K {}\r\n", trunc.with(BILI_PINK).bold()).ok();
        } else {
            write!(stdout, "\x1b[K {trunc}\r\n").ok();
        }
    }

    // ── 底部输入框 (3 行圆角框 + 1 行快捷键说明) ──
    let char_count = state.input_chars.len();
    let box_inner = cols.saturating_sub(4).max(20);
    let box_label = if state.can_send {
        format!("💬 发送弹幕 ({char_count}/{DANMAKU_CHAR_LIMIT}) ")
    } else {
        "🔒 游客只读模式 (登录后可发弹幕) ".to_string()
    };
    let top_dashes = box_inner.saturating_sub(display_width(&box_label) + 2);
    if use_color {
        let count_col = if char_count > DANMAKU_CHAR_LIMIT {
            Color::Red
        } else {
            BILI_PINK
        };
        write!(
            stdout,
            "\x1b[K {}{}{}\r\n",
            "╭─ ".dark_grey(),
            box_label.with(count_col).bold(),
            format!("{}╮", "─".repeat(top_dashes)).dark_grey()
        )
        .ok();
    } else {
        write!(
            stdout,
            "\x1b[K ╭─ {}{}╮\r\n",
            box_label,
            "─".repeat(top_dashes)
        )
        .ok();
    }

    // 计算输入框水平滚动窗口，确保光标始终在可见范围内
    let prompt_str = "❯ ";
    let text_slots = box_inner.saturating_sub(display_width(prompt_str) + 2).max(8);
    let (visible_input, cursor_col_offset) =
        slice_input_window(state.input_chars, state.cursor_idx, text_slots);

    let input_row_y = (rows.saturating_sub(3)) as u16;
    if state.input_chars.is_empty() {
        let placeholder = if state.can_send {
            "输入弹幕内容，按 Enter 直接发送到直播间..."
        } else {
            "当前未登录，仅可观看实时弹幕（执行 bt auth login 后可发言）"
        };
        let ph_padded = pad_to_width(&truncate_to_width(placeholder, text_slots), text_slots);
        if use_color {
            write!(
                stdout,
                "\x1b[K {} {}{} {}\r\n",
                "│".dark_grey(),
                prompt_str.with(BILI_CYAN).bold(),
                ph_padded.dark_grey(),
                "│".dark_grey()
            )
            .ok();
        } else {
            write!(stdout, "\x1b[K │ {prompt_str}{ph_padded} │\r\n").ok();
        }
    } else {
        let val_padded = pad_to_width(&visible_input, text_slots);
        if use_color {
            write!(
                stdout,
                "\x1b[K {} {}{} {}\r\n",
                "│".dark_grey(),
                prompt_str.with(BILI_CYAN).bold(),
                val_padded,
                "│".dark_grey()
            )
            .ok();
        } else {
            write!(stdout, "\x1b[K │ {prompt_str}{val_padded} │\r\n").ok();
        }
    }

    if use_color {
        write!(
            stdout,
            "\x1b[K {}\r\n",
            format!("╰{}╯", "─".repeat(box_inner)).dark_grey()
        )
        .ok();
    } else {
        write!(stdout, "\x1b[K ╰{}╯\r\n", "─".repeat(box_inner)).ok();
    }

    let help_bar = truncate_to_width(
        " Enter 发送  ·  /help 指令  ·  ↑/↓ 翻阅  ·  Ctrl+T 进房提示  ·  Ctrl+L 清屏  ·  Esc 清空/退出",
        cols.saturating_sub(1),
    );
    if use_color {
        write!(stdout, "\x1b[K{}", help_bar.as_str().dark_grey()).ok();
    } else {
        write!(stdout, "\x1b[K{help_bar}").ok();
    }

    // 将真实终端光标精确定位到输入框编辑处，完美适配中文输入法候选框跟随
    let cursor_x = (3 + display_width(prompt_str) + cursor_col_offset) as u16;
    queue!(stdout, MoveTo(cursor_x, input_row_y), Show).map_err(BiliLiveError::Io)?;
    stdout.flush().map_err(BiliLiveError::Io)?;
    Ok(())
}

fn slice_input_window(chars: &[char], cursor_idx: usize, max_cols: usize) -> (String, usize) {
    if chars.is_empty() {
        return (String::new(), 0);
    }
    let cursor_idx = cursor_idx.min(chars.len());
    let mut start = 0usize;

    // 保证 start..cursor_idx 的显示列宽不超过 max_cols
    loop {
        let w: usize = chars[start..cursor_idx]
            .iter()
            .map(|c| unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        if w <= max_cols.saturating_sub(2) || start >= cursor_idx {
            break;
        }
        start += 1;
    }

    let mut out = String::new();
    let mut used = 0usize;
    let mut cursor_col = 0usize;
    for (i, &ch) in chars[start..].iter().enumerate() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw > max_cols {
            break;
        }
        if start + i == cursor_idx {
            cursor_col = used;
        }
        out.push(ch);
        used += cw;
    }
    if cursor_idx >= start + out.chars().count() {
        cursor_col = used;
    }
    (out, cursor_col)
}

fn build_plain_badge(
    guard_level: u8,
    is_admin: bool,
    medal: Option<&(String, u32)>,
) -> String {
    let mut parts = Vec::new();
    if is_admin {
        parts.push("[房]".to_string());
    }
    match guard_level {
        1 => parts.push("[总督]".to_string()),
        2 => parts.push("[提督]".to_string()),
        3 => parts.push("[舰长]".to_string()),
        _ => {}
    }
    if let Some((name, lv)) = medal {
        parts.push(format!("[{name} {lv}]"));
    }
    parts.join("")
}

fn render_feed_item_lines(
    item: &FeedItem,
    max_width: usize,
    use_color: bool,
    out: &mut Vec<String>,
) {
    match &item.kind {
        FeedItemKind::Danmaku {
            uname,
            text,
            medal,
            guard_level,
            is_admin,
            is_self,
            color_rgb,
        } => {
            let mut prefix_plain = format!("{} ", item.time);
            let badge = build_plain_badge(*guard_level, *is_admin, medal.as_ref());
            if *is_self {
                prefix_plain.push_str("[我] ");
            } else if !badge.is_empty() {
                prefix_plain.push_str(&badge);
                prefix_plain.push(' ');
            }
            prefix_plain.push_str(uname);
            prefix_plain.push_str(": ");

            let prefix_w = display_width(&prefix_plain);
            let body_w = max_width.saturating_sub(prefix_w).max(12);
            let wrapped = wrap_to_width(text, body_w);

            let prefix_styled = if use_color {
                let mut s = format!("{} ", item.time.as_str().dark_grey());
                if *is_self {
                    s.push_str(&format!("{} ", "[我]".with(BILI_PINK).bold()));
                } else if !badge.is_empty() {
                    s.push_str(&format!("{} ", badge.as_str().with(BILI_CYAN)));
                }
                let name_styled = if *is_self {
                    format!("{uname}: ").with(BILI_PINK).bold().to_string()
                } else {
                    format!("{uname}: ").bold().to_string()
                };
                s.push_str(&name_styled);
                s
            } else {
                prefix_plain
            };

            let indent = " ".repeat(prefix_w.min(max_width / 2));
            for (idx, seg) in wrapped.into_iter().enumerate() {
                let seg_styled = if use_color && *color_rgb != 0xFFFFFF && *color_rgb != 0 {
                    let r = ((*color_rgb >> 16) & 0xFF) as u8;
                    let g = ((*color_rgb >> 8) & 0xFF) as u8;
                    let b = (*color_rgb & 0xFF) as u8;
                    seg.with(Color::Rgb { r, g, b }).to_string()
                } else {
                    seg
                };
                if idx == 0 {
                    out.push(format!("{prefix_styled}{seg_styled}"));
                } else {
                    out.push(format!("{indent}{seg_styled}"));
                }
            }
        }
        FeedItemKind::SuperChat { uname, price, text } => {
            let line = format!("{} 💎 [SC ¥{price}] {uname}: {text}", item.time);
            for seg in wrap_to_width(&line, max_width) {
                if use_color {
                    out.push(seg.with(BILI_PINK).bold().to_string());
                } else {
                    out.push(seg);
                }
            }
        }
        FeedItemKind::Gift {
            uname,
            action,
            gift_name,
            num,
        } => {
            let line = format!("{} 🎁 {uname} {action} {gift_name} × {num}", item.time);
            let trunc = truncate_to_width(&line, max_width);
            if use_color {
                out.push(trunc.yellow().to_string());
            } else {
                out.push(trunc);
            }
        }
        FeedItemKind::GuardBuy {
            uname,
            guard_name,
            num,
        } => {
            let line = format!("{} ⚓ {uname} 开通了 {guard_name} × {num}", item.time);
            let trunc = truncate_to_width(&line, max_width);
            if use_color {
                out.push(trunc.with(BILI_PINK).bold().to_string());
            } else {
                out.push(trunc);
            }
        }
        FeedItemKind::Interact { uname, kind, medal } => {
            let medal_str = medal
                .as_ref()
                .map(|(n, lv)| format!("[{n} {lv}] "))
                .unwrap_or_default();
            let line = format!("{} · {medal_str}{uname} {}", item.time, kind.label());
            let trunc = truncate_to_width(&line, max_width);
            if use_color {
                out.push(trunc.dark_grey().to_string());
            } else {
                out.push(trunc);
            }
        }
        FeedItemKind::System { is_error, message } => {
            let icon = if *is_error { "⚠" } else { "ℹ" };
            let line = format!("{} {icon} {message}", item.time);
            for seg in wrap_to_width(&line, max_width) {
                if use_color {
                    if *is_error {
                        out.push(seg.yellow().to_string());
                    } else {
                        out.push(seg.with(BILI_CYAN).to_string());
                    }
                } else {
                    out.push(seg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_window_tracks_cjk_cursor_cleanly() {
        let chars: Vec<char> = "这是一条非常长的中文弹幕输入测试文本".chars().collect();
        let (vis, col) = slice_input_window(&chars, chars.len(), 16);
        assert!(display_width(&vis) <= 16);
        assert!(col <= 16);
    }

    #[test]
    fn slash_commands_never_leak_as_public_danmaku() {
        assert_eq!(parse_chat_submission(""), ChatSubmission::Empty);
        assert_eq!(parse_chat_submission("/quit"), ChatSubmission::Quit);
        assert_eq!(parse_chat_submission("/clear"), ChatSubmission::Clear);
        assert_eq!(parse_chat_submission("/cls"), ChatSubmission::Clear);
        assert_eq!(parse_chat_submission("/enter"), ChatSubmission::ToggleEnter);
        assert_eq!(parse_chat_submission("/help"), ChatSubmission::Help);
        assert_eq!(parse_chat_submission("/?"), ChatSubmission::Help);
        assert_eq!(
            parse_chat_submission("/clea"),
            ChatSubmission::UnknownCommand("/clea".to_string())
        );
        assert_eq!(
            parse_chat_submission("//2333"),
            ChatSubmission::SendDanmaku("/2333".to_string())
        );
        assert_eq!(
            parse_chat_submission("主播好"),
            ChatSubmission::SendDanmaku("主播好".to_string())
        );
    }
}
