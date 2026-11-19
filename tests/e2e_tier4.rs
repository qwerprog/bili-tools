mod common;

use common::{
    CookieFixture, MOCK_JPEG_BYTES, MockBiliServer, MockFavItem, MockLaterItem, RecordedRequest,
    TestEnv, assert_mode_0600,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const MOCK_WBI_MIXIN_KEY: &str = "ea1db124af3c7062474693fa704f4ff8";

fn collect_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_files_recursive(&path));
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files
}

fn assert_valid_wbi_signature(req: &RecordedRequest, mixin_key: &str) {
    assert!(
        req.has_wbi_signature(),
        "expected valid wts & 32-char w_rid on {}?{}",
        req.path,
        req.raw_query
    );
    let w_rid = req.query_param("w_rid").expect("w_rid present");
    let mut pairs: Vec<(&str, &str)> = req
        .raw_query
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.split_once('='))
        .filter(|(k, _)| *k != "w_rid")
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let query: String = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let digest = format!("{:x}", md5::compute(format!("{query}{mixin_key}").as_bytes()));
    assert_eq!(
        w_rid, digest,
        "WBI w_rid mismatch for raw_query={}",
        req.raw_query
    );
}

fn json_contains_i64(val: &Value, expected: i64) -> bool {
    match val {
        Value::Number(n) => n.as_i64() == Some(expected),
        Value::String(s) => s == &expected.to_string(),
        Value::Array(arr) => arr.iter().any(|item| json_contains_i64(item, expected)),
        Value::Object(map) => map.values().any(|item| json_contains_i64(item, expected)),
        _ => false,
    }
}

fn json_contains_str(val: &Value, expected: &str) -> bool {
    match val {
        Value::String(s) => s.contains(expected),
        Value::Array(arr) => arr.iter().any(|item| json_contains_str(item, expected)),
        Value::Object(map) => map.values().any(|item| json_contains_str(item, expected)),
        _ => false,
    }
}

fn json_find_key_i64(val: &Value, key: &str) -> Option<i64> {
    match val {
        Value::Object(map) => {
            if let Some(v) = map.get(key) {
                if let Some(n) = v.as_i64() {
                    return Some(n);
                }
                if let Some(s) = v.as_str()
                    && let Ok(n) = s.parse::<i64>()
                {
                    return Some(n);
                }
            }
            for child in map.values() {
                if let Some(found) = json_find_key_i64(child, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|item| json_find_key_i64(item, key)),
        _ => None,
    }
}

fn json_find_key_bool(val: &Value, key: &str) -> Option<bool> {
    match val {
        Value::Object(map) => {
            if let Some(b) = map.get(key).and_then(Value::as_bool) {
                return Some(b);
            }
            for child in map.values() {
                if let Some(found) = json_find_key_bool(child, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|item| json_find_key_bool(item, key)),
        _ => None,
    }
}

#[test]
fn t4_01_full_auth_lifecycle() {
    let server = MockBiliServer::start();
    {
        let mut st = server.state().lock().unwrap();
        st.sessdata = "s1".to_string();
        st.bili_jct = "j1".to_string();
        st.refresh_token = "rt1".to_string();
    }
    let env = TestEnv::new().with_server(&server);
    let s_json = env.alt_cookie_path("s.json");
    let s_str = s_json.to_str().expect("utf8 s.json path");

    // Step 1: login --method cookie
    let login_json = env
        .run_bt(&[
            "-u",
            s_str,
            "--json",
            "auth",
            "login",
            "--method",
            "cookie",
            "--sessdata",
            "s1",
            "--bili-jct",
            "j1",
        ])
        .assert_json_ok();
    assert_eq!(login_json["success"], true);
    assert_eq!(login_json["uid"], "293793435");
    assert_eq!(login_json["room_id"], 31686846);
    assert_mode_0600(&s_json);
    assert!(!env.default_cookie_path().exists());
    let cookies_1 = env.read_cookie_file(&s_json);
    assert!(!cookies_1.buvid3.is_empty());
    assert!(!cookies_1.buvid4.is_empty());
    assert_eq!(cookies_1.room_id, 31686846);

    // Step 2: status
    let status_1 = env
        .run_bt(&["-u", s_str, "--json", "auth", "status"])
        .assert_json_ok();
    assert_eq!(status_1["is_login"], true);
    assert_eq!(status_1["mid"], 293793435);
    assert_eq!(status_1["room_id"], 31686846);

    // Step 3: daily
    let daily = env
        .run_bt(&["-u", s_str, "--json", "auth", "daily"])
        .assert_json_ok();
    assert_eq!(daily["exp_reward"]["login"], true);
    assert_eq!(daily["live_sign"]["signed"], true);

    // Prepare Step 4: enable need_refresh & seed refresh_token in s.json
    {
        server.state().lock().unwrap().need_refresh = true;
    }
    let seeded = env.read_cookie_file(&s_json).with_refresh_token("rt1");
    env.write_cookie_file(&s_json, &seeded);

    // Step 4: refresh
    server.reset_requests();
    let refresh_json = env
        .run_bt(&["-u", s_str, "--json", "auth", "refresh"])
        .assert_json_ok();
    assert_eq!(refresh_json["need_refresh"], true);
    assert_eq!(refresh_json["success"], true);

    let refresh_reqs = server.requests();
    assert!(
        refresh_reqs
            .iter()
            .any(|r| r.path == "/x/passport-login/web/cookie/info")
    );
    assert!(
        refresh_reqs
            .iter()
            .any(|r| r.path.starts_with("/correspond/1/"))
    );
    assert!(
        refresh_reqs
            .iter()
            .any(|r| r.method == "POST" && r.path == "/x/passport-login/web/cookie/refresh")
    );
    assert!(
        refresh_reqs
            .iter()
            .any(|r| r.method == "POST" && r.path == "/x/passport-login/web/confirm/refresh")
    );
    assert_mode_0600(&s_json);
    let cookies_2 = env.read_cookie_file(&s_json);
    assert_eq!(cookies_2.sessdata, "s1_refreshed");
    assert_eq!(cookies_2.bili_jct, "j1_refreshed");
    assert_eq!(cookies_2.refresh_token, "rt1_refreshed");

    // Step 5: status uses refreshed cookies
    server.reset_requests();
    let status_2 = env
        .run_bt(&["-u", s_str, "--json", "auth", "status"])
        .assert_json_ok();
    assert_eq!(status_2["is_login"], true);
    let nav_req = server
        .requests()
        .into_iter()
        .find(|r| r.path == "/x/web-interface/nav")
        .expect("nav request after refresh");
    let cookie_hdr = nav_req.header("cookie").expect("cookie header on nav");
    assert!(cookie_hdr.contains("SESSDATA=s1_refreshed"));
    assert!(cookie_hdr.contains("bili_jct=j1_refreshed"));

    // Step 6: logout
    server.reset_requests();
    let logout_json = env
        .run_bt(&["-u", s_str, "--json", "auth", "logout"])
        .assert_json_ok();
    assert_eq!(logout_json["success"], true);
    let exit_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/login/exit/v2")
        .expect("POST /login/exit/v2");
    assert_eq!(exit_req.form_param("biliCSRF"), Some("j1_refreshed"));
    assert!(!s_json.exists());
    {
        let st = server.state().lock().unwrap();
        assert!(!st.auth_logged_in);
    }

    // Step 7: status after logout
    let status_3 = env
        .run_bt(&["-u", s_str, "--json", "auth", "status"])
        .assert_json_ok();
    assert_eq!(status_3["is_login"], false);
    assert!(!s_json.exists());
}

#[test]
fn t4_02_video_resolution_inspection_and_engagement() {
    let server = MockBiliServer::start();
    {
        let mut st = server.state().lock().unwrap();
        st.short_links.insert(
            "vid99".to_string(),
            "https://www.bilibili.com/video/BV1uJ411r7hL".to_string(),
        );
    }
    let env = TestEnv::new()
        .with_server(&server)
        .with_alt_cookie_file("s.json", CookieFixture::valid());
    let s_json = env.alt_cookie_path("s.json");
    let s_str = s_json.to_str().expect("utf8 s.json");

    // 1. resolve short link
    let res = env
        .run_bt(&["--json", "resolve", "https://b23.tv/vid99"])
        .assert_json_ok();
    assert_eq!(res["kind"], "video");
    assert_eq!(res["bvid"], "BV1uJ411r7hL");
    assert_eq!(res["network_used"], true);

    // 2. video info
    let info = env
        .run_bt(&["--json", "video", "info", "https://b23.tv/vid99"])
        .assert_json_ok();
    assert_eq!(info["bvid"], "BV1uJ411r7hL");
    assert_eq!(info["pages"].as_array().map(Vec::len), Some(2));

    // 3. video streams
    let streams = env
        .run_bt(&["--json", "video", "streams", "BV1uJ411r7hL", "-p", "1"])
        .assert_json_ok();
    assert!(json_contains_i64(&streams, 80));
    assert!(json_contains_str(&streams, "avc1"));

    // 4. video subtitle
    let sub_srt = env.work_dir().join("sub.srt");
    let sub_srt_str = sub_srt.to_str().expect("utf8 sub.srt");
    env.run_bt(&[
        "video",
        "subtitle",
        "BV1uJ411r7hL",
        "--format",
        "srt",
        "-o",
        sub_srt_str,
    ])
    .assert_exit(0);
    let sub_text = fs::read_to_string(&sub_srt).expect("sub.srt written");
    assert!(sub_text.contains("00:00:00,500 --> 00:00:02,500"));
    assert!(sub_text.contains("Hello Bilibili"));

    // 5. video summary (WBI signed)
    server.reset_requests();
    let summary = env
        .run_bt(&["--json", "video", "summary", "BV1uJ411r7hL"])
        .assert_json_ok();
    assert!(json_contains_str(&summary, "测试视频的 AI 总结"));
    let summary_req = server
        .requests()
        .into_iter()
        .find(|r| r.path == "/x/web-interface/view/conclusion/get")
        .expect("conclusion/get request");
    assert_valid_wbi_signature(&summary_req, MOCK_WBI_MIXIN_KEY);

    // 6. tags & related
    let tags = env
        .run_bt(&["--json", "video", "tags", "BV1uJ411r7hL"])
        .assert_json_ok();
    assert!(json_contains_str(&tags, "Rust"));
    let related = env
        .run_bt(&["--json", "video", "related", "BV1uJ411r7hL"])
        .assert_json_ok();
    assert!(json_contains_str(&related, "BV17x411w7KC"));

    // 7. danmaku ASS
    let dm_ass = env.work_dir().join("dm.ass");
    let dm_ass_str = dm_ass.to_str().expect("utf8 dm.ass");
    env.run_bt(&[
        "video",
        "danmaku",
        "BV1uJ411r7hL",
        "--format",
        "ass",
        "-o",
        dm_ass_str,
    ])
    .assert_exit(0);
    let ass_text = fs::read_to_string(&dm_ass).expect("dm.ass written");
    assert!(ass_text.contains("[Script Info]"));
    assert!(ass_text.contains("[V4+ Styles]"));
    assert!(ass_text.contains("[Events]"));
    assert!(ass_text.contains("测试弹幕一条"));

    // 8. comment post
    server.reset_requests();
    let post_json = env
        .run_bt(&[
            "-u",
            s_str,
            "--json",
            "video",
            "comment",
            "post",
            "BV1uJ411r7hL",
            "Nice!",
        ])
        .assert_json_ok();
    let rpid = json_find_key_i64(&post_json, "rpid")
        .expect("comment post JSON should contain rpid");
    assert_eq!(rpid, 104192624481);
    let add_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/v2/reply/add")
        .expect("POST /x/v2/reply/add");
    assert_eq!(add_req.form_param("csrf"), Some("mock_bili_jct_csrf"));
    assert_eq!(add_req.form_param("oid"), Some("79677524"));
    assert_eq!(add_req.form_param("message"), Some("Nice!"));

    // 9. comments list
    let comments = env
        .run_bt(&[
            "--json",
            "video",
            "comments",
            "BV1uJ411r7hL",
            "--sort",
            "time",
        ])
        .assert_json_ok();
    assert!(json_contains_str(&comments, "Nice!"));
    assert!(json_contains_i64(&comments, rpid));

    // 10. comment like
    server.reset_requests();
    let rpid_str = rpid.to_string();
    env.run_bt(&[
        "-u",
        s_str,
        "--json",
        "video",
        "comment",
        "like",
        "BV1uJ411r7hL",
        &rpid_str,
    ])
    .assert_json_ok();
    let act_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/v2/reply/action")
        .expect("POST /x/v2/reply/action");
    assert_eq!(act_req.form_param("action"), Some("1"));
    assert_eq!(act_req.form_param("rpid"), Some(rpid_str.as_str()));
    assert_eq!(act_req.form_param("csrf"), Some("mock_bili_jct_csrf"));
    {
        let st = server.state().lock().unwrap();
        let comment = st.comments.iter().find(|c| c.rpid == rpid).expect("comment");
        assert_eq!(comment.like, 1);
    }

    // 11. like
    let like_json = env
        .run_bt(&["-u", s_str, "--json", "video", "like", "BV1uJ411r7hL"])
        .assert_json_ok();
    assert_eq!(like_json["liked"], true);
    {
        assert!(server.state().lock().unwrap().liked_videos.contains(&79677524));
    }

    // 12. coin
    let coin_json = env
        .run_bt(&[
            "-u",
            s_str,
            "--json",
            "video",
            "coin",
            "BV1uJ411r7hL",
            "-n",
            "2",
            "--also-like",
        ])
        .assert_json_ok();
    assert_eq!(coin_json["coins"], 2);
    assert_eq!(coin_json["also_like"], true);
    {
        assert_eq!(
            server.state().lock().unwrap().coined_videos.get(&79677524),
            Some(&2)
        );
    }

    // 13. triple
    let triple_json = env
        .run_bt(&["-u", s_str, "--json", "video", "triple", "BV1uJ411r7hL"])
        .assert_json_ok();
    assert_eq!(triple_json["like"], true);
    assert_eq!(triple_json["coin"], true);
    assert_eq!(triple_json["fav"], true);
}

#[test]
fn t4_03_video_download_and_sidecar_pipeline() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let dl_dir = env.work_dir().join("dl");
    fs::create_dir_all(&dl_dir).expect("create dl dir");
    let dl_tpl = dl_dir.join("{bvid}_p{page}.mp4");
    let dl_tpl_str = dl_tpl.to_str().expect("utf8 dl template");

    // 1. Multi-page download with sidecars
    let dl_out = env.run_bt(&[
        "--json",
        "video",
        "download",
        "https://www.bilibili.com/video/BV1uJ411r7hL",
        "-p",
        "1-2",
        "--quality",
        "80",
        "-e",
        "avc",
        "--danmaku",
        "--subtitle",
        "--cover",
        "-o",
        dl_tpl_str,
    ]);
    assert!(dl_out.stderr.trim().is_empty());
    dl_out.assert_json_ok();

    let p1 = dl_dir.join("BV1uJ411r7hL_p1.mp4");
    let p2 = dl_dir.join("BV1uJ411r7hL_p2.mp4");
    assert!(
        fs::read(&p1)
            .expect("p1 mp4 exists")
            .starts_with(b"\x00\x00\x00\x14ftypisom")
    );
    assert!(
        fs::read(&p2)
            .expect("p2 mp4 exists")
            .starts_with(b"\x00\x00\x00\x14ftypisom")
    );

    let files = collect_files_recursive(&dl_dir);
    let cover_files: Vec<&PathBuf> = files
        .iter()
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("jpg" | "jpeg")))
        .collect();
    assert!(!cover_files.is_empty(), "expected cover image in {files:?}");
    for cover in cover_files {
        assert_eq!(fs::read(cover).unwrap(), MOCK_JPEG_BYTES);
    }

    let sub_files: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("srt"))
        .collect();
    assert!(!sub_files.is_empty(), "expected srt sidecar in {files:?}");
    for sub in sub_files {
        assert!(fs::read_to_string(sub).unwrap().contains("Hello Bilibili"));
    }

    let dm_files: Vec<&PathBuf> = files
        .iter()
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("xml" | "ass")))
        .collect();
    assert!(!dm_files.is_empty(), "expected danmaku sidecar in {files:?}");
    for dm in dm_files {
        assert!(fs::read_to_string(dm).unwrap().contains("测试弹幕一条"));
    }

    let ff_log = env.read_ffmpeg_log();
    assert!(
        ff_log
            .iter()
            .any(|l| l.contains("-i") && l.ends_with(p1.to_str().unwrap())),
        "expected mux log for p1 in {ff_log:?}"
    );
    assert!(
        ff_log
            .iter()
            .any(|l| l.contains("-i") && l.ends_with(p2.to_str().unwrap())),
        "expected mux log for p2 in {ff_log:?}"
    );
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/mock/media/video_80_avc.m4s"),
        "expected AVC 80 stream download"
    );

    // 2. Audio-only download
    let audio_out = dl_dir.join("only_audio.m4a");
    let audio_str = audio_out.to_str().expect("utf8 audio_out");
    let audio_res = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "--audio-only",
        "-o",
        audio_str,
    ]);
    assert!(audio_res.stderr.trim().is_empty());
    audio_res.assert_json_ok();
    assert!(!fs::read(&audio_out).expect("only_audio.m4a exists").is_empty());

    // 3. Missing ffmpeg guard failure
    let env_no_ff = env.without_ffmpeg();
    let fail_out = dl_dir.join("fail.mp4");
    let fail_str = fail_out.to_str().expect("utf8 fail_out");
    let fail_res = env_no_ff.run_bt(&["--json", "video", "download", "BV1uJ411r7hL", "-o", fail_str]);
    assert!(fail_res.stderr.trim().is_empty());
    fail_res.assert_json_err(Some("ExternalTool"));
    assert!(!fail_out.exists());
}

#[test]
fn t4_04_live_broadcast_danmaku_and_recording() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_fake_ffmpeg()
        .with_alt_cookie_file("s.json", CookieFixture::valid());
    let s_json = env.alt_cookie_path("s.json");
    let s_str = s_json.to_str().expect("utf8 s.json");

    // 1. live area
    let areas = env
        .run_bt(&["--json", "live", "area", "网游"])
        .assert_json_ok();
    assert!(json_contains_str(&areas, "86"));

    // 2. live start
    let start_json = env
        .run_bt(&[
            "-u",
            s_str,
            "--json",
            "live",
            "start",
            "-a",
            "86",
            "-t",
            "初始标题",
            "-y",
        ])
        .assert_json_ok();
    assert_eq!(start_json["success"], true);
    assert!(start_json["rtmp_addr"].is_string());
    assert!(
        start_json["rtmp_code_masked"].is_string() || start_json["rtmp_code"].is_string(),
        "expected rtmp_code_masked or rtmp_code in {start_json}"
    );
    {
        assert_eq!(server.state().lock().unwrap().rooms[&31686846].live_status, 1);
    }
    let stream_info = env
        .default_cookie_path()
        .parent()
        .expect("cookie parent dir")
        .join("stream_info.txt");
    assert!(stream_info.exists());
    assert_mode_0600(&stream_info);
    assert_eq!(env.read_cookie_file(&s_json).live_key, Some(608336837537435443));

    // 3. live start -y while already live MUST NOT call stopLive
    server.reset_requests();
    let _ = env.run_bt(&["-u", s_str, "--json", "live", "start", "-y"]);
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.path == "/room/v1/Room/stopLive")
            .count(),
        0,
        "live start -y while live must NEVER call stopLive!"
    );
    {
        assert_eq!(server.state().lock().unwrap().rooms[&31686846].live_status, 1);
    }

    // 4. live title
    let title_json = env
        .run_bt(&["-u", s_str, "--json", "live", "title", "更新标题"])
        .assert_json_ok();
    assert_eq!(title_json["title"], "更新标题");
    {
        assert_eq!(
            server.state().lock().unwrap().rooms[&31686846].title,
            "更新标题"
        );
    }

    // 5. live status
    let status_json = env
        .run_bt(&["-u", s_str, "--json", "live", "status"])
        .assert_json_ok();
    assert_eq!(status_json["is_live"], true);
    assert_eq!(status_json["room_info"]["title"], "更新标题");

    // 6. live info
    let info_json = env
        .run_bt(&["--json", "live", "info", "31686846"])
        .assert_json_ok();
    assert!(json_contains_i64(&info_json, 31686846));
    assert!(json_contains_str(&info_json, "更新标题"));

    // 7. live stream
    let stream_json = env
        .run_bt(&["--json", "live", "stream", "31686846"])
        .assert_json_ok();
    assert!(json_contains_str(&stream_json, "/mock/live/31686846.flv"));

    // 8. live send
    server.reset_requests();
    env.run_bt(&["-u", s_str, "--json", "live", "send", "31686846", "开播啦"])
        .assert_json_ok();
    let send_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/msg/send")
        .expect("POST /msg/send");
    assert_eq!(send_req.form_param("msg"), Some("开播啦"));
    assert_eq!(send_req.form_param("roomid"), Some("31686846"));
    assert_eq!(send_req.form_param("csrf"), Some("mock_bili_jct_csrf"));

    // 9. live danmaku
    let dm_json = env
        .run_bt(&["--json", "live", "danmaku", "31686846", "--limit", "5"])
        .assert_json_ok();
    assert!(json_contains_str(&dm_json, "开播啦"));

    // 10. live record
    let live_flv = env.work_dir().join("live.flv");
    let live_flv_str = live_flv.to_str().expect("utf8 live.flv");
    env.run_bt(&["live", "record", "31686846", "-d", "1s", "-o", live_flv_str])
        .assert_exit(0);
    assert!(
        fs::read(&live_flv)
            .expect("live.flv exists")
            .starts_with(b"FLV\x01")
    );
    let ff_log = env.read_ffmpeg_log();
    assert!(
        ff_log
            .iter()
            .any(|l| l.contains("/mock/live/31686846.flv") && l.contains("-t 1")),
        "expected record log with -t 1 and stream url in {ff_log:?}"
    );

    // 11. live stop
    server.reset_requests();
    let stop_json = env
        .run_bt(&["-u", s_str, "--json", "live", "stop"])
        .assert_json_ok();
    assert_eq!(stop_json["success"], true);
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.method == "POST" && r.path == "/room/v1/Room/stopLive")
            .count(),
        1
    );
    {
        assert_eq!(server.state().lock().unwrap().rooms[&31686846].live_status, 0);
    }
}

#[test]
fn t4_05_favorites_curation_and_backup_lifecycle() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_alt_cookie_file("s.json", CookieFixture::valid());
    let s_json = env.alt_cookie_path("s.json");
    let s_str = s_json.to_str().expect("utf8 s.json");

    // 1. fav create
    server.reset_requests();
    let create_json = env
        .run_bt(&["-u", s_str, "--json", "fav", "create", "技术资料", "--private"])
        .assert_json_ok();
    let fid = json_find_key_i64(&create_json, "id")
        .or_else(|| json_find_key_i64(&create_json, "media_id"))
        .expect("fav create JSON should contain id or media_id");
    let fid_str = fid.to_string();
    let create_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/v3/fav/folder/add")
        .expect("POST /x/v3/fav/folder/add");
    assert_eq!(create_req.form_param("title"), Some("技术资料"));
    assert_eq!(create_req.form_param("privacy"), Some("1"));
    assert_eq!(create_req.form_param("csrf"), Some("mock_bili_jct_csrf"));

    // 2. fav list
    let list_json = env
        .run_bt(&["-u", s_str, "--json", "fav", "list"])
        .assert_json_ok();
    assert!(json_contains_i64(&list_json, fid));
    assert!(json_contains_str(&list_json, "技术资料"));

    // 3. fav add URL
    server.reset_requests();
    env.run_bt(&[
        "-u",
        s_str,
        "--json",
        "fav",
        "add",
        &fid_str,
        "https://www.bilibili.com/video/BV1uJ411r7hL",
    ])
    .assert_json_ok();
    let add_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/v3/fav/resource/deal")
        .expect("POST /x/v3/fav/resource/deal add");
    assert_eq!(add_req.form_param("add_media_ids"), Some(fid_str.as_str()));
    assert_eq!(add_req.form_param("rid"), Some("79677524"));

    // 4. fav show
    let show_json = env
        .run_bt(&["-u", s_str, "--json", "fav", "show", &fid_str])
        .assert_json_ok();
    assert_eq!(show_json["medias"].as_array().map(Vec::len), Some(1));
    assert_eq!(show_json["medias"][0]["bvid"], "BV1uJ411r7hL");

    // 5. fav export CSV
    let fav_csv = env.work_dir().join("fav.csv");
    let fav_csv_str = fav_csv.to_str().expect("utf8 fav.csv");
    env.run_bt(&[
        "-u",
        s_str,
        "fav",
        "export",
        &fid_str,
        "--format",
        "csv",
        "-o",
        fav_csv_str,
    ])
    .assert_exit(0);
    let csv_text = fs::read_to_string(&fav_csv).expect("fav.csv exists");
    assert!(csv_text.starts_with("bvid,aid,title,upper_name,play,duration,link\n"));
    assert!(csv_text.contains("BV1uJ411r7hL"));

    // 6. fav export JSON
    let fav_json_path = env.work_dir().join("fav.json");
    let fav_json_str = fav_json_path.to_str().expect("utf8 fav.json");
    env.run_bt(&[
        "-u",
        s_str,
        "fav",
        "export",
        &fid_str,
        "--format",
        "json",
        "-o",
        fav_json_str,
    ])
    .assert_exit(0);
    let exported: Value =
        serde_json::from_str(&fs::read_to_string(&fav_json_path).expect("fav.json exists"))
            .expect("parse fav.json");
    assert_eq!(exported["count"], 1);
    assert_eq!(exported["medias"][0]["bvid"], "BV1uJ411r7hL");

    // 7. fav remove
    server.reset_requests();
    env.run_bt(&[
        "-u",
        s_str,
        "--json",
        "fav",
        "remove",
        &fid_str,
        "BV1uJ411r7hL",
    ])
    .assert_json_ok();
    let del_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/v3/fav/resource/deal")
        .expect("POST /x/v3/fav/resource/deal remove");
    assert_eq!(del_req.form_param("del_media_ids"), Some(fid_str.as_str()));
    {
        let st = server.state().lock().unwrap();
        let folder = st.fav_folders.iter().find(|f| f.id == fid).unwrap();
        assert!(folder.items.is_empty());
    }

    // 8. Push invalid item then fav clean -y
    {
        let mut st = server.state().lock().unwrap();
        let folder = st.fav_folders.iter_mut().find(|f| f.id == fid).unwrap();
        folder.items.push(MockFavItem {
            aid: 999999,
            bvid: "BV1xx411c7mD".to_string(),
            title: "已失效视频".to_string(),
            duration: 60,
            upper_mid: 293793435,
            upper_name: "测试UP主".to_string(),
            valid: false,
        });
    }
    server.reset_requests();
    env.run_bt(&["-u", s_str, "-y", "--json", "fav", "clean", &fid_str])
        .assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v3/fav/resource/clean"
            && r.form_param("media_id") == Some(fid_str.as_str())
    }));
    {
        let st = server.state().lock().unwrap();
        let folder = st.fav_folders.iter().find(|f| f.id == fid).unwrap();
        assert!(folder.items.is_empty());
    }

    // 9. fav delete -y & verify list
    server.reset_requests();
    env.run_bt(&["-u", s_str, "-y", "--json", "fav", "delete", &fid_str])
        .assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v3/fav/folder/del"
            && r.form_param("media_ids") == Some(fid_str.as_str())
    }));
    let list_after = env
        .run_bt(&["-u", s_str, "--json", "fav", "list"])
        .assert_json_ok();
    assert!(!json_contains_i64(&list_after, fid));
}

#[test]
fn t4_06_creator_discovery_follow_feed_and_rankings() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_alt_cookie_file("s.json", CookieFixture::valid());
    let s_json = env.alt_cookie_path("s.json");
    let s_str = s_json.to_str().expect("utf8 s.json");

    // 1. search user
    server.reset_requests();
    let search_json = env
        .run_bt(&["--json", "search", "user", "测试", "--order", "fans"])
        .assert_json_ok();
    let mid = json_find_key_i64(&search_json, "mid")
        .expect("search user JSON should contain mid");
    assert_eq!(mid, 293793435);
    let mid_str = mid.to_string();
    let search_req = server
        .requests()
        .into_iter()
        .find(|r| r.path == "/x/web-interface/wbi/search/type")
        .expect("search/type request");
    assert_eq!(search_req.query_param("search_type"), Some("bili_user"));
    assert_eq!(search_req.query_param("order"), Some("fans"));
    assert_valid_wbi_signature(&search_req, MOCK_WBI_MIXIN_KEY);

    // 2. user info via space URL
    server.reset_requests();
    let user_url = format!("https://space.bilibili.com/{mid}");
    let info_before = env
        .run_bt(&["--json", "user", "info", &user_url])
        .assert_json_ok();
    assert!(json_contains_i64(&info_before, mid));
    assert!(json_contains_i64(&info_before, 50000));
    assert_eq!(json_find_key_bool(&info_before, "is_followed"), Some(false));
    let info_req = server
        .requests()
        .into_iter()
        .find(|r| r.path == "/x/space/wbi/acc/info")
        .expect("acc/info request");
    assert_valid_wbi_signature(&info_req, MOCK_WBI_MIXIN_KEY);

    // 3. user videos
    server.reset_requests();
    let videos_json = env
        .run_bt(&[
            "--json",
            "user",
            "videos",
            &mid_str,
            "--order",
            "click",
            "--page-size",
            "5",
        ])
        .assert_json_ok();
    assert!(json_contains_str(&videos_json, "BV1uJ411r7hL"));
    let arc_req = server
        .requests()
        .into_iter()
        .find(|r| r.path == "/x/space/wbi/arc/search")
        .expect("arc/search request");
    assert_eq!(arc_req.query_param("order"), Some("click"));
    assert_eq!(arc_req.query_param("ps"), Some("5"));
    assert_valid_wbi_signature(&arc_req, MOCK_WBI_MIXIN_KEY);

    // 4. user dynamics
    let dyn_json = env
        .run_bt(&["--json", "user", "dynamics", &mid_str])
        .assert_json_ok();
    assert!(json_contains_str(&dyn_json, "9000000001"));

    // 5. user follow + verify state & user info
    server.reset_requests();
    env.run_bt(&["-u", s_str, "--json", "user", "follow", &mid_str])
        .assert_json_ok();
    let follow_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/relation/modify")
        .expect("follow POST");
    assert_eq!(follow_req.form_param("act"), Some("1"));
    assert_eq!(follow_req.form_param("fid"), Some(mid_str.as_str()));
    assert_eq!(follow_req.form_param("csrf"), Some("mock_bili_jct_csrf"));
    {
        assert!(server.state().lock().unwrap().followed_mids.contains(&mid));
    }
    let info_followed = env
        .run_bt(&["--json", "user", "info", &mid_str])
        .assert_json_ok();
    assert_eq!(json_find_key_bool(&info_followed, "is_followed"), Some(true));

    // 6. feed
    server.reset_requests();
    let feed_json = env
        .run_bt(&["-u", s_str, "--json", "feed", "--type", "video"])
        .assert_json_ok();
    assert!(json_contains_str(&feed_json, "9000000002"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/polymer/web-dynamic/v1/feed/all"
            && r.query_param("type") == Some("video")
            && r.header("cookie").is_some()
    }));

    // 7. rank video
    let rank_vid = env
        .run_bt(&["--json", "rank", "video", "--rid", "0", "--type", "all"])
        .assert_json_ok();
    assert!(json_contains_str(&rank_vid, "BV1uJ411r7hL"));

    // 8. rank popular
    let rank_pop = env
        .run_bt(&["--json", "rank", "popular", "--page", "1", "--page-size", "5"])
        .assert_json_ok();
    assert!(json_contains_str(&rank_pop, "BV1uJ411r7hL"));

    // 9. user unfollow + verify state & user info
    server.reset_requests();
    env.run_bt(&["-u", s_str, "--json", "user", "unfollow", &mid_str])
        .assert_json_ok();
    let unfollow_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/relation/modify")
        .expect("unfollow POST");
    assert_eq!(unfollow_req.form_param("act"), Some("2"));
    assert_eq!(unfollow_req.form_param("fid"), Some(mid_str.as_str()));
    assert_eq!(unfollow_req.form_param("csrf"), Some("mock_bili_jct_csrf"));
    {
        assert!(!server.state().lock().unwrap().followed_mids.contains(&mid));
    }
    let info_unfollowed = env
        .run_bt(&["--json", "user", "info", &mid_str])
        .assert_json_ok();
    assert_eq!(
        json_find_key_bool(&info_unfollowed, "is_followed"),
        Some(false)
    );
}

#[test]
fn t4_07_watch_later_and_history_management() {
    let server = MockBiliServer::start();
    {
        let mut st = server.state().lock().unwrap();
        st.toview.retain(|item| item.aid != 79677524);
        st.toview.push(MockLaterItem {
            aid: 888888,
            bvid: "BV1xx411c7mD".to_string(),
            title: "稍后再看保留视频".to_string(),
            duration: 150,
            progress: 0,
            viewed: false,
        });
    }
    let env = TestEnv::new()
        .with_server(&server)
        .with_alt_cookie_file("s.json", CookieFixture::valid());
    let s_json = env.alt_cookie_path("s.json");
    let s_str = s_json.to_str().expect("utf8 s.json");

    // 1. later add URL
    env.run_bt(&[
        "-u",
        s_str,
        "--json",
        "later",
        "add",
        "https://www.bilibili.com/video/BV1uJ411r7hL",
    ])
    .assert_json_ok();
    {
        let st = server.state().lock().unwrap();
        assert_eq!(st.toview.len(), 3);
        assert!(st.toview.iter().any(|i| i.aid == 79677524));
    }

    // 2. later list
    let later_list = env
        .run_bt(&["-u", s_str, "--json", "later", "list"])
        .assert_json_ok();
    assert!(json_contains_str(&later_list, "BV1uJ411r7hL"));
    assert!(json_contains_str(&later_list, "BV1GJ411x7h7"));

    // 3. later remove
    server.reset_requests();
    env.run_bt(&["-u", s_str, "--json", "later", "remove", "BV1uJ411r7hL"])
        .assert_json_ok();
    let del_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/v2/history/toview/del")
        .expect("POST /x/v2/history/toview/del");
    assert_eq!(del_req.form_param("aid"), Some("79677524"));
    assert_eq!(del_req.form_param("csrf"), Some("mock_bili_jct_csrf"));
    {
        let st = server.state().lock().unwrap();
        assert!(!st.toview.iter().any(|i| i.aid == 79677524));
    }

    // 4. later clear --viewed-only -y
    server.reset_requests();
    env.run_bt(&[
        "-u",
        s_str,
        "-y",
        "--json",
        "later",
        "clear",
        "--viewed-only",
    ])
    .assert_json_ok();
    let viewed_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/v2/history/toview/del")
        .expect("POST /x/v2/history/toview/del viewed");
    assert_eq!(viewed_req.form_param("viewed"), Some("true"));
    assert_eq!(viewed_req.form_param("csrf"), Some("mock_bili_jct_csrf"));
    {
        let st = server.state().lock().unwrap();
        assert!(!st.toview.iter().any(|i| i.aid == 170001));
        assert!(st.toview.iter().any(|i| i.aid == 888888));
    }

    // 5. later clear -y
    server.reset_requests();
    env.run_bt(&["-u", s_str, "-y", "--json", "later", "clear"])
        .assert_json_ok();
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.method == "POST" && r.path == "/x/v2/history/toview/clear")
    );
    {
        assert!(server.state().lock().unwrap().toview.is_empty());
    }

    // 6. history list -c archive --limit 10
    server.reset_requests();
    let hist_list = env
        .run_bt(&[
            "-u",
            s_str,
            "--json",
            "history",
            "list",
            "-c",
            "archive",
            "--limit",
            "10",
        ])
        .assert_json_ok();
    assert!(json_contains_str(&hist_list, "BV1uJ411r7hL"));
    assert!(!json_contains_str(&hist_list, "官方测试直播"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/history/cursor"
            && r.query_param("type") == Some("archive")
            && r.query_param("ps") == Some("10")
    }));

    // 7. history delete
    server.reset_requests();
    env.run_bt(&["-u", s_str, "--json", "history", "delete", "BV1uJ411r7hL"])
        .assert_json_ok();
    let hdel_req = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/x/v2/history/delete")
        .expect("POST /x/v2/history/delete");
    assert_eq!(hdel_req.form_param("kid"), Some("archive_79677524"));
    assert_eq!(hdel_req.form_param("csrf"), Some("mock_bili_jct_csrf"));
    {
        let st = server.state().lock().unwrap();
        assert_eq!(st.history.len(), 1);
        assert_eq!(st.history[0].business, "live");
    }

    // 8. history clear -y
    server.reset_requests();
    env.run_bt(&["-u", s_str, "-y", "--json", "history", "clear"])
        .assert_json_ok();
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.method == "POST" && r.path == "/x/v2/history/clear")
    );
    {
        assert!(server.state().lock().unwrap().history.is_empty());
    }

    // 9. history list empty
    let hist_empty = env
        .run_bt(&["-u", s_str, "--json", "history", "list"])
        .assert_json_ok();
    let is_empty_list = hist_empty
        .as_array()
        .is_some_and(Vec::is_empty)
        || hist_empty["list"].as_array().is_some_and(Vec::is_empty);
    assert!(is_empty_list, "expected empty history list: {hist_empty}");
}

#[test]
fn t4_08_non_interactive_ci_and_agent_automation() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_no_color(false)
        .with_alt_cookie_file("s.json", CookieFixture::valid());
    let s_json = env.alt_cookie_path("s.json");
    let s_str = s_json.to_str().expect("utf8 s.json");

    // 1. --json --help
    let help_out = env.run_bt(&["--json", "--help"]);
    help_out.assert_no_ansi();
    assert!(help_out.stderr.trim().is_empty());
    let help_json = help_out.assert_json_ok();
    assert_eq!(help_json["name"], "bt");
    let modules = help_json["modules"].as_array().expect("modules array");
    for module in [
        "resolve",
        "auth",
        "live",
        "search",
        "video",
        "user",
        "feed",
        "rank",
        "fav",
        "later",
        "history",
        "completions",
    ] {
        assert!(
            modules.iter().any(|m| m.as_str() == Some(module)),
            "missing module {module} in {modules:?}"
        );
    }

    // 2. -q --no-color --json search hot --limit 3
    let hot_out = env.run_bt(&["-q", "--no-color", "--json", "search", "hot", "--limit", "3"]);
    hot_out.assert_no_ansi();
    assert!(hot_out.stderr.trim().is_empty());
    let hot_json = hot_out.assert_json_ok();
    let hot_len = hot_json
        .as_array()
        .map(Vec::len)
        .or_else(|| hot_json["list"].as_array().map(Vec::len));
    assert_eq!(hot_len, Some(3), "expected 3 hot search items: {hot_json}");

    // 3. -q --no-color fav export csv to stdout
    let csv_out = env.run_bt(&[
        "-q",
        "--no-color",
        "fav",
        "export",
        "1052622027",
        "--format",
        "csv",
    ]);
    csv_out.assert_exit(0).assert_no_ansi();
    assert!(csv_out.stderr.trim().is_empty());
    assert!(
        csv_out
            .stdout
            .starts_with("bvid,aid,title,upper_name,play,duration,link\n")
    );
    assert!(csv_out.stdout.contains("BV1uJ411r7hL"));

    // 4. missing default cookie file -> Auth JSON error
    server.reset_requests();
    let auth_err = env.run_bt(&["--json", "-y", "fav", "clean", "1052622027"]);
    auth_err.assert_no_ansi();
    assert!(auth_err.stderr.trim().is_empty());
    auth_err.assert_json_err(Some("Auth"));
    assert!(
        !server
            .requests()
            .iter()
            .any(|r| r.path == "/x/v3/fav/resource/clean")
    );

    // 5. valid cookie without -y -> Input JSON error
    server.reset_requests();
    let input_err = env.run_bt(&["-u", s_str, "--json", "fav", "clean", "1052622027"]);
    input_err.assert_no_ansi();
    assert!(input_err.stderr.trim().is_empty());
    input_err.assert_json_err(Some("Input"));
    assert!(
        !server
            .requests()
            .iter()
            .any(|r| r.path == "/x/v3/fav/resource/clean")
    );

    // 6. completions zsh --install --json
    let comp_out = env.run_bt(&["completions", "zsh", "--install", "--json"]);
    comp_out.assert_no_ansi();
    assert!(comp_out.stderr.trim().is_empty());
    let comp_json = comp_out.assert_json_ok();
    assert_eq!(comp_json["installed"], true);
    assert_eq!(comp_json["shell"], "zsh");
    let expected_zsh = env.home_dir().join(".zsh/completions/_bt");
    assert_eq!(
        comp_json["path"].as_str(),
        Some(expected_zsh.to_str().expect("utf8 zsh path"))
    );
    let zsh_script = fs::read_to_string(&expected_zsh).expect("installed _bt");
    assert!(!zsh_script.trim().is_empty());
}
