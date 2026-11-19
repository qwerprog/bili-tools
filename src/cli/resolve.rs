use crate::cli::output::{is_json, is_quiet, print_json, print_spacer};
use crate::error::Result;
use crate::ui::components::{Cell, InfoCard};
use crate::ui::theme::BILI_CYAN;
use crossterm::style::Color;

pub fn handle_resolve(input: &str, no_network: bool) -> Result<()> {
    let resolved = crate::utils::id::parse_input(input, no_network)?;

    if is_json() {
        return print_json(&resolved);
    }

    if is_quiet() {
        return Ok(());
    }

    let mut card = InfoCard::new("🔎 标识解析")
        .with_badge(resolved.kind.display_label(), BILI_CYAN)
        .add_plain_row("输入", &resolved.input)
        .add_row("规范 ID", Cell::bold_colored(&resolved.id, Color::White));

    if let Some(ref bvid) = resolved.bvid {
        card = card.add_row("BV号", Cell::cyan(bvid));
    }
    if let Some(aid) = resolved.aid {
        card = card.add_plain_row("AV号", format!("av{aid}"));
    }
    if let Some(page) = resolved.page {
        card = card.add_plain_row("分P", format!("P{page}"));
    }
    if let Some(ep) = resolved.episode_id {
        card = card.add_plain_row("剧集 ID", format!("ep{ep}"));
    }
    if let Some(ss) = resolved.season_id {
        card = card.add_plain_row("季度 ID", format!("ss{ss}"));
    }
    if let Some(md) = resolved.media_id {
        card = card.add_plain_row("Media ID", format!("md{md}"));
    }
    if let Some(room) = resolved.room_id {
        card = card.add_row("直播间", Cell::cyan(room.to_string()));
    }
    if let Some(mid) = resolved.mid {
        card = card.add_row("UID", Cell::cyan(mid.to_string()));
    }
    if let Some(fav) = resolved.fav_id {
        card = card.add_row("收藏夹 ID", Cell::cyan(fav.to_string()));
    }
    if let Some(ref short_url) = resolved.short_url {
        card = card.add_row("短链接", Cell::muted(short_url));
    }
    if let Some(ref url) = resolved.resolved_url {
        card = card.add_row("目标链接", Cell::pink(url));
    }

    print_spacer();
    card.print();
    print_spacer();
    Ok(())
}
