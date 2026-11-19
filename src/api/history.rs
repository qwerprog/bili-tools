use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};

/// 获取历史记录列表（带可选翻页游标 `max` 与 `view_at`），返回 `(list_array, next_cursor)`
pub fn get_history_page(
    cookies: &Cookies,
    ps: u32,
    hist_type: Option<&str>,
    max: Option<i64>,
    view_at: Option<i64>,
) -> Result<(serde_json::Value, Option<(i64, i64)>)> {
    let ps_val = ps.clamp(1, 30);
    let t_val = hist_type.unwrap_or("all");

    let mut url = format!(
        "{}/x/web-interface/history/cursor?ps={ps_val}&type={t_val}",
        crate::api::client::api_base_url()
    );
    if let Some(m) = max.filter(|&v| v > 0) {
        url.push_str(&format!("&max={m}"));
    }
    if let Some(va) = view_at.filter(|&v| v > 0) {
        url.push_str(&format!("&view_at={va}"));
    }

    let resp = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "获取历史记录失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取历史记录失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let next_max = json["data"]["cursor"]["max"].as_i64().unwrap_or(0);
    let next_va = json["data"]["cursor"]["view_at"].as_i64().unwrap_or(0);
    let next_cursor = (next_max > 0 && next_va > 0).then_some((next_max, next_va));

    let mut list = json["data"]["list"].clone();
    if list.is_null() {
        list = serde_json::Value::Array(Vec::new());
    } else if let Some(arr) = list.as_array_mut() {
        arr.truncate(ps_val as usize);
    }

    Ok((list, next_cursor))
}

/// 获取历史记录列表
#[allow(dead_code)]
pub fn get_history(
    cookies: &Cookies,
    ps: u32,
    hist_type: Option<&str>,
) -> Result<serde_json::Value> {
    let (list, _) = get_history_page(cookies, ps, hist_type, None, None)?;
    Ok(list)
}

/// 获取稍后再看列表
pub fn get_toview_list(cookies: &Cookies) -> Result<serde_json::Value> {
    let url = format!(
        "{}/x/v2/history/toview",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "获取稍后再看列表失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取稍后再看列表失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    if data.is_null() {
        data = serde_json::json!({ "count": 0, "list": [] });
    } else if data.is_object() && data["list"].is_null() {
        data["list"] = serde_json::json!([]);
    }
    Ok(data)
}

/// 添加视频到稍后再看
pub fn add_toview(cookies: &Cookies, id: &str) -> Result<()> {
    let target = crate::utils::id::resolve_video(id, false)?;
    let mut params = vec![("csrf", cookies.bili_jct.clone())];

    match (target.bvid, target.aid) {
        (Some(b), _) => params.push(("bvid", b)),
        (_, Some(a)) => params.push(("aid", a.to_string())),
        _ => return Err(BiliLiveError::Input(format!("无效的视频ID: {id}"))),
    }

    let body = serde_urlencoded::to_string(&params)
        .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/v2/history/toview/add",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "添加稍后再看失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "添加稍后再看失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(())
}

/// 从稍后再看列表移除指定视频 (`/x/v2/history/toview/del`)
pub fn remove_toview(cookies: &Cookies, id: &str) -> Result<i64> {
    let aid = crate::api::video::resolve_video_aid(id, Some(cookies))?;
    let body = serde_urlencoded::to_string([
        ("aid", aid.to_string()),
        ("csrf", cookies.bili_jct.clone()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/v2/history/toview/del",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "移除稍后再看视频失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "移除稍后再看视频失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(aid)
}

/// 清空稍后再看列表（支持全量清空或仅清空已观看）
pub fn clear_toview(cookies: &Cookies, viewed_only: bool) -> Result<()> {
    let api_base = crate::api::client::api_base_url();
    if viewed_only {
        let body = serde_urlencoded::to_string([
            ("viewed", "true"),
            ("csrf", cookies.bili_jct.as_str()),
        ])
        .map_err(|e| BiliLiveError::Parse(e.to_string()))?;
        let url = format!("{api_base}/x/v2/history/toview/del");
        let resp = crate::api::client::post(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_header("Referer", "https://www.bilibili.com/")
            .with_header("Cookie", cookies.cookie_header())
            .with_body(body)
            .send()?;

        let json: serde_json::Value =
            crate::api::client::parse_json_response(&resp, "清理已观看记录失败")?;
        if json["code"].as_i64() != Some(0) {
            return Err(BiliLiveError::Api(format!(
                "清理已观看记录失败: {}",
                json["message"].as_str().unwrap_or("未知错误")
            )));
        }
    } else {
        let body = serde_urlencoded::to_string([("csrf", cookies.bili_jct.as_str())])
            .map_err(|e| BiliLiveError::Parse(e.to_string()))?;
        let url = format!("{api_base}/x/v2/history/toview/clear");
        let resp = crate::api::client::post(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_header("Referer", "https://www.bilibili.com/")
            .with_header("Cookie", cookies.cookie_header())
            .with_body(body)
            .send()?;

        let json: serde_json::Value =
            crate::api::client::parse_json_response(&resp, "清空稍后再看列表失败")?;
        if json["code"].as_i64() != Some(0) {
            return Err(BiliLiveError::Api(format!(
                "清空稍后再看列表失败: {}",
                json["message"].as_str().unwrap_or("未知错误")
            )));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedHistoryItem {
    pub business: String,
    pub oid: i64,
    pub kid: String,
}

fn parse_positive_digits(s: &str) -> Option<i64> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok().filter(|&v| v > 0)
}

pub(crate) fn resolve_history_kid(
    raw: &str,
    cookies: Option<&Cookies>,
) -> Result<DeletedHistoryItem> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(BiliLiveError::Input("历史记录目标标识不能为空".to_string()));
    }

    let lower = trimmed.to_ascii_lowercase();

    // 1. 直接匹配 `bit history list` 展示的 `room:<id>` 与 `cv<id>`
    if let Some(rest) = lower.strip_prefix("room:")
        && let Some(rid) = parse_positive_digits(rest)
    {
        return Ok(DeletedHistoryItem {
            business: "live".to_string(),
            oid: rid,
            kid: format!("live_{rid}"),
        });
    }
    if let Some(rest) = lower.strip_prefix("cv")
        && let Some(cvid) = parse_positive_digits(rest)
    {
        return Ok(DeletedHistoryItem {
            business: "article".to_string(),
            oid: cvid,
            kid: format!("article_{cvid}"),
        });
    }

    // 2. 支持原始 `archive_<id>` / `live_<id>` / `article_<id>` / `pgc_<id>` kid
    for biz in ["archive", "live", "article", "pgc"] {
        let prefix = format!("{biz}_");
        if let Some(rest) = lower.strip_prefix(&prefix)
            && let Some(oid) = parse_positive_digits(rest)
        {
            return Ok(DeletedHistoryItem {
                business: biz.to_string(),
                oid,
                kid: format!("{biz}_{oid}"),
            });
        }
    }

    // 3. 支持专栏 URL `bilibili.com/read/cv12345`
    if lower.contains("bilibili.com/read/cv")
        && let Some(after) = lower.split("bilibili.com/read/cv").nth(1)
    {
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Some(cvid) = parse_positive_digits(&digits) {
            return Ok(DeletedHistoryItem {
                business: "article".to_string(),
                oid: cvid,
                kid: format!("article_{cvid}"),
            });
        }
    }

    // 4. 解析直播间 URL / 番剧 SS / 视频 BV/av/URL
    let parsed = crate::utils::id::parse_input(trimmed, false)?;
    match parsed.kind {
        crate::utils::id::IdKind::LiveRoom => {
            let rid = parsed
                .room_id
                .ok_or_else(|| BiliLiveError::Input(format!("无效的直播间标识: {trimmed}")))?;
            Ok(DeletedHistoryItem {
                business: "live".to_string(),
                oid: rid,
                kid: format!("live_{rid}"),
            })
        }
        crate::utils::id::IdKind::BangumiSeason => {
            let ssid = parsed
                .season_id
                .ok_or_else(|| BiliLiveError::Input(format!("无效的番剧标识: {trimmed}")))?;
            Ok(DeletedHistoryItem {
                business: "pgc".to_string(),
                oid: ssid,
                kid: format!("pgc_{ssid}"),
            })
        }
        _ => {
            let aid = crate::api::video::resolve_video_aid(trimmed, cookies)?;
            Ok(DeletedHistoryItem {
                business: "archive".to_string(),
                oid: aid,
                kid: format!("archive_{aid}"),
            })
        }
    }
}

/// 删除单条观看历史记录（支持视频/直播/专栏/番剧，调用 `/x/v2/history/delete`）
pub fn delete_history_item(cookies: &Cookies, id: &str) -> Result<DeletedHistoryItem> {
    let target = resolve_history_kid(id, Some(cookies))?;
    let body = serde_urlencoded::to_string([
        ("kid", target.kid.as_str()),
        ("csrf", cookies.bili_jct.as_str()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/v2/history/delete",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "删除历史记录失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "删除历史记录失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(target)
}

/// 删除单条视频观看历史记录 (`/x/v2/history/delete`)
#[allow(dead_code)]
pub fn delete_history(cookies: &Cookies, id: &str) -> Result<i64> {
    delete_history_item(cookies, id).map(|d| d.oid)
}

/// 清空全部观看历史记录 (`/x/v2/history/clear`)
pub fn clear_history(cookies: &Cookies) -> Result<()> {
    let body = serde_urlencoded::to_string([("csrf", cookies.bili_jct.as_str())])
        .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/v2/history/clear",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "清空观看历史记录失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "清空观看历史记录失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    struct RestoreApi(Option<String>);
    impl Drop for RestoreApi {
        fn drop(&mut self) {
            unsafe {
                match &self.0 {
                    Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                    None => std::env::remove_var("BT_API_BASE_URL"),
                }
            }
        }
    }

    #[test]
    fn later_and_history_null_normalization_truncation_and_crud() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _restore = RestoreApi(std::env::var("BT_API_BASE_URL").ok());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());

        let handle = std::thread::spawn(move || {
            let mut captured = Vec::new();
            let responses = [
                // 1. get_toview_list (list: null)
                r#"{"code":0,"data":{"count":0,"list":null}}"#,
                // 2. get_history (list: null)
                r#"{"code":0,"data":{"list":null}}"#,
                // 3. get_history (ps=2 with 4 items returned)
                r#"{"code":0,"data":{"list":[{"title":"a"},{"title":"b"},{"title":"c"},{"title":"d"}]}}"#,
                // 4. remove_toview(av111) -> POST toview/del
                r#"{"code":0,"message":"0"}"#,
                // 5. clear_toview(viewed_only=true) -> POST toview/del
                r#"{"code":0,"message":"0"}"#,
                // 6. clear_toview(viewed_only=false) -> POST toview/clear
                r#"{"code":0,"message":"0"}"#,
                // 7. delete_history(BV...) -> nav (fail -> fallback)
                r#"{"code":-101}"#,
                // 8. delete_history(BV...) -> view
                r#"{"code":0,"data":{"aid":79677524,"bvid":"BV1uJ411r7hL"}}"#,
                // 9. delete_history(BV...) -> POST /x/v2/history/delete
                r#"{"code":0,"message":"0"}"#,
                // 10. clear_history -> POST /x/v2/history/clear
                r#"{"code":0,"message":"0"}"#,
            ];
            for resp_body in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        content_len = rest.trim().parse().unwrap_or(0);
                    }
                }
                let mut buf = vec![0u8; content_len];
                if content_len > 0 {
                    reader.read_exact(&mut buf).unwrap();
                }
                captured.push((first.trim().to_string(), String::from_utf8_lossy(&buf).to_string()));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body
                )
                .unwrap();
            }
            captured
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &base);
        }

        let cookies = Cookies {
            sessdata: "s".into(),
            bili_jct: "jct_hist".into(),
            ..Default::default()
        };

        let toview = get_toview_list(&cookies).unwrap();
        assert_eq!(toview["list"], serde_json::json!([]));

        let empty_hist = get_history(&cookies, 20, Some("all")).unwrap();
        assert_eq!(empty_hist, serde_json::json!([]));

        let limited_hist = get_history(&cookies, 2, Some("archive")).unwrap();
        assert_eq!(limited_hist.as_array().unwrap().len(), 2);

        assert_eq!(remove_toview(&cookies, "av111").unwrap(), 111);
        clear_toview(&cookies, true).unwrap();
        clear_toview(&cookies, false).unwrap();
        assert_eq!(delete_history(&cookies, "BV1uJ411r7hL").unwrap(), 79677524);
        clear_history(&cookies).unwrap();

        let captured = handle.join().unwrap();
        assert!(captured[3].0.starts_with("POST /x/v2/history/toview/del"));
        let rem_form: Vec<(String, String)> = serde_urlencoded::from_str(&captured[3].1).unwrap();
        assert!(rem_form.contains(&("aid".into(), "111".into())));

        assert!(captured[4].0.starts_with("POST /x/v2/history/toview/del"));
        let viewed_form: Vec<(String, String)> = serde_urlencoded::from_str(&captured[4].1).unwrap();
        assert!(viewed_form.contains(&("viewed".into(), "true".into())));

        assert!(captured[5].0.starts_with("POST /x/v2/history/toview/clear"));

        assert!(captured[8].0.starts_with("POST /x/v2/history/delete"));
        let del_hist_form: Vec<(String, String)> = serde_urlencoded::from_str(&captured[8].1).unwrap();
        assert!(del_hist_form.contains(&("kid".into(), "archive_79677524".into())));

        assert!(captured[9].0.starts_with("POST /x/v2/history/clear"));

        let live_item = resolve_history_kid("room:76", None).unwrap();
        assert_eq!(live_item.kid, "live_76");
        let cv_item = resolve_history_kid("cv12345", None).unwrap();
        assert_eq!(cv_item.kid, "article_12345");
        let url_cv = resolve_history_kid("https://www.bilibili.com/read/cv9988?spm=1", None).unwrap();
        assert_eq!(url_cv.kid, "article_9988");
        assert!(resolve_history_kid("invalid_id", None).is_err());
    }
}
