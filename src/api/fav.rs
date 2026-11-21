use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};

/// 获取用户创建的所有收藏夹列表
pub fn get_user_fav_folders(mid: i64, cookies: Option<&Cookies>) -> Result<serde_json::Value> {
    let url = format!(
        "{}/x/v3/fav/folder/created/list-all?up_mid={mid}",
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
        crate::api::client::parse_json_response(&resp, "获取收藏夹列表失败")?;

    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取收藏夹列表失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    if data.is_null() {
        data = serde_json::json!({ "count": 0, "list": [] });
    } else if data.is_object() && data["list"].is_null() {
        data["list"] = serde_json::json!([]);
    }
    Ok(data)
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
        "{}/x/v3/fav/resource/list?media_id={media_id}&pn={page}&ps={ps}&platform=web",
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
        crate::api::client::parse_json_response(&resp, "获取收藏夹内容失败")?;

    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取收藏夹内容失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    if data.is_null() {
        data = serde_json::json!({ "info": null, "medias": [], "has_more": false });
    } else if data.is_object() && data["medias"].is_null() {
        data["medias"] = serde_json::json!([]);
    }
    Ok(data)
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

fn join_media_ids(ids: &[i64]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// 添加或移除收藏夹中的视频资源 (`/x/v3/fav/resource/deal`)
pub fn deal_fav_resource(
    cookies: &Cookies,
    rid: i64,
    add_media_ids: &[i64],
    del_media_ids: &[i64],
) -> Result<serde_json::Value> {
    let mut params = vec![
        ("rid", rid.to_string()),
        ("type", "2".to_string()),
        ("csrf", cookies.bili_jct.clone()),
        ("platform", "web".to_string()),
    ];
    if !add_media_ids.is_empty() {
        params.push(("add_media_ids", join_media_ids(add_media_ids)));
    }
    if !del_media_ids.is_empty() {
        params.push(("del_media_ids", join_media_ids(del_media_ids)));
    }

    let body =
        serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let url = format!(
        "{}/x/v3/fav/resource/deal",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "收藏夹资源操作失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "收藏夹资源操作失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

/// 清理收藏夹中的失效视频 (`/x/v3/fav/resource/clean`)
pub fn clean_fav_folder(cookies: &Cookies, media_id: i64) -> Result<()> {
    let body = serde_urlencoded::to_string([
        ("media_id", media_id.to_string()),
        ("platform", "web".to_string()),
        ("csrf", cookies.bili_jct.clone()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/v3/fav/resource/clean",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "清理收藏夹失效视频失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "清理收藏夹失效视频失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(())
}

/// 新建收藏夹 (`/x/v3/fav/folder/add`)
pub fn create_fav_folder(
    cookies: &Cookies,
    title: &str,
    private: bool,
) -> Result<serde_json::Value> {
    let privacy = if private { "1" } else { "0" };
    let body = serde_urlencoded::to_string([
        ("title", title),
        ("privacy", privacy),
        ("csrf", cookies.bili_jct.as_str()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!("{}/x/v3/fav/folder/add", crate::api::client::api_base_url());
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = crate::api::client::parse_json_response(&resp, "创建收藏夹失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "创建收藏夹失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(json["data"].clone())
}

/// 删除收藏夹 (`/x/v3/fav/folder/del`)
pub fn delete_fav_folder(cookies: &Cookies, media_id: i64) -> Result<()> {
    let body = serde_urlencoded::to_string([
        ("media_ids", media_id.to_string()),
        ("platform", "web".to_string()),
        ("csrf", cookies.bili_jct.clone()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!("{}/x/v3/fav/folder/del", crate::api::client::api_base_url());
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = crate::api::client::parse_json_response(&resp, "删除收藏夹失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "删除收藏夹失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    struct RestoreApi(Option<String>);
    impl Drop for RestoreApi {
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
    fn fav_null_normalization_and_crud_endpoints() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _restore = RestoreApi(std::env::var("BT_API_BASE_URL").ok());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());

        let handle = std::thread::spawn(move || {
            let mut captured = Vec::new();
            let responses = [
                r#"{"code":0,"data":{"count":0,"list":null}}"#,
                r#"{"code":0,"data":{"info":{"id":100,"title":"Empty","media_count":0},"medias":null,"has_more":false}}"#,
                r#"{"code":0,"data":{"prompt":false}}"#,
                r#"{"code":0,"data":{"prompt":false}}"#,
                r#"{"code":0,"data":0}"#,
                r#"{"code":0,"data":{"id":200,"fid":20,"title":"NewFav"}}"#,
                r#"{"code":0,"data":0}"#,
            ];
            for resp_body in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        content_len = rest.trim().parse().unwrap_or(0);
                    }
                }
                let mut buf = vec![0u8; content_len];
                if content_len > 0 {
                    reader.read_exact(&mut buf).unwrap();
                }
                captured.push((
                    first.trim().to_string(),
                    String::from_utf8_lossy(&buf).to_string(),
                ));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body
                )
                .unwrap();
            }
            captured
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &base);
        }

        let cookies = Cookies {
            sessdata: "s".into(),
            bili_jct: "jct_fav".into(),
            ..Default::default()
        };

        let folders = get_user_fav_folders(42, Some(&cookies)).unwrap();
        assert_eq!(folders["list"], serde_json::json!([]));

        let res = get_fav_resources(100, 1, 20, Some(&cookies)).unwrap();
        assert_eq!(res["medias"], serde_json::json!([]));

        deal_fav_resource(&cookies, 555, &[100], &[]).unwrap();
        deal_fav_resource(&cookies, 555, &[], &[100]).unwrap();
        clean_fav_folder(&cookies, 100).unwrap();
        let created = create_fav_folder(&cookies, "NewFav", true).unwrap();
        assert_eq!(created["id"], 200);
        delete_fav_folder(&cookies, 200).unwrap();

        let captured = handle.join().unwrap();
        assert!(captured[2].0.starts_with("POST /x/v3/fav/resource/deal"));
        let add_form: Vec<(String, String)> = serde_urlencoded::from_str(&captured[2].1).unwrap();
        assert!(add_form.contains(&("rid".into(), "555".into())));
        assert!(add_form.contains(&("type".into(), "2".into())));
        assert!(add_form.contains(&("add_media_ids".into(), "100".into())));
        assert!(add_form.contains(&("csrf".into(), "jct_fav".into())));

        let del_form: Vec<(String, String)> = serde_urlencoded::from_str(&captured[3].1).unwrap();
        assert!(del_form.contains(&("del_media_ids".into(), "100".into())));

        assert!(captured[4].0.starts_with("POST /x/v3/fav/resource/clean"));
        let create_form: Vec<(String, String)> =
            serde_urlencoded::from_str(&captured[5].1).unwrap();
        assert!(create_form.contains(&("privacy".into(), "1".into())));

        assert!(captured[6].0.starts_with("POST /x/v3/fav/folder/del"));
        let del_folder_form: Vec<(String, String)> =
            serde_urlencoded::from_str(&captured[6].1).unwrap();
        assert!(del_folder_form.contains(&("media_ids".into(), "200".into())));
    }
}
