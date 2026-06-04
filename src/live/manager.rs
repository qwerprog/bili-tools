use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::{read_cookies, update_live_key};
use crate::error::{BiliLiveError, Result};
use crate::live::stats::get_live_info;
use crate::utils::paths::data_file;
use crate::utils::string::mask_rtmp_code;
use std::fs;
use std::io::Write;

// 调用 B站 API 开始直播，获取推流地址和推流码
pub fn start_live(area_id: &str, show_full_code: bool) -> Result<()> {
    let cookies = read_cookies()?;

    let form_data = format!(
        "room_id={}&area_v2={}&csrf={}&platform=pc_link",
        cookies.room_id, area_id, cookies.csrf_token
    );

    let response = minreq::post("https://api.live.bilibili.com/room/v1/Room/startLive")
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
    let rtmp_addr = res["data"]["rtmp"]["addr"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("缺少rtmp地址".to_string()))?;
    let rtmp_code = res["data"]["rtmp"]["code"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("缺少rtmp code".to_string()))?;
    let live_key = res["data"]["live_key"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("缺少live_key".to_string()))?;

    update_live_key(
        live_key
            .parse::<u64>()
            .map_err(|e| BiliLiveError::Parse(format!("live_key转换失败: {}", e)))?,
    )?;

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

    let path = data_file("stream_info.txt");
    let mut file = fs::File::create(&path)?;
    writeln!(file, "{}", rtmp_addr)?;
    writeln!(file, "{}", rtmp_code)?;
    println!(
        "  {:>8}  {}",
        "·".dark_grey(),
        format!("推流信息已写入 {}", path.display()).dark_grey()
    );

    Ok(())
}

// 调用 B站 API 停止直播
pub fn stop_live() -> Result<()> {
    let cookies = read_cookies()?;

    let form_data = format!(
        "room_id={}&csrf={}&platform=web_electron_link",
        cookies.room_id, cookies.csrf_token
    );

    let response = minreq::post("https://api.live.bilibili.com/room/v1/Room/stopLive")
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

    if let Some(live_key) = cookies.live_key {
        get_live_info(live_key)?;
    }

    Ok(())
}
