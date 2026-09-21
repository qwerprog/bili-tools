use crate::api::client::DEFAULT_USER_AGENT;
use crate::api::wbi::sign_params;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};

const SEARCH_TYPE_URL: &str = "https://api.bilibili.com/x/web-interface/wbi/search/type";

fn perform_wbi_search(
    params: &[(&str, &str)],
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let signed_query = sign_params(params, sessdata)?;
    let url = format!("{SEARCH_TYPE_URL}?{signed_query}");

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
            "搜索请求失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

/// 搜索视频
pub fn search_videos(
    keyword: &str,
    page: u32,
    order: Option<&str>,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let page_str = page.to_string();
    let order_val = order.unwrap_or("totalrank");

    let params = [
        ("search_type", "video"),
        ("keyword", keyword),
        ("page", &page_str),
        ("order", order_val),
    ];

    perform_wbi_search(&params, cookies)
}

/// 搜索用户
pub fn search_users(
    keyword: &str,
    page: u32,
    order: Option<&str>,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let page_str = page.to_string();
    let order_val = order.unwrap_or("0");

    let params = [
        ("search_type", "bili_user"),
        ("keyword", keyword),
        ("page", &page_str),
        ("order", order_val),
    ];

    perform_wbi_search(&params, cookies)
}

/// 搜索直播间
pub fn search_live(
    keyword: &str,
    page: u32,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let page_str = page.to_string();

    let params = [
        ("search_type", "live"),
        ("keyword", keyword),
        ("page", &page_str),
    ];

    perform_wbi_search(&params, cookies)
}

/// 获取热搜列表
pub fn get_hot_search(limit: Option<u32>) -> Result<serde_json::Value> {
    let lim = limit.unwrap_or(20).clamp(1, 50);
    let lim_str = lim.to_string();

    let url = format!(
        "https://api.bilibili.com/x/web-interface/wbi/search/square?limit={lim_str}&platform=web"
    );

    let resp = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/")
        .send();

    if let Ok(res) = resp {
        let text = res.as_str()?;
        let json: serde_json::Value = serde_json::from_str(text)?;
        if json["code"].as_i64() == Some(0) {
            return Ok(json["data"]["trending"]["list"].clone());
        }
    }

    // 备用接口：hotword
    let fallback_url = "https://s.search.bilibili.com/main/hotword";
    let fallback_resp = crate::api::client::get(fallback_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/")
        .send()?;

    let json: serde_json::Value = serde_json::from_str(fallback_resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取热搜失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["list"].clone())
}
