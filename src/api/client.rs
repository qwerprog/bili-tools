pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";

const REQUEST_TIMEOUT_SECS: u64 = 30;

fn resolve_base_url_from(value: Option<&str>, default: &str) -> String {
    value
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn resolve_base_url(env_key: &str, default: &str) -> String {
    let val = std::env::var(env_key).ok();
    resolve_base_url_from(val.as_deref(), default)
}

pub fn api_base_url() -> String {
    resolve_base_url("BT_API_BASE_URL", "https://api.bilibili.com")
}

pub fn live_api_base_url() -> String {
    resolve_base_url("BT_LIVE_API_BASE_URL", "https://api.live.bilibili.com")
}

pub fn passport_base_url() -> String {
    resolve_base_url("BT_PASSPORT_BASE_URL", "https://passport.bilibili.com")
}

pub fn comment_base_url() -> String {
    resolve_base_url("BT_COMMENT_BASE_URL", "https://comment.bilibili.com")
}

pub fn short_link_base_url() -> String {
    resolve_base_url("BT_SHORT_LINK_BASE_URL", "https://b23.tv")
}

pub fn web_base_url() -> String {
    resolve_base_url("BT_WEB_BASE_URL", "https://www.bilibili.com")
}

pub fn search_fallback_base_url() -> String {
    resolve_base_url("BT_SEARCH_BASE_URL", "https://s.search.bilibili.com")
}

pub fn member_api_base_url() -> String {
    resolve_base_url("BT_MEMBER_API_BASE_URL", "https://member.bilibili.com")
}

pub fn get(url: impl AsRef<str>) -> minreq::Request {
    minreq::get(url.as_ref()).with_timeout(REQUEST_TIMEOUT_SECS)
}

pub fn post(url: impl AsRef<str>) -> minreq::Request {
    minreq::post(url.as_ref()).with_timeout(REQUEST_TIMEOUT_SECS)
}

pub fn put_with_timeout(url: impl AsRef<str>, timeout_secs: u64) -> minreq::Request {
    minreq::put(url.as_ref()).with_timeout(timeout_secs)
}

/// 校验 HTTP 状态码 < 400 并将响应体反序列化为 JSON，避免 4xx/5xx HTML 错误页被误判为 Json 解析错误
pub fn parse_json_response<T: serde::de::DeserializeOwned>(
    resp: &minreq::Response,
    context: &str,
) -> crate::error::Result<T> {
    if resp.status_code >= 400 {
        return Err(crate::error::BiliLiveError::Api(format!(
            "{context}: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let text = resp.as_str()?;
    let val = serde_json::from_str(text)?;
    Ok(val)
}

/// 原始二进制 HTTP 响应（保留响应头及未解码字节体，适用于压缩 XML / 媒体流等）
#[derive(Debug, Clone)]
pub struct RawResponse {
    #[allow(dead_code)]
    pub status_code: i32,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Vec<u8>,
}

/// 发送 GET 请求并返回原始字节响应（不调用 `as_str()`，并校验 HTTP 状态码 < 400）
pub fn get_bytes(
    url: &str,
    referer: Option<&str>,
    cookies: Option<&crate::auth::cookies::Cookies>,
) -> crate::error::Result<RawResponse> {
    let mut req = get(url).with_header("User-Agent", DEFAULT_USER_AGENT);
    if let Some(ref_url) = referer {
        req = req.with_header("Referer", ref_url);
    }
    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            req = req.with_header("Cookie", header);
        }
    }
    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(crate::error::BiliLiveError::Api(format!(
            "HTTP 请求失败: {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let status_code = resp.status_code;
    let headers = resp.headers.clone();
    let body = resp.into_bytes();
    Ok(RawResponse {
        status_code,
        headers,
        body,
    })
}

#[cfg(test)]
pub(crate) static TEST_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_base_url_from_handles_defaults_and_trailing_slashes() {
        assert_eq!(
            resolve_base_url_from(None, "https://api.bilibili.com"),
            "https://api.bilibili.com"
        );
        assert_eq!(
            resolve_base_url_from(Some("   "), "https://api.bilibili.com"),
            "https://api.bilibili.com"
        );
        assert_eq!(
            resolve_base_url_from(Some("http://127.0.0.1:8080/// "), "https://api.bilibili.com"),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn parse_json_response_surfaces_http_errors_as_api_error() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for idx in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                if idx == 0 {
                    let body = "<html>502 Bad Gateway</html>";
                    write!(
                        stream,
                        "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                } else {
                    let body = r#"{"code":0,"ok":true}"#;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
            }
        });

        let r1 = get(format!("{base}/err")).send().unwrap();
        let err = parse_json_response::<serde_json::Value>(&r1, "测试接口失败").unwrap_err();
        assert!(matches!(err, crate::error::BiliLiveError::Api(ref m) if m.contains("502")));

        let r2 = get(format!("{base}/ok")).send().unwrap();
        let ok: serde_json::Value = parse_json_response(&r2, "测试接口失败").unwrap();
        assert_eq!(ok["ok"], true);
        handle.join().unwrap();
    }
}
