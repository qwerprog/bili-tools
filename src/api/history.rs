use crate::api::client::DEFAULT_USER_AGENT;
use crate::api::video::parse_video_param;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};

/// 获取历史记录列表
pub fn get_history(
    cookies: &Cookies,
    ps: u32,
    hist_type: Option<&str>,
) -> Result<serde_json::Value> {
    let ps_val = ps.clamp(1, 30);
    let t_val = hist_type.unwrap_or("all");

    let url = format!(
        "https://api.bilibili.com/x/web-interface/history/cursor?ps={ps_val}&type={t_val}"
    );

    let resp = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取历史记录失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"]["list"].clone())
}

/// 获取稍后再看列表
pub fn get_toview_list(cookies: &Cookies) -> Result<serde_json::Value> {
    let url = "https://api.bilibili.com/x/v2/history/toview";
    let resp = crate::api::client::get(url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取稍后再看列表失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

/// 添加视频到稍后再看
pub fn add_toview(cookies: &Cookies, id: &str) -> Result<()> {
    let (bvid, aid) = parse_video_param(id);
    let mut params = vec![("csrf", cookies.bili_jct.clone())];

    match (bvid, aid) {
        (Some(b), _) => params.push(("bvid", b)),
        (_, Some(a)) => params.push(("aid", a.to_string())),
        _ => return Err(BiliLiveError::Input(format!("无效的视频ID: {id}"))),
    }

    let body = serde_urlencoded::to_string(&params)
        .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let resp = crate::api::client::post("https://api.bilibili.com/x/v2/history/toview/add")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "添加稍后再看失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(())
}

/// 清空稍后再看列表（支持全量清空或仅清空已观看）
pub fn clear_toview(cookies: &Cookies, viewed_only: bool) -> Result<()> {
    if viewed_only {
        let body = format!("viewed=true&csrf={}", cookies.bili_jct);
        let resp = crate::api::client::post("https://api.bilibili.com/x/v2/history/toview/del")
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_header("Referer", "https://www.bilibili.com/")
            .with_header("Cookie", cookies.cookie_header())
            .with_body(body)
            .send()?;

        let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
        if json["code"].as_i64() != Some(0) {
            return Err(BiliLiveError::Api(format!(
                "清理已观看记录失败: {}",
                json["message"].as_str().unwrap_or("未知错误")
            )));
        }
    } else {
        let body = format!("csrf={}", cookies.bili_jct);
        let resp = crate::api::client::post("https://api.bilibili.com/x/v2/history/toview/clear")
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_header("Referer", "https://www.bilibili.com/")
            .with_header("Cookie", cookies.cookie_header())
            .with_body(body)
            .send()?;

        let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
        if json["code"].as_i64() != Some(0) {
            return Err(BiliLiveError::Api(format!(
                "清空稍后再看列表失败: {}",
                json["message"].as_str().unwrap_or("未知错误")
            )));
        }
    }

    Ok(())
}
