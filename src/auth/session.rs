use crate::auth::cookies::{Cookies, read_cookies};
use crate::error::{BiliLiveError, Result};

pub enum SessionState {
    NoCookies,
    Unauthenticated {
        #[allow(dead_code)]
        cookies: Cookies,
        #[allow(dead_code)]
        info: serde_json::Value,
    },
    Authenticated {
        cookies: Cookies,
        info: serde_json::Value,
    },
}

fn evaluate_session_info(cookies: Cookies, info: serde_json::Value) -> Result<SessionState> {
    let code = info["code"].as_i64().unwrap_or(-1);
    let is_login = info["data"]["isLogin"].as_bool().unwrap_or(false);

    if code == -101 {
        return Ok(SessionState::Unauthenticated { cookies, info });
    }
    if code != 0 {
        return Err(BiliLiveError::Api(format!(
            "检查登录状态失败: {}",
            info["message"].as_str().unwrap_or("未知错误")
        )));
    }
    if !is_login {
        return Ok(SessionState::Unauthenticated { cookies, info });
    }

    Ok(SessionState::Authenticated { cookies, info })
}

/// 校验当前保存的 Cookie 登录态是否有效（复用 passport::get_user_info）
pub fn inspect_session() -> Result<SessionState> {
    let cookies = match read_cookies() {
        Ok(c) => c,
        Err(BiliLiveError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionState::NoCookies);
        }
        Err(e) => return Err(e),
    };

    if cookies.sessdata.trim().is_empty() {
        return Ok(SessionState::NoCookies);
    }

    let info = crate::api::passport::get_user_info(&cookies)?;
    evaluate_session_info(cookies, info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_cookies() -> Cookies {
        Cookies {
            sessdata: "test_sess".to_string(),
            bili_jct: "test_jct".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn evaluate_session_info_handles_unauth_api_error_and_authenticated() {
        assert!(matches!(
            evaluate_session_info(
                sample_cookies(),
                json!({"code": -101, "message": "账号未登录"})
            )
            .unwrap(),
            SessionState::Unauthenticated { .. }
        ));

        assert!(matches!(
            evaluate_session_info(
                sample_cookies(),
                json!({"code": 0, "data": {"isLogin": false}})
            )
            .unwrap(),
            SessionState::Unauthenticated { .. }
        ));

        assert!(matches!(
            evaluate_session_info(
                sample_cookies(),
                json!({"code": 0, "data": {"isLogin": true, "mid": 12345}})
            )
            .unwrap(),
            SessionState::Authenticated { .. }
        ));

        let err = evaluate_session_info(
            sample_cookies(),
            json!({"code": -500, "message": "internal error"}),
        )
        .err()
        .expect("code -500 should return Api error");
        assert!(matches!(err, BiliLiveError::Api(msg) if msg.contains("internal error")));
    }

    #[test]
    fn inspect_session_propagates_api_error_and_classifies_nav_responses() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct RestoreState(Option<String>);
        impl Drop for RestoreState {
            fn drop(&mut self) {
                crate::utils::paths::set_cookie_file_override(None);
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                        None => std::env::remove_var("BT_API_BASE_URL"),
                    }
                }
            }
        }
        let _restore = RestoreState(std::env::var("BT_API_BASE_URL").ok());

        let dir = tempfile::tempdir().unwrap();
        let cookie_path = dir.path().join("cookies.json");
        sample_cookies().save_to(&cookie_path).unwrap();
        crate::utils::paths::set_cookie_file_override(Some(cookie_path));

        let responses = vec![
            r#"{"code":-101,"message":"账号未登录"}"#,
            r#"{"code":0,"data":{"isLogin":false}}"#,
            r#"{"code":0,"data":{"isLogin":true,"mid":12345}}"#,
            r#"{"code":-500,"message":"internal error"}"#,
        ];

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for body in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first_line = String::new();
                reader.read_line(&mut first_line).unwrap();
                assert!(first_line.starts_with("GET /x/web-interface/nav "));
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &base_url);
        }

        assert!(matches!(
            inspect_session().unwrap(),
            SessionState::Unauthenticated { .. }
        ));
        assert!(matches!(
            inspect_session().unwrap(),
            SessionState::Unauthenticated { .. }
        ));
        assert!(matches!(
            inspect_session().unwrap(),
            SessionState::Authenticated { .. }
        ));
        let err = inspect_session()
            .err()
            .expect("HTTP mock returning code -500 must fail");
        assert!(matches!(err, BiliLiveError::Api(msg) if msg.contains("internal error")));

        handle.join().unwrap();
    }
}
