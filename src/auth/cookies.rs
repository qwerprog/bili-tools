use crate::api::passport::get_roomid;
use crate::error::{BiliLiveError, Result};
use crate::user_success;
use crate::utils::paths::data_file;

use serde::{Deserialize, Serialize};
use std::fs;

// 登录凭据结构，存储到 cookies.json
#[derive(Debug, Serialize, Deserialize)]
pub struct Cookies {
    pub room_id: i32,
    pub sessdata: String,
    pub csrf_token: String,
    #[serde(default)]
    pub live_key: Option<u64>,
}

pub fn save_cookies_from_credentials(sessdata: &str, csrf_token: &str) -> Result<()> {
    let cookies = Cookies {
        room_id: get_roomid(sessdata)?,
        sessdata: sessdata.to_string(),
        csrf_token: csrf_token.to_string(),
        live_key: None,
    };
    let path = data_file("cookies.json");
    let cookies_json = serde_json::to_string_pretty(&cookies)?;
    fs::write(&path, cookies_json)?;
    user_success!("Cookies保存成功");
    Ok(())
}

pub fn update_live_key(live_key: u64) -> Result<()> {
    let mut cookies = read_cookies()?;
    cookies.live_key = Some(live_key);
    let path = data_file("cookies.json");
    let cookies_json = serde_json::to_string_pretty(&cookies)?;
    fs::write(&path, cookies_json)?;
    Ok(())
}

pub fn delete_cookies() -> Result<()> {
    let path = data_file("cookies.json");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn read_cookies() -> Result<Cookies> {
    let path = data_file("cookies.json");
    let cookies_str = std::fs::read_to_string(&path).map_err(BiliLiveError::Io)?;
    let cookies: Cookies = serde_json::from_str(&cookies_str).map_err(BiliLiveError::Json)?;
    Ok(cookies)
}
