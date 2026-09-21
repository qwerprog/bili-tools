use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::read_cookies;
use crate::error::Result;

/// 校验当前保存的 Cookie 登录态是否有效（静默网络校验）
#[allow(dead_code)]
pub fn check_status() -> Result<bool> {
    let Ok(cookies) = read_cookies() else {
        return Ok(false);
    };
    if cookies.sessdata.is_empty() {
        return Ok(false);
    }

    let response = crate::api::client::get("https://api.bilibili.com/x/web-interface/nav")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let response_text = response.as_str()?;
    let response_json: serde_json::Value = serde_json::from_str(response_text)?;
    let code = response_json["code"].as_i64().unwrap_or(-1);
    let is_login = response_json["data"]["isLogin"].as_bool().unwrap_or(false);

    Ok(code == 0 && is_login)
}
