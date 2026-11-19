use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::read_cookies;
use crate::cli::output::{is_json, is_quiet};
use crate::error::{BiliLiveError, Result};
use crate::ui::components::{Cell, InfoCard};

/// 拉取本场直播统计原始 JSON 数据（静默模式下跳过请求）
pub fn fetch_live_stats_data(live_id: u64) -> Result<Option<serde_json::Value>> {
    if is_quiet() {
        return Ok(None);
    }

    let cookies = read_cookies()?;
    let url = format!(
        "{}/xlive/app-blink/v1/live/StopLiveData?live_key={}",
        crate::api::client::live_api_base_url(),
        live_id
    );

    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/json, text/plain, */*")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let res: serde_json::Value =
        crate::api::client::parse_json_response(&response, "获取直播统计失败")?;

    if res["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "API返回错误: {}",
            res["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(Some(res["data"].clone()))
}

// 通过 live_key 获取并打印直播统计信息（下播后或查看状态时调用）
pub fn get_live_info(live_id: u64) -> Result<()> {
    if is_json() || is_quiet() {
        return Ok(());
    }

    let Some(data) = fetch_live_stats_data(live_id)? else {
        return Ok(());
    };

    let add_fans = data["AddFans"].as_i64().unwrap_or(0);
    let fans_cell = if add_fans > 0 {
        Cell::pink(format!("+{add_fans}"))
    } else {
        Cell::plain(add_fans.to_string())
    };

    let card = InfoCard::new("📊 本场直播统计")
        .add_row(
            "直播时长",
            Cell::cyan(format_duration(data["LiveTime"].as_i64().unwrap_or(0))),
        )
        .add_plain_row(
            "累计观看",
            data["WatchedCount"].as_i64().unwrap_or(0).to_string(),
        )
        .add_plain_row(
            "最大在线",
            data["MaxOnline"].as_i64().unwrap_or(0).to_string(),
        )
        .add_plain_row(
            "弹幕数量",
            data["DanmuNum"].as_i64().unwrap_or(0).to_string(),
        )
        .add_row("新增粉丝", fans_cell)
        .add_plain_row(
            "粉丝勋章",
            data["NewFansClub"].as_i64().unwrap_or(0).to_string(),
        )
        .add_plain_row(
            "金仓鼠",
            data["HamsterRmb"].as_i64().unwrap_or(0).to_string(),
        );

    card.print();
    Ok(())
}

fn format_duration(seconds: i64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{}小时 {}分 {}秒", h, m, s)
    } else if m > 0 {
        format!("{}分 {}秒", m, s)
    } else {
        format!("{}秒", s)
    }
}
