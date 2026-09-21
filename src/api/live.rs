use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::{Cookies, read_cookies};
use crate::error::{BiliLiveError, Result};

pub fn check_live_status(room_id: i32) -> Result<bool> {
    let url = format!(
        "https://api.live.bilibili.com/room/v1/Room/get_info?room_id={}",
        room_id
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let response_text = response.as_str()?;
    let json: serde_json::Value = serde_json::from_str(response_text)?;
    let live_status = json["data"]["live_status"]
        .as_i64()
        .ok_or_else(|| BiliLiveError::Parse("无法解析直播状态".to_string()))?;
    Ok(live_status == 1)
}

pub fn get_recent_live() -> Result<(String, String)> {
    let room_id = read_cookies()?.room_id;
    let url = format!(
        "https://api.live.bilibili.com/room/v1/Area/getMyChooseArea?roomid={}",
        room_id
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let response_text = response.as_str()?;
    let json: serde_json::Value = serde_json::from_str(response_text)?;
    let data = &json["data"][0];
    let id = data["id"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("无法解析分区ID".to_string()))?
        .to_string();
    let name = data["name"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("无法解析分区名称".to_string()))?
        .to_string();
    Ok((id, name))
}

pub fn update_title(cookies: &Cookies, title: &str) -> Result<()> {
    let form = title_form(cookies, title)?;
    let response = crate::api::client::post("https://api.live.bilibili.com/room/v1/Room/update")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Cookie", format!("SESSDATA={}", cookies.sessdata))
        .with_body(form)
        .send()?;

    let json: serde_json::Value = serde_json::from_str(response.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "更新标题失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(())
}

fn title_form(cookies: &Cookies, title: &str) -> Result<String> {
    serde_urlencoded::to_string([
        ("room_id", cookies.room_id.to_string()),
        ("title", title.to_string()),
        ("csrf_token", cookies.csrf_token.clone()),
        ("csrf", cookies.csrf_token.clone()),
    ])
    .map_err(|e| BiliLiveError::Parse(format!("表单编码失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_special_characters_round_trip() {
        let cookies = Cookies {
            room_id: 1,
            sessdata: String::new(),
            csrf_token: "test".into(),
            live_key: None,
        };
        let title = "C++ & 聊天=游戏 100%20完成";
        let fields: Vec<(String, String)> =
            serde_urlencoded::from_str(&title_form(&cookies, title).unwrap()).unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[1], ("title".into(), title.into()));
    }
}
