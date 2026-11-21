use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};
use serde_json::{Value, json};

fn send_get_json(url: &str, cookies: Option<&Cookies>) -> Result<Value> {
    let mut req = crate::api::client::get(url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/");

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

/// 获取分区/全站视频排行榜 (`/x/web-interface/ranking/v2`)
pub fn get_video_ranking(rid: u32, rank_type: &str, cookies: Option<&Cookies>) -> Result<Value> {
    let query =
        serde_urlencoded::to_string([("rid", rid.to_string()), ("type", rank_type.to_string())])
            .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/web-interface/ranking/v2?{query}",
        crate::api::client::api_base_url()
    );
    let json = send_get_json(&url, cookies)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取视频排行榜失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    if data.is_null() {
        data = json!({ "note": "", "list": [] });
    } else if data.is_object() && data["list"].is_null() {
        data["list"] = json!([]);
    }
    Ok(data)
}

/// 获取当前综合热门视频 (`/x/web-interface/popular`)
pub fn get_popular_videos(page: u32, page_size: u32, cookies: Option<&Cookies>) -> Result<Value> {
    let pn = page.max(1);
    let ps = page_size.clamp(1, 50);
    let url = format!(
        "{}/x/web-interface/popular?pn={pn}&ps={ps}",
        crate::api::client::api_base_url()
    );
    let json = send_get_json(&url, cookies)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取热门视频失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    if data.is_null() {
        data = json!({ "list": [], "no_more": true });
    } else if data.is_object() && data["list"].is_null() {
        data["list"] = json!([]);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
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

    #[test]
    fn rank_video_and_popular_normalize_null_lists_and_map_api_errors() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _restore = RestoreApiEnv(std::env::var("BT_API_BASE_URL").ok());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());

        let handle = std::thread::spawn(move || {
            let mut req_lines = Vec::new();
            let responses = [
                r#"{"code":0,"data":{"note":"ok","list":null}}"#,
                r#"{"code":0,"data":{"list":[{"bvid":"BV1uJ411r7hL","title":"Mock"}],"no_more":false}}"#,
                r#"{"code":-101,"message":"账号未登录"}"#,
            ];
            for body in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                req_lines.push(first.trim().to_string());
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
            req_lines
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &base);
        }

        let r = get_video_ranking(36, "rookie", None).unwrap();
        assert_eq!(r["list"], json!([]));

        let p = get_popular_videos(2, 10, None).unwrap();
        assert_eq!(p["list"][0]["bvid"], "BV1uJ411r7hL");

        let err = get_popular_videos(1, 20, None).unwrap_err();
        assert!(matches!(err, BiliLiveError::Api(ref msg) if msg.contains("账号未登录")));

        let reqs = handle.join().unwrap();
        assert!(reqs[0].contains("/x/web-interface/ranking/v2?rid=36&type=rookie"));
        assert!(reqs[1].contains("/x/web-interface/popular?pn=2&ps=10"));
    }
}
