#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tempfile::TempDir;

pub const MOCK_RSA_PUBLIC_KEY_PEM: &str = "\
-----BEGIN PUBLIC KEY-----\n\
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDgJYesVbzKivDebo1nL4q2Gjgu\n\
awahawsPntlCyfWiC0G0NqYiQ6EKk71SIQ2Ag7ITe/Uf50ubdBwaI0aT/xMnvYLo\n\
LbaIHrpNxZka5iu+9/Oar+oz0dd68891KHg8VQvLeCS+Fug5wkuiDuxcu/GgUYft\n\
sQ6XGkvIhbz1ZzNLdwIDAQAB\n\
-----END PUBLIC KEY-----\n";

pub const MOCK_JPEG_BYTES: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x02, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xFF, 0xFE, 0x00, 0x0F, 0x4C, 0x61, 0x76, 0x63, 0x36, 0x33, 0x2E, 0x31,
    0x2E, 0x31, 0x30, 0x32, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
    0x06, 0x06, 0x06, 0x07, 0x07, 0x07, 0x08, 0x08, 0x08, 0x07, 0x07, 0x07, 0x06, 0x06, 0x07, 0x07,
    0x08, 0x08, 0x08, 0x08, 0x09, 0x09, 0x09, 0x08, 0x08, 0x08, 0x08, 0x09, 0x09, 0x0A, 0x0A, 0x0A,
    0x0C, 0x0C, 0x0B, 0x0B, 0x0E, 0x0E, 0x0E, 0x11, 0x11, 0x14, 0xFF, 0xC4, 0x00, 0x4B, 0x00, 0x01,
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x08, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x11, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x02, 0x00, 0x02,
    0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01,
    0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00, 0x9F, 0xC0, 0x07, 0xFF, 0xD9,
];

pub const MOCK_MP4_BYTES: &[u8] = &[
    0x00, 0x00, 0x00, 0x18, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0x00, 0x00, 0x02, 0x00,
    b'i', b's', b'o', b'm', b'm', b'p', b'4', b'1', 0x00, 0x00, 0x00, 0x10, b'm', b'd', b'a', b't',
    b'M', b'O', b'C', b'K', b'M', b'P', b'4', 0x00,
];

pub const MOCK_FLV_BYTES: &[u8] = &[
    0x46, 0x4C, 0x56, 0x01, 0x01, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00,
    0xB7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x0A, 0x6F, 0x6E, 0x4D, 0x65, 0x74,
    0x61, 0x44, 0x61, 0x74, 0x61, 0x08, 0x00, 0x00, 0x00, 0x08, 0x00, 0x08, 0x64, 0x75, 0x72, 0x61,
    0x74, 0x69, 0x6F, 0x6E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x77,
    0x69, 0x64, 0x74, 0x68, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x68,
    0x65, 0x69, 0x67, 0x68, 0x74, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0D,
    0x76, 0x69, 0x64, 0x65, 0x6F, 0x64, 0x61, 0x74, 0x61, 0x72, 0x61, 0x74, 0x65, 0x00, 0x40, 0x68,
    0x6A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x66, 0x72, 0x61, 0x6D, 0x65, 0x72, 0x61, 0x74,
    0x65, 0x00, 0x40, 0x39, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x76, 0x69, 0x64, 0x65,
    0x6F, 0x63, 0x6F, 0x64, 0x65, 0x63, 0x69, 0x64, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x07, 0x65, 0x6E, 0x63, 0x6F, 0x64, 0x65, 0x72, 0x02, 0x00, 0x0C, 0x4C, 0x61, 0x76,
    0x66, 0x36, 0x33, 0x2E, 0x31, 0x2E, 0x31, 0x30, 0x32, 0x00, 0x08, 0x66, 0x69, 0x6C, 0x65, 0x73,
    0x69, 0x7A, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00,
    0x00, 0x00, 0xC2, 0x09, 0x00, 0x00, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x00,
    0x00, 0x84, 0x00, 0x01, 0x01, 0x11, 0x26, 0x20, 0x20, 0x20, 0x21, 0xFF, 0xFE, 0x00, 0x00, 0x00,
    0x1A,
];

/// Pure-std RFC-1951 uncompressed stored-block encoder.
pub fn deflate_raw_stored(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0x01, 0x00, 0x00, 0xFF, 0xFF];
    }
    let mut out = Vec::with_capacity(data.len() + (data.len() / 65535 + 1) * 5);
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + 65535).min(data.len());
        let chunk = &data[offset..end];
        let is_final = end == data.len();
        out.push(if is_final { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
        offset = end;
    }
    out
}

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub raw_query: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub form_params: Vec<(String, String)>,
}

impl RecordedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        let target = name.to_ascii_lowercase();
        self.headers
            .iter()
            .rev()
            .find(|(k, _)| k == &target)
            .map(|(_, v)| v.as_str())
    }

    pub fn query_param(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn form_param(&self, key: &str) -> Option<&str> {
        self.form_params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn body_str(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap_or("")
    }

    pub fn has_wbi_signature(&self) -> bool {
        self.query_param("wts").is_some_and(|v| !v.is_empty())
            && self.query_param("w_rid").is_some_and(|v| v.len() == 32)
    }
}

#[derive(Debug, Clone)]
pub enum FaultConfig {
    HttpStatus {
        status: u16,
        content_type: String,
        body: Vec<u8>,
        headers: Vec<(String, String)>,
    },
    JsonBody(Value),
    ApiError {
        code: i64,
        message: String,
        data: Value,
    },
    EmptyBody,
    MalformedJson,
    Redirect {
        status: u16,
        location: Option<String>,
    },
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self::data_null()
    }
}

impl FaultConfig {
    pub fn http(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::HttpStatus {
            status,
            content_type: "application/json; charset=utf-8".to_string(),
            body: body.into(),
            headers: Vec::new(),
        }
    }

    pub fn api_code(code: i64, message: &str) -> Self {
        Self::ApiError {
            code,
            message: message.to_string(),
            data: Value::Null,
        }
    }

    pub fn data_null() -> Self {
        Self::JsonBody(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": null
        }))
    }

    pub fn json(val: Value) -> Self {
        Self::JsonBody(val)
    }

    pub fn redirect(location: &str) -> Self {
        Self::Redirect {
            status: 302,
            location: Some(location.to_string()),
        }
    }

    pub fn missing_location() -> Self {
        Self::Redirect {
            status: 200,
            location: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrMockStatus {
    Waiting,
    Scanned,
    Expired,
    Success,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoSignMode {
    Success,
    AlreadySigned,
    Retired,
}

#[derive(Debug, Clone)]
pub struct MockRoom {
    pub room_id: i64,
    pub short_id: i64,
    pub uid: i64,
    pub live_status: i64,
    pub title: String,
    pub area_id: String,
    pub area_name: String,
    pub parent_area_name: String,
    pub online: i64,
    pub attention: i64,
    pub live_key: u64,
}

#[derive(Debug, Clone)]
pub struct MockLiveDanmaku {
    pub text: String,
    pub uid: i64,
    pub nickname: String,
    pub timeline: String,
    pub isadmin: i64,
}

#[derive(Debug, Clone)]
pub struct MockFavItem {
    pub aid: i64,
    pub bvid: String,
    pub title: String,
    pub duration: i64,
    pub upper_mid: i64,
    pub upper_name: String,
    pub valid: bool,
}

#[derive(Debug, Clone)]
pub struct MockFavFolder {
    pub id: i64,
    pub fid: i64,
    pub mid: i64,
    pub attr: i64,
    pub title: String,
    pub items: Vec<MockFavItem>,
}

#[derive(Debug, Clone)]
pub struct MockLaterItem {
    pub aid: i64,
    pub bvid: String,
    pub title: String,
    pub duration: i64,
    pub progress: i64,
    pub viewed: bool,
}

#[derive(Debug, Clone)]
pub struct MockHistoryItem {
    pub oid: i64,
    pub bvid: String,
    pub title: String,
    pub author_name: String,
    pub business: String,
    pub cid: i64,
    pub progress: i64,
    pub duration: i64,
    pub view_at: i64,
}

#[derive(Debug, Clone)]
pub struct MockComment {
    pub rpid: i64,
    pub oid: i64,
    pub mid: i64,
    pub uname: String,
    pub message: String,
    pub like: i64,
    pub action: i64,
    pub ctime: i64,
}

#[derive(Debug, Clone)]
pub struct MockState {
    pub port: u16,
    pub auth_logged_in: bool,
    pub mid: i64,
    pub uname: String,
    pub need_refresh: bool,
    pub refresh_csrf: String,
    pub tv_qr_status: QrMockStatus,
    pub web_qr_status: QrMockStatus,
    pub sms_require_recaptcha: bool,
    pub dosign_mode: DoSignMode,
    pub sessdata: String,
    pub bili_jct: String,
    pub refresh_token: String,
    pub own_room_id: i64,
    pub rooms: HashMap<i64, MockRoom>,
    pub live_danmaku: Vec<MockLiveDanmaku>,
    pub fav_folders: Vec<MockFavFolder>,
    pub next_fav_id: i64,
    pub toview: Vec<MockLaterItem>,
    pub history: Vec<MockHistoryItem>,
    pub liked_videos: HashSet<i64>,
    pub coined_videos: HashMap<i64, u32>,
    pub comments: Vec<MockComment>,
    pub next_rpid: i64,
    pub danmaku_xml_override: Option<String>,
    pub followed_mids: HashSet<i64>,
    pub short_links: HashMap<String, String>,
}

impl Default for MockState {
    fn default() -> Self {
        Self::new(0)
    }
}

impl MockState {
    pub fn new(port: u16) -> Self {
        let mut rooms = HashMap::new();
        rooms.insert(
            31686846,
            MockRoom {
                room_id: 31686846,
                short_id: 0,
                uid: 293793435,
                live_status: 0,
                title: "我的测试直播间".to_string(),
                area_id: "86".to_string(),
                area_name: "英雄联盟".to_string(),
                parent_area_name: "网游".to_string(),
                online: 120,
                attention: 500,
                live_key: 608336837537435443,
            },
        );
        rooms.insert(
            14073662,
            MockRoom {
                room_id: 14073662,
                short_id: 76,
                uid: 50333369,
                live_status: 1,
                title: "官方测试直播".to_string(),
                area_id: "235".to_string(),
                area_name: "单机其他".to_string(),
                parent_area_name: "单机游戏".to_string(),
                online: 9876,
                attention: 42000,
                live_key: 708884257747206334,
            },
        );

        let fav_folders = vec![MockFavFolder {
            id: 1052622027,
            fid: 10526220,
            mid: 293793435,
            attr: 0,
            title: "默认收藏夹".to_string(),
            items: vec![MockFavItem {
                aid: 79677524,
                bvid: "BV1uJ411r7hL".to_string(),
                title: "测试收藏视频".to_string(),
                duration: 225,
                upper_mid: 293793435,
                upper_name: "测试UP主".to_string(),
                valid: true,
            }],
        }];

        let toview = vec![
            MockLaterItem {
                aid: 79677524,
                bvid: "BV1uJ411r7hL".to_string(),
                title: "稍后再看未看视频".to_string(),
                duration: 225,
                progress: 0,
                viewed: false,
            },
            MockLaterItem {
                aid: 170001,
                bvid: "BV1GJ411x7h7".to_string(),
                title: "稍后再看已看视频".to_string(),
                duration: 212,
                progress: -1,
                viewed: true,
            },
        ];

        let history = vec![
            MockHistoryItem {
                oid: 79677524,
                bvid: "BV1uJ411r7hL".to_string(),
                title: "测试历史视频".to_string(),
                author_name: "测试UP主".to_string(),
                business: "archive".to_string(),
                cid: 146044693,
                progress: 45,
                duration: 225,
                view_at: 1700000100,
            },
            MockHistoryItem {
                oid: 14073662,
                bvid: String::new(),
                title: "官方测试直播".to_string(),
                author_name: "测试主播".to_string(),
                business: "live".to_string(),
                cid: 0,
                progress: 0,
                duration: 0,
                view_at: 1700000050,
            },
        ];

        let comments = vec![MockComment {
            rpid: 104192624480,
            oid: 79677524,
            mid: 293793435,
            uname: "测试评论用户".to_string(),
            message: "第一条测试评论".to_string(),
            like: 5,
            action: 0,
            ctime: 1700000000,
        }];

        let live_danmaku = vec![MockLiveDanmaku {
            text: "欢迎来到直播间".to_string(),
            uid: 50333369,
            nickname: "测试主播".to_string(),
            timeline: "2026-01-01 12:00:00".to_string(),
            isadmin: 1,
        }];

        let mut short_links = HashMap::new();
        short_links.insert(
            "pigt3PQ".to_string(),
            "https://www.bilibili.com/video/BV1uJ411r7hL?spm_id_from=333.337".to_string(),
        );
        short_links.insert(
            "hop1".to_string(),
            format!("http://127.0.0.1:{port}/pigt3PQ"),
        );
        short_links.insert("loop".to_string(), format!("http://127.0.0.1:{port}/loop"));

        Self {
            port,
            auth_logged_in: true,
            mid: 293793435,
            uname: "测试用户".to_string(),
            need_refresh: false,
            refresh_csrf: "mock_refresh_csrf".to_string(),
            tv_qr_status: QrMockStatus::Success,
            web_qr_status: QrMockStatus::Success,
            sms_require_recaptcha: false,
            dosign_mode: DoSignMode::Success,
            sessdata: "mock_sessdata_token".to_string(),
            bili_jct: "mock_bili_jct_csrf".to_string(),
            refresh_token: "mock_refresh_token_abc".to_string(),
            own_room_id: 31686846,
            rooms,
            live_danmaku,
            fav_folders,
            next_fav_id: 2000000001,
            toview,
            history,
            liked_videos: HashSet::new(),
            coined_videos: HashMap::new(),
            comments,
            next_rpid: 104192624481,
            danmaku_xml_override: None,
            followed_mids: HashSet::new(),
            short_links,
        }
    }

    fn resolve_room(&self, room_or_short: i64) -> Option<&MockRoom> {
        self.rooms.get(&room_or_short).or_else(|| {
            self.rooms
                .values()
                .find(|r| r.short_id > 0 && r.short_id == room_or_short)
        })
    }

    fn resolve_room_mut(&mut self, room_or_short: i64) -> Option<&mut MockRoom> {
        if self.rooms.contains_key(&room_or_short) {
            return self.rooms.get_mut(&room_or_short);
        }
        let target_id = self
            .rooms
            .values()
            .find(|r| r.short_id > 0 && r.short_id == room_or_short)
            .map(|r| r.room_id)?;
        self.rooms.get_mut(&target_id)
    }
}

pub struct MockBiliServer {
    port: u16,
    state: Arc<Mutex<MockState>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    faults: Arc<Mutex<HashMap<String, FaultConfig>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockBiliServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let port = listener.local_addr().expect("local_addr").port();
        let state = Arc::new(Mutex::new(MockState::new(port)));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let faults = Arc::new(Mutex::new(HashMap::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let state_bg = Arc::clone(&state);
        let requests_bg = Arc::clone(&requests);
        let faults_bg = Arc::clone(&faults);
        let stop_bg = Arc::clone(&stop);

        let handle = std::thread::spawn(move || {
            for stream_res in listener.incoming() {
                if stop_bg.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(mut stream) = stream_res {
                    let state_w = Arc::clone(&state_bg);
                    let requests_w = Arc::clone(&requests_bg);
                    let faults_w = Arc::clone(&faults_bg);
                    std::thread::spawn(move || {
                        if let Ok(Some(req)) = read_http_request(&mut stream) {
                            requests_w
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push(req.clone());
                            let resp = dispatch(&req, &state_w, &faults_w);
                            let _ = resp.write_to(&mut stream);
                        }
                    });
                }
            }
        });

        Self {
            port,
            state,
            requests,
            faults,
            stop,
            handle: Some(handle),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn state(&self) -> &Arc<Mutex<MockState>> {
        &self.state
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn reset_requests(&self) {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    pub fn set_fault(&self, route: impl Into<String>, fault: FaultConfig) {
        self.faults
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(route.into(), fault);
    }

    pub fn clear_fault(&self, route: &str) {
        self.faults
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(route);
    }

    pub fn clear_all_faults(&self) {
        self.faults
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

impl Drop for MockBiliServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct HttpResponse {
    status: u16,
    content_type: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn json(val: Value) -> Self {
        Self {
            status: 200,
            content_type: "application/json; charset=utf-8".to_string(),
            headers: Vec::new(),
            body: serde_json::to_vec(&val).expect("serialize mock json"),
        }
    }

    fn text(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: content_type.to_string(),
            headers: Vec::new(),
            body,
        }
    }

    fn write_to(self, stream: &mut TcpStream) -> std::io::Result<()> {
        let reason = match self.status {
            200 => "OK",
            301 => "Moved Permanently",
            302 => "Found",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            412 => "Precondition Failed",
            500 => "Internal Server Error",
            502 => "Bad Gateway",
            _ => "OK",
        };
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n",
            self.status,
            reason,
            self.content_type,
            self.body.len()
        );
        for (k, v) in &self.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        stream.write_all(head.as_bytes())?;
        if !self.body.is_empty() {
            stream.write_all(&self.body)?;
        }
        stream.flush()?;
        let _ = stream.shutdown(Shutdown::Both);
        Ok(())
    }
}

fn decode_percent_plus(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_urlencoded_pairs(raw: &str) -> Vec<(String, String)> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((k, v)) => (decode_percent_plus(k), decode_percent_plus(v)),
            None => (decode_percent_plus(part), String::new()),
        })
        .collect()
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn read_line_crlf(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    pos: &mut usize,
) -> std::io::Result<Vec<u8>> {
    let mut tmp = [0u8; 4096];
    loop {
        if let Some(rel) = buf[*pos..].windows(2).position(|w| w == b"\r\n") {
            let line = buf[*pos..*pos + rel].to_vec();
            *pos += rel + 2;
            return Ok(line);
        }
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF reading HTTP chunked line",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

fn read_exact_buffered(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    pos: &mut usize,
    len: usize,
) -> std::io::Result<Vec<u8>> {
    let mut tmp = [0u8; 4096];
    while buf.len() - *pos < len {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF reading HTTP chunk payload",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let slice = buf[*pos..*pos + len].to_vec();
    *pos += len;
    Ok(slice)
}

fn read_chunked_body(stream: &mut TcpStream, initial: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut buf = initial.to_vec();
    let mut pos = 0usize;
    let mut out = Vec::new();
    loop {
        let line = read_line_crlf(stream, &mut buf, &mut pos)?;
        let size_line = std::str::from_utf8(&line).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid UTF-8 chunk size")
        })?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        if size_hex.is_empty() || !size_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid HTTP chunk size",
            ));
        }
        let chunk_len = usize::from_str_radix(size_hex, 16).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "chunk size overflow")
        })?;
        if chunk_len == 0 {
            loop {
                let trailer = read_line_crlf(stream, &mut buf, &mut pos)?;
                if trailer.is_empty() {
                    break;
                }
            }
            break;
        }
        let data = read_exact_buffered(stream, &mut buf, &mut pos, chunk_len)?;
        out.extend_from_slice(&data);
        let crlf = read_exact_buffered(stream, &mut buf, &mut pos, 2)?;
        if crlf != b"\r\n" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing CRLF after chunk payload",
            ));
        }
        if pos > 65536 {
            buf.drain(..pos);
            pos = 0;
        }
    }
    Ok(out)
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Option<RecordedRequest>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let head_str = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = head_str.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let raw_target = parts.next().unwrap_or("/");

    let normalized_target = if raw_target.starts_with("http://") || raw_target.starts_with("https://")
    {
        if let Some((_, rest)) = raw_target.split_once("://") {
            match rest.find('/') {
                Some(idx) => &rest[idx..],
                None => "/",
            }
        } else {
            raw_target
        }
    } else {
        raw_target
    };

    let (path, raw_query) = match normalized_target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (normalized_target.to_string(), String::new()),
    };
    let query = parse_urlencoded_pairs(&raw_query);

    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }

    let is_chunked = headers.iter().any(|(k, v)| {
        k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked")
    });
    let content_length = headers
        .iter()
        .rev()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);

    let body_prefix = &buf[header_end + 4..];
    let body = if is_chunked {
        read_chunked_body(stream, body_prefix)?
    } else if content_length > body_prefix.len() {
        let mut body = body_prefix.to_vec();
        let remaining = content_length - body.len();
        let mut rest = vec![0u8; remaining];
        stream.read_exact(&mut rest)?;
        body.extend_from_slice(&rest);
        body
    } else {
        body_prefix[..content_length].to_vec()
    };

    let is_form = headers.iter().any(|(k, v)| {
        k == "content-type"
            && v.to_ascii_lowercase()
                .starts_with("application/x-www-form-urlencoded")
    });
    let form_params = if is_form {
        parse_urlencoded_pairs(std::str::from_utf8(&body).unwrap_or(""))
    } else {
        Vec::new()
    };

    Ok(Some(RecordedRequest {
        method,
        path,
        raw_query,
        query,
        headers,
        body,
        form_params,
    }))
}

fn find_fault(faults: &HashMap<String, FaultConfig>, path: &str) -> Option<FaultConfig> {
    if let Some(exact) = faults.get(path) {
        return Some(exact.clone());
    }
    let mut best: Option<(&str, &FaultConfig)> = None;
    for (pattern, cfg) in faults {
        if let Some(prefix) = pattern.strip_suffix('*')
            && path.starts_with(prefix)
            && best.is_none_or(|(best_p, _)| prefix.len() > best_p.len())
        {
            best = Some((prefix, cfg));
        }
    }
    best.map(|(_, cfg)| cfg.clone())
}

fn apply_fault(fault: FaultConfig) -> HttpResponse {
    match fault {
        FaultConfig::HttpStatus {
            status,
            content_type,
            body,
            headers,
        } => HttpResponse {
            status,
            content_type,
            headers,
            body,
        },
        FaultConfig::JsonBody(val) => HttpResponse::json(val),
        FaultConfig::ApiError {
            code,
            message,
            data,
        } => HttpResponse::json(json!({
            "code": code,
            "message": message,
            "ttl": 1,
            "data": data
        })),
        FaultConfig::EmptyBody => {
            HttpResponse::text(200, "application/json; charset=utf-8", Vec::new())
        }
        FaultConfig::MalformedJson => HttpResponse::text(
            200,
            "application/json; charset=utf-8",
            b"{bad_json".to_vec(),
        ),
        FaultConfig::Redirect { status, location } => {
            let mut headers = Vec::new();
            if let Some(loc) = location {
                headers.push(("Location".to_string(), loc));
            }
            HttpResponse {
                status,
                content_type: "text/plain; charset=utf-8".to_string(),
                headers,
                body: Vec::new(),
            }
        }
    }
}

fn unauth_response() -> HttpResponse {
    HttpResponse::json(json!({
        "code": -101,
        "message": "账号未登录",
        "ttl": 1
    }))
}

fn cookie_info_json(st: &MockState) -> Value {
    json!({
        "cookie_info": {
            "cookies": [
                { "name": "SESSDATA", "value": st.sessdata },
                { "name": "bili_jct", "value": st.bili_jct },
                { "name": "DedeUserID", "value": st.mid.to_string() },
                { "name": "DedeUserID__ckMd5", "value": "mock_ck_md5_12345678" }
            ]
        },
        "token_info": {
            "refresh_token": st.refresh_token
        },
        "refresh_token": st.refresh_token
    })
}

fn set_cookie_headers(st: &MockState) -> Vec<(String, String)> {
    vec![
        (
            "Set-Cookie".to_string(),
            format!("SESSDATA={}; Path=/; HttpOnly", st.sessdata),
        ),
        (
            "Set-Cookie".to_string(),
            format!("bili_jct={}; Path=/", st.bili_jct),
        ),
        (
            "Set-Cookie".to_string(),
            format!("DedeUserID={}; Path=/", st.mid),
        ),
        (
            "Set-Cookie".to_string(),
            "DedeUserID__ckMd5=mock_ck_md5_12345678; Path=/".to_string(),
        ),
    ]
}

fn mock_video_info_json(bvid: &str, aid: i64, port: u16) -> Value {
    json!({
        "bvid": bvid,
        "aid": aid,
        "videos": 2,
        "tid": 17,
        "tname": "单机游戏",
        "copyright": 1,
        "pic": format!("http://127.0.0.1:{port}/mock/cover/{bvid}.jpg"),
        "title": format!("Mock Video {bvid}"),
        "pubdate": 1700000000,
        "ctime": 1700000000,
        "desc": "Mock video description for E2E testing",
        "duration": 225,
        "owner": {
            "mid": 293793435,
            "name": "测试UP主",
            "face": format!("http://127.0.0.1:{port}/mock/cover/face.jpg")
        },
        "stat": {
            "aid": aid,
            "view": 12345,
            "danmaku": 67,
            "reply": 23,
            "favorite": 89,
            "coin": 45,
            "share": 12,
            "like": 345
        },
        "cid": 146044693,
        "pages": [
            {
                "cid": 146044693,
                "page": 1,
                "from": "vupload",
                "part": "P1 开篇",
                "duration": 120
            },
            {
                "cid": 146044694,
                "page": 2,
                "from": "vupload",
                "part": "P2 进阶",
                "duration": 105
            }
        ]
    })
}

fn default_danmaku_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?><i><chatserver>chat.bilibili.com</chatserver><chatid>146044693</chatid><mission>0</mission><maxlimit>1000</maxlimit><state>0</state><real_name>0</real_name><source>k-v</source><d p="12.345,1,25,16777215,1700000000,0,abcd1234,100001">测试弹幕一条</d><d p="25.500,5,25,16711680,1700000001,0,efgh5678,100002">顶部红字弹幕</d></i>"#
}

fn dispatch(
    req: &RecordedRequest,
    state_lock: &Arc<Mutex<MockState>>,
    faults_lock: &Arc<Mutex<HashMap<String, FaultConfig>>>,
) -> HttpResponse {
    if let Some(fault) = find_fault(&faults_lock.lock().unwrap_or_else(|e| e.into_inner()), &req.path)
    {
        return apply_fault(fault);
    }

    let mut st = state_lock.lock().unwrap_or_else(|e| e.into_inner());
    let port = st.port;

    match (req.method.as_str(), req.path.as_str()) {
        // ── Group A: Main API ───────────────────────────────────────────────
        ("GET", "/x/web-interface/nav") => {
            let wbi_img = json!({
                "img_url": "https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png",
                "sub_url": "https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"
            });
            if st.auth_logged_in {
                HttpResponse::json(json!({
                    "code": 0,
                    "message": "0",
                    "ttl": 1,
                    "data": {
                        "isLogin": true,
                        "mid": st.mid,
                        "uname": st.uname,
                        "money": 123.5,
                        "vipStatus": 1,
                        "level_info": { "current_level": 6 },
                        "wbi_img": wbi_img
                    }
                }))
            } else {
                HttpResponse::json(json!({
                    "code": -101,
                    "message": "账号未登录",
                    "ttl": 1,
                    "data": {
                        "isLogin": false,
                        "wbi_img": wbi_img
                    }
                }))
            }
        }

        ("GET", "/x/frontend/finger/spi") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "data": {
                "b_3": "D9656DA8-1111-2222-3333-444455556666infoc",
                "b_4": "F6E0FD4B-1111-2222-3333-444455556666-00000=="
            }
        })),

        ("GET", "/x/member/web/exp/reward") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "login": true,
                    "watch": true,
                    "coins": 30,
                    "share": false,
                    "email": true,
                    "tel": true,
                    "safe_question": false,
                    "identify_card": true
                }
            }))
        }

        ("GET", "/x/web-interface/wbi/search/type") => {
            let search_type = req.query_param("search_type").unwrap_or("video");
            let page = req
                .query_param("page")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(1);
            let data = match search_type {
                "bili_user" => json!({
                    "page": page,
                    "pagesize": 20,
                    "numResults": 1,
                    "numPages": 1,
                    "result": [{
                        "mid": 293793435,
                        "uname": "测试UP主",
                        "fans": 50000,
                        "videos": 42,
                        "usign": "Rust 开发者",
                        "level": 6
                    }]
                }),
                "live" => json!({
                    "page": page,
                    "pagesize": 20,
                    "numResults": 1,
                    "numPages": 1,
                    "result": {
                        "live_room": [{
                            "roomid": 14073662,
                            "uname": "测试主播",
                            "cate_name": "英雄联盟",
                            "online": 9876,
                            "title": "官方测试直播",
                            "live_status": 1
                        }]
                    }
                }),
                _ => json!({
                    "page": page,
                    "pagesize": 20,
                    "numResults": 1,
                    "numPages": 1,
                    "result": [{
                        "bvid": "BV1uJ411r7hL",
                        "aid": 79677524,
                        "title": "Mock <em class=\"keyword\">Rust</em> Video",
                        "author": "测试UP主",
                        "mid": 293793435,
                        "play": 12345,
                        "duration": "03:45",
                        "pubdate": 1700000000
                    }]
                }),
            };
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": data
            }))
        }

        ("GET", "/x/web-interface/wbi/search/square") => {
            let limit = req
                .query_param("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(20)
                .clamp(1, 50);
            let list: Vec<Value> = (1..=limit)
                .map(|i| {
                    json!({
                        "keyword": format!("热搜词{i}"),
                        "show_name": format!("热搜词{i}"),
                        "icon_name": if i == 1 { "热" } else { "" }
                    })
                })
                .collect();
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "trending": {
                        "title": "bilibili热搜",
                        "list": list
                    }
                }
            }))
        }

        ("GET", "/main/hotword") => HttpResponse::json(json!({
            "code": 0,
            "list": [
                { "keyword": "热搜词1", "show_name": "热搜词1" },
                { "keyword": "热搜词2", "show_name": "热搜词2" }
            ]
        })),

        ("GET", "/x/web-interface/wbi/view") => {
            let req_bvid = req.query_param("bvid");
            let req_aid = req.query_param("aid").and_then(|v| v.parse::<i64>().ok());
            let (bvid, aid) = match (req_bvid, req_aid) {
                (Some(b), Some(a)) => (b.to_string(), a),
                (Some(b), None) => {
                    let derived_aid = if b == "BV1GJ411x7h7" {
                        170001
                    } else {
                        79677524
                    };
                    (b.to_string(), derived_aid)
                }
                (None, Some(a)) => {
                    let derived_bvid = if a == 170001 {
                        "BV1GJ411x7h7"
                    } else {
                        "BV1uJ411r7hL"
                    };
                    (derived_bvid.to_string(), a)
                }
                (None, None) => ("BV1uJ411r7hL".to_string(), 79677524),
            };
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": mock_video_info_json(&bvid, aid, port)
            }))
        }

        ("GET", "/x/player/wbi/playurl" | "/x/player/playurl") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "quality": 80,
                "format": "mp4",
                "timelength": 225000,
                "accept_description": ["高清 1080P", "高清 720P", "清晰 480P"],
                "accept_quality": [80, 64, 32],
                "support_formats": [
                    { "quality": 80, "format": "hdflv2", "new_description": "1080P 高清", "display_desc": "1080P", "codecs": ["avc1.640028", "hev1.1.6.L120.90", "av01.0.04M.08"] },
                    { "quality": 64, "format": "flv720", "new_description": "720P 高清", "display_desc": "720P", "codecs": ["avc1.64001f"] },
                    { "quality": 32, "format": "flv480", "new_description": "480P 清晰", "display_desc": "480P", "codecs": ["avc1.64001e"] }
                ],
                "durl": [{
                    "order": 1,
                    "length": 225000,
                    "size": MOCK_MP4_BYTES.len(),
                    "url": format!("http://127.0.0.1:{port}/mock/media/video.mp4"),
                    "backup_url": []
                }],
                "dash": {
                    "duration": 225,
                    "minBufferTime": 1.5,
                    "min_buffer_time": 1.5,
                    "video": [
                        {
                            "id": 80,
                            "codecid": 7,
                            "codecs": "avc1.640028",
                            "mimeType": "video/mp4",
                            "mime_type": "video/mp4",
                            "width": 1920,
                            "height": 1080,
                            "bandwidth": 2500000,
                            "frameRate": "30",
                            "frame_rate": "30",
                            "baseUrl": format!("http://127.0.0.1:{port}/mock/media/video_80_avc.m4s"),
                            "base_url": format!("http://127.0.0.1:{port}/mock/media/video_80_avc.m4s"),
                            "backupUrl": [],
                            "backup_url": []
                        },
                        {
                            "id": 80,
                            "codecid": 12,
                            "codecs": "hev1.1.6.L120.90",
                            "mimeType": "video/mp4",
                            "mime_type": "video/mp4",
                            "width": 1920,
                            "height": 1080,
                            "bandwidth": 1800000,
                            "frameRate": "30",
                            "frame_rate": "30",
                            "baseUrl": format!("http://127.0.0.1:{port}/mock/media/video_80_hevc.m4s"),
                            "base_url": format!("http://127.0.0.1:{port}/mock/media/video_80_hevc.m4s"),
                            "backupUrl": [],
                            "backup_url": []
                        },
                        {
                            "id": 80,
                            "codecid": 13,
                            "codecs": "av01.0.04M.08",
                            "mimeType": "video/mp4",
                            "mime_type": "video/mp4",
                            "width": 1920,
                            "height": 1080,
                            "bandwidth": 1500000,
                            "frameRate": "30",
                            "frame_rate": "30",
                            "baseUrl": format!("http://127.0.0.1:{port}/mock/media/video_80_av1.m4s"),
                            "base_url": format!("http://127.0.0.1:{port}/mock/media/video_80_av1.m4s"),
                            "backupUrl": [],
                            "backup_url": []
                        },
                        {
                            "id": 64,
                            "codecid": 7,
                            "codecs": "avc1.64001f",
                            "mimeType": "video/mp4",
                            "mime_type": "video/mp4",
                            "width": 1280,
                            "height": 720,
                            "bandwidth": 1200000,
                            "frameRate": "30",
                            "frame_rate": "30",
                            "baseUrl": format!("http://127.0.0.1:{port}/mock/media/video_64_avc.m4s"),
                            "base_url": format!("http://127.0.0.1:{port}/mock/media/video_64_avc.m4s"),
                            "backupUrl": [],
                            "backup_url": []
                        }
                    ],
                    "audio": [{
                        "id": 30280,
                        "codecid": 0,
                        "codecs": "mp4a.40.2",
                        "mimeType": "audio/mp4",
                        "mime_type": "audio/mp4",
                        "bandwidth": 192000,
                        "baseUrl": format!("http://127.0.0.1:{port}/mock/media/audio_30280.m4s"),
                        "base_url": format!("http://127.0.0.1:{port}/mock/media/audio_30280.m4s"),
                        "backupUrl": [],
                        "backup_url": []
                    }]
                }
            }
        })),

        ("GET", "/x/player/wbi/v2" | "/x/player/v2") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "aid": 79677524,
                "bvid": "BV1uJ411r7hL",
                "cid": 146044693,
                "subtitle": {
                    "allow_submit": false,
                    "subtitles": [
                        {
                            "id": 1,
                            "lan": "zh-Hans",
                            "lan_doc": "中文（简体）",
                            "subtitle_url": format!("http://127.0.0.1:{port}/mock/subtitle/zh-Hans.json")
                        },
                        {
                            "id": 2,
                            "lan": "en-US",
                            "lan_doc": "英语（美国）",
                            "subtitle_url": format!("http://127.0.0.1:{port}/mock/subtitle/en-US.json")
                        }
                    ]
                },
                "view_points": [{
                    "type": 2,
                    "from": 0,
                    "to": 60,
                    "content": "开场简介"
                }]
            }
        })),

        ("GET", "/x/web-interface/view/conclusion/get") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "code": 0,
                "stid": "mock_stid_123",
                "status": 0,
                "like_num": 6,
                "dislike_num": 2,
                "model_result": {
                    "result_type": 2,
                    "summary": "这是一个测试视频的 AI 总结。",
                    "outline": [{
                        "title": "核心内容",
                        "part_outline": [{ "timestamp": 10, "content": "介绍基础概念" }],
                        "timestamp": 10
                    }],
                    "subtitle": [{
                        "title": "",
                        "timestamp": 1,
                        "part_subtitle": [
                            { "content": "字幕片段", "start_timestamp": 0, "end_timestamp": 2 }
                        ]
                    }]
                }
            }
        })),

        ("GET", "/x/web-interface/view/detail/tag" | "/x/tag/archive/tags") => {
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": [
                    { "tag_id": 1001, "tag_name": "Rust", "tag_type": "old_channel", "subscribed_count": 100 },
                    { "tag_id": 1002, "tag_name": "CLI", "tag_type": "new_channel", "subscribed_count": 50 }
                ]
            }))
        }

        ("GET", "/x/web-interface/archive/related") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": [mock_video_info_json("BV17x411w7KC", 2, port)]
        })),

        ("POST", "/x/web-interface/archive/like") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let aid = req
                .form_param("aid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(79677524);
            let like = req.form_param("like").unwrap_or("1");
            if like == "2" {
                st.liked_videos.remove(&aid);
            } else {
                st.liked_videos.insert(aid);
            }
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("POST", "/x/web-interface/coin/add") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let aid = req
                .form_param("aid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(79677524);
            let multiply = req
                .form_param("multiply")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(1);
            let select_like = req.form_param("select_like") == Some("1");
            *st.coined_videos.entry(aid).or_insert(0) += multiply;
            if select_like {
                st.liked_videos.insert(aid);
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": { "like": select_like }
            }))
        }

        ("POST", "/x/web-interface/archive/like/triple") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let aid = req
                .form_param("aid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(79677524);
            st.liked_videos.insert(aid);
            st.coined_videos.insert(aid, 2);
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "like": true,
                    "coin": true,
                    "fav": true,
                    "multiply": 2
                }
            }))
        }

        ("GET", "/x/v1/dm/list.so") => {
            let xml = st
                .danmaku_xml_override
                .as_deref()
                .unwrap_or(default_danmaku_xml());
            HttpResponse::text(200, "text/xml", deflate_raw_stored(xml.as_bytes()))
        }

        ("GET", "/x/v2/reply" | "/x/v2/reply/wbi/main" | "/x/v2/reply/hot") => {
            let replies: Vec<Value> = st
                .comments
                .iter()
                .map(|c| {
                    json!({
                        "rpid": c.rpid,
                        "rpid_str": c.rpid.to_string(),
                        "oid": c.oid,
                        "mid": c.mid,
                        "rcount": 1,
                        "like": c.like,
                        "action": c.action,
                        "ctime": c.ctime,
                        "member": {
                            "mid": c.mid.to_string(),
                            "uname": c.uname,
                            "level_info": { "current_level": 5 }
                        },
                        "content": {
                            "message": c.message
                        },
                        "reply_control": { "time_desc": "1小时前发布" }
                    })
                })
                .collect();
            let count = replies.len();
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "page": { "num": 1, "size": 20, "count": count, "acount": count },
                    "cursor": { "is_begin": true, "is_end": true, "all_count": count },
                    "replies": replies,
                    "hots": replies
                }
            }))
        }

        ("POST", "/x/v2/reply/add") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let oid = req
                .form_param("oid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(79677524);
            let message = req.form_param("message").unwrap_or("").to_string();
            let rpid = st.next_rpid;
            st.next_rpid += 1;
            let mid = st.mid;
            let uname = st.uname.clone();
            st.comments.push(MockComment {
                rpid,
                oid,
                mid,
                uname,
                message,
                like: 0,
                action: 0,
                ctime: 1700000200,
            });
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "rpid": rpid,
                    "rpid_str": rpid.to_string(),
                    "success_toast": "发送成功"
                }
            }))
        }

        ("POST", "/x/v2/reply/action") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let rpid = req
                .form_param("rpid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let action = req
                .form_param("action")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(1);
            if let Some(c) = st.comments.iter_mut().find(|c| c.rpid == rpid) {
                if action == 1 && c.action == 0 {
                    c.like += 1;
                } else if action == 0 && c.action == 1 && c.like > 0 {
                    c.like -= 1;
                }
                c.action = action;
            }
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("POST", "/x/v2/reply/del") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let rpid = req
                .form_param("rpid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            st.comments.retain(|c| c.rpid != rpid);
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("GET", "/x/space/wbi/acc/info") => {
            let mid = req
                .query_param("mid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(st.mid);
            let is_followed = st.followed_mids.contains(&mid);
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "mid": mid,
                    "name": if mid == st.mid { st.uname.clone() } else { format!("用户{mid}") },
                    "sex": "保密",
                    "face": format!("http://127.0.0.1:{port}/mock/cover/face.jpg"),
                    "sign": "Mock Bilibili user bio",
                    "level": 6,
                    "is_followed": is_followed,
                    "live_room": {
                        "roomStatus": 1,
                        "liveStatus": 0,
                        "roomid": st.own_room_id,
                        "title": "测试直播间"
                    }
                }
            }))
        }

        ("GET", "/x/relation/stat") => {
            let vmid = req
                .query_param("vmid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(st.mid);
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "mid": vmid,
                    "following": 120,
                    "whisper": 0,
                    "black": 0,
                    "follower": 50000
                }
            }))
        }

        ("GET", "/x/space/upstat") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "archive": { "view": 100000 },
                "article": { "view": 5000 },
                "likes": 8888
            }
        })),

        ("GET", "/x/space/wbi/arc/search") => {
            let kw = req.query_param("keyword").unwrap_or("");
            let mut vlist = vec![json!({
                "bvid": "BV1uJ411r7hL",
                "aid": 79677524,
                "title": "Mock Space Video Rust",
                "author": "测试UP主",
                "mid": 293793435,
                "play": 12345,
                "length": "03:45",
                "created": 1700000000
            })];
            if !kw.is_empty() {
                vlist.retain(|item| {
                    item["title"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&kw.to_lowercase())
                });
            }
            let count = vlist.len();
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "list": { "vlist": vlist },
                    "page": { "pn": 1, "ps": 30, "count": count }
                }
            }))
        }

        ("GET", "/x/polymer/web-dynamic/v1/feed/space") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "has_more": false,
                "offset": "",
                "items": [{
                    "id_str": "9000000001",
                    "type": "DYNAMIC_TYPE_AV",
                    "modules": {
                        "module_author": { "mid": 293793435, "name": "测试UP主", "pub_time": "2026-01-01" },
                        "module_dynamic": {
                            "desc": { "text": "动态正文描述" },
                            "major": {
                                "type": "MAJOR_TYPE_ARCHIVE",
                                "archive": {
                                    "bvid": "BV1uJ411r7hL",
                                    "aid": "79677524",
                                    "title": "Mock Dynamic Video",
                                    "duration_text": "03:45",
                                    "stat": { "play": "1.2万", "danmaku": "67" }
                                }
                            }
                        },
                        "module_stat": {
                            "comment": { "count": 10, "forbidden": false },
                            "forward": { "count": 2, "forbidden": false },
                            "like": { "count": 56, "forbidden": false, "status": false }
                        }
                    }
                }]
            }
        })),

        ("GET", "/x/polymer/web-dynamic/v1/feed/all") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "has_more": false,
                    "offset": "",
                    "items": [{
                        "id_str": "9000000002",
                        "type": "DYNAMIC_TYPE_AV",
                        "modules": {
                            "module_author": { "mid": 293793435, "name": "测试UP主", "pub_time": "2026-01-01" },
                            "module_dynamic": {
                                "desc": { "text": "动态正文描述" },
                                "major": {
                                    "type": "MAJOR_TYPE_ARCHIVE",
                                    "archive": {
                                        "bvid": "BV1uJ411r7hL",
                                        "aid": "79677524",
                                        "title": "Mock Feed All Video",
                                        "duration_text": "03:45",
                                        "stat": { "play": "1.2万", "danmaku": "67" }
                                    }
                                }
                            },
                            "module_stat": {
                                "comment": { "count": 10, "forbidden": false },
                                "forward": { "count": 2, "forbidden": false },
                                "like": { "count": 56, "forbidden": false, "status": false }
                            }
                        }
                    }]
                }
            }))
        }

        ("POST", "/x/relation/modify") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let fid = req
                .form_param("fid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let act = req
                .form_param("act")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(1);
            if fid > 0 {
                if act == 2 {
                    st.followed_mids.remove(&fid);
                } else {
                    st.followed_mids.insert(fid);
                }
            }
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("GET", "/x/web-interface/ranking/v2") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "note": "根据稿件内容质量、近期的数据综合展示，动态更新",
                "list": [mock_video_info_json("BV1uJ411r7hL", 79677524, port)]
            }
        })),

        ("GET", "/x/web-interface/popular") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "list": [mock_video_info_json("BV1uJ411r7hL", 79677524, port)],
                "no_more": false
            }
        })),

        ("GET", "/x/v3/fav/folder/created/list-all") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let list: Vec<Value> = st
                .fav_folders
                .iter()
                .map(|f| {
                    json!({
                        "id": f.id,
                        "fid": f.fid,
                        "mid": f.mid,
                        "attr": f.attr,
                        "title": f.title,
                        "media_count": f.items.len()
                    })
                })
                .collect();
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "count": list.len(),
                    "list": list
                }
            }))
        }

        ("GET", "/x/v3/fav/resource/list") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let media_id = req
                .query_param("media_id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(1052622027);
            let pn = req
                .query_param("pn")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1);
            let ps = req
                .query_param("ps")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(20)
                .clamp(1, 20);
            if let Some(folder) = st.fav_folders.iter().find(|f| f.id == media_id) {
                let total = folder.items.len();
                let start = (pn - 1) * ps;
                let end = (start + ps).min(total);
                let has_more = end < total;
                let medias: Vec<Value> = if start < total {
                    folder.items[start..end]
                        .iter()
                        .map(|it| {
                            json!({
                                "id": it.aid,
                                "bvid": it.bvid,
                                "title": it.title,
                                "duration": it.duration,
                                "attr": if it.valid { 0 } else { 9 },
                                "upper": { "mid": it.upper_mid, "name": it.upper_name },
                                "cnt_info": { "play": 12345, "collect": 67, "danmaku": 89 }
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                HttpResponse::json(json!({
                    "code": 0,
                    "message": "0",
                    "ttl": 1,
                    "data": {
                        "info": {
                            "id": folder.id,
                            "fid": folder.fid,
                            "mid": folder.mid,
                            "attr": folder.attr,
                            "title": folder.title,
                            "media_count": total
                        },
                        "medias": medias,
                        "has_more": has_more
                    }
                }))
            } else {
                HttpResponse::json(json!({
                    "code": -404,
                    "message": "收藏夹不存在",
                    "ttl": 1
                }))
            }
        }

        ("POST", "/x/v3/fav/resource/deal") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let rid = req
                .form_param("rid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(79677524);
            let add_ids = req.form_param("add_media_ids").unwrap_or("");
            let del_ids = req.form_param("del_media_ids").unwrap_or("");
            for id_str in add_ids.split(',').filter(|s| !s.is_empty()) {
                if let Ok(mid_id) = id_str.parse::<i64>()
                    && let Some(folder) = st.fav_folders.iter_mut().find(|f| f.id == mid_id)
                    && !folder.items.iter().any(|i| i.aid == rid)
                {
                    folder.items.push(MockFavItem {
                        aid: rid,
                        bvid: if rid == 170001 {
                            "BV1GJ411x7h7".to_string()
                        } else {
                            "BV1uJ411r7hL".to_string()
                        },
                        title: format!("收藏视频 av{rid}"),
                        duration: 200,
                        upper_mid: 293793435,
                        upper_name: "测试UP主".to_string(),
                        valid: true,
                    });
                }
            }
            for id_str in del_ids.split(',').filter(|s| !s.is_empty()) {
                if let Ok(mid_id) = id_str.parse::<i64>()
                    && let Some(folder) = st.fav_folders.iter_mut().find(|f| f.id == mid_id)
                {
                    folder.items.retain(|i| i.aid != rid);
                }
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": { "prompt": false, "success_num": 0 }
            }))
        }

        ("POST", "/x/v3/fav/resource/clean") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let media_id = req
                .form_param("media_id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            if let Some(folder) = st.fav_folders.iter_mut().find(|f| f.id == media_id) {
                folder.items.retain(|i| i.valid);
            }
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1, "data": 0 }))
        }

        ("POST", "/x/v3/fav/folder/add") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let title = req.form_param("title").unwrap_or("新收藏夹").to_string();
            let privacy = req
                .form_param("privacy")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let id = st.next_fav_id;
            st.next_fav_id += 1;
            let fid = id / 100;
            let mid = st.mid;
            st.fav_folders.push(MockFavFolder {
                id,
                fid,
                mid,
                attr: privacy,
                title: title.clone(),
                items: Vec::new(),
            });
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "id": id,
                    "fid": fid,
                    "mid": mid,
                    "attr": privacy,
                    "title": title,
                    "media_count": 0
                }
            }))
        }

        ("POST", "/x/v3/fav/folder/del") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let ids_str = req.form_param("media_ids").unwrap_or("");
            let del_set: HashSet<i64> = ids_str
                .split(',')
                .filter_map(|s| s.trim().parse::<i64>().ok())
                .collect();
            st.fav_folders.retain(|f| !del_set.contains(&f.id));
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1, "data": 0 }))
        }

        ("GET", "/x/v2/history/toview") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let list: Vec<Value> = st
                .toview
                .iter()
                .map(|it| {
                    json!({
                        "aid": it.aid,
                        "bvid": it.bvid,
                        "title": it.title,
                        "duration": it.duration,
                        "progress": it.progress,
                        "owner": { "mid": 293793435, "name": "测试UP主" }
                    })
                })
                .collect();
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "count": list.len(),
                    "list": list
                }
            }))
        }

        ("POST", "/x/v2/history/toview/add") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let bvid = req.form_param("bvid").map(str::to_string);
            let aid = req
                .form_param("aid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or_else(|| {
                    if bvid.as_deref() == Some("BV1GJ411x7h7") {
                        170001
                    } else {
                        79677524
                    }
                });
            let resolved_bvid = bvid.unwrap_or_else(|| format!("BV_av{aid}"));
            if !st.toview.iter().any(|i| i.aid == aid) {
                st.toview.push(MockLaterItem {
                    aid,
                    bvid: resolved_bvid.clone(),
                    title: format!("稍后再看 {resolved_bvid}"),
                    duration: 180,
                    progress: 0,
                    viewed: false,
                });
            }
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("POST", "/x/v2/history/toview/del") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            if req.form_param("viewed") == Some("true") {
                st.toview.retain(|i| !i.viewed);
            } else if let Some(aid) = req.form_param("aid").and_then(|v| v.parse::<i64>().ok()) {
                st.toview.retain(|i| i.aid != aid);
            }
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("POST", "/x/v2/history/toview/clear") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            st.toview.clear();
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("GET", "/x/web-interface/history/cursor") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let htype = req.query_param("type").unwrap_or("all");
            let ps = req
                .query_param("ps")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(20)
                .clamp(1, 30);
            let list: Vec<Value> = st
                .history
                .iter()
                .filter(|item| htype == "all" || item.business == htype)
                .take(ps)
                .map(|item| {
                    json!({
                        "title": item.title,
                        "author_name": item.author_name,
                        "progress": item.progress,
                        "duration": item.duration,
                        "view_at": item.view_at,
                        "history": {
                            "oid": item.oid,
                            "bvid": item.bvid,
                            "business": item.business,
                            "cid": item.cid
                        }
                    })
                })
                .collect();
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1,
                "data": {
                    "cursor": { "max": 0, "view_at": 0, "business": "", "ps": ps },
                    "list": list
                }
            }))
        }

        ("POST", "/x/v2/history/delete") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            if let Some(kid) = req.form_param("kid")
                && let Some((biz, oid_str)) = kid.split_once('_')
                && let Ok(oid) = oid_str.parse::<i64>()
            {
                st.history.retain(|h| !(h.business == biz && h.oid == oid));
            }
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("POST", "/x/v2/history/clear") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            st.history.clear();
            HttpResponse::json(json!({ "code": 0, "message": "0", "ttl": 1 }))
        }

        ("GET", "/pgc/view/web/season") => HttpResponse::json(json!({
            "code": 0,
            "message": "success",
            "result": {
                "season_id": 28615,
                "title": "测试番剧",
                "episodes": [{
                    "id": 285151,
                    "aid": 79677524,
                    "bvid": "BV1uJ411r7hL",
                    "cid": 146044693,
                    "title": "1",
                    "long_title": "第一话"
                }]
            }
        })),

        ("GET", "/pgc/review/user") => HttpResponse::json(json!({
            "code": 0,
            "message": "success",
            "result": {
                "media": {
                    "media_id": 28229233,
                    "season_id": 28615,
                    "title": "测试番剧"
                }
            }
        })),

        // ── Group B: Live API ───────────────────────────────────────────────
        ("GET", "/room/v1/Room/getRoomInfoOld") => HttpResponse::json(json!({
            "code": 0,
            "message": "ok",
            "data": {
                "roomid": st.own_room_id,
                "roomStatus": 1,
                "liveStatus": st.rooms.get(&st.own_room_id).map_or(0, |r| r.live_status)
            }
        })),

        ("GET", "/room/v1/Room/room_init") => {
            let id = req
                .query_param("id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(st.own_room_id);
            if let Some(room) = st.resolve_room(id) {
                HttpResponse::json(json!({
                    "code": 0,
                    "message": "ok",
                    "data": {
                        "room_id": room.room_id,
                        "short_id": room.short_id,
                        "uid": room.uid,
                        "live_status": room.live_status
                    }
                }))
            } else {
                HttpResponse::json(json!({ "code": 1, "message": "房间不存在" }))
            }
        }

        ("GET", "/room/v1/Room/get_info") => {
            let id = req
                .query_param("room_id")
                .or_else(|| req.query_param("id"))
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(st.own_room_id);
            if let Some(room) = st.resolve_room(id) {
                HttpResponse::json(json!({
                    "code": 0,
                    "message": "ok",
                    "data": {
                        "room_id": room.room_id,
                        "short_id": room.short_id,
                        "uid": room.uid,
                        "live_status": room.live_status,
                        "title": room.title,
                        "area_id": room.area_id.parse::<i64>().unwrap_or(86),
                        "area_name": room.area_name,
                        "parent_area_name": room.parent_area_name,
                        "online": room.online,
                        "attention": room.attention
                    }
                }))
            } else {
                HttpResponse::json(json!({ "code": 1, "message": "房间不存在" }))
            }
        }

        ("GET", "/room/v1/Area/getList") => HttpResponse::json(json!({
            "code": 0,
            "message": "ok",
            "data": [
                {
                    "id": "2",
                    "name": "网游",
                    "list": [
                        { "id": "86", "name": "英雄联盟", "parent_id": "2", "parent_name": "网游" },
                        { "id": "240", "name": "CS:GO", "parent_id": "2", "parent_name": "网游" }
                    ]
                },
                {
                    "id": "6",
                    "name": "单机游戏",
                    "list": [
                        { "id": "235", "name": "单机其他", "parent_id": "6", "parent_name": "单机游戏" }
                    ]
                }
            ]
        })),

        ("GET", "/room/v1/Area/getMyChooseArea") => {
            let rid = req
                .query_param("roomid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(st.own_room_id);
            let (id, name) = st
                .resolve_room(rid)
                .map(|r| (r.area_id.clone(), r.area_name.clone()))
                .unwrap_or_else(|| ("86".to_string(), "英雄联盟".to_string()));
            HttpResponse::json(json!({
                "code": 0,
                "message": "ok",
                "data": [{ "id": id, "name": name }]
            }))
        }

        ("POST", "/room/v1/Room/update") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let room_id = req
                .form_param("room_id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(st.own_room_id);
            let title = req.form_param("title").map(str::to_string);
            let area_id = req.form_param("area_id").map(str::to_string);
            if let Some(room) = st.resolve_room_mut(room_id) {
                if let Some(t) = title {
                    room.title = t;
                }
                if let Some(a) = area_id {
                    room.area_id = a;
                }
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "ok",
                "data": { "sub_session_key": "", "audit_info": {} }
            }))
        }

        ("POST", "/room/v1/Room/startLive") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let room_id = req
                .form_param("room_id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(st.own_room_id);
            let area_v2 = req.form_param("area_v2").map(str::to_string);
            let live_key = if let Some(room) = st.resolve_room_mut(room_id) {
                room.live_status = 1;
                if let Some(a) = area_v2 {
                    room.area_id = a;
                }
                room.live_key
            } else {
                608336837537435443
            };
            HttpResponse::json(json!({
                "code": 0,
                "message": "ok",
                "data": {
                    "rtmp": {
                        "addr": "rtmp://live-push.bilivideo.com/live-bvc/",
                        "code": "?streamname=live_mock_stream&key=mock_secret_key_12345"
                    },
                    "live_key": live_key.to_string()
                }
            }))
        }

        ("POST", "/room/v1/Room/stopLive") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let room_id = req
                .form_param("room_id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(st.own_room_id);
            if let Some(room) = st.resolve_room_mut(room_id) {
                room.live_status = 0;
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "",
                "data": {
                    "change": 1,
                    "status": "PREPARING"
                }
            }))
        }

        ("GET", "/xlive/app-blink/v1/live/StopLiveData") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "LiveTime": 3661,
                    "WatchedCount": 512,
                    "MaxOnline": 88,
                    "DanmuNum": 120,
                    "AddFans": 15,
                    "NewFansClub": 3,
                    "HamsterRmb": 100
                }
            }))
        }

        ("GET", "/xlive/web-ucenter/user/following") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let list: Vec<Value> = st
                .rooms
                .values()
                .filter(|r| r.room_id != st.own_room_id)
                .map(|r| {
                    json!({
                        "roomid": r.room_id,
                        "uid": r.uid,
                        "uname": "测试主播",
                        "title": r.title,
                        "area_name": r.area_name,
                        "live_status": r.live_status
                    })
                })
                .collect();
            let count = list.len();
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "count": count,
                    "list": list
                }
            }))
        }

        ("GET", "/xlive/web-room/v2/index/getRoomPlayInfo") => {
            let room_id = req
                .query_param("room_id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(14073662);
            let canonical_id = st.resolve_room(room_id).map_or(room_id, |r| r.room_id);
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "room_id": canonical_id,
                    "live_status": 1,
                    "playurl_info": {
                        "playurl": {
                            "stream": [{
                                "protocol_name": "http_stream",
                                "format": [{
                                    "format_name": "flv",
                                    "codec": [{
                                        "codec_name": "avc",
                                        "current_qn": 10000,
                                        "accept_qn": [10000, 400],
                                        "base_url": format!("/mock/live/{canonical_id}.flv"),
                                        "url_info": [{
                                            "host": format!("http://127.0.0.1:{port}"),
                                            "extra": "?mock=1"
                                        }]
                                    }]
                                }]
                            }]
                        }
                    }
                }
            }))
        }

        ("GET", "/room/v1/Room/playUrl") => {
            let cid = req
                .query_param("cid")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(14073662);
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "current_qn": 10000,
                    "durl": [{
                        "url": format!("http://127.0.0.1:{port}/mock/live/{cid}.flv"),
                        "order": 1
                    }]
                }
            }))
        }

        ("GET", "/xlive/web-room/v1/dM/gethistory") => {
            let room: Vec<Value> = st
                .live_danmaku
                .iter()
                .map(|d| {
                    json!({
                        "text": d.text,
                        "uid": d.uid,
                        "nickname": d.nickname,
                        "timeline": d.timeline,
                        "isadmin": d.isadmin
                    })
                })
                .collect();
            HttpResponse::json(json!({
                "code": 0,
                "message": "",
                "data": {
                    "admin": [],
                    "room": room
                }
            }))
        }

        ("POST", "/msg/send") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            let text = req.form_param("msg").unwrap_or("").to_string();
            let uid = st.mid;
            let nickname = st.uname.clone();
            st.live_danmaku.push(MockLiveDanmaku {
                text,
                uid,
                nickname,
                timeline: "2026-01-01 12:01:00".to_string(),
                isadmin: 0,
            });
            HttpResponse::json(json!({
                "code": 0,
                "message": "",
                "msg": ""
            }))
        }

        ("GET", "/xlive/web-ucenter/v1/sign/DoSign") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            match st.dosign_mode {
                DoSignMode::Success => HttpResponse::json(json!({
                    "code": 0,
                    "message": "0",
                    "ttl": 1,
                    "data": {
                        "text": "3000点用户经验",
                        "specialText": "再签到1天可获得额外奖励",
                        "hadSignDays": 1,
                        "allDays": 30
                    }
                })),
                DoSignMode::AlreadySigned => HttpResponse::json(json!({
                    "code": 1011040,
                    "message": "今天已经签到过了",
                    "ttl": 1
                })),
                DoSignMode::Retired => HttpResponse::json(json!({
                    "code": 1,
                    "message": "签到活动已下线",
                    "ttl": 1
                })),
            }
        }

        // ── Group C: Passport / Auth ────────────────────────────────────────
        ("POST", "/x/passport-tv-login/qrcode/auth_code") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "data": {
                "url": format!("http://127.0.0.1:{port}/mock/qr/tv"),
                "auth_code": "mock_tv_auth_code"
            }
        })),

        ("POST", "/x/passport-tv-login/qrcode/poll") => match st.tv_qr_status {
            QrMockStatus::Waiting => HttpResponse::json(json!({
                "code": 86039,
                "message": "二维码尚未确认"
            })),
            QrMockStatus::Scanned => HttpResponse::json(json!({
                "code": 86090,
                "message": "已扫码未确认"
            })),
            QrMockStatus::Expired => HttpResponse::json(json!({
                "code": 86038,
                "message": "二维码已过期"
            })),
            QrMockStatus::Success => {
                st.auth_logged_in = true;
                HttpResponse::json(json!({
                    "code": 0,
                    "message": "0",
                    "data": cookie_info_json(&st)
                }))
            }
        },

        ("GET", "/x/passport-login/web/qrcode/generate") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "data": {
                "url": format!("http://127.0.0.1:{port}/mock/qr/web"),
                "qrcode_key": "mock_web_qrcode_key"
            }
        })),

        ("GET", "/x/passport-login/web/qrcode/poll") => match st.web_qr_status {
            QrMockStatus::Waiting => HttpResponse::json(json!({
                "code": 0,
                "data": { "code": 86101, "message": "未扫码" }
            })),
            QrMockStatus::Scanned => HttpResponse::json(json!({
                "code": 0,
                "data": { "code": 86090, "message": "已扫码未确认" }
            })),
            QrMockStatus::Expired => HttpResponse::json(json!({
                "code": 0,
                "data": { "code": 86038, "message": "二维码已失效" }
            })),
            QrMockStatus::Success => {
                st.auth_logged_in = true;
                let mut resp = HttpResponse::json(json!({
                    "code": 0,
                    "data": {
                        "code": 0,
                        "message": "",
                        "url": "https://www.bilibili.com",
                        "refresh_token": st.refresh_token
                    }
                }));
                resp.headers.extend(set_cookie_headers(&st));
                resp
            }
        },

        ("GET", "/x/passport-login/web/key") => HttpResponse::json(json!({
            "code": 0,
            "message": "0",
            "data": {
                "hash": "1234567890abcdef",
                "key": MOCK_RSA_PUBLIC_KEY_PEM
            }
        })),

        ("POST", "/x/passport-login/oauth2/login") => {
            st.auth_logged_in = true;
            let mut data = cookie_info_json(&st);
            data["status"] = json!(0);
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": data
            }))
        }

        ("POST", "/x/passport-login/sms/send") => {
            if st.sms_require_recaptcha {
                HttpResponse::json(json!({
                    "code": 0,
                    "message": "0",
                    "data": {
                        "recaptcha_url": "https://passport.bilibili.com/h5-app/passport/login/recaptcha?recaptcha_token=mock_recap_token"
                    }
                }))
            } else {
                HttpResponse::json(json!({
                    "code": 0,
                    "message": "0",
                    "data": {
                        "captcha_key": "mock_captcha_key_123"
                    }
                }))
            }
        }

        ("POST", "/x/passport-login/login/sms") => {
            st.auth_logged_in = true;
            let mut data = cookie_info_json(&st);
            data["status"] = json!(0);
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": data
            }))
        }

        ("POST", "/login/exit/v2") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            st.auth_logged_in = false;
            HttpResponse::json(json!({
                "code": 0,
                "status": true,
                "data": {
                    "redirectUrl": "https://www.bilibili.com"
                }
            }))
        }

        ("GET", "/x/passport-login/web/cookie/info") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "refresh": st.need_refresh,
                    "timestamp": 1700000000000i64
                }
            }))
        }

        ("GET", path) if path.starts_with("/correspond/1/") => {
            let html = format!(
                "<!DOCTYPE html><html><body><div id=\"1-name\">{}</div></body></html>",
                st.refresh_csrf
            );
            HttpResponse::text(200, "text/html; charset=utf-8", html.into_bytes())
        }

        ("POST", "/x/passport-login/web/cookie/refresh") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            st.need_refresh = false;
            st.sessdata = format!("{}_refreshed", st.sessdata);
            st.bili_jct = format!("{}_refreshed", st.bili_jct);
            st.refresh_token = format!("{}_refreshed", st.refresh_token);
            let mut resp = HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "status": 0,
                    "refresh_token": st.refresh_token
                }
            }));
            resp.headers.extend(set_cookie_headers(&st));
            resp
        }

        ("POST", "/x/passport-login/web/confirm/refresh") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "ttl": 1
            }))
        }

        // ── Group D: Danmaku XML ────────────────────────────────────────────
        ("GET", path) if path.ends_with(".xml") => {
            let xml = st
                .danmaku_xml_override
                .as_deref()
                .unwrap_or(default_danmaku_xml());
            HttpResponse::text(200, "text/xml", deflate_raw_stored(xml.as_bytes()))
        }

        // ── Group F: Mock Static / Media Assets ─────────────────────────────
        ("GET", path) if path.starts_with("/mock/subtitle/") => HttpResponse::json(json!({
            "font_size": 0.4,
            "font_color": "#FFFFFF",
            "body": [
                { "from": 0.5, "to": 2.5, "location": 2, "content": "Hello Bilibili" },
                { "from": 3.0, "to": 5.0, "location": 2, "content": "Mock Subtitle Line 2" }
            ]
        })),

        ("GET", path) if path.starts_with("/mock/cover/") => {
            HttpResponse::text(200, "image/jpeg", MOCK_JPEG_BYTES.to_vec())
        }

        ("GET", path) if path.starts_with("/mock/media/") => {
            HttpResponse::text(200, "video/mp4", MOCK_MP4_BYTES.to_vec())
        }

        ("GET", path) if path.starts_with("/mock/live/") => {
            HttpResponse::text(200, "video/x-flv", MOCK_FLV_BYTES.to_vec())
        }

        // ── Group G: Sub-Replies, UPOS Upload & Creative Center Studio ──────
        ("GET", "/x/v2/reply/reply") => {
            let root_id = req
                .query_param("root")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(1001);
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "page": { "num": 1, "size": 20, "count": 2 },
                    "root": {
                        "rpid": root_id,
                        "oid": 79677524,
                        "like": 42,
                        "rcount": 2,
                        "ctime": 1700000000,
                        "member": { "mid": "100", "uname": "楼主用户", "level_info": { "current_level": 6 } },
                        "content": { "message": "这是楼主的一级长评论内容" }
                    },
                    "replies": [
                        {
                            "rpid": 2001,
                            "oid": 79677524,
                            "like": 5,
                            "rcount": 0,
                            "ctime": 1700000100,
                            "member": { "mid": "101", "uname": "层主A", "level_info": { "current_level": 5 } },
                            "content": { "message": "楼中楼第一条回复" }
                        },
                        {
                            "rpid": 2002,
                            "oid": 79677524,
                            "like": 3,
                            "rcount": 0,
                            "ctime": 1700000200,
                            "member": { "mid": "102", "uname": "层主B", "level_info": { "current_level": 4 } },
                            "content": { "message": "回复 @层主A : 楼中楼第二条回复" }
                        }
                    ]
                }
            }))
        }

        ("GET", "/mock/upos_probe_ok") => HttpResponse::text(200, "text/plain", b"OK".to_vec()),

        ("GET", "/preupload") => {
            if req.query_param("r") == Some("probe") {
                return HttpResponse::json(json!({
                    "OK": 1,
                    "lines": [{
                        "os": "upos",
                        "query": "probe_version=20221109&upcdn=bda2&zone=cs",
                        "probe_url": format!("http://127.0.0.1:{port}/mock/upos_probe_ok")
                    }]
                }));
            }
            if !st.auth_logged_in {
                return unauth_response();
            }
            let raw_name = req.query_param("name").unwrap_or("part.mp4");
            let stem = raw_name.split('.').next().unwrap_or("part");
            HttpResponse::json(json!({
                "OK": 1,
                "auth": "mock_upos_auth_token",
                "biz_id": 500009999i64,
                "chunk_size": 8,
                "endpoint": format!("http://127.0.0.1:{port}"),
                "upos_uri": format!("upos://ugcfx/server_{stem}.mp4")
            }))
        }

        ("POST", path) if path.starts_with("/ugcfx/") => {
            if req.header("x-upos-auth") != Some("mock_upos_auth_token") {
                return HttpResponse::text(403, "text/plain", b"missing auth".to_vec());
            }
            if req.query_param("uploads").is_some() {
                HttpResponse::json(json!({
                    "OK": 1,
                    "bucket": "ugcfx",
                    "key": path,
                    "upload_id": "mock_upload_id_123"
                }))
            } else {
                HttpResponse::json(json!({
                    "OK": 1,
                    "location": path,
                    "bucket": "ugcfx",
                    "key": path
                }))
            }
        }

        ("PUT", path) if path.starts_with("/ugcfx/") => {
            if req.header("x-upos-auth") != Some("mock_upos_auth_token") {
                return HttpResponse::text(403, "text/plain", b"missing auth".to_vec());
            }
            HttpResponse::text(200, "text/plain", b"MULTIPART_PUT_SUCCESS".to_vec())
        }

        ("POST", "/x/vu/web/cover/up") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": { "url": "https://archive.biliimg.com/bfs/archive/mock_uploaded_cover.png" }
            }))
        }

        ("POST", "/x/vu/web/add/v3") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": { "aid": 99887766i64, "bvid": "BV1Up411x7mK" }
            }))
        }

        ("GET", "/x/vupre/web/archive/view") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "archive": {
                        "aid": 79677524i64,
                        "bvid": "BV1uJ411r7hL",
                        "title": "已有录播稿件",
                        "tid": 171,
                        "tag": "录播,Rust",
                        "desc": "原简介",
                        "cover": "https://archive.biliimg.com/bfs/archive/orig.jpg",
                        "copyright": 1,
                        "state": 0,
                        "state_desc": "开放浏览",
                        "reject_reason": ""
                    },
                    "videos": [
                        { "filename": "orig_p1", "title": "P1 上半场", "desc": "", "cid": 10001, "status": 0, "status_desc": "转码通过" },
                        { "filename": "orig_p2", "title": "P2 下半场", "desc": "", "cid": 10002, "status": 0, "status_desc": "转码通过" }
                    ]
                }
            }))
        }

        ("POST", "/x/vu/web/edit") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": { "aid": 79677524i64, "bvid": "BV1uJ411r7hL" }
            }))
        }

        ("GET", "/x/web/archives") => {
            if !st.auth_logged_in {
                return unauth_response();
            }
            HttpResponse::json(json!({
                "code": 0,
                "message": "0",
                "data": {
                    "page": { "pn": 1, "ps": 20, "count": 2 },
                    "arc_audits": [
                        {
                            "Archive": {
                                "aid": 99887766i64,
                                "bvid": "BV1Up411x7mK",
                                "title": "新上传多P录播",
                                "state": 0,
                                "state_desc": "已通过",
                                "ptime": 1700000500
                            },
                            "stat": { "view": 1024 }
                        },
                        {
                            "Archive": {
                                "aid": 99887767i64,
                                "bvid": "BV1Up411x7mL",
                                "title": "转码中测试稿件",
                                "state": -30,
                                "state_desc": "转码中",
                                "ctime": 1700000600
                            },
                            "stat": { "view": 0 }
                        }
                    ]
                }
            }))
        }

        // ── Group E: Short-Link Redirects (fallback single-segment path) ────
        ("GET", path) if path.len() > 1 && !path[1..].contains('/') => {
            let code = &path[1..];
            if let Some(target) = st.short_links.get(code) {
                HttpResponse {
                    status: 302,
                    content_type: "text/plain; charset=utf-8".to_string(),
                    headers: vec![("Location".to_string(), target.clone())],
                    body: Vec::new(),
                }
            } else {
                HttpResponse {
                    status: 404,
                    content_type: "application/json; charset=utf-8".to_string(),
                    headers: Vec::new(),
                    body: serde_json::to_vec(&json!({
                        "code": -404,
                        "message": "Not Found"
                    }))
                    .unwrap_or_default(),
                }
            }
        }

        _ => HttpResponse {
            status: 404,
            content_type: "application/json; charset=utf-8".to_string(),
            headers: Vec::new(),
            body: serde_json::to_vec(&json!({
                "code": -404,
                "message": format!("No mock route for {} {}", req.method, req.path)
            }))
            .unwrap_or_default(),
        },
    }
}

// ── Cookie Fixture ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CookieFixture {
    pub dede_user_id: String,
    pub dede_user_id_ck_md5: String,
    pub sessdata: String,
    pub bili_jct: String,
    pub buvid3: String,
    pub buvid4: String,
    pub refresh_token: String,
    pub room_id: i32,
    pub live_key: Option<u64>,
}

impl Default for CookieFixture {
    fn default() -> Self {
        Self::valid()
    }
}

impl CookieFixture {
    pub fn valid() -> Self {
        Self {
            dede_user_id: "293793435".to_string(),
            dede_user_id_ck_md5: "mock_ck_md5_12345678".to_string(),
            sessdata: "mock_sessdata_token".to_string(),
            bili_jct: "mock_bili_jct_csrf".to_string(),
            buvid3: "MOCK-BUVID3-0000-0000-0000infoc".to_string(),
            buvid4: "MOCK-BUVID4-0000-0000-0000-000000".to_string(),
            refresh_token: "mock_refresh_token_abc".to_string(),
            room_id: 31686846,
            live_key: None,
        }
    }

    pub fn without_csrf() -> Self {
        Self {
            bili_jct: String::new(),
            ..Self::valid()
        }
    }

    pub fn without_buvid() -> Self {
        Self {
            buvid3: String::new(),
            buvid4: String::new(),
            ..Self::valid()
        }
    }

    pub fn without_room_id() -> Self {
        Self {
            room_id: 0,
            ..Self::valid()
        }
    }

    pub fn without_refresh_token() -> Self {
        Self {
            refresh_token: String::new(),
            ..Self::valid()
        }
    }

    pub fn with_sessdata(mut self, sessdata: impl Into<String>) -> Self {
        self.sessdata = sessdata.into();
        self
    }

    pub fn with_bili_jct(mut self, bili_jct: impl Into<String>) -> Self {
        self.bili_jct = bili_jct.into();
        self
    }

    pub fn with_dede_user_id(mut self, uid: impl Into<String>) -> Self {
        self.dede_user_id = uid.into();
        self
    }

    pub fn with_buvid(mut self, b3: impl Into<String>, b4: impl Into<String>) -> Self {
        self.buvid3 = b3.into();
        self.buvid4 = b4.into();
        self
    }

    pub fn with_refresh_token(mut self, token: impl Into<String>) -> Self {
        self.refresh_token = token.into();
        self
    }

    pub fn with_room_id(mut self, room_id: i32) -> Self {
        self.room_id = room_id;
        self
    }

    pub fn with_live_key(mut self, live_key: Option<u64>) -> Self {
        self.live_key = live_key;
        self
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialize CookieFixture")
    }
}

// ── TestEnv & BtOutput ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegMode {
    Missing,
    FakeOk,
    FakeFailAll,
    FakeFailExec,
}

pub struct TestEnv {
    temp_dir: TempDir,
    home_dir: PathBuf,
    xdg_dir: PathBuf,
    work_dir: PathBuf,
    bin_dir: PathBuf,
    empty_bin_dir: PathBuf,
    alt_dir: PathBuf,
    server_base_url: Option<String>,
    ffmpeg_mode: FfmpegMode,
    no_color: bool,
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TestEnv {
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("create TempDir");
        let root = temp_dir.path();
        let home_dir = root.join("home");
        let xdg_dir = root.join("xdg");
        let work_dir = root.join("work");
        let bin_dir = root.join("bin");
        let empty_bin_dir = root.join("empty_bin");
        let alt_dir = root.join("alt");

        for dir in [
            &home_dir,
            &xdg_dir,
            &work_dir,
            &bin_dir,
            &empty_bin_dir,
            &alt_dir,
        ] {
            std::fs::create_dir_all(dir).expect("create TestEnv subdir");
        }

        Self {
            temp_dir,
            home_dir,
            xdg_dir,
            work_dir,
            bin_dir,
            empty_bin_dir,
            alt_dir,
            server_base_url: None,
            ffmpeg_mode: FfmpegMode::Missing,
            no_color: true,
        }
    }

    pub fn temp_path(&self) -> &Path {
        self.temp_dir.path()
    }

    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    pub fn xdg_dir(&self) -> &Path {
        &self.xdg_dir
    }

    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    pub fn bin_dir(&self) -> &Path {
        &self.bin_dir
    }

    pub fn default_cookie_path(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            self.home_dir.join(".config").join("bt").join("cookies.json")
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.xdg_dir.join("bt").join("cookies.json")
        }
    }

    pub fn alt_cookie_path(&self, name: &str) -> PathBuf {
        self.alt_dir.join(name)
    }

    pub fn ffmpeg_log_path(&self) -> PathBuf {
        self.temp_dir.path().join("ffmpeg.log")
    }

    pub fn read_ffmpeg_log(&self) -> Vec<String> {
        let path = self.ffmpeg_log_path();
        let Ok(file) = std::fs::File::open(path) else {
            return Vec::new();
        };
        std::io::BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .collect()
    }

    pub fn with_server(mut self, server: &MockBiliServer) -> Self {
        self.server_base_url = Some(server.base_url());
        self
    }

    pub fn with_valid_cookies(self) -> Self {
        let path = self.default_cookie_path();
        self.write_cookie_file(&path, &CookieFixture::valid());
        self
    }

    pub fn with_cookie_fixture(self, fixture: CookieFixture) -> Self {
        let path = self.default_cookie_path();
        self.write_cookie_file(&path, &fixture);
        self
    }

    pub fn with_alt_cookie_file(self, name: &str, fixture: CookieFixture) -> Self {
        let path = self.alt_cookie_path(name);
        self.write_cookie_file(&path, &fixture);
        self
    }

    pub fn with_fake_ffmpeg(mut self) -> Self {
        self.install_fake_ffmpeg();
        self.ffmpeg_mode = FfmpegMode::FakeOk;
        self
    }

    pub fn with_failing_ffmpeg(mut self) -> Self {
        self.install_fake_ffmpeg();
        self.ffmpeg_mode = FfmpegMode::FakeFailAll;
        self
    }

    pub fn with_failing_ffmpeg_exec(mut self) -> Self {
        self.install_fake_ffmpeg();
        self.ffmpeg_mode = FfmpegMode::FakeFailExec;
        self
    }

    pub fn without_ffmpeg(mut self) -> Self {
        let ff = self.bin_dir.join("ffmpeg");
        if ff.exists() {
            let _ = std::fs::remove_file(ff);
        }
        self.ffmpeg_mode = FfmpegMode::Missing;
        self
    }

    pub fn with_no_color(mut self, enabled: bool) -> Self {
        self.no_color = enabled;
        self
    }

    pub fn write_cookie_file(&self, path: &Path, fixture: &CookieFixture) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create cookie parent dir");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        std::fs::write(path, fixture.to_json_pretty()).expect("write cookie fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .expect("chmod 0600 cookie fixture");
        }
    }

    pub fn read_cookie_file(&self, path: &Path) -> CookieFixture {
        let text = std::fs::read_to_string(path).expect("read cookie file");
        serde_json::from_str(&text).expect("parse CookieFixture JSON")
    }

    pub fn write_work_file(&self, rel: &str, content: &[u8]) -> PathBuf {
        let target = self.work_dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("create work file parent");
        }
        std::fs::write(&target, content).expect("write work file");
        target
    }

    pub fn install_fake_ffmpeg(&self) {
        let default_log = self.ffmpeg_log_path();
        let default_log_escaped = default_log.to_string_lossy().replace('\'', "'\\''");
        let script = format!(
            r#"#!/bin/sh
set -eu

log_file="${{FAKE_FFMPEG_LOG:-}}"
if [ -z "$log_file" ]; then
  log_file='{default_log_escaped}'
fi
first=1
for arg in "$@"; do
  if [ "$first" -eq 1 ]; then
    printf '%s' "$arg" >> "$log_file"
    first=0
  else
    printf ' %s' "$arg" >> "$log_file"
  fi
done
printf '\n' >> "$log_file"

if [ "${{FAKE_FFMPEG_FAIL:-0}}" = "1" ]; then
  printf 'mock ffmpeg forced failure\n' >&2
  exit 1
fi

if [ "${{1:-}}" = "-version" ]; then
  printf 'ffmpeg version 6.0-mock\n'
  printf 'built with mock harness\n'
  exit 0
fi

if [ "${{FAKE_FFMPEG_FAIL:-0}}" = "exec" ]; then
  printf 'mock ffmpeg execution failure\n' >&2
  exit 1
fi

last_arg=""
for arg in "$@"; do
  last_arg="$arg"
done

case "$last_arg" in
  ""|-*|"null") ;;
  *.[fF][lL][vV])
    printf '\106\114\126\001\005\000\000\000\011\000\000\000\000' > "$last_arg"
    ;;
  *.[mM][pP]4|*.[mM]4[aA]|*.[mM]4[sS])
    printf '\000\000\000\024ftypisom\000\000\002\000isomiso2' > "$last_arg"
    ;;
  *)
    printf 'FAKE_FFMPEG_OUTPUT\n' > "$last_arg"
    ;;
esac
exit 0
"#
        );

        let ff_path = self.bin_dir.join("ffmpeg");
        std::fs::write(&ff_path, script).expect("write fake ffmpeg script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&ff_path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod 0755 fake ffmpeg");
        }
    }

    pub fn run_bt(&self, args: &[&str]) -> BtOutput {
        self.run_bt_env(args, &[])
    }

    pub fn run_bt_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> BtOutput {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_bt"));
        cmd.args(args)
            .current_dir(&self.work_dir)
            .stdin(Stdio::null())
            .env("HOME", &self.home_dir)
            .env("XDG_CONFIG_HOME", &self.xdg_dir)
            .env("XDG_DATA_HOME", self.xdg_dir.join("data"))
            .env("XDG_CACHE_HOME", self.xdg_dir.join("cache"))
            .env_remove("CLICOLOR_FORCE")
            .env_remove("FORCE_COLOR")
            .env_remove("RUST_LOG")
            .env_remove("BT_FFMPEG_PATH")
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy")
            .env_remove("NO_PROXY")
            .env_remove("no_proxy");

        if self.no_color {
            cmd.env("NO_COLOR", "1");
        } else {
            cmd.env_remove("NO_COLOR");
        }

        let base_vars = [
            "BT_API_BASE_URL",
            "BT_LIVE_API_BASE_URL",
            "BT_PASSPORT_BASE_URL",
            "BT_COMMENT_BASE_URL",
            "BT_SHORT_LINK_BASE_URL",
            "BT_WEB_BASE_URL",
            "BT_SEARCH_BASE_URL",
            "BT_MEMBER_API_BASE_URL",
        ];
        if let Some(ref base) = self.server_base_url {
            for var in base_vars {
                cmd.env(var, base);
            }
        } else {
            for var in base_vars {
                cmd.env_remove(var);
            }
        }

        match self.ffmpeg_mode {
            FfmpegMode::Missing => {
                cmd.env("PATH", &self.empty_bin_dir)
                    .env_remove("FAKE_FFMPEG_FAIL")
                    .env_remove("FAKE_FFMPEG_LOG");
            }
            FfmpegMode::FakeOk => {
                cmd.env("PATH", &self.bin_dir)
                    .env("FAKE_FFMPEG_LOG", self.ffmpeg_log_path())
                    .env_remove("FAKE_FFMPEG_FAIL");
            }
            FfmpegMode::FakeFailAll => {
                cmd.env("PATH", &self.bin_dir)
                    .env("FAKE_FFMPEG_LOG", self.ffmpeg_log_path())
                    .env("FAKE_FFMPEG_FAIL", "1");
            }
            FfmpegMode::FakeFailExec => {
                cmd.env("PATH", &self.bin_dir)
                    .env("FAKE_FFMPEG_LOG", self.ffmpeg_log_path())
                    .env("FAKE_FFMPEG_FAIL", "exec");
            }
        }

        for &(k, v) in extra_env {
            cmd.env(k, v);
        }

        let out = cmd.output().expect("spawn bt binary");
        let code = out.status.code().unwrap_or(-1);
        BtOutput {
            status: out.status,
            code,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BtOutput {
    pub status: ExitStatus,
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl BtOutput {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!(
                "stdout is not valid JSON ({e}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
                self.stdout, self.stderr
            )
        })
    }

    pub fn assert_exit(&self, expected: i32) -> &Self {
        assert_eq!(
            self.code, expected,
            "unexpected exit code (expected {expected}, got {}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.code, self.stdout, self.stderr
        );
        self
    }

    pub fn assert_json_ok(&self) -> Value {
        self.assert_exit(0);
        let v = self.json();
        assert_ne!(
            v.get("ok").and_then(|b| b.as_bool()),
            Some(false),
            "expected success JSON, got error envelope: {v}"
        );
        v
    }

    pub fn assert_json_err(&self, expected_kind_substr: Option<&str>) -> Value {
        self.assert_exit(1);
        let v = self.json();
        assert_eq!(
            v.get("ok").and_then(|b| b.as_bool()),
            Some(false),
            "expected ok=false in JSON error: {v}"
        );
        let kind = v["error"]["kind"]
            .as_str()
            .unwrap_or_else(|| panic!("missing error.kind in JSON error: {v}"));
        let msg = v["error"]["message"]
            .as_str()
            .unwrap_or_else(|| panic!("missing error.message in JSON error: {v}"));
        assert!(!msg.is_empty(), "error.message must be non-empty: {v}");
        if let Some(substr) = expected_kind_substr {
            assert!(
                kind.contains(substr),
                "expected error.kind '{kind}' to contain '{substr}': {v}"
            );
        }
        v
    }

    pub fn assert_no_ansi(&self) -> &Self {
        assert!(
            !self.stdout.contains('\x1b') && !self.stderr.contains('\x1b'),
            "unexpected ANSI escape sequence:\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.stdout,
            self.stderr
        );
        self
    }

    pub fn assert_stdout_empty(&self) -> &Self {
        assert!(
            self.stdout.trim().is_empty(),
            "expected empty stdout, got:\n{}",
            self.stdout
        );
        self
    }

    pub fn assert_stderr_contains(&self, substr: &str) -> &Self {
        assert!(
            self.stderr.contains(substr),
            "expected stderr to contain '{substr}', got:\n{}",
            self.stderr
        );
        self
    }

    pub fn assert_stdout_contains(&self, substr: &str) -> &Self {
        assert!(
            self.stdout.contains(substr),
            "expected stdout to contain '{substr}', got:\n{}",
            self.stdout
        );
        self
    }
}

pub fn assert_mode_0600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .unwrap_or_else(|e| panic!("stat {}: {e}", path.display()))
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode,
            0o600,
            "expected 0o600 on {}, got {mode:#o}",
            path.display()
        );
    }
}
