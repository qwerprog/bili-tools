use crate::api::passport::get_roomid;
use crate::error::{BiliLiveError, Result};
use crate::user_success;

use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct Cookies {
    pub room_id: i32,
    pub sessdata: String,
    pub csrf_token: String,
    #[serde(default)]
    pub live_key: Option<u64>,
}

/// 从 TV API 直接拿到的凭证保存到 cookies.json
pub fn save_cookies_from_credentials(sessdata: &str, csrf_token: &str) -> Result<()> {
    let cookies = Cookies {
        room_id: get_roomid(sessdata)?,
        sessdata: sessdata.to_string(),
        csrf_token: csrf_token.to_string(),
        live_key: None,
    };
    let cookies_json = serde_json::to_string_pretty(&cookies)?;
    fs::write("cookies.json", cookies_json)?;
    user_success!("Cookies保存成功");
    Ok(())
}

pub fn update_live_key(live_key: u64) -> Result<()> {
    let mut cookies = read_cookies()?;
    cookies.live_key = Some(live_key);
    let cookies_json = serde_json::to_string_pretty(&cookies)?;
    fs::write("cookies.json", cookies_json)?;
    Ok(())
}

pub fn delete_cookies() -> Result<()> {
    if std::path::Path::new("cookies.json").exists() {
        std::fs::remove_file("cookies.json")?;
    }
    Ok(())
}

pub fn read_cookies() -> Result<Cookies> {
    let cookies_str = std::fs::read_to_string("./cookies.json").map_err(BiliLiveError::Io)?;
    let cookies: Cookies = serde_json::from_str(&cookies_str).map_err(BiliLiveError::Json)?;
    Ok(cookies)
}
