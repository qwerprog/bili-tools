use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::read_cookies;
use crate::error::{BiliLiveError, Result};
use crossterm::style::Stylize;
use unicode_width::UnicodeWidthStr;

// 通过 live_key 获取直播统计信息（下播后调用）
pub fn get_live_info(live_id: u64) -> Result<()> {
    let cookies = read_cookies()?;
    let url = format!(
        "https://api.live.bilibili.com/xlive/app-blink/v1/live/StopLiveData?live_key={}",
        live_id
    );

    let response = minreq::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/json, text/plain, */*")
        .with_header("Cookie", format!("SESSDATA={}", cookies.sessdata))
        .send()?;

    let response_text = response.as_str()?;
    let res: serde_json::Value = serde_json::from_str(response_text)?;

    if res["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "API返回错误: {}",
            res["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let data = &res["data"];

    println!("  {:>6}", "统计".dark_grey());

    let stats: &[(&str, String)] = &[
        ("新增粉丝", data["AddFans"].as_i64().unwrap_or(0).to_string()),
        ("弹幕数量", data["DanmuNum"].as_i64().unwrap_or(0).to_string()),
        ("直播时长", format_duration(data["LiveTime"].as_i64().unwrap_or(0))),
        ("最大在线", data["MaxOnline"].as_i64().unwrap_or(0).to_string()),
        ("累计观看", data["WatchedCount"].as_i64().unwrap_or(0).to_string()),
        ("粉丝勋章", data["NewFansClub"].as_i64().unwrap_or(0).to_string()),
        ("金仓鼠", data["HamsterRmb"].as_i64().unwrap_or(0).to_string()),
    ];
    // 计算最长 key 的终端显示宽度，按宽度补齐空格实现对齐
    let max_width = stats
        .iter()
        .map(|(k, _)| UnicodeWidthStr::width(*k))
        .max()
        .unwrap_or(8);
    for (key, val) in stats {
        let pad = max_width - UnicodeWidthStr::width(*key);
        println!("  {}{}  {}", key.dark_grey(), " ".repeat(pad), val);
    }

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
