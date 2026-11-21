use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::{Cookies, read_cookies, update_live_key};
use crate::cli::output::{is_json, is_quiet, is_yes, print_spacer};
use crate::error::{BiliLiveError, Result};
use crate::live::stats::{fetch_live_stats_data, get_live_info};
use crate::user_warning;
use crate::utils::paths::{data_file, write_private};
use crate::utils::string::mask_rtmp_code;
use std::io::IsTerminal;

fn post_start_live_once(cookies: &Cookies, area_id: &str) -> Result<serde_json::Value> {
    let form_data = serde_urlencoded::to_string([
        ("room_id", cookies.room_id.to_string()),
        ("area_v2", area_id.to_string()),
        ("csrf", cookies.bili_jct.clone()),
        ("csrf_token", cookies.bili_jct.clone()),
        ("platform", "pc_link".to_string()),
    ])
    .map_err(|e| BiliLiveError::Parse(format!("表单编码失败: {e}")))?;

    let url = format!(
        "{}/room/v1/Room/startLive",
        crate::api::client::live_api_base_url()
    );
    if crate::cli::output::is_direct() {
        let ck = cookies.cookie_header();
        return crate::utils::direct::send_direct_json(
            "POST",
            &url,
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Cookie", &ck),
            ],
            Some(&form_data),
            "开播请求失败",
        );
    }
    let response = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(form_data)
        .send()?;

    crate::api::client::parse_json_response(&response, "开播请求失败")
}

fn extract_srt_protocol(data: &serde_json::Value) -> Option<(String, String)> {
    let protocols = data.get("protocols")?.as_array()?;
    for p in protocols {
        if p.get("protocol").and_then(|v| v.as_str()) == Some("srt") {
            let addr = p.get("addr").and_then(|v| v.as_str()).unwrap_or("").trim();
            let code = p.get("code").and_then(|v| v.as_str()).unwrap_or("").trim();
            if !addr.is_empty() && !code.is_empty() {
                return Some((addr.to_string(), code.to_string()));
            }
        }
    }
    None
}

// 调用 B站 API 开始直播，获取推流地址和推流码（含 60024 人脸识别二维码闭环与 SRT 协议提取）
pub fn start_live(area_id: &str, show_full_code: bool) -> Result<serde_json::Value> {
    let cookies = read_cookies()?;
    let mut res = post_start_live_once(&cookies, area_id)?;

    // 处理 B 站第三方开播人脸识别拦截 (code == 60024)
    if res["code"].as_i64() == Some(60024) {
        let qr_url = res["data"]["qr"].as_str().unwrap_or("").trim().to_string();
        if is_json() || is_yes() || !std::io::stdin().is_terminal() || qr_url.is_empty() {
            let suffix = if qr_url.is_empty() {
                String::new()
            } else {
                format!("；请使用哔哩哔哩 App 扫码认证后重试: {qr_url}")
            };
            return Err(BiliLiveError::Api(format!(
                "开播需要完成 B 站人脸识别认证 (code=60024){suffix}"
            )));
        }

        user_warning!(
            "触发 B 站开播人脸识别认证 (code=60024)，请使用哔哩哔哩手机客户端扫描下方二维码："
        );
        let _ = crate::utils::qrcode::print_qrcode_in_terminal(&qr_url);
        crate::user_info!("认证链接: {}", qr_url);

        let retry = dialoguer::Confirm::with_theme(&crate::ui::theme::bili_dialoguer_theme())
            .with_prompt("已在手机上完成人脸识别认证，是否立即重试开播？")
            .default(true)
            .interact()
            .map_err(|e| BiliLiveError::Input(format!("读取确认失败: {e}")))?;

        if !retry {
            return Err(BiliLiveError::Interrupted);
        }
        res = post_start_live_once(&cookies, area_id)?;
    }

    if res["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "API返回错误: {}",
            res["message"]
                .as_str()
                .or_else(|| res["msg"].as_str())
                .unwrap_or("未知错误")
        )));
    }

    // 解析 B站 返回的 RTMP 与可选 SRT 推流信息
    let rtmp_addr = res["data"]["rtmp"]["addr"].as_str().ok_or_else(|| {
        BiliLiveError::Parse("直播已开启，但响应缺少推流地址，请到直播中心获取".to_string())
    })?;
    let rtmp_code = res["data"]["rtmp"]["code"].as_str().ok_or_else(|| {
        BiliLiveError::Parse("直播已开启，但响应缺少推流码，请到直播中心获取".to_string())
    })?;
    let srt_info = extract_srt_protocol(&res["data"]);
    let direct_rtmp_addr = if crate::cli::output::is_direct() {
        crate::utils::direct::resolve_rtmp_direct_ip_url(rtmp_addr)
    } else {
        None
    };

    let mut file_content = format!("{rtmp_addr}\n{rtmp_code}\n");
    if let Some(ref d_addr) = direct_rtmp_addr {
        file_content.push_str(&format!(
            "\n# Direct RTMP (Bypass Proxy)\n{d_addr}\n{rtmp_code}\n"
        ));
    }
    if let Some((ref s_addr, ref s_code)) = srt_info {
        file_content.push_str(&format!("\n# SRT\n{s_addr}\n{s_code}\n"));
    }

    let saved_path = match data_file("stream_info.txt") {
        Ok(path) => match write_private(&path, file_content.as_bytes()) {
            Ok(()) => Some(path),
            Err(e) => {
                user_warning!("直播已开启，但推流信息保存失败: {}", e);
                if !show_full_code {
                    user_warning!("完整推流码未显示，请到直播中心获取推流信息");
                }
                None
            }
        },
        Err(e) => {
            user_warning!("直播已开启，但推流信息保存失败: {}", e);
            if !show_full_code {
                user_warning!("完整推流码未显示，请到直播中心获取推流信息");
            }
            None
        }
    };

    if !is_json() && !is_quiet() {
        use crate::ui::components::{Cell, InfoCard};
        let code_display = if show_full_code {
            rtmp_code.to_string()
        } else {
            format!("{}  (加 -s 查看完整码)", mask_rtmp_code(rtmp_code))
        };

        let mut card = InfoCard::new("🎬 直播已开启")
            .with_badge(
                format!("房间 {}", cookies.room_id),
                crossterm::style::Color::Green,
            )
            .add_row("推流地址", Cell::cyan(rtmp_addr))
            .add_plain_row("推流码", code_display);

        if let Some(ref d_addr) = direct_rtmp_addr {
            card = card.add_row("OBS免代理地址", Cell::cyan(d_addr.clone()));
            if let Some((iface, lip)) = crate::utils::direct::detect_physical_ipv4() {
                card = card.add_row(
                    "OBS绑定网卡",
                    Cell::pink(format!(
                        "{iface} ({lip}) · 在 OBS 设置→高级→网络→绑定到IP 中选择"
                    )),
                );
            }
        }

        if let Some((ref s_addr, ref s_code)) = srt_info {
            let srt_code_disp = if show_full_code {
                s_code.clone()
            } else {
                mask_rtmp_code(s_code)
            };
            card = card
                .add_row("SRT 地址", Cell::cyan(s_addr.clone()))
                .add_plain_row("SRT 推流码", srt_code_disp);
        }

        if let Some(path) = &saved_path {
            card = card.add_row("本地保存", Cell::muted(path.display().to_string()));
        }

        print_spacer();
        card.print();
        print_spacer();
    }

    let live_key = parse_live_key(&res["data"]["live_key"]);
    if live_key.is_none() && !is_json() && !is_quiet() {
        user_warning!("直播已开启，但未取得有效统计标识，本次统计不可用");
    }
    // 缺失时清空旧标识，避免显示上一次直播的数据。
    if let Err(e) = update_live_key(live_key) {
        user_warning!("直播已开启，但统计标识保存失败: {}", e);
    }

    let masked_code = mask_rtmp_code(rtmp_code);
    let mut payload = serde_json::json!({
        "success": true,
        "already_live": false,
        "room_id": cookies.room_id,
        "area_id": area_id,
        "rtmp_addr": rtmp_addr,
        "rtmp_code_masked": masked_code,
        "live_key": live_key
    });
    if show_full_code {
        payload["rtmp_code"] = serde_json::json!(rtmp_code);
    }
    if let Some((s_addr, s_code)) = srt_info {
        payload["srt_addr"] = serde_json::json!(s_addr);
        payload["srt_code_masked"] = serde_json::json!(mask_rtmp_code(&s_code));
        if show_full_code {
            payload["srt_code"] = serde_json::json!(s_code);
        }
    }
    Ok(payload)
}

// 调用 B站 API 停止直播
pub fn stop_live() -> Result<serde_json::Value> {
    let cookies = read_cookies()?;

    let form_data = serde_urlencoded::to_string([
        ("room_id", cookies.room_id.to_string()),
        ("csrf", cookies.bili_jct.clone()),
        ("csrf_token", cookies.bili_jct.clone()),
        ("platform", "pc_link".to_string()),
    ])
    .map_err(|e| BiliLiveError::Parse(format!("表单编码失败: {e}")))?;

    let url = format!(
        "{}/room/v1/Room/stopLive",
        crate::api::client::live_api_base_url()
    );
    let res: serde_json::Value = if crate::cli::output::is_direct() {
        let ck = cookies.cookie_header();
        crate::utils::direct::send_direct_json(
            "POST",
            &url,
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Cookie", &ck),
            ],
            Some(&form_data),
            "下播请求失败",
        )?
    } else {
        let response = crate::api::client::post(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_header("Cookie", cookies.cookie_header())
            .with_body(form_data)
            .send()?;
        crate::api::client::parse_json_response(&response, "下播请求失败")?
    };

    if res["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "API返回错误: {}",
            res["message"].as_str().unwrap_or("未知错误")
        )));
    }

    if !is_json() && !is_quiet() {
        crate::user_success!("🎬 直播已关闭");

        if let Some(live_key) = cookies.live_key {
            print_spacer();
            if let Err(e) = get_live_info(live_key) {
                user_warning!("直播已关闭，但统计获取失败: {}", e);
            }
            print_spacer();
        }
    }

    let mut payload = serde_json::json!({
        "success": true,
        "room_id": cookies.room_id,
        "message": "直播已关闭",
        "live_key": cookies.live_key
    });
    if is_json()
        && let Some(live_key) = cookies.live_key
        && let Ok(Some(stats)) = fetch_live_stats_data(live_key)
    {
        payload["stats"] = stats;
    }
    Ok(payload)
}

fn parse_live_key(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn live_key_accepts_string_or_number_and_rejects_invalid_values() {
        assert_eq!(parse_live_key(&json!("123")), Some(123));
        assert_eq!(parse_live_key(&json!(123)), Some(123));
        for value in [
            json!(null),
            json!(-1),
            json!("invalid"),
            json!("18446744073709551616"),
        ] {
            assert_eq!(parse_live_key(&value), None);
        }
    }

    #[test]
    fn start_and_stop_live_mask_code_and_align_platform_pc_link() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct Guard {
            live_url: Option<String>,
            xdg_data: Option<String>,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                crate::utils::paths::set_cookie_file_override(None);
                crate::cli::output::init_output(false, false, false, false, None);
                unsafe {
                    match &self.live_url {
                        Some(v) => std::env::set_var("BT_LIVE_API_BASE_URL", v),
                        None => std::env::remove_var("BT_LIVE_API_BASE_URL"),
                    }
                    match &self.xdg_data {
                        Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                        None => std::env::remove_var("XDG_DATA_HOME"),
                    }
                }
            }
        }
        let _guard = Guard {
            live_url: std::env::var("BT_LIVE_API_BASE_URL").ok(),
            xdg_data: std::env::var("XDG_DATA_HOME").ok(),
        };

        let tmp = tempfile::TempDir::new().unwrap();
        let cookie_path = tmp.path().join("cookies.json");
        let cookies = crate::auth::cookies::Cookies {
            sessdata: "sess".into(),
            bili_jct: "jct_tok".into(),
            dede_user_id: "1001".into(),
            buvid3: "b3".into(),
            buvid4: "b4".into(),
            room_id: 2233,
            ..Default::default()
        };
        cookies.save_to(&cookie_path).unwrap();
        crate::utils::paths::set_cookie_file_override(Some(cookie_path.clone()));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());

        let handle = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for idx in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first_line = String::new();
                reader.read_line(&mut first_line).unwrap();
                let mut headers = Vec::new();
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(rest) = lower.strip_prefix("content-length:") {
                        content_len = rest.trim().parse().unwrap_or(0);
                    }
                    headers.push(lower);
                }
                let mut body_bytes = vec![0u8; content_len];
                if content_len > 0 {
                    reader.read_exact(&mut body_bytes).unwrap();
                }
                let body_str = String::from_utf8_lossy(&body_bytes).to_string();
                captured.push((first_line, headers, body_str));

                let resp_body = if idx < 2 {
                    r#"{"code":0,"data":{"rtmp":{"addr":"rtmp://live-push.example.com/live/","code":"secret_stream_key_12345"},"live_key":"998877"}}"#
                } else {
                    r#"{"code":0,"data":{"change":1,"status":"PREPARING"}}"#
                };
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

        crate::cli::output::init_output(true, true, false, true, Some(cookie_path.clone()));
        unsafe {
            std::env::set_var("BT_LIVE_API_BASE_URL", &base);
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }

        let masked_res = start_live("371", false).unwrap();
        assert_eq!(masked_res["already_live"], false);
        assert!(masked_res.get("rtmp_code").is_none());
        assert!(
            masked_res["rtmp_code_masked"]
                .as_str()
                .unwrap()
                .contains('*')
        );

        let full_res = start_live("371", true).unwrap();
        assert_eq!(full_res["rtmp_code"], "secret_stream_key_12345");
        assert!(full_res["rtmp_code_masked"].as_str().unwrap().contains('*'));

        let stop_res = stop_live().unwrap();
        assert_eq!(stop_res["success"], true);

        let captured = handle.join().unwrap();
        for (idx, (first, headers, body)) in captured.iter().enumerate() {
            if idx < 2 {
                assert!(first.starts_with("POST /room/v1/Room/startLive "));
            } else {
                assert!(first.starts_with("POST /room/v1/Room/stopLive "));
            }
            assert!(
                !headers
                    .iter()
                    .any(|h| h.starts_with("platform:") && h.contains("web_electron_link")),
                "unexpected web_electron_link header in {headers:?}"
            );
            let form: Vec<(String, String)> = serde_urlencoded::from_str(body).unwrap();
            assert!(form.contains(&("platform".into(), "pc_link".into())));
            assert!(!form.contains(&("platform".into(), "web_electron_link".into())));
        }
    }
}
