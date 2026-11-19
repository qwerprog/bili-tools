mod common;

use common::{
    BtOutput, CookieFixture, DoSignMode, FaultConfig, MOCK_FLV_BYTES, MOCK_JPEG_BYTES,
    MOCK_MP4_BYTES, MOCK_RSA_PUBLIC_KEY_PEM, MockBiliServer, QrMockStatus, TestEnv,
    assert_mode_0600, deflate_raw_stored,
};
use rsa::RsaPublicKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::traits::PublicKeyParts;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::{Duration, Instant};

struct RawHttpResp {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RawHttpResp {
    fn header_values(&self, name: &str) -> Vec<&str> {
        let lower = name.to_ascii_lowercase();
        self.headers
            .iter()
            .filter(|(k, _)| k == &lower)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("parse response JSON")
    }
}

fn read_http_response(stream: &mut TcpStream) -> RawHttpResp {
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response header terminator");
    let head_str = std::str::from_utf8(&raw[..head_end]).expect("UTF-8 response headers");
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().expect("status line");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .expect("status code")
        .parse::<u16>()
        .expect("parse status u16");

    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let body = raw[head_end + 4..].to_vec();
    RawHttpResp {
        status,
        headers,
        body,
    }
}

fn http_probe(port: u16, method: &str, target: &str, form_body: Option<&str>) -> RawHttpResp {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect MockBiliServer");
    let body_bytes = form_body.unwrap_or("").as_bytes();
    let mut req = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body_bytes.len()
    );
    if form_body.is_some() {
        req.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).expect("write request");
    if !body_bytes.is_empty() {
        stream.write_all(body_bytes).expect("write body");
    }
    stream.flush().expect("flush request");

    read_http_response(&mut stream)
}

fn inflate_raw_stored(compressed: &[u8]) -> Vec<u8> {
    let mut pos = 0;
    let mut out = Vec::new();
    loop {
        assert!(pos + 5 <= compressed.len(), "truncated stored block header");
        let bfinal = compressed[pos] & 0x01;
        let btype = (compressed[pos] >> 1) & 0x03;
        assert_eq!(btype, 0, "expected BTYPE=00 stored block");
        let len = u16::from_le_bytes([compressed[pos + 1], compressed[pos + 2]]);
        let nlen = u16::from_le_bytes([compressed[pos + 3], compressed[pos + 4]]);
        assert_eq!(len ^ nlen, 0xFFFF, "LEN and NLEN must be one's complement");
        pos += 5;
        let end = pos + len as usize;
        assert!(end <= compressed.len(), "stored block payload out of bounds");
        out.extend_from_slice(&compressed[pos..end]);
        pos = end;
        if bfinal == 1 {
            break;
        }
    }
    assert_eq!(pos, compressed.len(), "trailing bytes after final block");
    out
}

#[test]
fn smoke_mock_server_groups_a_through_f_and_request_capture() {
    let server = MockBiliServer::start();
    let p = server.port();

    // Group A
    assert_eq!(
        http_probe(p, "GET", "/x/web-interface/nav", None).json()["data"]["isLogin"],
        true
    );
    assert_eq!(
        http_probe(p, "GET", "/x/frontend/finger/spi", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/member/web/exp/reward", None).json()["data"]["coins"],
        30
    );
    assert_eq!(
        http_probe(
            p,
            "GET",
            "/x/web-interface/wbi/search/type?search_type=video&keyword=Rust&wts=1&w_rid=0123456789abcdef0123456789abcdef",
            None
        )
        .json()["data"]["result"][0]["bvid"],
        "BV1uJ411r7hL"
    );
    assert_eq!(
        http_probe(
            p,
            "GET",
            "/x/web-interface/wbi/search/square?limit=3",
            None
        )
        .json()["data"]["trending"]["list"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        http_probe(p, "GET", "/main/hotword", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(
            p,
            "GET",
            "/x/web-interface/wbi/view?bvid=BV1uJ411r7hL",
            None
        )
        .json()["data"]["pages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let playurl = http_probe(
        p,
        "GET",
        "/x/player/wbi/playurl?bvid=BV1uJ411r7hL&cid=146044693",
        None,
    )
    .json();
    assert_eq!(playurl["data"]["dash"]["video"].as_array().unwrap().len(), 4);
    assert_eq!(playurl["data"]["dash"]["min_buffer_time"], 1.5);
    assert_eq!(playurl["data"]["dash"]["video"][0]["mimeType"], "video/mp4");
    assert_eq!(playurl["data"]["dash"]["video"][0]["mime_type"], "video/mp4");
    assert_eq!(playurl["data"]["dash"]["video"][0]["frame_rate"], "30");
    assert!(playurl["data"]["dash"]["video"][0]["base_url"].is_string());
    assert!(playurl["data"]["dash"]["video"][0]["backup_url"].is_array());
    assert_eq!(playurl["data"]["dash"]["audio"][0]["mime_type"], "audio/mp4");
    assert!(playurl["data"]["dash"]["audio"][0]["base_url"].is_string());
    assert!(playurl["data"]["dash"]["audio"][0]["backup_url"].is_array());
    assert_eq!(
        http_probe(
            p,
            "GET",
            "/x/player/wbi/v2?bvid=BV1uJ411r7hL&cid=146044693",
            None
        )
        .json()["data"]["subtitle"]["subtitles"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let conclusion = http_probe(p, "GET", "/x/web-interface/view/conclusion/get", None).json();
    assert_eq!(conclusion["data"]["code"], 0);
    assert_eq!(conclusion["data"]["status"], 0);
    assert_eq!(conclusion["data"]["like_num"], 6);
    assert_eq!(conclusion["data"]["dislike_num"], 2);
    let concl_sub = &conclusion["data"]["model_result"]["subtitle"][0];
    assert_eq!(concl_sub["title"], "");
    assert_eq!(concl_sub["timestamp"], 1);
    assert_eq!(concl_sub["part_subtitle"][0]["content"], "字幕片段");
    assert_eq!(concl_sub["part_subtitle"][0]["start_timestamp"], 0);
    assert_eq!(concl_sub["part_subtitle"][0]["end_timestamp"], 2);
    assert!(concl_sub["part_title"].is_null());
    assert!(concl_sub["sentences"].is_null());
    assert_eq!(
        http_probe(p, "GET", "/x/web-interface/view/detail/tag", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/web-interface/archive/related", None).json()["code"],
        0
    );
    let dm_so = http_probe(p, "GET", "/x/v1/dm/list.so?oid=146044693", None);
    assert_eq!(dm_so.status, 200);
    assert!(String::from_utf8_lossy(&inflate_raw_stored(&dm_so.body)).contains("测试弹幕一条"));
    let reply_json = http_probe(p, "GET", "/x/v2/reply", None).json();
    assert_eq!(reply_json["code"], 0);
    assert_eq!(reply_json["data"]["replies"][0]["rcount"], 1);
    assert_eq!(
        reply_json["data"]["replies"][0]["member"]["level_info"]["current_level"],
        5
    );
    assert_eq!(
        reply_json["data"]["replies"][0]["reply_control"]["time_desc"],
        "1小时前发布"
    );
    assert_eq!(
        http_probe(p, "GET", "/x/space/wbi/acc/info?mid=293793435", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/space/wbi/arc/search?mid=293793435", None).json()["code"],
        0
    );
    let feed_space = http_probe(p, "GET", "/x/polymer/web-dynamic/v1/feed/space", None).json();
    assert_eq!(feed_space["code"], 0);
    assert!(
        feed_space["data"]["items"][0]["modules"]["module_dynamic"]["desc"]["text"].is_string()
    );
    assert!(
        feed_space["data"]["items"][0]["modules"]["module_dynamic"]["major"]["archive"]
            ["duration_text"]
            .is_string()
    );
    assert!(
        feed_space["data"]["items"][0]["modules"]["module_dynamic"]["major"]["archive"]["stat"]
            ["play"]
            .is_string()
    );
    assert!(feed_space["data"]["items"][0]["modules"]["module_stat"]["like"]["count"].is_number());
    let feed_all = http_probe(p, "GET", "/x/polymer/web-dynamic/v1/feed/all", None).json();
    assert_eq!(feed_all["code"], 0);
    assert!(feed_all["data"]["items"][0]["modules"]["module_dynamic"]["desc"]["text"].is_string());
    assert!(feed_all["data"]["items"][0]["modules"]["module_stat"]["comment"]["count"].is_number());
    let rank_json = http_probe(p, "GET", "/x/web-interface/ranking/v2", None).json();
    assert_eq!(rank_json["code"], 0);
    assert!(rank_json["data"]["note"].is_string());
    let popular_json = http_probe(p, "GET", "/x/web-interface/popular", None).json();
    assert_eq!(popular_json["code"], 0);
    assert_eq!(popular_json["data"]["no_more"], false);
    assert_eq!(
        http_probe(p, "GET", "/x/v3/fav/folder/created/list-all", None).json()["code"],
        0
    );
    let toview_json = http_probe(p, "GET", "/x/v2/history/toview", None).json();
    assert_eq!(toview_json["code"], 0);
    assert_eq!(toview_json["data"]["list"][1]["bvid"], "BV1GJ411x7h7");
    assert_eq!(toview_json["data"]["list"][1]["aid"], 170001);
    assert_eq!(
        http_probe(p, "GET", "/x/web-interface/history/cursor", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/pgc/view/web/season?ep_id=285151", None).json()["code"],
        0
    );

    // Group B
    assert_eq!(
        http_probe(p, "GET", "/room/v1/Room/getRoomInfoOld?mid=293793435", None).json()["data"]
            ["roomid"],
        31686846
    );
    assert_eq!(
        http_probe(p, "GET", "/room/v1/Room/room_init?id=76", None).json()["data"]["room_id"],
        14073662
    );
    assert_eq!(
        http_probe(p, "GET", "/room/v1/Room/get_info?room_id=31686846", None).json()["data"]
            ["live_status"],
        0
    );
    assert_ne!(
        http_probe(p, "GET", "/room/v1/Room/get_info?room_id=0", None).json()["code"],
        0
    );
    assert_ne!(
        http_probe(p, "GET", "/room/v1/Room/room_init?id=0", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/room/v1/Area/getList", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(
            p,
            "GET",
            "/room/v1/Area/getMyChooseArea?roomid=31686846",
            None
        )
        .json()["data"][0]["id"],
        "86"
    );
    assert_eq!(
        http_probe(p, "GET", "/xlive/web-ucenter/user/following", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(
            p,
            "GET",
            "/xlive/web-room/v2/index/getRoomPlayInfo?room_id=14073662",
            None
        )
        .json()["code"],
        0
    );
    assert_eq!(
        http_probe(
            p,
            "GET",
            "/xlive/web-room/v1/dM/gethistory?roomid=14073662",
            None
        )
        .json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/xlive/web-ucenter/v1/sign/DoSign", None).json()["code"],
        0
    );

    // Group C
    assert_eq!(
        http_probe(p, "POST", "/x/passport-tv-login/qrcode/auth_code", Some("")).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/passport-login/web/qrcode/generate", None).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/passport-login/web/cookie/info", None).json()["code"],
        0
    );
    let correspond = http_probe(p, "GET", "/correspond/1/deadbeef", None);
    assert_eq!(correspond.status, 200);
    assert!(
        String::from_utf8_lossy(&correspond.body).contains("<div id=\"1-name\">mock_refresh_csrf</div>")
    );

    // Group D
    let dm_xml = http_probe(p, "GET", "/146044693.xml", None);
    assert_eq!(dm_xml.status, 200);
    assert!(String::from_utf8_lossy(&inflate_raw_stored(&dm_xml.body)).contains("<chatid>146044693</chatid>"));

    // Group E
    let short_resp = http_probe(p, "GET", "/pigt3PQ", None);
    assert_eq!(short_resp.status, 302);
    assert_eq!(
        short_resp.header_values("location"),
        vec!["https://www.bilibili.com/video/BV1uJ411r7hL?spm_id_from=333.337"]
    );

    // Group F
    assert_eq!(
        http_probe(p, "GET", "/mock/subtitle/zh-Hans.json", None).json()["body"][0]["content"],
        "Hello Bilibili"
    );
    assert_eq!(
        http_probe(p, "GET", "/mock/cover/test.jpg", None).body,
        MOCK_JPEG_BYTES
    );
    assert_eq!(
        http_probe(p, "GET", "/mock/media/test.mp4", None).body,
        MOCK_MP4_BYTES
    );
    assert_eq!(
        http_probe(p, "GET", "/mock/live/14073662.flv", None).body,
        MOCK_FLV_BYTES
    );

    // Request capture & reset
    let reqs = server.requests();
    assert!(reqs.iter().any(|r| {
        r.method == "GET"
            && r.path == "/x/web-interface/wbi/search/type"
            && r.query_param("keyword") == Some("Rust")
            && r.has_wbi_signature()
            && r.header("host").is_some()
    }));
    server.reset_requests();
    assert!(server.requests().is_empty());
}

#[test]
fn smoke_mock_state_mutations_auth_enforcement_and_fault_overrides() {
    let server = MockBiliServer::start();
    let p = server.port();

    // Live start / update / stop mutations
    assert_eq!(
        http_probe(
            p,
            "POST",
            "/room/v1/Room/startLive",
            Some("room_id=31686846&area_v2=235")
        )
        .json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/room/v1/Room/get_info?room_id=31686846", None).json()["data"]
            ["live_status"],
        1
    );
    assert_eq!(
        http_probe(
            p,
            "POST",
            "/room/v1/Room/update",
            Some("room_id=31686846&title=Updated+Title")
        )
        .json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/room/v1/Room/get_info?room_id=31686846", None).json()["data"]
            ["title"],
        "Updated Title"
    );
    assert_eq!(
        http_probe(p, "POST", "/room/v1/Room/stopLive", Some("room_id=31686846")).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/room/v1/Room/get_info?room_id=31686846", None).json()["data"]
            ["live_status"],
        0
    );

    // Fav create / deal / clean / delete
    let created = http_probe(p, "POST", "/x/v3/fav/folder/add", Some("title=MyFav")).json();
    let new_fid = created["data"]["id"].as_i64().expect("new fav id");
    assert_eq!(
        http_probe(
            p,
            "POST",
            "/x/v3/fav/resource/deal",
            Some(&format!("rid=170001&add_media_ids={new_fid}"))
        )
        .json()["code"],
        0
    );
    {
        let mut st = server.state().lock().unwrap();
        let f = st.fav_folders.iter_mut().find(|f| f.id == new_fid).unwrap();
        f.items[0].valid = false;
    }
    assert_eq!(
        http_probe(
            p,
            "POST",
            "/x/v3/fav/resource/clean",
            Some(&format!("media_id={new_fid}"))
        )
        .json()["code"],
        0
    );
    assert_eq!(
        http_probe(
            p,
            "GET",
            &format!("/x/v3/fav/resource/list?media_id={new_fid}"),
            None
        )
        .json()["data"]["medias"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        http_probe(
            p,
            "POST",
            "/x/v3/fav/folder/del",
            Some(&format!("media_ids={new_fid}"))
        )
        .json()["code"],
        0
    );

    // Later & History mutations
    assert_eq!(
        http_probe(p, "POST", "/x/v2/history/toview/del", Some("viewed=true")).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/v2/history/toview", None).json()["data"]["count"],
        1
    );
    assert_eq!(
        http_probe(p, "POST", "/x/v2/history/toview/clear", Some("")).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/v2/history/toview", None).json()["data"]["count"],
        0
    );
    assert_eq!(
        http_probe(
            p,
            "POST",
            "/x/v2/history/delete",
            Some("kid=archive_79677524")
        )
        .json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/web-interface/history/cursor", None).json()["data"]["list"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        http_probe(p, "POST", "/x/v2/history/clear", Some("")).json()["code"],
        0
    );

    // Comments & Follow & Video engagement
    let added = http_probe(
        p,
        "POST",
        "/x/v2/reply/add",
        Some("oid=79677524&message=Hello"),
    )
    .json();
    let rpid = added["data"]["rpid"].as_i64().expect("rpid");
    assert_eq!(
        http_probe(
            p,
            "POST",
            "/x/v2/reply/action",
            Some(&format!("rpid={rpid}&action=1"))
        )
        .json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "POST", "/x/v2/reply/del", Some(&format!("rpid={rpid}"))).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "POST", "/x/relation/modify", Some("fid=50333369&act=1")).json()["code"],
        0
    );
    assert_eq!(
        http_probe(p, "GET", "/x/space/wbi/acc/info?mid=50333369", None).json()["data"]
            ["is_followed"],
        true
    );
    assert_eq!(
        http_probe(
            p,
            "POST",
            "/x/web-interface/archive/like/triple",
            Some("aid=79677524")
        )
        .json()["data"]["coin"],
        true
    );

    // DoSign & QR state knobs
    {
        let mut st = server.state().lock().unwrap();
        st.dosign_mode = DoSignMode::Retired;
        st.tv_qr_status = QrMockStatus::Waiting;
        st.web_qr_status = QrMockStatus::Expired;
    }
    assert_eq!(
        http_probe(p, "GET", "/xlive/web-ucenter/v1/sign/DoSign", None).json()["code"],
        1
    );
    assert_eq!(
        http_probe(p, "POST", "/x/passport-tv-login/qrcode/poll", Some("")).json()["code"],
        86039
    );
    assert_eq!(
        http_probe(
            p,
            "GET",
            "/x/passport-login/web/qrcode/poll?qrcode_key=k",
            None
        )
        .json()["data"]["code"],
        86038
    );

    // auth_logged_in == false -> -101 enforcement
    server.state().lock().unwrap().auth_logged_in = false;
    for (method, path) in [
        ("GET", "/x/member/web/exp/reward"),
        ("GET", "/x/v3/fav/folder/created/list-all"),
        ("GET", "/x/v2/history/toview"),
        ("GET", "/x/web-interface/history/cursor"),
        ("POST", "/room/v1/Room/update"),
        ("POST", "/room/v1/Room/startLive"),
        ("POST", "/room/v1/Room/stopLive"),
        ("GET", "/xlive/app-blink/v1/live/StopLiveData"),
        ("GET", "/xlive/web-ucenter/user/following"),
        ("POST", "/msg/send"),
        ("POST", "/x/web-interface/archive/like"),
        ("POST", "/x/web-interface/coin/add"),
        ("POST", "/x/web-interface/archive/like/triple"),
        ("POST", "/x/v2/reply/add"),
        ("POST", "/x/v2/reply/action"),
        ("POST", "/x/v2/reply/del"),
        ("POST", "/x/relation/modify"),
        ("GET", "/x/polymer/web-dynamic/v1/feed/all"),
        ("POST", "/login/exit/v2"),
        ("GET", "/x/passport-login/web/cookie/info"),
        ("POST", "/x/passport-login/web/cookie/refresh"),
        ("POST", "/x/passport-login/web/confirm/refresh"),
    ] {
        let resp = http_probe(p, method, path, if method == "POST" { Some("") } else { None });
        assert_eq!(
            resp.json()["code"],
            -101,
            "expected -101 for {method} {path} when logged out"
        );
    }

    // FaultConfig exact & wildcard prefix overrides + clear_fault / clear_all_faults
    server.set_fault(
        "/x/web-interface/nav",
        FaultConfig::http(500, b"boom".to_vec()),
    );
    assert_eq!(http_probe(p, "GET", "/x/web-interface/nav", None).status, 500);
    server.clear_fault("/x/web-interface/nav");
    assert_eq!(http_probe(p, "GET", "/x/web-interface/nav", None).status, 200);

    server.set_fault(
        "/x/web-interface/wbi/search/type",
        FaultConfig::api_code(-412, "blocked"),
    );
    assert_eq!(
        http_probe(p, "GET", "/x/web-interface/wbi/search/type", None).json()["code"],
        -412
    );

    server.set_fault("/x/v3/fav/resource/list", FaultConfig::data_null());
    assert!(
        http_probe(p, "GET", "/x/v3/fav/resource/list", None).json()["data"].is_null()
    );

    server.set_fault(
        "/x/v2/reply",
        FaultConfig::json(json!({ "code": 0, "data": { "replies": null } })),
    );
    assert!(http_probe(p, "GET", "/x/v2/reply", None).json()["data"]["replies"].is_null());

    server.set_fault("/pigt3PQ", FaultConfig::missing_location());
    let miss_loc = http_probe(p, "GET", "/pigt3PQ", None);
    assert_eq!(miss_loc.status, 200);
    assert!(miss_loc.header_values("location").is_empty());

    server.set_fault(
        "/hop1",
        FaultConfig::redirect("https://www.bilibili.com/video/BV1GJ411x7h7"),
    );
    let redir = http_probe(p, "GET", "/hop1", None);
    assert_eq!(redir.status, 302);
    assert_eq!(
        redir.header_values("location"),
        vec!["https://www.bilibili.com/video/BV1GJ411x7h7"]
    );

    server.set_fault("/x/web-interface/wbi/view", FaultConfig::EmptyBody);
    assert!(
        http_probe(p, "GET", "/x/web-interface/wbi/view", None)
            .body
            .is_empty()
    );

    server.set_fault("/main/hotword", FaultConfig::MalformedJson);
    assert_eq!(
        http_probe(p, "GET", "/main/hotword", None).body,
        b"{bad_json"
    );

    server.set_fault(
        "/correspond/1/*",
        FaultConfig::http(403, b"forbidden".to_vec()),
    );
    assert_eq!(
        http_probe(p, "GET", "/correspond/1/abcdef", None).status,
        403
    );

    server.clear_all_faults();
    assert_eq!(
        http_probe(p, "GET", "/correspond/1/abcdef", None).status,
        200
    );
}

#[test]
fn smoke_deflate_raw_stored_empty_small_and_multi_block() {
    // Empty input
    let empty_def = deflate_raw_stored(b"");
    assert_eq!(empty_def, vec![0x01, 0x00, 0x00, 0xFF, 0xFF]);
    assert_eq!(inflate_raw_stored(&empty_def), b"");

    // 1-byte input
    let one_def = deflate_raw_stored(b"x");
    assert_eq!(one_def, vec![0x01, 0x01, 0x00, 0xFE, 0xFF, b'x']);
    assert_eq!(inflate_raw_stored(&one_def), b"x");

    // Exact 65535-byte single stored block
    let exact_65535 = vec![b'a'; 65_535];
    let exact_def = deflate_raw_stored(&exact_65535);
    assert_eq!(exact_def.len(), 5 + 65_535);
    assert_eq!(exact_def[0], 0x01, "exact 65535-byte payload fits in 1 final block");
    assert_eq!(&exact_def[1..5], &[0xFF, 0xFF, 0x00, 0x00]);
    assert_eq!(inflate_raw_stored(&exact_def), exact_65535);

    // Small XML payload
    let small_xml = b"<i><d p=\"1,1,25,16777215,0,0,a,1\">hello</d></i>";
    let small_def = deflate_raw_stored(small_xml);
    assert_eq!(small_def[0], 0x01);
    assert_eq!(inflate_raw_stored(&small_def), small_xml);

    // > 70_000 bytes multi-block payload
    let mut big_payload = Vec::with_capacity(70_500);
    big_payload.extend_from_slice(b"<?xml version=\"1.0\"?><i>");
    while big_payload.len() < 70_496 {
        big_payload.extend_from_slice(b"<d p=\"1,1,25,16777215,0,0,a,1\">multi-block</d>");
    }
    big_payload.extend_from_slice(b"</i>");
    assert!(big_payload.len() > 70_000);

    let big_def = deflate_raw_stored(&big_payload);
    assert_eq!(big_def[0], 0x00, "first block must have BFINAL=0");
    let first_len = u16::from_le_bytes([big_def[1], big_def[2]]);
    let first_nlen = u16::from_le_bytes([big_def[3], big_def[4]]);
    assert_eq!(first_len, 65535);
    assert_eq!(first_nlen, 0x0000);
    let second_hdr = 5 + 65535;
    assert_eq!(big_def[second_hdr], 0x01, "second block must have BFINAL=1");
    assert_eq!(inflate_raw_stored(&big_def), big_payload);
}

#[test]
fn smoke_multi_set_cookie_and_rsa_spki_pem() {
    let server = MockBiliServer::start();
    let p = server.port();

    let poll_resp = http_probe(
        p,
        "GET",
        "/x/passport-login/web/qrcode/poll?qrcode_key=mock",
        None,
    );
    assert_eq!(poll_resp.status, 200);
    let poll_cookies = poll_resp.header_values("set-cookie");
    assert_eq!(poll_cookies.len(), 4);
    assert!(poll_cookies.iter().any(|c| c.starts_with("SESSDATA=")));
    assert!(poll_cookies.iter().any(|c| c.starts_with("bili_jct=")));
    assert!(poll_cookies.iter().any(|c| c.starts_with("DedeUserID=")));
    assert!(poll_cookies.iter().any(|c| c.starts_with("DedeUserID__ckMd5=")));

    let refresh_resp = http_probe(
        p,
        "POST",
        "/x/passport-login/web/cookie/refresh",
        Some("csrf=mock&refresh_csrf=mock&refresh_token=mock"),
    );
    assert_eq!(refresh_resp.status, 200);
    let refresh_cookies = refresh_resp.header_values("set-cookie");
    assert_eq!(refresh_cookies.len(), 4);
    assert!(refresh_cookies.iter().any(|c| c.contains("_refreshed")));

    let key_json = http_probe(p, "GET", "/x/passport-login/web/key", None).json();
    let pem = key_json["data"]["key"].as_str().expect("RSA PEM string");
    assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----"));
    assert!(pem.trim_end().ends_with("-----END PUBLIC KEY-----"));
    assert_eq!(pem, MOCK_RSA_PUBLIC_KEY_PEM);
    let parsed_key = RsaPublicKey::from_public_key_pem(pem).expect("parse 1024-bit SPKI PEM");
    assert_eq!(parsed_key.size(), 128, "1024-bit RSA key must be 128 bytes");
}

#[test]
fn smoke_test_env_cookie_isolation_and_ffmpeg_modes() {
    // Cookie fixture presets & permissions
    let env = TestEnv::new()
        .with_cookie_fixture(CookieFixture::valid())
        .with_alt_cookie_file("alt.json", CookieFixture::without_csrf());
    assert!(env.default_cookie_path().starts_with(env.temp_path()));
    assert_mode_0600(&env.default_cookie_path());
    assert_mode_0600(&env.alt_cookie_path("alt.json"));

    let loaded = env.read_cookie_file(&env.default_cookie_path());
    assert_eq!(loaded, CookieFixture::valid());
    let alt_loaded = env.read_cookie_file(&env.alt_cookie_path("alt.json"));
    assert!(alt_loaded.bili_jct.is_empty());

    assert!(CookieFixture::without_buvid().buvid3.is_empty());
    assert_eq!(CookieFixture::without_room_id().room_id, 0);
    assert!(CookieFixture::without_refresh_token().refresh_token.is_empty());

    let custom = CookieFixture::default()
        .with_sessdata("s2")
        .with_bili_jct("j2")
        .with_dede_user_id("99")
        .with_buvid("b3", "b4")
        .with_refresh_token("rt")
        .with_room_id(77)
        .with_live_key(Some(12345));
    assert_eq!(custom.sessdata, "s2");
    assert_eq!(custom.live_key, Some(12345));

    let work_file = env.write_work_file("sub/note.txt", b"hello");
    assert_eq!(std::fs::read(work_file).unwrap(), b"hello");

    // 1. without_ffmpeg -> no ffmpeg binary in bin_dir or PATH
    let env_missing = TestEnv::new().without_ffmpeg();
    assert!(!env_missing.fake_ffmpeg_path().exists());

    // 2. with_fake_ffmpeg -> -version, .mp4, .flv + ffmpeg.log
    let env_fake = TestEnv::new().with_fake_ffmpeg();
    let ff = env_fake.fake_ffmpeg_path();
    let ver_out = Command::new(&ff)
        .arg("-version")
        .env("PATH", env_fake.bin_dir())
        .env("FAKE_FFMPEG_LOG", env_fake.ffmpeg_log_path())
        .output()
        .expect("run fake ffmpeg -version");
    assert!(ver_out.status.success());
    assert!(String::from_utf8_lossy(&ver_out.stdout).contains("ffmpeg version 6.0-mock"));

    let mp4_out = env_fake.work_dir().join("out.mp4");
    let flv_out = env_fake.work_dir().join("out.flv");
    assert!(
        Command::new(&ff)
            .args(["-i", "in.m4s", mp4_out.to_str().unwrap()])
            .env("PATH", env_fake.bin_dir())
            .env("FAKE_FFMPEG_LOG", env_fake.ffmpeg_log_path())
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(&ff)
            .args(["-i", "in.flv", flv_out.to_str().unwrap()])
            .env("PATH", env_fake.bin_dir())
            .env("FAKE_FFMPEG_LOG", env_fake.ffmpeg_log_path())
            .status()
            .unwrap()
            .success()
    );

    let mp4_bytes = std::fs::read(&mp4_out).unwrap();
    assert_eq!(&mp4_bytes[4..12], b"ftypisom");
    let flv_bytes = std::fs::read(&flv_out).unwrap();
    assert_eq!(&flv_bytes[0..4], b"FLV\x01");

    let log_lines = env_fake.read_ffmpeg_log();
    assert_eq!(log_lines.len(), 3);
    assert_eq!(log_lines[0], "-version");
    assert!(log_lines[1].contains("out.mp4"));
    assert!(log_lines[2].contains("out.flv"));

    // 2b. Fake ffmpeg with FAKE_FFMPEG_LOG removed from env, PATH strictly restricted to env.bin_dir(),
    //     and uppercase container extensions (.MP4, .FLV)
    let env_default_log = TestEnv::new().with_fake_ffmpeg();
    let ff_default = env_default_log.fake_ffmpeg_path();
    let no_log_ver = Command::new(&ff_default)
        .arg("-version")
        .env("PATH", env_default_log.bin_dir())
        .env_remove("FAKE_FFMPEG_LOG")
        .output()
        .expect("run fake ffmpeg without FAKE_FFMPEG_LOG");
    assert!(no_log_ver.status.success());
    assert!(String::from_utf8_lossy(&no_log_ver.stdout).contains("ffmpeg version 6.0-mock"));

    let upper_mp4 = env_default_log.work_dir().join("UPPER.MP4");
    let upper_flv = env_default_log.work_dir().join("UPPER.FLV");
    assert!(
        Command::new(&ff_default)
            .args(["-i", "in.m4s", upper_mp4.to_str().unwrap()])
            .env("PATH", env_default_log.bin_dir())
            .env_remove("FAKE_FFMPEG_LOG")
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(&ff_default)
            .args(["-i", "in.flv", upper_flv.to_str().unwrap()])
            .env("PATH", env_default_log.bin_dir())
            .env_remove("FAKE_FFMPEG_LOG")
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(&std::fs::read(&upper_mp4).unwrap()[4..12], b"ftypisom");
    assert_eq!(&std::fs::read(&upper_flv).unwrap()[0..4], b"FLV\x01");
    let default_log_lines = env_default_log.read_ffmpeg_log();
    assert_eq!(default_log_lines.len(), 3);
    assert_eq!(default_log_lines[0], "-version");

    // 3. with_failing_ffmpeg -> FAKE_FFMPEG_FAIL=1 fails -version
    let env_fail = TestEnv::new().with_failing_ffmpeg();
    let fail_ver = Command::new(env_fail.fake_ffmpeg_path())
        .arg("-version")
        .env("FAKE_FFMPEG_FAIL", "1")
        .env("FAKE_FFMPEG_LOG", env_fail.ffmpeg_log_path())
        .output()
        .unwrap();
    assert_eq!(fail_ver.status.code(), Some(1));

    // 4. with_failing_ffmpeg_exec -> -version passes, mux fails
    let env_fail_exec = TestEnv::new().with_failing_ffmpeg_exec();
    let ok_ver = Command::new(env_fail_exec.fake_ffmpeg_path())
        .arg("-version")
        .env("FAKE_FFMPEG_FAIL", "exec")
        .env("FAKE_FFMPEG_LOG", env_fail_exec.ffmpeg_log_path())
        .output()
        .unwrap();
    assert!(ok_ver.status.success());
    let fail_mux = Command::new(env_fail_exec.fake_ffmpeg_path())
        .args(["-i", "in.m4s", "out.mp4"])
        .current_dir(env_fail_exec.work_dir())
        .env("FAKE_FFMPEG_FAIL", "exec")
        .env("FAKE_FFMPEG_LOG", env_fail_exec.ffmpeg_log_path())
        .output()
        .unwrap();
    assert_eq!(fail_mux.status.code(), Some(1));
}

#[test]
fn smoke_bt_output_helpers() {
    #[cfg(unix)]
    let ok_status = Command::new("/bin/sh")
        .args(["-c", "printf '{\"hello\":\"world\"}'"])
        .output()
        .unwrap();
    #[cfg(windows)]
    let ok_status = Command::new("powershell")
        .args(["-NoProfile", "-Command", "[Console]::Write('{\"hello\":\"world\"}')"])
        .output()
        .unwrap();
    let ok_out = BtOutput {
        status: ok_status.status,
        code: ok_status.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&ok_status.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&ok_status.stderr).into_owned(),
    };
    let ok_json = ok_out.assert_json_ok();
    assert_eq!(ok_json["hello"], "world");
    ok_out
        .assert_no_ansi()
        .assert_stdout_contains("world");

    #[cfg(unix)]
    let err_status = Command::new("/bin/sh")
        .args([
            "-c",
            "printf '{\"ok\":false,\"error\":{\"kind\":\"Input\",\"message\":\"bad\"}}' && printf 'diag' >&2 && exit 1",
        ])
        .output()
        .unwrap();
    #[cfg(windows)]
    let err_status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[Console]::Write('{\"ok\":false,\"error\":{\"kind\":\"Input\",\"message\":\"bad\"}}'); [Console]::Error.Write('diag'); exit 1",
        ])
        .output()
        .unwrap();
    let err_out = BtOutput {
        status: err_status.status,
        code: err_status.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&err_status.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&err_status.stderr).into_owned(),
    };
    err_out.assert_json_err(Some("Input"));
    err_out.assert_stderr_contains("diag");

    #[cfg(unix)]
    let empty_status = Command::new("/bin/sh").args(["-c", "true"]).output().unwrap();
    #[cfg(windows)]
    let empty_status = Command::new("powershell")
        .args(["-NoProfile", "-Command", "exit 0"])
        .output()
        .unwrap();
    let empty_out = BtOutput {
        status: empty_status.status,
        code: empty_status.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&empty_status.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&empty_status.stderr).into_owned(),
    };
    empty_out.assert_exit(0).assert_stdout_empty();
}

#[test]
fn smoke_bt_binary_opaque_box_cli_and_mock_server() {
    let server = MockBiliServer::start();

    // 1. bt --json metadata
    let env_meta = TestEnv::new().with_server(&server);
    let meta = env_meta.run_bt(&["--json"]).assert_json_ok();
    assert_eq!(meta["name"], "bt");
    let modules = meta["modules"].as_array().expect("modules array");
    for expected in [
        "auth",
        "live",
        "search",
        "video",
        "fav",
        "later",
        "history",
        "completions",
    ] {
        assert!(
            modules.iter().any(|m| m.as_str() == Some(expected)),
            "missing module {expected} in {modules:?}"
        );
    }

    // 2. bt --help (no ANSI)
    let help = env_meta.run_bt(&["--help"]);
    help.assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("bt")
        .assert_stdout_contains("auth")
        .assert_stdout_contains("live")
        .assert_stdout_contains("search")
        .assert_stdout_contains("video");

    // 3. bt --json search hot --limit 5
    server.reset_requests();
    let hot = env_meta
        .run_bt(&["--json", "search", "hot", "--limit", "5"])
        .assert_json_ok();
    assert_eq!(hot.as_array().expect("hot array").len(), 5);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.method == "GET" && r.path == "/x/web-interface/wbi/search/square")
    );

    // 4. bt --json search video Rust --page 1
    server.reset_requests();
    let search_v = env_meta
        .run_bt(&["--json", "search", "video", "Rust", "--page", "1"])
        .assert_json_ok();
    assert!(!search_v["result"].as_array().expect("search result").is_empty());
    let reqs = server.requests();
    assert!(reqs.iter().any(|r| r.path == "/x/web-interface/nav"));
    assert!(reqs.iter().any(|r| {
        r.path == "/x/web-interface/wbi/search/type" && r.has_wbi_signature()
    }));

    // 5. bt --json video info BV1GJ411x7h7
    server.reset_requests();
    let video = env_meta
        .run_bt(&["--json", "video", "info", "BV1GJ411x7h7"])
        .assert_json_ok();
    assert_eq!(video["bvid"], "BV1GJ411x7h7");
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/web-interface/wbi/view")
    );

    // 6. bt --json auth status with isolated valid cookies
    server.reset_requests();
    let env_auth = TestEnv::new().with_server(&server).with_valid_cookies();
    let status = env_auth.run_bt(&["--json", "auth", "status"]).assert_json_ok();
    assert_eq!(status["is_login"], true);
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/nav"
            && r.header("cookie")
                .is_some_and(|c| c.contains("SESSDATA="))
    }));
}

#[test]
fn smoke_chunked_transfer_encoding_and_percent_decode() {
    let server = MockBiliServer::start();
    let p = server.port();

    // 1. Chunked POST with literal `0\r\n\r\n` embedded inside the chunk payload
    let payload_with_marker = b"oid=79677524&message=prefix0\r\n\r\nsuffix_after_marker";
    let chunk_hdr = format!("{:x}\r\n", payload_with_marker.len());
    let mut stream = TcpStream::connect(("127.0.0.1", p)).expect("connect chunked 1");
    let head = format!(
        "POST /x/v2/reply/add HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nContent-Type: application/x-www-form-urlencoded\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(chunk_hdr.as_bytes()).unwrap();
    // Split write immediately after embedded `0\r\n\r\n` to simulate fragmented TCP delivery
    stream.write_all(&payload_with_marker[..32]).unwrap();
    stream.flush().unwrap();
    stream.write_all(&payload_with_marker[32..]).unwrap();
    stream.write_all(b"\r\n0\r\n\r\n").unwrap();
    stream.flush().unwrap();

    let resp1 = read_http_response(&mut stream);
    assert_eq!(resp1.status, 200);
    assert_eq!(resp1.json()["code"], 0);
    let recorded1 = server.requests().pop().expect("recorded chunked 1");
    assert_eq!(recorded1.body, payload_with_marker);
    assert_eq!(
        recorded1.form_param("message"),
        Some("prefix0\r\n\r\nsuffix_after_marker")
    );

    // 2. Chunked POST with chunk extension and HTTP trailer (`0\r\nX-Trailer: val\r\n\r\n`)
    // Must complete promptly without hitting the 5s socket read timeout.
    let started = Instant::now();
    let mut stream2 = TcpStream::connect(("127.0.0.1", p)).expect("connect chunked 2");
    let raw_req2 = format!(
        "POST /x/v2/reply/add HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nContent-Type: application/x-www-form-urlencoded\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n15;ext=1\r\noid=79677524&message=\r\n2\r\nhi\r\n0\r\nX-Trailer: val\r\n\r\n"
    );
    stream2.write_all(raw_req2.as_bytes()).unwrap();
    stream2.flush().unwrap();

    let resp2 = read_http_response(&mut stream2);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "chunked trailer request stalled for {elapsed:?}"
    );
    assert_eq!(resp2.status, 200);
    assert_eq!(resp2.json()["code"], 0);
    let recorded2 = server.requests().pop().expect("recorded chunked 2");
    assert_eq!(recorded2.body, b"oid=79677524&message=hi");
    assert_eq!(recorded2.form_param("message"), Some("hi"));

    // 3. decode_percent_plus must not treat `%+1` as hex `\u{1}` (`+` still decodes as space)
    let pct_resp = http_probe(
        p,
        "GET",
        "/x/web-interface/wbi/search/type?keyword=%25%2B1_%+1_%+A",
        None,
    );
    assert_eq!(pct_resp.status, 200);
    let recorded3 = server.requests().pop().expect("recorded percent decode");
    assert_eq!(recorded3.query_param("keyword"), Some("%+1_% 1_% A"));
}
