use crate::api::client::DEFAULT_USER_AGENT;
use crate::api::wbi::sign_params;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};

fn perform_wbi_search(
    params: &[(&str, &str)],
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let signed_query = sign_params(params, sessdata)?;
    let url = format!(
        "{}/x/web-interface/wbi/search/type?{signed_query}",
        crate::api::client::api_base_url()
    );

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
    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "搜索请求失败")?;

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

fn normalize_hot_list(mut val: serde_json::Value, limit: usize) -> serde_json::Value {
    if val.is_null() {
        return serde_json::Value::Array(Vec::new());
    }
    if let Some(arr) = val.as_array_mut() {
        arr.truncate(limit);
    }
    val
}

/// 获取热搜列表（优先 WBI `/x/web-interface/wbi/search/square`，失败时回退至 `hotword`）
pub fn get_hot_search(
    limit: Option<u32>,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let lim = limit.unwrap_or(20).clamp(1, 50);
    let lim_str = lim.to_string();
    let sessdata = cookies.map(|c| c.sessdata.as_str());

    if let Ok(signed_query) =
        sign_params(&[("limit", lim_str.as_str()), ("platform", "web")], sessdata)
    {
        let url = format!(
            "{}/x/web-interface/wbi/search/square?{signed_query}",
            crate::api::client::api_base_url()
        );

        let mut req = crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Referer", "https://www.bilibili.com/");

        if let Some(c) = cookies {
            let header = c.cookie_header();
            if !header.is_empty() {
                req = req.with_header("Cookie", header);
            }
        }

        if let Ok(res) = req.send()
            && res.status_code < 400
            && let Ok(text) = res.as_str()
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
            && json["code"].as_i64() == Some(0)
        {
            return Ok(normalize_hot_list(
                json["data"]["trending"]["list"].clone(),
                lim as usize,
            ));
        }
    }

    // 备用接口：hotword
    let fallback_url = format!("{}/main/hotword", crate::api::client::search_fallback_base_url());
    let mut fallback_req = crate::api::client::get(&fallback_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/");
    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            fallback_req = fallback_req.with_header("Cookie", header);
        }
    }
    let fallback_resp = fallback_req.send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&fallback_resp, "获取热搜失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取热搜失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(normalize_hot_list(json["list"].clone(), lim as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    struct RestoreSearchEnv {
        api: Option<String>,
        search: Option<String>,
    }
    impl Drop for RestoreSearchEnv {
        fn drop(&mut self) {
            unsafe {
                match &self.api {
                    Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                    None => std::env::remove_var("BT_API_BASE_URL"),
                }
                match &self.search {
                    Some(v) => std::env::set_var("BT_SEARCH_BASE_URL", v),
                    None => std::env::remove_var("BT_SEARCH_BASE_URL"),
                }
            }
        }
    }

    #[test]
    fn hot_search_uses_wbi_signing_cookies_and_truncates_and_falls_back() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _restore = RestoreSearchEnv {
            api: std::env::var("BT_API_BASE_URL").ok(),
            search: std::env::var("BT_SEARCH_BASE_URL").ok(),
        };

        let api_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api_base = format!("http://{}", api_listener.local_addr().unwrap());

        let api_handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            // 1: /nav + /wbi/search/square (success)
            // 2: /wbi/search/square (cached WBI key, returns non-0 -> triggers fallback)
            // 3: /wbi/search/type (search_users with order=0)
            for idx in 0..4 {
                let (mut stream, _) = api_listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut has_cookie = false;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if line.to_ascii_lowercase().starts_with("cookie:") && line.contains("SESSDATA=s1") {
                        has_cookie = true;
                    }
                }
                requests.push((first.trim().to_string(), has_cookie));

                let body = if first.contains("/x/web-interface/nav") {
                    r#"{"code":0,"data":{"wbi_img":{"img_url":"https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png","sub_url":"https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"}}}"#.to_string()
                } else if idx == 1 {
                    r#"{"code":0,"data":{"trending":{"list":[{"keyword":"k1"},{"keyword":"k2"},{"keyword":"k3"},{"keyword":"k4"}]}}}"#.to_string()
                } else if idx == 2 {
                    r#"{"code":-400,"message":"bad"}"#.to_string()
                } else {
                    r#"{"code":0,"data":{"numResults":1,"result":[{"mid":42,"uname":"up"}]}}"#.to_string()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
            requests
        });

        let fb_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fb_base = format!("http://{}", fb_listener.local_addr().unwrap());
        let fb_handle = std::thread::spawn(move || {
            let (mut stream, _) = fb_listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            assert!(first.starts_with("GET /main/hotword "));
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let body = r#"{"code":0,"list":[{"keyword":"fb1"},{"keyword":"fb2"},{"keyword":"fb3"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &api_base);
            std::env::set_var("BT_SEARCH_BASE_URL", &fb_base);
        }

        let cookies = Cookies {
            sessdata: "s1".into(),
            ..Default::default()
        };

        let hot1 = get_hot_search(Some(2), Some(&cookies)).unwrap();
        assert_eq!(hot1.as_array().unwrap().len(), 2);

        let hot2 = get_hot_search(Some(2), Some(&cookies)).unwrap();
        assert_eq!(hot2.as_array().unwrap().len(), 2);
        assert_eq!(hot2[0]["keyword"], "fb1");

        let users = search_users("up", 1, Some("0"), Some(&cookies)).unwrap();
        assert_eq!(users["numResults"], 1);

        fb_handle.join().unwrap();
        let reqs = api_handle.join().unwrap();
        assert!(reqs[1].0.contains("/x/web-interface/wbi/search/square?"));
        assert!(reqs[1].0.contains("w_rid=") && reqs[1].0.contains("wts="));
        assert!(reqs[1].1, "expected Cookie header on search/square");
        assert!(reqs[3].0.contains("order=0"));
    }
}
