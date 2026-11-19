use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};
use base64::{Engine as _, engine::general_purpose};
use flate2::read::ZlibDecoder;
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::Message;
use tungstenite::stream::MaybeTlsStream;

pub const OP_HEARTBEAT: u32 = 2;
pub const OP_HEARTBEAT_REPLY: u32 = 3;
pub const OP_SEND_MSG_REPLY: u32 = 5;
pub const OP_AUTH: u32 = 7;
pub const OP_AUTH_REPLY: u32 = 8;

const MAX_DECOMPRESSED_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractKind {
    Enter,
    Follow,
    Share,
}

impl InteractKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Enter => "进入直播间",
            Self::Follow => "关注了主播",
            Self::Share => "分享了直播间",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LiveEvent {
    Connected {
        url: String,
    },
    Disconnected {
        reason: String,
    },
    Danmaku {
        time: String,
        uid: i64,
        uname: String,
        text: String,
        medal: Option<(String, u32)>,
        guard_level: u8,
        is_admin: bool,
        color_rgb: u32,
        is_self: bool,
    },
    SuperChat {
        time: String,
        uname: String,
        price: i64,
        text: String,
    },
    Gift {
        time: String,
        uname: String,
        action: String,
        gift_name: String,
        num: u64,
    },
    GuardBuy {
        time: String,
        uname: String,
        guard_name: String,
        num: u64,
    },
    Interact {
        time: String,
        uname: String,
        kind: InteractKind,
        medal: Option<(String, u32)>,
    },
    RoomStats {
        online: Option<u64>,
        watched: Option<String>,
        likes: Option<u64>,
        popularity: Option<u32>,
    },
    RoomMeta {
        title: Option<String>,
        area_name: Option<String>,
        is_live: Option<bool>,
    },
}

pub type MedalBadge = (String, u32);

#[derive(Debug, Clone)]
pub enum DecodedFrame {
    AuthReply(Value),
    Popularity(u32),
    Command(Value),
}

pub fn now_hhmmss() -> String {
    let now = chrono::Utc::now();
    let beijing = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    now.with_timezone(&beijing).format("%H:%M:%S").to_string()
}

/// 组装 16 字节大端头部的 B 站直播信息流封包
pub fn encode_ws_packet(proto_ver: u16, op: u32, seq: u32, body: &[u8]) -> Vec<u8> {
    let packet_len = (16 + body.len()) as u32;
    let mut out = Vec::with_capacity(packet_len as usize);
    out.extend_from_slice(&packet_len.to_be_bytes());
    out.extend_from_slice(&16u16.to_be_bytes());
    out.extend_from_slice(&proto_ver.to_be_bytes());
    out.extend_from_slice(&op.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// 递归解包 B 站直播二进制数据帧（支持未压缩 0/1、zlib 压缩 2、brotli 压缩 3）
pub fn decode_ws_packets(data: &[u8]) -> Result<Vec<DecodedFrame>> {
    decode_ws_packets_depth(data, 0)
}

fn decode_ws_packets_depth(data: &[u8], depth: u8) -> Result<Vec<DecodedFrame>> {
    if depth > 4 {
        return Err(BiliLiveError::Parse("弹幕封包嵌套层数过深".to_string()));
    }
    let mut frames = Vec::new();
    let mut offset = 0usize;

    while offset + 16 <= data.len() {
        let packet_len = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        let header_len = u16::from_be_bytes(data[offset + 4..offset + 6].try_into().unwrap()) as usize;
        let proto_ver = u16::from_be_bytes(data[offset + 6..offset + 8].try_into().unwrap());
        let op = u32::from_be_bytes(data[offset + 8..offset + 12].try_into().unwrap());

        if packet_len < header_len || header_len < 16 || offset + packet_len > data.len() {
            break;
        }

        let body = &data[offset + header_len..offset + packet_len];
        match proto_ver {
            2 => {
                let mut decoder = ZlibDecoder::new(body).take(MAX_DECOMPRESSED_BYTES);
                let mut decompressed = Vec::new();
                if decoder.read_to_end(&mut decompressed).is_ok()
                    && let Ok(sub) = decode_ws_packets_depth(&decompressed, depth + 1)
                {
                    frames.extend(sub);
                }
            }
            3 => {
                let mut reader = brotli_decompressor::Decompressor::new(body, 4096)
                    .take(MAX_DECOMPRESSED_BYTES);
                let mut decompressed = Vec::new();
                if reader.read_to_end(&mut decompressed).is_ok()
                    && let Ok(sub) = decode_ws_packets_depth(&decompressed, depth + 1)
                {
                    frames.extend(sub);
                }
            }
            _ => match op {
                OP_AUTH_REPLY => {
                    if let Ok(val) = serde_json::from_slice::<Value>(body) {
                        frames.push(DecodedFrame::AuthReply(val));
                    }
                }
                OP_HEARTBEAT_REPLY => {
                    if body.len() >= 4 {
                        let pop = u32::from_be_bytes(body[0..4].try_into().unwrap());
                        frames.push(DecodedFrame::Popularity(pop));
                    }
                }
                OP_SEND_MSG_REPLY => {
                    if let Ok(val) = serde_json::from_slice::<Value>(body) {
                        frames.push(DecodedFrame::Command(val));
                    }
                }
                _ => {}
            },
        }

        offset += packet_len;
    }

    Ok(frames)
}

/// 纯 Rust 轻量解码 INTERACT_WORD_V2 的 base64 protobuf 负载（零 protoc 依赖）
pub fn decode_interact_word_v2(
    b64_pb: &str,
) -> Option<(String, InteractKind, Option<MedalBadge>)> {
    let bytes = general_purpose::STANDARD.decode(b64_pb.trim()).ok()?;
    let mut pos = 0usize;
    let mut uname = String::new();
    let mut uinfo_uname = String::new();
    let mut msg_type = 0u64;
    let mut medal: Option<(String, u32)> = None;

    while pos < bytes.len() {
        let tag = read_varint(&bytes, &mut pos)?;
        let field_num = tag >> 3;
        let wire_type = tag & 0x07;

        match (field_num, wire_type) {
            (2, 2) => {
                let slice = read_len_delimited(&bytes, &mut pos)?;
                if let Ok(s) = std::str::from_utf8(slice) {
                    uname = s.to_string();
                }
            }
            (5, 0) => {
                msg_type = read_varint(&bytes, &mut pos)?;
            }
            (9, 2) => {
                let slice = read_len_delimited(&bytes, &mut pos)?;
                medal = decode_pb_fans_medal(slice);
            }
            (22, 2) => {
                let slice = read_len_delimited(&bytes, &mut pos)?;
                if let Some(u) = decode_pb_uinfo_name(slice) {
                    uinfo_uname = u;
                }
            }
            _ => skip_pb_field(&bytes, &mut pos, wire_type)?,
        }
    }

    let final_name = if !uname.is_empty() {
        uname
    } else if !uinfo_uname.is_empty() {
        uinfo_uname
    } else {
        return None;
    };

    let kind = match msg_type {
        1 => InteractKind::Enter,
        2 => InteractKind::Follow,
        3 => InteractKind::Share,
        _ => return None,
    };

    Some((final_name, kind, medal))
}

fn decode_pb_fans_medal(buf: &[u8]) -> Option<(String, u32)> {
    let mut pos = 0usize;
    let mut level = 0u32;
    let mut name = String::new();
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        match (tag >> 3, tag & 0x07) {
            (2, 0) => level = read_varint(buf, &mut pos)? as u32,
            (3, 2) => {
                let s = read_len_delimited(buf, &mut pos)?;
                if let Ok(txt) = std::str::from_utf8(s) {
                    name = txt.to_string();
                }
            }
            (_, wt) => skip_pb_field(buf, &mut pos, wt)?,
        }
    }
    if !name.is_empty() && level > 0 {
        Some((name, level))
    } else {
        None
    }
}

fn decode_pb_uinfo_name(buf: &[u8]) -> Option<String> {
    let mut pos = 0usize;
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        if (tag >> 3) == 2 && (tag & 0x07) == 2 {
            let base = read_len_delimited(buf, &mut pos)?;
            let mut bpos = 0usize;
            while bpos < base.len() {
                let btag = read_varint(base, &mut bpos)?;
                if (btag >> 3) == 1 && (btag & 0x07) == 2 {
                    let s = read_len_delimited(base, &mut bpos)?;
                    return std::str::from_utf8(s).ok().map(str::to_string);
                }
                skip_pb_field(base, &mut bpos, btag & 0x07)?;
            }
        } else {
            skip_pb_field(buf, &mut pos, tag & 0x07)?;
        }
    }
    None
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    while *pos < buf.len() {
        let b = buf[*pos];
        *pos += 1;
        val |= ((b & 0x7f) as u64) << shift;
        if (b & 0x80) == 0 {
            return Some(val);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

fn read_len_delimited<'a>(buf: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    let len = read_varint(buf, pos)? as usize;
    if *pos + len > buf.len() {
        return None;
    }
    let slice = &buf[*pos..*pos + len];
    *pos += len;
    Some(slice)
}

fn skip_pb_field(buf: &[u8], pos: &mut usize, wire_type: u64) -> Option<()> {
    match wire_type {
        0 => {
            read_varint(buf, pos)?;
            Some(())
        }
        1 => {
            if *pos + 8 > buf.len() {
                None
            } else {
                *pos += 8;
                Some(())
            }
        }
        2 => {
            read_len_delimited(buf, pos)?;
            Some(())
        }
        5 => {
            if *pos + 4 > buf.len() {
                None
            } else {
                *pos += 4;
                Some(())
            }
        }
        _ => None,
    }
}

/// 将 B 站下行命令 JSON 解析为统一的强类型 `LiveEvent`
pub fn parse_live_command(json: &Value, self_uid: i64) -> Option<LiveEvent> {
    let cmd = json["cmd"].as_str()?;
    let time = now_hhmmss();

    if cmd.starts_with("DANMU_MSG") {
        let info = json["info"].as_array()?;
        let text = info.get(1)?.as_str()?.to_string();
        let meta = info.first().and_then(Value::as_array);
        let color_rgb = meta
            .and_then(|m| m.get(3))
            .and_then(Value::as_u64)
            .unwrap_or(0xFFFFFF) as u32;

        let sender_arr = info.get(2).and_then(Value::as_array);
        let uid = sender_arr
            .and_then(|a| a.first())
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let mut uname = sender_arr
            .and_then(|a| a.get(1))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if (uname.is_empty() || uname.contains('*'))
            && let Some(extra_name) = meta
                .and_then(|m| m.get(15))
                .and_then(|e| e["user"]["base"]["name"].as_str())
                .filter(|s| !s.is_empty())
        {
            uname = extra_name.to_string();
        }
        if uname.is_empty() {
            uname = "观众".to_string();
        }

        let is_admin = sender_arr
            .and_then(|a| a.get(2))
            .and_then(Value::as_i64)
            == Some(1);

        let medal = info
            .get(3)
            .and_then(Value::as_array)
            .and_then(|m| {
                let lv = m.first()?.as_u64()? as u32;
                let name = m.get(1)?.as_str()?;
                if !name.is_empty() && lv > 0 {
                    Some((name.to_string(), lv))
                } else {
                    None
                }
            });

        let guard_level = info
            .get(7)
            .and_then(Value::as_u64)
            .unwrap_or(0) as u8;

        return Some(LiveEvent::Danmaku {
            time,
            uid,
            uname,
            text,
            medal,
            guard_level,
            is_admin,
            color_rgb,
            is_self: self_uid > 0 && uid == self_uid,
        });
    }

    if cmd.starts_with("SUPER_CHAT_MESSAGE") {
        let data = &json["data"];
        let uname = data["user_info"]["uname"]
            .as_str()
            .unwrap_or("观众")
            .to_string();
        let price = data["price"].as_i64().unwrap_or(0);
        let text = data["message"].as_str().unwrap_or("").to_string();
        if !text.is_empty() {
            return Some(LiveEvent::SuperChat {
                time,
                uname,
                price,
                text,
            });
        }
    }

    if cmd.starts_with("SEND_GIFT") {
        let data = &json["data"];
        let uname = data["uname"].as_str().unwrap_or("观众").to_string();
        let action = data["action"].as_str().unwrap_or("投喂").to_string();
        let gift_name = data["giftName"]
            .as_str()
            .or_else(|| data["gift_name"].as_str())
            .unwrap_or("礼物")
            .to_string();
        let num = data["num"].as_u64().unwrap_or(1).max(1);
        return Some(LiveEvent::Gift {
            time,
            uname,
            action,
            gift_name,
            num,
        });
    }

    if cmd.starts_with("COMBO_SEND") {
        let data = &json["data"];
        let uname = data["uname"].as_str().unwrap_or("观众").to_string();
        let action = data["action"].as_str().unwrap_or("连击投喂").to_string();
        let gift_name = data["gift_name"]
            .as_str()
            .or_else(|| data["giftName"].as_str())
            .unwrap_or("礼物")
            .to_string();
        let num = data["combo_num"]
            .as_u64()
            .or_else(|| data["total_num"].as_u64())
            .unwrap_or(1)
            .max(1);
        return Some(LiveEvent::Gift {
            time,
            uname,
            action,
            gift_name,
            num,
        });
    }

    if cmd == "GUARD_BUY" {
        let data = &json["data"];
        let uname = data["username"].as_str().unwrap_or("观众").to_string();
        let guard_name = data["gift_name"].as_str().unwrap_or("舰长").to_string();
        let num = data["num"].as_u64().unwrap_or(1).max(1);
        return Some(LiveEvent::GuardBuy {
            time,
            uname,
            guard_name,
            num,
        });
    }

    if cmd == "INTERACT_WORD" {
        let data = &json["data"];
        let uname = data["uname"].as_str().filter(|s| !s.is_empty())?.to_string();
        let msg_type = data["msg_type"]
            .as_u64()
            .or_else(|| data["msg_type"].as_str()?.parse().ok())?;
        let kind = match msg_type {
            1 => InteractKind::Enter,
            2 => InteractKind::Follow,
            3 => InteractKind::Share,
            _ => return None,
        };
        let medal = data["fans_medal"]["medal_name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .and_then(|name| {
                let lv = data["fans_medal"]["medal_level"].as_u64()? as u32;
                if lv > 0 {
                    Some((name.to_string(), lv))
                } else {
                    None
                }
            });
        return Some(LiveEvent::Interact {
            time,
            uname,
            kind,
            medal,
        });
    }

    if cmd.starts_with("INTERACT_WORD_V2") {
        let b64_pb = json["data"]["pb"].as_str()?;
        let (uname, kind, medal) = decode_interact_word_v2(b64_pb)?;
        return Some(LiveEvent::Interact {
            time,
            uname,
            kind,
            medal,
        });
    }

    if cmd == "WATCHED_CHANGE" {
        let watched = json["data"]["text_large"]
            .as_str()
            .or_else(|| json["data"]["text_small"].as_str())
            .map(str::to_string)
            .or_else(|| json["data"]["num"].as_u64().map(|n| n.to_string()));
        return Some(LiveEvent::RoomStats {
            online: None,
            watched,
            likes: None,
            popularity: None,
        });
    }

    if cmd == "ONLINE_RANK_COUNT" {
        let online = json["data"]["online_count"]
            .as_u64()
            .or_else(|| json["data"]["count"].as_u64());
        return Some(LiveEvent::RoomStats {
            online,
            watched: None,
            likes: None,
            popularity: None,
        });
    }

    if cmd == "LIKE_INFO_V3_UPDATE" {
        let likes = json["data"]["click_count"].as_u64();
        return Some(LiveEvent::RoomStats {
            online: None,
            watched: None,
            likes,
            popularity: None,
        });
    }

    if cmd == "ROOM_CHANGE" {
        let data = &json["data"];
        let title = data["title"].as_str().map(str::to_string);
        let area_name = data["area_name"].as_str().map(str::to_string);
        return Some(LiveEvent::RoomMeta {
            title,
            area_name,
            is_live: None,
        });
    }

    if cmd == "LIVE" {
        return Some(LiveEvent::RoomMeta {
            title: None,
            area_name: None,
            is_live: Some(true),
        });
    }

    if cmd == "PREPARING" {
        return Some(LiveEvent::RoomMeta {
            title: None,
            area_name: None,
            is_live: Some(false),
        });
    }

    None
}

fn set_ws_read_timeout(stream: &MaybeTlsStream<TcpStream>, timeout: Option<Duration>) {
    match stream {
        MaybeTlsStream::Plain(tcp) => {
            let _ = tcp.set_read_timeout(timeout);
        }
        MaybeTlsStream::Rustls(tls) => {
            let _ = tls.get_ref().set_read_timeout(timeout);
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

fn connect_bili_ws(
    ws_url: &str,
    room_id: i64,
) -> Result<tungstenite::WebSocket<MaybeTlsStream<TcpStream>>> {
    use std::net::ToSocketAddrs;

    let parsed = url::Url::parse(ws_url)
        .map_err(|e| BiliLiveError::Parse(format!("WebSocket URL 无效 ({ws_url}): {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| BiliLiveError::Parse(format!("WebSocket URL 缺少主机名: {ws_url}")))?;
    let default_port = if parsed.scheme() == "wss" { 443 } else { 80 };
    let port = parsed.port().unwrap_or(default_port);

    let addr = (host, port)
        .to_socket_addrs()
        .map_err(BiliLiveError::Io)?
        .next()
        .ok_or_else(|| BiliLiveError::Api(format!("无法解析弹幕节点地址: {host}:{port}")))?;

    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(6))
        .map_err(|e| BiliLiveError::Api(format!("连接弹幕节点超时 ({host}:{port}): {e}")))?;
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(10)));

    let mut req = ws_url
        .into_client_request()
        .map_err(|e| BiliLiveError::Parse(format!("WebSocket 请求构建失败 ({ws_url}): {e}")))?;
    if let Ok(ua) = DEFAULT_USER_AGENT.parse() {
        req.headers_mut().insert("User-Agent", ua);
    }
    if let Ok(origin) = "https://live.bilibili.com".parse() {
        req.headers_mut().insert("Origin", origin);
    }
    let _ = room_id;

    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls_cfg = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let connector = tungstenite::Connector::Rustls(Arc::new(tls_cfg));

    let (socket, _) = tungstenite::client_tls_with_config(req, tcp, None, Some(connector))
        .map_err(|e| BiliLiveError::Api(format!("连接弹幕 WebSocket 失败 ({ws_url}): {e}")))?;

    Ok(socket)
}

/// 启动后台直播间弹幕 WebSocket 监听线程（支持自动心跳与断线重连）
pub fn spawn_live_ws_listener(
    room_id: i64,
    cookies: Option<Cookies>,
    stop_flag: Arc<AtomicBool>,
    tx: Sender<LiveEvent>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut attempt = 0u32;
        while !stop_flag.load(Ordering::Relaxed) {
            let server_info = match crate::api::live::get_danmu_info(room_id, cookies.as_ref()) {
                Ok(info) => info,
                Err(e) => {
                    let _ = tx.send(LiveEvent::Disconnected {
                        reason: e.to_string(),
                    });
                    attempt = attempt.saturating_add(1);
                    if sleep_interruptible(
                        Duration::from_secs((attempt as u64 * 3).clamp(2, 15)),
                        &stop_flag,
                    ) {
                        break;
                    }
                    continue;
                }
            };

            let mut connected_any = false;
            for ws_url in &server_info.ws_urls {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(mut ws) = connect_bili_ws(ws_url, room_id) else {
                    continue;
                };

                let auth_json = serde_json::json!({
                    "uid": server_info.uid,
                    "roomid": room_id,
                    "protover": 3,
                    "platform": "web",
                    "type": 2,
                    "key": server_info.token,
                });
                let auth_pkt = encode_ws_packet(1, OP_AUTH, 1, auth_json.to_string().as_bytes());
                if ws.send(Message::Binary(auth_pkt.into())).is_err() {
                    continue;
                }

                // 立即发首帧心跳获取房间人气值
                let hb_pkt = encode_ws_packet(1, OP_HEARTBEAT, 2, b"[object Object]");
                let _ = ws.send(Message::Binary(hb_pkt.into()));

                set_ws_read_timeout(ws.get_ref(), Some(Duration::from_millis(120)));
                connected_any = true;
                attempt = 0;
                let _ = tx.send(LiveEvent::Connected {
                    url: ws_url.clone(),
                });

                let mut seq = 3u32;
                let mut last_hb = Instant::now();

                while !stop_flag.load(Ordering::Relaxed) {
                    if last_hb.elapsed() >= Duration::from_secs(25) {
                        let pkt = encode_ws_packet(1, OP_HEARTBEAT, seq, b"[object Object]");
                        seq = seq.wrapping_add(1);
                        if ws.send(Message::Binary(pkt.into())).is_err() {
                            let _ = tx.send(LiveEvent::Disconnected {
                                reason: "心跳发送失败，正在重连...".to_string(),
                            });
                            break;
                        }
                        last_hb = Instant::now();
                    }

                    match ws.read() {
                        Ok(Message::Binary(bytes)) => {
                            if let Ok(frames) = decode_ws_packets(&bytes) {
                                for frame in frames {
                                    match frame {
                                        DecodedFrame::AuthReply(val) => {
                                            if val["code"].as_i64() != Some(0) {
                                                let _ = tx.send(LiveEvent::Disconnected {
                                                    reason: format!("弹幕认证被拒: {val}"),
                                                });
                                            }
                                        }
                                        DecodedFrame::Popularity(pop) => {
                                            let _ = tx.send(LiveEvent::RoomStats {
                                                online: None,
                                                watched: None,
                                                likes: None,
                                                popularity: Some(pop),
                                            });
                                        }
                                        DecodedFrame::Command(cmd_json) => {
                                            if let Some(ev) =
                                                parse_live_command(&cmd_json, server_info.uid)
                                                && tx.send(ev).is_err()
                                            {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Ok(Message::Ping(payload)) => {
                            let _ = ws.send(Message::Pong(payload));
                        }
                        Ok(Message::Close(_)) => {
                            let _ = tx.send(LiveEvent::Disconnected {
                                reason: "弹幕连接已关闭，准备重连...".to_string(),
                            });
                            break;
                        }
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(ref io_err))
                            if matches!(
                                io_err.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(e) => {
                            let _ = tx.send(LiveEvent::Disconnected {
                                reason: format!("弹幕连接中断 ({e})，准备重连..."),
                            });
                            break;
                        }
                    }
                }

                let _ = ws.close(None);
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                break;
            }

            if !connected_any {
                let _ = tx.send(LiveEvent::Disconnected {
                    reason: "所有弹幕服务器节点均连接失败，稍后重试...".to_string(),
                });
            }

            attempt = attempt.saturating_add(1);
            if sleep_interruptible(
                Duration::from_secs((attempt as u64 * 2).clamp(2, 10)),
                &stop_flag,
            ) {
                break;
            }
        }
    })
}

fn sleep_interruptible(dur: Duration, stop_flag: &AtomicBool) -> bool {
    let start = Instant::now();
    while start.elapsed() < dur {
        if stop_flag.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    stop_flag.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    #[test]
    fn encode_and_decode_zlib_nested_packets() {
        let cmd1 = serde_json::json!({
            "cmd": "DANMU_MSG",
            "info": [
                [0, 1, 25, 16777215, 1700000000],
                "前排支持主播！",
                [12345, "测试观众", 0],
                [12, "小电视", "主播", 31686846],
                [],
                [],
                0,
                3
            ]
        });
        let cmd2 = serde_json::json!({
            "cmd": "WATCHED_CHANGE",
            "data": { "num": 888, "text_large": "888人看过" }
        });

        let inner1 = encode_ws_packet(0, OP_SEND_MSG_REPLY, 1, cmd1.to_string().as_bytes());
        let inner2 = encode_ws_packet(0, OP_SEND_MSG_REPLY, 2, cmd2.to_string().as_bytes());
        let mut combined = Vec::new();
        combined.extend_from_slice(&inner1);
        combined.extend_from_slice(&inner2);

        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&combined).unwrap();
        let compressed = enc.finish().unwrap();

        let outer = encode_ws_packet(2, OP_SEND_MSG_REPLY, 1, &compressed);
        let decoded = decode_ws_packets(&outer).unwrap();
        assert_eq!(decoded.len(), 2);

        if let DecodedFrame::Command(v) = &decoded[0] {
            let ev = parse_live_command(v, 12345).unwrap();
            match ev {
                LiveEvent::Danmaku {
                    uname,
                    text,
                    medal,
                    guard_level,
                    is_self,
                    ..
                } => {
                    assert_eq!(uname, "测试观众");
                    assert_eq!(text, "前排支持主播！");
                    assert_eq!(medal, Some(("小电视".to_string(), 12)));
                    assert_eq!(guard_level, 3);
                    assert!(is_self);
                }
                _ => panic!("expected Danmaku"),
            }
        } else {
            panic!("expected Command");
        }
    }

    #[test]
    fn decode_interact_word_v2_protobuf_wire() {
        // Construct minimal protobuf: field 1 (uid=100), field 2 (uname="路人甲"), field 5 (msg_type=1)
        let mut pb = Vec::new();
        pb.extend_from_slice(&[0x08, 100]); // uid = 100
        let name = "路人甲".as_bytes();
        pb.push(0x12);
        pb.push(name.len() as u8);
        pb.extend_from_slice(name);
        pb.extend_from_slice(&[0x28, 1]); // msg_type = 1 (Enter)

        let b64 = general_purpose::STANDARD.encode(&pb);
        let (uname, kind, medal) = decode_interact_word_v2(&b64).unwrap();
        assert_eq!(uname, "路人甲");
        assert_eq!(kind, InteractKind::Enter);
        assert!(medal.is_none());
    }
}
