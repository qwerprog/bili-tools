use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};

/// 获取用户创建的所有收藏夹列表
pub fn get_user_fav_folders(mid: i64, cookies: Option<&Cookies>) -> Result<serde_json::Value> {
    let url = format!("https://api.bilibili.com/x/v3/fav/folder/created/list-all?up_mid={mid}");
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
    let text = resp.as_str()?;
    let json: serde_json::Value = serde_json::from_str(text)?;

    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取收藏夹列表失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

/// 分页获取指定收藏夹内容
pub fn get_fav_resources(
    media_id: i64,
    page: u32,
    page_size: u32,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let ps = page_size.clamp(1, 20);
    let url = format!(
        "https://api.bilibili.com/x/v3/fav/resource/list?media_id={media_id}&pn={page}&ps={ps}&platform=web"
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
    let text = resp.as_str()?;
    let json: serde_json::Value = serde_json::from_str(text)?;

    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取收藏夹内容失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

/// 获取收藏夹全部内容（自动翻页，用于导出）
pub fn get_all_fav_resources(
    media_id: i64,
    cookies: Option<&Cookies>,
) -> Result<(serde_json::Value, Vec<serde_json::Value>)> {
    let mut page = 1;
    let mut all_medias = Vec::new();
    let mut folder_info = serde_json::Value::Null;

    loop {
        let data = get_fav_resources(media_id, page, 20, cookies)?;
        if folder_info.is_null() {
            folder_info = data["info"].clone();
        }

        if let Some(medias) = data["medias"].as_array() {
            if medias.is_empty() {
                break;
            }
            all_medias.extend(medias.iter().cloned());
        } else {
            break;
        }

        let has_more = data["has_more"].as_bool().unwrap_or(false);
        if !has_more {
            break;
        }
        page += 1;
        // 防请求过频
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    Ok((folder_info, all_medias))
}
