use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::{Cookies, read_cookies};
use crate::error::{BiliLiveError, Result};
use std::collections::HashMap;

pub fn check_live_status(room_id: i64) -> Result<bool> {
    let url = format!(
        "{}/room/v1/Room/get_info?room_id={}",
        crate::api::client::live_api_base_url(),
        room_id
    );
    let json: serde_json::Value = if crate::cli::output::is_direct() {
        crate::utils::direct::send_direct_json("GET", &url, &[], None, "获取直播状态失败")?
    } else {
        let response = crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .send()?;
        crate::api::client::parse_json_response(&response, "获取直播状态失败")?
    };
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取直播状态失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    let live_status = json["data"]["live_status"]
        .as_i64()
        .ok_or_else(|| BiliLiveError::Parse("无法解析直播状态".to_string()))?;
    Ok(live_status == 1)
}

pub fn get_recent_live() -> Result<(String, String)> {
    let room_id = read_cookies()?.room_id;
    let url = format!(
        "{}/room/v1/Area/getMyChooseArea?roomid={}",
        crate::api::client::live_api_base_url(),
        room_id
    );
    let json: serde_json::Value = if crate::cli::output::is_direct() {
        crate::utils::direct::send_direct_json("GET", &url, &[], None, "获取上次直播分区失败")?
    } else {
        let response = crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .send()?;
        crate::api::client::parse_json_response(&response, "获取上次直播分区失败")?
    };
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取上次直播分区失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    let data = &json["data"][0];
    let id = data["id"]
        .as_str()
        .map(str::to_string)
        .or_else(|| data["id"].as_i64().map(|n| n.to_string()))
        .ok_or_else(|| BiliLiveError::Parse("无法解析分区ID".to_string()))?;
    let name = data["name"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("无法解析分区名称".to_string()))?
        .to_string();
    Ok((id, name))
}

pub fn update_title(cookies: &Cookies, title: &str) -> Result<()> {
    let form = title_form(cookies, title)?;
    let url = format!("{}/room/v1/Room/update", crate::api::client::live_api_base_url());
    let json: serde_json::Value = if crate::cli::output::is_direct() {
        let ck = cookies.cookie_header();
        crate::utils::direct::send_direct_json(
            "POST",
            &url,
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Cookie", &ck),
            ],
            Some(&form),
            "更新标题失败",
        )?
    } else {
        let response = crate::api::client::post(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_header("Cookie", cookies.cookie_header())
            .with_body(form)
            .send()?;
        crate::api::client::parse_json_response(&response, "更新标题失败")?
    };
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "更新标题失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(())
}

pub fn get_room_info(room_id: i64) -> Result<serde_json::Value> {
    let url = format!(
        "{}/room/v1/Room/get_info?room_id={}",
        crate::api::client::live_api_base_url(),
        room_id
    );
    let json: serde_json::Value = if crate::cli::output::is_direct() {
        crate::utils::direct::send_direct_json("GET", &url, &[], None, "获取直播间信息失败")?
    } else {
        let response = crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .send()?;
        crate::api::client::parse_json_response(&response, "获取直播间信息失败")?
    };
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取直播间信息失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(json["data"].clone())
}

pub fn room_init(id: i64) -> Result<serde_json::Value> {
    let url = format!(
        "{}/room/v1/Room/room_init?id={}",
        crate::api::client::live_api_base_url(),
        id
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&response, "初始化直播间信息失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "初始化直播间信息失败: {}",
            json["message"].as_str().unwrap_or("直播间不存在或参数错误")
        )));
    }
    Ok(json["data"].clone())
}

pub fn get_anchor_in_room(room_id: i64) -> Result<serde_json::Value> {
    let url = format!(
        "{}/live_user/v1/UserInfo/get_anchor_in_room?roomid={}",
        crate::api::client::live_api_base_url(),
        room_id
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&response, "获取主播信息失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取主播信息失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(json["data"].clone())
}

pub fn get_following_live(
    cookies: &Cookies,
    page: u32,
    page_size: u32,
) -> Result<serde_json::Value> {
    let url = format!(
        "{}/xlive/web-ucenter/user/following?page={}&page_size={}&ignoreRecord=1&hit_ab=true",
        crate::api::client::live_api_base_url(),
        page,
        page_size
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&response, "获取关注直播列表失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取关注直播列表失败: {}",
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

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct LiveStreamEntry {
    pub protocol: String,
    pub format: String,
    pub codec: String,
    pub current_qn: u32,
    pub quality_desc: String,
    pub accept_qn: Vec<u32>,
    pub url: String,
    pub backup_urls: Vec<String>,
}

pub fn qn_description(qn: u32) -> &'static str {
    match qn {
        30000 => "杜比",
        20000 => "4K",
        10000 => "原画",
        400 => "蓝光",
        250 => "超清",
        150 => "高清",
        80 => "流畅",
        _ => "未知",
    }
}

pub fn get_room_play_info(
    room_id: i64,
    qn: u32,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let url = format!(
        "{}/xlive/web-room/v2/index/getRoomPlayInfo?room_id={}&protocol=0,1&format=0,1,2&codec=0,1&qn={}&platform=web&ptype=8&dolby=5&panorama=1",
        crate::api::client::live_api_base_url(),
        room_id,
        qn
    );
    let mut req = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://live.bilibili.com/");
    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            req = req.with_header("Cookie", header);
        }
    }
    let response = req.send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&response, "获取直播流信息失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取直播流信息失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(json["data"].clone())
}

pub fn extract_live_streams(play_info: &serde_json::Value) -> Vec<LiveStreamEntry> {
    let playurl = &play_info["playurl_info"]["playurl"];
    if playurl.is_null() {
        return Vec::new();
    }

    let mut qn_map: HashMap<u32, String> = HashMap::new();
    if let Some(descs) = playurl["g_qn_desc"].as_array() {
        for item in descs {
            if let Some(qn) = item["qn"].as_u64()
                && let Some(desc) = item["desc"].as_str()
            {
                qn_map.insert(qn as u32, desc.to_string());
            }
        }
    }

    let mut entries = Vec::new();
    let Some(streams) = playurl["stream"].as_array() else {
        return entries;
    };

    for stream in streams {
        let protocol = stream["protocol_name"].as_str().unwrap_or("").to_string();
        let Some(formats) = stream["format"].as_array() else {
            continue;
        };
        for fmt in formats {
            let format = fmt["format_name"].as_str().unwrap_or("").to_string();
            let Some(codecs) = fmt["codec"].as_array() else {
                continue;
            };
            for cdc in codecs {
                let codec = cdc["codec_name"].as_str().unwrap_or("").to_string();
                let current_qn = cdc["current_qn"].as_u64().unwrap_or(0) as u32;
                let quality_desc = qn_map
                    .get(&current_qn)
                    .cloned()
                    .unwrap_or_else(|| qn_description(current_qn).to_string());
                let accept_qn: Vec<u32> = cdc["accept_qn"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_u64().map(|n| n as u32))
                            .collect()
                    })
                    .unwrap_or_default();
                let base_url = cdc["base_url"].as_str().unwrap_or("");
                let mut urls = Vec::new();
                if let Some(url_infos) = cdc["url_info"].as_array() {
                    for info in url_infos {
                        let host = info["host"].as_str().unwrap_or("");
                        let extra = info["extra"].as_str().unwrap_or("");
                        if host.is_empty() && base_url.is_empty() {
                            continue;
                        }
                        let assembled = if extra.is_empty() {
                            format!("{host}{base_url}")
                        } else if base_url.contains('?') {
                            let clean_extra = extra.strip_prefix('?').unwrap_or(extra);
                            if base_url.ends_with('?') || base_url.ends_with('&') || clean_extra.starts_with('&') {
                                format!("{host}{base_url}{clean_extra}")
                            } else {
                                format!("{host}{base_url}&{clean_extra}")
                            }
                        } else if extra.starts_with('?') {
                            format!("{host}{base_url}{extra}")
                        } else {
                            format!("{host}{base_url}?{extra}")
                        };
                        urls.push(assembled);
                    }
                }
                if let Some((first, rest)) = urls.split_first() {
                    entries.push(LiveStreamEntry {
                        protocol: protocol.clone(),
                        format: format.clone(),
                        codec,
                        current_qn,
                        quality_desc,
                        accept_qn,
                        url: first.clone(),
                        backup_urls: rest.to_vec(),
                    });
                }
            }
        }
    }

    entries
}

pub fn get_live_danmaku_history(room_id: i64) -> Result<serde_json::Value> {
    let url = format!(
        "{}/xlive/web-room/v1/dM/gethistory?roomid={}",
        crate::api::client::live_api_base_url(),
        room_id
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&response, "获取直播间弹幕失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取直播间弹幕失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    if data.is_null() {
        data = serde_json::json!({ "admin": [], "room": [] });
    } else if data.is_object() {
        if data["admin"].is_null() {
            data["admin"] = serde_json::json!([]);
        }
        if data["room"].is_null() {
            data["room"] = serde_json::json!([]);
        }
    }
    Ok(data)
}

fn live_danmaku_form(
    cookies: &Cookies,
    room_id: i64,
    msg: &str,
    rnd: u64,
) -> Result<String> {
    serde_urlencoded::to_string([
        ("bubble", "0".to_string()),
        ("msg", msg.to_string()),
        ("color", "16777215".to_string()),
        ("mode", "1".to_string()),
        ("fontsize", "25".to_string()),
        ("rnd", rnd.to_string()),
        ("roomid", room_id.to_string()),
        ("csrf", cookies.bili_jct.clone()),
        ("csrf_token", cookies.bili_jct.clone()),
    ])
    .map_err(|e| BiliLiveError::Parse(format!("表单编码失败: {e}")))
}

pub fn send_live_danmaku(
    cookies: &Cookies,
    room_id: i64,
    msg: &str,
) -> Result<serde_json::Value> {
    let wbi_query =
        crate::api::wbi::sign_params(&[("web_location", "444.8")], Some(&cookies.sessdata)).ok();
    let base = format!("{}/msg/send", crate::api::client::live_api_base_url());
    let url = match wbi_query {
        Some(q) if !q.is_empty() => format!("{base}?{q}"),
        _ => base,
    };

    let rnd = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let body = live_danmaku_form(cookies, room_id, msg, rnd)?;
    let referer = format!("https://live.bilibili.com/{room_id}");

    let response = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Cookie", cookies.cookie_header())
        .with_header("Referer", &referer)
        .with_header("Origin", "https://live.bilibili.com")
        .with_body(body)
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&response, "发送直播弹幕失败")?;
    if json["code"].as_i64() != Some(0) {
        let err_msg = json["message"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| json["msg"].as_str().filter(|s| !s.is_empty()))
            .unwrap_or("未知错误");
        return Err(BiliLiveError::Api(format!("发送直播弹幕失败: {err_msg}")));
    }

    for key in ["msg", "message"] {
        if let Some(token) = json[key].as_str() {
            let trimmed = token.trim();
            if matches!(trimmed, "f" | "k" | "max limit exceeded") {
                return Err(BiliLiveError::Api(format!(
                    "弹幕被系统或房间屏蔽拦截 ({trimmed})"
                )));
            }
        }
    }

    Ok(json["data"].clone())
}

#[derive(Debug, Clone)]
pub struct DanmuServerInfo {
    pub uid: i64,
    pub token: String,
    pub ws_urls: Vec<String>,
}

fn temp_guest_buvid3() -> String {
    format!("{}infoc", uuid::Uuid::new_v4().to_string().to_uppercase())
}

/// 获取直播间实时信息流 WebSocket 节点列表与认证 token（WBI 签名 + buvid3 保证）
pub fn get_danmu_info(room_id: i64, cookies: Option<&Cookies>) -> Result<DanmuServerInfo> {
    let mut uid = 0i64;
    let mut auth_cookie: Option<Cookies> = None;

    if let Some(c) = cookies
        && !c.sessdata.is_empty()
    {
        let mut cloned = c.clone();
        if cloned.buvid3.is_empty() {
            cloned.buvid3 = temp_guest_buvid3();
        }
        let parsed_uid = cloned.dede_user_id.trim().parse::<i64>().unwrap_or(0);
        if parsed_uid > 0 {
            uid = parsed_uid;
            auth_cookie = Some(cloned);
        } else if let Ok(info) = crate::api::passport::get_user_info(&cloned)
            && info["data"]["isLogin"].as_bool() == Some(true)
            && let Some(mid) = info["data"]["mid"].as_i64()
            && mid > 0
        {
            uid = mid;
            cloned.dede_user_id = mid.to_string();
            auth_cookie = Some(cloned);
        }
    }

    let cookie_header = match &auth_cookie {
        Some(c) => c.cookie_header(),
        None => format!("buvid3={}", temp_guest_buvid3()),
    };
    let sess_opt = auth_cookie.as_ref().map(|c| c.sessdata.as_str());

    let room_str = room_id.to_string();
    let params = [
        ("id", room_str.as_str()),
        ("type", "0"),
        ("web_location", "444.8"),
    ];
    let query = match crate::api::wbi::sign_params(&params, sess_opt) {
        Ok(q) => q,
        Err(_) => format!("id={room_id}&type=0&web_location=444.8"),
    };

    let url = format!(
        "{}/xlive/web-room/v1/index/getDanmuInfo?{}",
        crate::api::client::live_api_base_url(),
        query
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Cookie", cookie_header)
        .with_header("Referer", format!("https://live.bilibili.com/{room_id}"))
        .send()?;

    let json: serde_json::Value =
        crate::api::client::parse_json_response(&response, "获取弹幕服务器认证信息失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取弹幕服务器认证信息失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let data = &json["data"];
    let token = data["token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| BiliLiveError::Parse("缺少弹幕认证 token".to_string()))?
        .to_string();

    let mut ws_urls = Vec::new();
    if let Ok(override_url) = std::env::var("BT_LIVE_WS_URL") {
        let trimmed = override_url.trim();
        if !trimmed.is_empty() {
            ws_urls.push(trimmed.to_string());
        }
    }
    if let Some(hosts) = data["host_list"].as_array() {
        for item in hosts {
            if let Some(host) = item["host"].as_str()
                && !host.is_empty()
            {
                let wss_port = item["wss_port"].as_u64().unwrap_or(2245);
                let url_candidate = format!("wss://{host}:{wss_port}/sub");
                if !ws_urls.contains(&url_candidate) {
                    ws_urls.push(url_candidate);
                }
            }
        }
    }
    if ws_urls.is_empty() {
        ws_urls.push("wss://broadcastlv.chat.bilibili.com:2245/sub".to_string());
    }

    Ok(DanmuServerInfo {
        uid,
        token,
        ws_urls,
    })
}

fn title_form(cookies: &Cookies, title: &str) -> Result<String> {
    serde_urlencoded::to_string([
        ("room_id", cookies.room_id.to_string()),
        ("title", title.to_string()),
        ("csrf_token", cookies.bili_jct.clone()),
        ("csrf", cookies.bili_jct.clone()),
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
            bili_jct: "test".into(),
            ..Default::default()
        };
        let title = "C++ & 聊天=游戏 100%20完成";
        let fields: Vec<(String, String)> =
            serde_urlencoded::from_str(&title_form(&cookies, title).unwrap()).unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[1], ("title".into(), title.into()));
    }

    #[test]
    fn extract_live_streams_parses_playinfo_and_handles_offline_null() {
        let offline = serde_json::json!({
            "room_id": 14073662,
            "live_status": 0,
            "playurl_info": null
        });
        assert!(extract_live_streams(&offline).is_empty());

        let online = serde_json::json!({
            "room_id": 14073662,
            "live_status": 1,
            "playurl_info": {
                "playurl": {
                    "g_qn_desc": [
                        { "qn": 10000, "desc": "原画" },
                        { "qn": 400, "desc": "蓝光" }
                    ],
                    "stream": [
                        {
                            "protocol_name": "http_stream",
                            "format": [{
                                "format_name": "flv",
                                "codec": [{
                                    "codec_name": "avc",
                                    "current_qn": 400,
                                    "accept_qn": [10000, 400, 250],
                                    "base_url": "/live/14073662.flv?",
                                    "url_info": [
                                        { "host": "https://cdn1.example.com", "extra": "token=abc" },
                                        { "host": "https://cdn2.example.com", "extra": "token=def" }
                                    ]
                                }]
                            }]
                        }
                    ]
                }
            }
        });
        let streams = extract_live_streams(&online);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].protocol, "http_stream");
        assert_eq!(streams[0].format, "flv");
        assert_eq!(streams[0].codec, "avc");
        assert_eq!(streams[0].current_qn, 400);
        assert_eq!(streams[0].quality_desc, "蓝光");
        assert_eq!(streams[0].url, "https://cdn1.example.com/live/14073662.flv?token=abc");
        assert_eq!(
            streams[0].backup_urls,
            vec!["https://cdn2.example.com/live/14073662.flv?token=def".to_string()]
        );
    }

    #[test]
    fn send_live_danmaku_checks_silent_filter_and_form_fields() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct RestoreLive(Option<String>, Option<String>);
        impl Drop for RestoreLive {
            fn drop(&mut self) {
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("BT_LIVE_API_BASE_URL", v),
                        None => std::env::remove_var("BT_LIVE_API_BASE_URL"),
                    }
                    match &self.1 {
                        Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                        None => std::env::remove_var("BT_API_BASE_URL"),
                    }
                }
            }
        }
        let _restore = RestoreLive(
            std::env::var("BT_LIVE_API_BASE_URL").ok(),
            std::env::var("BT_API_BASE_URL").ok(),
        );

        let api_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api_base = format!("http://{}", api_listener.local_addr().unwrap());
        let api_handle = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = api_listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 0 && line != "\r\n" {
                    line.clear();
                }
                let body = r#"{"code":-101}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        let live_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let live_base = format!("http://{}", live_listener.local_addr().unwrap());
        let live_handle = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for idx in 0..2 {
                let (mut stream, _) = live_listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                assert!(first.starts_with("POST /msg/send"));
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
                reader.read_exact(&mut buf).unwrap();
                bodies.push(String::from_utf8_lossy(&buf).to_string());

                let resp = if idx == 0 {
                    r#"{"code":0,"message":"","msg":""}"#
                } else {
                    r#"{"code":0,"message":"","msg":"f"}"#
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp.len(),
                    resp
                )
                .unwrap();
            }
            bodies
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &api_base);
            std::env::set_var("BT_LIVE_API_BASE_URL", &live_base);
        }

        let cookies = Cookies {
            sessdata: "s".into(),
            bili_jct: "jct_123".into(),
            ..Default::default()
        };
        assert!(send_live_danmaku(&cookies, 14073662, "hello").is_ok());
        let err = send_live_danmaku(&cookies, 14073662, "blocked").unwrap_err();
        assert!(err.to_string().contains("拦截"));

        api_handle.join().unwrap();
        let bodies = live_handle.join().unwrap();
        let form: Vec<(String, String)> = serde_urlencoded::from_str(&bodies[0]).unwrap();
        assert!(form.contains(&("roomid".into(), "14073662".into())));
        assert!(form.contains(&("msg".into(), "hello".into())));
        assert!(form.contains(&("csrf".into(), "jct_123".into())));
        assert!(form.contains(&("csrf_token".into(), "jct_123".into())));
        assert!(form.contains(&("fontsize".into(), "25".into())));
        assert!(form.contains(&("color".into(), "16777215".into())));
    }
}
