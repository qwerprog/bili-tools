use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};

/// 解析视频标识，返回 (Option<bvid>, Option<aid>)
pub fn parse_video_param(id: &str) -> (Option<String>, Option<i64>) {
    let trimmed = id.trim();
    if trimmed.starts_with("BV") || trimmed.starts_with("bv") {
        (Some(trimmed.to_string()), None)
    } else if let Some(stripped) = trimmed.strip_prefix("AV").or_else(|| trimmed.strip_prefix("av")) {
        (None, stripped.parse::<i64>().ok())
    } else if let Ok(aid) = trimmed.parse::<i64>() {
        (None, Some(aid))
    } else {
        (Some(trimmed.to_string()), None)
    }
}

/// 获取视频详情
pub fn get_video_info(id: &str, cookies: Option<&Cookies>) -> Result<serde_json::Value> {
    let (bvid, aid) = parse_video_param(id);
    let aid_str = aid.map(|a| a.to_string());
    let mut params = Vec::new();
    if let Some(ref b) = bvid {
        params.push(("bvid", b.as_str()));
    } else if let Some(ref a) = aid_str {
        params.push(("aid", a.as_str()));
    } else {
        return Err(BiliLiveError::Input(format!("无效的视频ID: {id}")));
    }

    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let query = match crate::api::wbi::sign_params(&params, sessdata) {
        Ok(q) => q,
        Err(_) => {
            if let Some(ref b) = bvid {
                format!("bvid={b}")
            } else {
                format!("aid={}", aid_str.as_deref().unwrap_or(""))
            }
        }
    };

    let url = format!("https://api.bilibili.com/x/web-interface/wbi/view?{query}");
    let mut req = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/");

    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            req = req.with_header("Cookie", header);
        }
    }

    let resp = req.send()?;
    let text = resp.as_str()?;
    let json: serde_json::Value = serde_json::from_str(text)?;

    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取视频信息失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

/// 解析并补全 (bvid, aid)，若缺少一方则自动通过接口查询补齐
pub fn resolve_aid_and_bvid(id: &str, cookies: Option<&Cookies>) -> Result<(String, i64)> {
    let (bvid_opt, aid_opt) = parse_video_param(id);
    match (bvid_opt, aid_opt) {
        (Some(b), Some(a)) => Ok((b, a)),
        (Some(b), None) => {
            let info = get_video_info(&b, cookies)?;
            let aid = info["aid"]
                .as_i64()
                .ok_or_else(|| BiliLiveError::Parse("未能获取到视频aid".to_string()))?;
            Ok((b, aid))
        }
        (None, Some(a)) => {
            let info = get_video_info(&format!("av{a}"), cookies)?;
            let bvid = info["bvid"].as_str().unwrap_or("").to_string();
            Ok((bvid, a))
        }
        (None, None) => Err(BiliLiveError::Input(format!("无效的视频标识: {id}"))),
    }
}

/// 点赞或取消点赞
pub fn like_video(cookies: &Cookies, id: &str, cancel: bool) -> Result<()> {
    let (bvid, aid) = resolve_aid_and_bvid(id, Some(cookies))?;
    let like_val = if cancel { "2" } else { "1" };

    let params = [
        ("bvid", bvid.as_str()),
        ("aid", &aid.to_string()),
        ("like", like_val),
        ("csrf", cookies.bili_jct.as_str()),
    ];

    let body = serde_urlencoded::to_string(params)
        .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/{bvid}");
    let resp = crate::api::client::post("https://api.bilibili.com/x/web-interface/archive/like")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "点赞操作失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(())
}

/// 投币视频
pub fn coin_video(cookies: &Cookies, id: &str, multiply: u32, also_like: bool) -> Result<bool> {
    let (bvid, aid) = resolve_aid_and_bvid(id, Some(cookies))?;
    let mult = multiply.clamp(1, 2).to_string();
    let sel_like = if also_like { "1" } else { "0" };

    let params = [
        ("bvid", bvid.as_str()),
        ("aid", &aid.to_string()),
        ("multiply", &mult),
        ("select_like", sel_like),
        ("csrf", cookies.bili_jct.as_str()),
    ];

    let body = serde_urlencoded::to_string(params)
        .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/{bvid}");
    let resp = crate::api::client::post("https://api.bilibili.com/x/web-interface/coin/add")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "投币操作失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let liked = json["data"]["like"].as_bool().unwrap_or(false);
    Ok(liked)
}

/// 一键三连
pub fn triple_video(cookies: &Cookies, id: &str) -> Result<serde_json::Value> {
    let (bvid, aid) = resolve_aid_and_bvid(id, Some(cookies))?;

    let params = [
        ("bvid", bvid.as_str()),
        ("aid", &aid.to_string()),
        ("csrf", cookies.bili_jct.as_str()),
    ];

    let body = serde_urlencoded::to_string(params)
        .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/{bvid}");
    let resp = crate::api::client::post("https://api.bilibili.com/x/web-interface/archive/like/triple")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "一键三连失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_video_param() {
        assert_eq!(
            parse_video_param("BV1uJ411r7hL"),
            (Some("BV1uJ411r7hL".to_string()), None)
        );
        assert_eq!(
            parse_video_param("bv1uJ411r7hL"),
            (Some("bv1uJ411r7hL".to_string()), None)
        );
        assert_eq!(parse_video_param("av79677524"), (None, Some(79677524)));
        assert_eq!(parse_video_param("AV79677524"), (None, Some(79677524)));
        assert_eq!(parse_video_param("79677524"), (None, Some(79677524)));
    }
}
