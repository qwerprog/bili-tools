use crate::api::client::DEFAULT_USER_AGENT;
use crate::api::wbi::sign_params;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};
use serde_json::{Value, json};

fn check_v_voucher(data: &Value) -> Result<()> {
    if data
        .get("v_voucher")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty())
    {
        return Err(BiliLiveError::Api(
            "触发风控验证 (v_voucher)，请稍后重试或更新登录凭据".to_string(),
        ));
    }
    Ok(())
}

fn send_get_json(url: &str, referer: &str, cookies: Option<&Cookies>) -> Result<Value> {
    let mut req = crate::api::client::get(url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", referer);

    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            req = req.with_header("Cookie", header);
        }
    }

    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "HTTP 请求失败: {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let text = resp.as_str()?;
    let val: Value = serde_json::from_str(text)?;
    Ok(val)
}

fn normalize_dynamic_feed_data(mut data: Value) -> Value {
    if data.is_null() {
        return json!({
            "items": [],
            "has_more": false,
            "offset": "",
            "next_offset": ""
        });
    }
    if let Some(obj) = data.as_object_mut() {
        if obj.get("items").is_none_or(|v| v.is_null()) {
            obj.insert("items".to_string(), json!([]));
        }
        if obj.get("has_more").is_none_or(|v| v.is_null()) {
            obj.insert("has_more".to_string(), json!(false));
        }
        let offset_val = obj
            .get("offset")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if obj.get("offset").is_none_or(|v| v.is_null()) {
            obj.insert("offset".to_string(), json!(offset_val));
        }
        if obj.get("next_offset").is_none_or(|v| v.is_null()) {
            obj.insert("next_offset".to_string(), json!(offset_val));
        }
    }
    data
}

/// 获取用户空间主页详情 (`/x/space/wbi/acc/info`) 并补充粉丝/关注统计 (`/x/relation/stat`)
pub fn get_user_info(mid: i64, cookies: Option<&Cookies>) -> Result<Value> {
    let mid_str = mid.to_string();
    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let signed_query = sign_params(&[("mid", mid_str.as_str())], sessdata)?;
    let url = format!(
        "{}/x/space/wbi/acc/info?{signed_query}",
        crate::api::client::api_base_url()
    );
    let referer = format!("https://space.bilibili.com/{mid}/");

    let json = send_get_json(&url, &referer, cookies)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取用户信息失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    check_v_voucher(&data)?;

    if data.is_null() {
        return Err(BiliLiveError::Api("未获取到用户信息".to_string()));
    }
    if let Some(obj) = data.as_object_mut() {
        if obj.get("tags").is_some_and(|v| v.is_null()) {
            obj.insert("tags".to_string(), json!([]));
        }
        if obj.get("is_followed").is_none_or(|v| v.is_null()) {
            obj.insert("is_followed".to_string(), json!(false));
        }
    }

    // 补充关注/粉丝数 (`/x/relation/stat`)
    let stat_url = format!(
        "{}/x/relation/stat?vmid={mid}",
        crate::api::client::api_base_url()
    );
    if let Ok(stat_json) = send_get_json(&stat_url, &referer, cookies)
        && stat_json["code"].as_i64() == Some(0)
        && stat_json["data"].is_object()
        && let Some(obj) = data.as_object_mut()
    {
        let stat_data = stat_json["data"].clone();
        if obj.get("follower").is_none_or(|v| v.is_null()) {
            obj.insert("follower".to_string(), stat_data["follower"].clone());
        }
        if obj.get("following").is_none_or(|v| v.is_null()) {
            obj.insert("following".to_string(), stat_data["following"].clone());
        }
        obj.insert("relation".to_string(), stat_data);
    }

    Ok(data)
}

/// 获取用户投稿视频列表 (`/x/space/wbi/arc/search`)
pub fn get_user_videos(
    mid: i64,
    page: u32,
    page_size: u32,
    order: &str,
    keyword: Option<&str>,
    cookies: Option<&Cookies>,
) -> Result<Value> {
    let mid_str = mid.to_string();
    let pn_str = page.max(1).to_string();
    let ps_str = page_size.clamp(1, 50).to_string();
    let mut params: Vec<(&str, &str)> = vec![
        ("mid", &mid_str),
        ("pn", &pn_str),
        ("ps", &ps_str),
        ("order", order),
    ];
    if let Some(kw) = keyword.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("keyword", kw));
    }

    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let signed_query = sign_params(&params, sessdata)?;
    let url = format!(
        "{}/x/space/wbi/arc/search?{signed_query}",
        crate::api::client::api_base_url()
    );
    let referer = format!("https://space.bilibili.com/{mid}/video");

    let json = send_get_json(&url, &referer, cookies)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取用户投稿视频失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    check_v_voucher(&data)?;

    if data.is_null() {
        data = json!({
            "list": { "vlist": [], "tlist": {} },
            "page": { "pn": page, "ps": page_size.clamp(1, 50), "count": 0 }
        });
    } else if data.is_object() {
        if data["list"].is_null() {
            data["list"] = json!({ "vlist": [], "tlist": {} });
        } else if data["list"].is_object() && data["list"]["vlist"].is_null() {
            data["list"]["vlist"] = json!([]);
        }
    }

    Ok(data)
}

/// 获取用户空间动态列表 (`/x/polymer/web-dynamic/v1/feed/space`)
pub fn get_user_dynamics(
    mid: i64,
    offset: Option<&str>,
    cookies: Option<&Cookies>,
) -> Result<Value> {
    let mid_str = mid.to_string();
    let mut params: Vec<(&str, &str)> = vec![("host_mid", &mid_str)];
    if let Some(off) = offset.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("offset", off));
    }
    let query =
        serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let url = format!(
        "{}/x/polymer/web-dynamic/v1/feed/space?{query}",
        crate::api::client::api_base_url()
    );
    let referer = format!("https://space.bilibili.com/{mid}/dynamic");

    let json = send_get_json(&url, &referer, cookies)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取用户动态失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(normalize_dynamic_feed_data(json["data"].clone()))
}

/// 关注 (`follow = true`) 或取关 (`follow = false`) 指定用户 (`/x/relation/modify`)
pub fn modify_relation(cookies: &Cookies, fid: i64, follow: bool) -> Result<()> {
    let act = if follow { "1" } else { "2" };
    let body = serde_urlencoded::to_string([
        ("fid", fid.to_string()),
        ("act", act.to_string()),
        ("re_src", "11".to_string()),
        ("csrf", cookies.bili_jct.clone()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/relation/modify",
        crate::api::client::api_base_url()
    );
    let referer = format!("https://space.bilibili.com/{fid}/");
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "HTTP 请求失败: {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }

    let json: Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        let action = if follow { "关注" } else { "取关" };
        return Err(BiliLiveError::Api(format!(
            "{action}失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(())
}

/// 获取已关注 UP 主的动态时间线 (`/x/polymer/web-dynamic/v1/feed/all`)
pub fn get_feed(
    cookies: &Cookies,
    feed_type: &str,
    offset: Option<&str>,
) -> Result<Value> {
    let mut params: Vec<(&str, &str)> = vec![("type", feed_type)];
    if let Some(off) = offset.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("offset", off));
    }
    let query =
        serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let url = format!(
        "{}/x/polymer/web-dynamic/v1/feed/all?{query}",
        crate::api::client::api_base_url()
    );

    let json = send_get_json(&url, "https://t.bilibili.com/", Some(cookies))?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取关注动态失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(normalize_dynamic_feed_data(json["data"].clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    struct RestoreApiEnv(Option<String>);
    impl Drop for RestoreApiEnv {
        fn drop(&mut self) {
            unsafe {
                match &self.0 {
                    Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                    None => std::env::remove_var("BT_API_BASE_URL"),
                }
            }
        }
    }

    fn read_http_req(stream: &mut std::net::TcpStream) -> (String, Vec<(String, String)>, String) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        let mut headers = Vec::new();
        let mut content_len = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                let kl = k.trim().to_ascii_lowercase();
                let vt = v.trim().to_string();
                if kl == "content-length" {
                    content_len = vt.parse().unwrap_or(0);
                }
                headers.push((kl, vt));
            }
        }
        let mut buf = vec![0u8; content_len];
        if content_len > 0 {
            reader.read_exact(&mut buf).unwrap();
        }
        (
            first.trim().to_string(),
            headers,
            String::from_utf8_lossy(&buf).to_string(),
        )
    }

    fn write_json_ok(stream: &mut std::net::TcpStream, body: &str) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    }

    #[test]
    fn user_info_videos_dynamics_follow_and_feed_work_with_wbi_nulls_and_errors() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _restore = RestoreApiEnv(std::env::var("BT_API_BASE_URL").ok());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());

        let handle = std::thread::spawn(move || {
            let mut captured = Vec::new();
            // 1. /x/web-interface/nav
            // 2. /x/space/wbi/acc/info
            // 3. /x/relation/stat
            // 4. /x/space/wbi/arc/search (vlist: null)
            // 5. /x/polymer/web-dynamic/v1/feed/space (items: null)
            // 6. POST /x/relation/modify (follow)
            // 7. POST /x/relation/modify (unfollow)
            // 8. /x/polymer/web-dynamic/v1/feed/all (items: null)
            // 9. /x/space/wbi/acc/info (code: -404)
            // 10. /x/space/wbi/arc/search (v_voucher)
            for _ in 0..10 {
                let (mut stream, _) = listener.accept().unwrap();
                let (first, headers, body) = read_http_req(&mut stream);
                let resp = if first.contains("/x/web-interface/nav") {
                    r#"{"code":0,"data":{"wbi_img":{"img_url":"https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png","sub_url":"https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"}}}"#
                } else if first.contains("/x/space/wbi/acc/info") && captured.len() < 3 {
                    r#"{"code":0,"data":{"mid":123456,"name":"用户123456","level":6,"sign":"bio","is_followed":false,"live_room":null,"tags":null}}"#
                } else if first.contains("/x/relation/stat") {
                    r#"{"code":0,"data":{"mid":123456,"following":120,"follower":50000}}"#
                } else if first.contains("/x/space/wbi/arc/search") && captured.len() < 5 {
                    r#"{"code":0,"data":{"list":{"vlist":null},"page":{"pn":1,"ps":20,"count":0}}}"#
                } else if first.contains("/x/polymer/web-dynamic/v1/feed/space") {
                    r#"{"code":0,"data":{"has_more":false,"offset":"cur_space","items":null}}"#
                } else if first.contains("/x/relation/modify") {
                    r#"{"code":0,"message":"0","ttl":1}"#
                } else if first.contains("/x/polymer/web-dynamic/v1/feed/all") {
                    r#"{"code":0,"data":{"has_more":true,"offset":"cur_feed","items":null}}"#
                } else if first.contains("/x/space/wbi/acc/info") {
                    r#"{"code":-404,"message":"啥都木有"}"#
                } else {
                    r#"{"code":0,"data":{"v_voucher":"voucher_abc"}}"#
                };
                captured.push((first, headers, body));
                write_json_ok(&mut stream, resp);
            }
            captured
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &base);
        }

        let cookies = Cookies {
            sessdata: "sess1".into(),
            bili_jct: "jct1".into(),
            ..Default::default()
        };

        let info = get_user_info(123456, Some(&cookies)).unwrap();
        assert_eq!(info["mid"], 123456);
        assert_eq!(info["name"], "用户123456");
        assert_eq!(info["follower"], 50000);
        assert_eq!(info["following"], 120);
        assert_eq!(info["is_followed"], false);
        assert_eq!(info["tags"], json!([]));

        let videos = get_user_videos(123456, 1, 20, "click", Some("Rust"), Some(&cookies)).unwrap();
        assert_eq!(videos["list"]["vlist"], json!([]));

        let dyn_data = get_user_dynamics(123456, Some("off0"), Some(&cookies)).unwrap();
        assert_eq!(dyn_data["items"], json!([]));
        assert_eq!(dyn_data["offset"], "cur_space");
        assert_eq!(dyn_data["next_offset"], "cur_space");

        modify_relation(&cookies, 123456, true).unwrap();
        modify_relation(&cookies, 123456, false).unwrap();

        let feed_data = get_feed(&cookies, "video", Some("off1")).unwrap();
        assert_eq!(feed_data["items"], json!([]));
        assert_eq!(feed_data["next_offset"], "cur_feed");
        assert_eq!(feed_data["has_more"], true);

        let err_404 = get_user_info(999999999, None).unwrap_err();
        assert!(matches!(err_404, BiliLiveError::Api(ref msg) if msg.contains("啥都木有")));

        let err_voucher = get_user_videos(123456, 1, 20, "pubdate", None, None).unwrap_err();
        assert!(matches!(err_voucher, BiliLiveError::Api(ref msg) if msg.contains("v_voucher")));

        let captured = handle.join().unwrap();
        assert!(captured[1].0.contains("w_rid=") && captured[1].0.contains("wts="));
        assert!(captured[2].0.contains("/x/relation/stat?vmid=123456"));
        assert!(
            captured[3].0.contains("order=click")
                && captured[3].0.contains("keyword=Rust")
                && captured[3].0.contains("w_rid=")
        );
        assert!(captured[4].0.contains("host_mid=123456") && captured[4].0.contains("offset=off0"));

        let follow_form: Vec<(String, String)> =
            serde_urlencoded::from_str(&captured[5].2).unwrap();
        assert!(follow_form.contains(&("fid".into(), "123456".into())));
        assert!(follow_form.contains(&("act".into(), "1".into())));
        assert!(follow_form.contains(&("re_src".into(), "11".into())));
        assert!(follow_form.contains(&("csrf".into(), "jct1".into())));

        let unfollow_form: Vec<(String, String)> =
            serde_urlencoded::from_str(&captured[6].2).unwrap();
        assert!(unfollow_form.contains(&("act".into(), "2".into())));

        assert!(captured[7].0.contains("type=video") && captured[7].0.contains("offset=off1"));
        assert!(
            captured[7]
                .1
                .iter()
                .any(|(k, v)| k == "cookie" && v.contains("SESSDATA=sess1"))
        );
    }
}
