use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::{read_cookies, update_live_key};
use crate::error::{BiliLiveError, Result};
use crate::live::stats::get_live_info;
use crate::user_warning;
use crate::utils::paths::{data_file, write_private};
use crate::utils::string::mask_rtmp_code;

// 调用 B站 API 开始直播，获取推流地址和推流码
pub fn start_live(area_id: &str, show_full_code: bool) -> Result<()> {
    let cookies = read_cookies()?;

    let form_data = format!(
        "room_id={}&area_v2={}&csrf={}&platform=pc_link",
        cookies.room_id, area_id, cookies.csrf_token
    );

    let response = crate::api::client::post("https://api.live.bilibili.com/room/v1/Room/startLive")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Cookie", format!("SESSDATA={}", cookies.sessdata))
        .with_header("platform", "web_electron_link")
        .with_body(form_data)
        .send()?;

    let response_text = response.as_str()?;
    let res: serde_json::Value = serde_json::from_str(response_text)?;

    if res["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "API返回错误: {}",
            res["message"].as_str().unwrap_or("未知错误")
        )));
    }

    // 解析 B站 返回的 RTMP 推流信息
    let rtmp_addr = res["data"]["rtmp"]["addr"].as_str().ok_or_else(|| {
        BiliLiveError::Parse("直播已开启，但响应缺少推流地址，请到直播中心获取".to_string())
    })?;
    let rtmp_code = res["data"]["rtmp"]["code"].as_str().ok_or_else(|| {
        BiliLiveError::Parse("直播已开启，但响应缺少推流码，请到直播中心获取".to_string())
    })?;
    use crossterm::style::Stylize;
    println!();
    println!("{}", "🎬 直播已开启".green());
    println!("  {:>8}  {}", "推流地址".dark_grey(), rtmp_addr);
    if show_full_code {
        println!("  {:>8}  {}", "推流码".dark_grey(), rtmp_code);
    } else {
        println!(
            "  {:>8}  {}",
            "推流码".dark_grey(),
            mask_rtmp_code(rtmp_code)
        );
    }

    let save_stream = || -> Result<()> {
        let path = data_file("stream_info.txt")?;
        write_private(&path, format!("{rtmp_addr}\n{rtmp_code}\n").as_bytes())?;
        println!(
            "  {:>8}  {}",
            "·".dark_grey(),
            format!("推流信息已写入 {}", path.display()).dark_grey()
        );
        Ok(())
    };
    if let Err(e) = save_stream() {
        user_warning!("直播已开启，但推流信息保存失败: {}", e);
        if !show_full_code {
            user_warning!("完整推流码未显示，请到直播中心获取推流信息");
        }
    }

    let live_key = parse_live_key(&res["data"]["live_key"]);
    if live_key.is_none() {
        user_warning!("直播已开启，但未取得有效统计标识，本次统计不可用");
    }
    // 缺失时清空旧标识，避免显示上一次直播的数据。
    if let Err(e) = update_live_key(live_key) {
        user_warning!("直播已开启，但统计标识保存失败: {}", e);
    }

    Ok(())
}

// 调用 B站 API 停止直播
pub fn stop_live() -> Result<()> {
    let cookies = read_cookies()?;

    let form_data = format!(
        "room_id={}&csrf={}&platform=web_electron_link",
        cookies.room_id, cookies.csrf_token
    );

    let response = crate::api::client::post("https://api.live.bilibili.com/room/v1/Room/stopLive")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Cookie", format!("SESSDATA={}", cookies.sessdata))
        .with_body(form_data)
        .send()?;

    let response_text = response.as_str()?;
    let res: serde_json::Value = serde_json::from_str(response_text)?;

    if res["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "API返回错误: {}",
            res["message"].as_str().unwrap_or("未知错误")
        )));
    }

    use crossterm::style::Stylize;
    println!("{}", "🎬 直播已关闭".green());

    if let Some(live_key) = cookies.live_key
        && let Err(e) = get_live_info(live_key)
    {
        user_warning!("直播已关闭，但统计获取失败: {}", e);
    }

    Ok(())
}

fn parse_live_key(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn live_key_accepts_string_or_number_and_rejects_invalid_values() {
        assert_eq!(parse_live_key(&json!("123")), Some(123));
        assert_eq!(parse_live_key(&json!(123)), Some(123));
        for value in [
            json!(null),
            json!(-1),
            json!("invalid"),
            json!("18446744073709551616"),
        ] {
            assert_eq!(parse_live_key(&value), None);
        }
    }
}
