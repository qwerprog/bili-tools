mod common;

use common::{
    CookieFixture, FaultConfig, MOCK_JPEG_BYTES, MOCK_MP4_BYTES, MockBiliServer, RecordedRequest,
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
        "WBI w_rid mismatch for raw_query={} (sign_base={query})",
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

#[test]
fn t3_01_json_error_envelope_across_all_modules() {
    let server = MockBiliServer::start();
    let fault = FaultConfig::api_code(-101, "未登录或风控");
    for route in [
        "/x/passport-login/web/cookie/info",
        "/room/v1/Room/get_info",
        "/x/web-interface/wbi/search/type",
        "/x/web-interface/wbi/view",
        "/x/v3/fav/folder/created/list-all",
        "/x/v2/history/toview",
        "/x/web-interface/history/cursor",
        "/x/space/wbi/acc/info",
        "/x/polymer/web-dynamic/v1/feed/all",
        "/x/web-interface/popular",
    ] {
        server.set_fault(route, fault.clone());
    }

    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let cases: &[(&[&str], &str)] = &[
        (&["--json", "resolve", "bad_input_!!!"], "Input"),
        (&["--json", "auth", "refresh"], "Api"),
        (&["--json", "live", "status"], "Api"),
        (&["--json", "search", "video", "x"], "Api"),
        (&["--json", "video", "info", "BV1uJ411r7hL"], "Api"),
        (&["--json", "fav", "list"], "Api"),
        (&["--json", "later", "list"], "Api"),
        (&["--json", "history", "list"], "Api"),
        (&["--json", "user", "info", "293793435"], "Api"),
        (&["--json", "feed"], "Api"),
        (&["--json", "rank", "popular"], "Api"),
    ];

    for (args, expected_kind) in cases {
        let out = env.run_bt(args);
        out.assert_no_ansi();
        assert!(
            out.stderr.trim().is_empty(),
            "expected empty stderr for {args:?}, got: {}",
            out.stderr
        );
        let err = out.assert_json_err(Some(expected_kind));
        assert!(
            !err["error"]["message"]
                .as_str()
                .unwrap_or("")
                .trim()
                .is_empty(),
            "empty error message for {args:?}: {err}"
        );
    }
}

#[test]
fn t3_02_cookie_file_isolation_across_auth_and_live() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let u1 = env.alt_cookie_path("u1.json");
    let u1_str = u1.to_str().expect("utf8 alt cookie path");

    let login_out = env.run_bt(&[
        "-u",
        u1_str,
        "--json",
        "auth",
        "login",
        "--method",
        "cookie",
        "--sessdata",
        "s_u1",
        "--bili-jct",
        "j_u1",
    ]);
    let login_json = login_out.assert_json_ok();
    assert_eq!(login_json["success"], true);
    assert_eq!(login_json["uid"], "293793435");
    assert_eq!(login_json["room_id"], 31686846);
    assert_mode_0600(&u1);
    assert!(!env.default_cookie_path().exists());

    let saved = env.read_cookie_file(&u1);
    assert_eq!(saved.sessdata, "s_u1");
    assert_eq!(saved.bili_jct, "j_u1");
    assert_eq!(saved.room_id, 31686846);
    assert!(!saved.buvid3.is_empty());
    assert!(!saved.buvid4.is_empty());

    server.reset_requests();
    let title_out = env.run_bt(&[
        "-u",
        u1_str,
        "--json",
        "live",
        "title",
        "CustomCookieTitle",
    ]);
    let title_json = title_out.assert_json_ok();
    assert_eq!(title_json["success"], true);
    assert_eq!(title_json["title"], "CustomCookieTitle");
    assert_eq!(title_json["room_id"], 31686846);

    let reqs = server.requests();
    let update_req = reqs
        .iter()
        .find(|r| r.method == "POST" && r.path == "/room/v1/Room/update")
        .expect("POST /room/v1/Room/update recorded");
    assert_eq!(update_req.form_param("room_id"), Some("31686846"));
    assert_eq!(update_req.form_param("title"), Some("CustomCookieTitle"));
    assert_eq!(update_req.form_param("csrf"), Some("j_u1"));
    assert!(
        update_req
            .header("cookie")
            .is_some_and(|c| c.contains("SESSDATA=s_u1")),
        "expected SESSDATA=s_u1 in cookie header: {:?}",
        update_req.header("cookie")
    );
    {
        let st = server.state().lock().unwrap();
        assert_eq!(st.rooms[&31686846].title, "CustomCookieTitle");
    }

    let status_out = env.run_bt(&["--json", "auth", "status"]);
    let status_json = status_out.assert_json_ok();
    assert_eq!(status_json["is_login"], false);
    assert!(!env.default_cookie_path().exists());
}

#[test]
fn t3_03_multi_account_cookie_switching_across_video_fav_user() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_alt_cookie_file(
            "u1.json",
            CookieFixture::valid()
                .with_sessdata("sess1")
                .with_bili_jct("jct1"),
        )
        .with_alt_cookie_file(
            "u2.json",
            CookieFixture::valid()
                .with_sessdata("sess2")
                .with_bili_jct("jct2"),
        );

    let u1 = env.alt_cookie_path("u1.json");
    let u2 = env.alt_cookie_path("u2.json");
    let u1_str = u1.to_str().expect("utf8 u1 path");
    let u2_str = u2.to_str().expect("utf8 u2 path");

    let like_out = env.run_bt(&["-u", u1_str, "--json", "video", "like", "BV1uJ411r7hL"]);
    assert!(like_out.stderr.trim().is_empty());
    let like_json = like_out.assert_json_ok();
    assert_eq!(like_json["liked"], true);

    let fav_out = env.run_bt(&[
        "-u",
        u2_str,
        "--json",
        "fav",
        "add",
        "1052622027",
        "BV1GJ411x7h7",
    ]);
    assert!(fav_out.stderr.trim().is_empty());
    fav_out.assert_json_ok();

    let follow_out = env.run_bt(&["-u", u1_str, "--json", "user", "follow", "50333369"]);
    assert!(follow_out.stderr.trim().is_empty());
    follow_out.assert_json_ok();

    let reqs = server.requests();
    let like_req = reqs
        .iter()
        .find(|r| r.method == "POST" && r.path == "/x/web-interface/archive/like")
        .expect("like POST recorded");
    assert_eq!(like_req.form_param("csrf"), Some("jct1"));
    assert_eq!(like_req.form_param("aid"), Some("79677524"));
    assert!(
        like_req
            .header("cookie")
            .is_some_and(|c| c.contains("SESSDATA=sess1"))
    );

    let deal_req = reqs
        .iter()
        .find(|r| r.method == "POST" && r.path == "/x/v3/fav/resource/deal")
        .expect("fav deal POST recorded");
    assert_eq!(deal_req.form_param("csrf"), Some("jct2"));
    assert_eq!(deal_req.form_param("add_media_ids"), Some("1052622027"));
    assert_eq!(deal_req.form_param("rid"), Some("170001"));
    assert!(
        deal_req
            .header("cookie")
            .is_some_and(|c| c.contains("SESSDATA=sess2"))
    );

    let follow_req = reqs
        .iter()
        .find(|r| r.method == "POST" && r.path == "/x/relation/modify")
        .expect("relation modify POST recorded");
    assert_eq!(follow_req.form_param("csrf"), Some("jct1"));
    assert_eq!(follow_req.form_param("fid"), Some("50333369"));
    assert_eq!(follow_req.form_param("act"), Some("1"));
    assert!(
        follow_req
            .header("cookie")
            .is_some_and(|c| c.contains("SESSDATA=sess1"))
    );

    {
        let st = server.state().lock().unwrap();
        assert!(st.liked_videos.contains(&79677524));
        assert!(st.fav_folders[0].items.iter().any(|i| i.aid == 170001));
        assert!(st.followed_mids.contains(&50333369));
    }
}

#[test]
fn t3_04_short_link_resolution_chained_into_video_info() {
    let server = MockBiliServer::start();
    {
        let mut st = server.state().lock().unwrap();
        st.short_links.insert(
            "mock123".to_string(),
            "https://www.bilibili.com/video/BV1uJ411r7hL?p=2".to_string(),
        );
    }
    let env = TestEnv::new().with_server(&server);

    let res_out = env.run_bt(&["--json", "resolve", "https://b23.tv/mock123"]);
    let res_json = res_out.assert_json_ok();
    assert_eq!(res_json["kind"], "video");
    assert_eq!(res_json["bvid"], "BV1uJ411r7hL");
    assert_eq!(res_json["page"], 2);
    assert_eq!(res_json["network_used"], true);
    assert_eq!(res_json["short_url"], "https://b23.tv/mock123");

    server.reset_requests();
    let info_out = env.run_bt(&["--json", "video", "info", "https://b23.tv/mock123"]);
    let info_json = info_out.assert_json_ok();
    assert_eq!(info_json["bvid"], "BV1uJ411r7hL");
    assert_eq!(info_json["aid"], 79677524);
    assert_eq!(
        info_json["pages"].as_array().map(Vec::len),
        Some(2),
        "expected 2 video pages in {info_json}"
    );

    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.method == "GET" && r.path == "/mock123"),
        "expected GET /mock123 in {reqs:?}"
    );
    assert!(
        reqs.iter()
            .any(|r| r.path == "/x/web-interface/wbi/view"
                && r.query_param("bvid") == Some("BV1uJ411r7hL")),
        "expected view query with bvid=BV1uJ411r7hL in {reqs:?}"
    );
}

#[test]
fn t3_05_full_url_resolution_across_live_fav_user() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let live_out = env.run_bt(&["--json", "live", "info", "https://live.bilibili.com/14073662"]);
    let live_json = live_out.assert_json_ok();
    assert!(json_contains_i64(&live_json, 14073662));
    assert!(json_contains_i64(&live_json, 76));
    assert!(json_contains_str(&live_json, "官方测试直播"));

    server.reset_requests();
    let fav_ml = env.run_bt(&[
        "--json",
        "fav",
        "show",
        "https://www.bilibili.com/medialist/detail/ml1052622027",
    ]);
    let fav_ml_json = fav_ml.assert_json_ok();
    assert_eq!(fav_ml_json["info"]["id"], 1052622027);
    assert_eq!(fav_ml_json["medias"][0]["bvid"], "BV1uJ411r7hL");
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/v3/fav/resource/list" && r.query_param("media_id") == Some("1052622027")
    }));

    server.reset_requests();
    let fav_fid = env.run_bt(&[
        "--json",
        "fav",
        "show",
        "https://space.bilibili.com/293793435/favlist?fid=1052622027",
    ]);
    let fav_fid_json = fav_fid.assert_json_ok();
    assert_eq!(fav_fid_json["info"]["id"], 1052622027);
    assert_eq!(fav_fid_json["medias"][0]["bvid"], "BV1uJ411r7hL");
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/v3/fav/resource/list" && r.query_param("media_id") == Some("1052622027")
    }));

    server.reset_requests();
    let user_out = env.run_bt(&["--json", "user", "info", "https://space.bilibili.com/123456"]);
    let user_json = user_out.assert_json_ok();
    assert!(
        json_contains_i64(&user_json, 123456),
        "expected mid 123456 in user info JSON: {user_json}"
    );
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/space/wbi/acc/info"
            && r.query_param("mid") == Some("123456")
            && r.has_wbi_signature()
    }));
}

#[test]
fn t3_06_quiet_and_no_color_fav_export_csv_stdout() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_no_color(false);

    let out = env.run_bt(&[
        "-q",
        "--no-color",
        "fav",
        "export",
        "1052622027",
        "--format",
        "csv",
    ]);
    out.assert_exit(0).assert_no_ansi();
    assert!(
        out.stderr.trim().is_empty(),
        "expected empty stderr in quiet mode, got: {}",
        out.stderr
    );

    let lines: Vec<&str> = out.stdout.lines().collect();
    assert!(lines.len() >= 2, "expected CSV header + row: {}", out.stdout);
    assert_eq!(lines[0], "bvid,aid,title,upper_name,play,duration,link");
    assert!(lines[1].contains("BV1uJ411r7hL"));
    assert!(lines[1].contains("79677524"));
    assert!(lines[1].contains("https://www.bilibili.com/video/BV1uJ411r7hL"));
}

#[test]
fn t3_07_quiet_no_color_json_fav_export_to_file() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_no_color(false);
    let fav_path = env.work_dir().join("fav.json");
    let fav_str = fav_path.to_str().expect("utf8 fav.json path");

    let out = env.run_bt(&[
        "-q",
        "--no-color",
        "--json",
        "fav",
        "export",
        "1052622027",
        "--format",
        "json",
        "-o",
        fav_str,
    ]);
    out.assert_no_ansi();
    assert!(
        out.stderr.trim().is_empty(),
        "expected empty stderr, got: {}",
        out.stderr
    );
    let summary = out.assert_json_ok();
    assert_eq!(summary["media_id"], 1052622027);
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["format"], "json");
    let reported_path = summary["file"]
        .as_str()
        .or_else(|| summary["path"].as_str())
        .or_else(|| summary["output"].as_str());
    assert_eq!(reported_path, Some(fav_str));

    let raw = fs::read_to_string(&fav_path).expect("read exported fav.json");
    let file_json: Value = serde_json::from_str(&raw).expect("parse exported fav.json");
    assert_eq!(file_json["info"]["id"], 1052622027);
    assert_eq!(file_json["count"], 1);
    assert_eq!(file_json["medias"][0]["bvid"], "BV1uJ411r7hL");
}

#[test]
fn t3_08_non_tty_yes_guard_and_execution_for_destructive_ops() {
    let server = MockBiliServer::start();
    {
        let mut st = server.state().lock().unwrap();
        st.fav_folders[0].items[0].valid = false;
    }
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    // Phase A: without -y -> must fail with Input JSON error before making HTTP calls
    for args in [
        &["--json", "fav", "clean", "1052622027"][..],
        &["--json", "fav", "delete", "1052622027"][..],
        &["--json", "later", "clear"][..],
        &["--json", "history", "clear"][..],
    ] {
        let out = env.run_bt(args);
        assert!(out.stderr.trim().is_empty());
        out.assert_json_err(Some("Input"));
    }
    assert!(
        server.requests().is_empty(),
        "expected 0 HTTP requests without -y, got: {:?}",
        server.requests()
    );

    // Phase B: with -y (fav clean MUST run before fav delete!)
    for args in [
        &["-y", "--json", "fav", "clean", "1052622027"][..],
        &["-y", "--json", "fav", "delete", "1052622027"][..],
        &["-y", "--json", "later", "clear"][..],
        &["-y", "--json", "history", "clear"][..],
    ] {
        let out = env.run_bt(args);
        assert!(out.stderr.trim().is_empty());
        out.assert_json_ok();
    }

    let reqs = server.requests();
    for path in [
        "/x/v3/fav/resource/clean",
        "/x/v3/fav/folder/del",
        "/x/v2/history/toview/clear",
        "/x/v2/history/clear",
    ] {
        assert!(
            reqs.iter().any(|r| {
                r.method == "POST"
                    && r.path == path
                    && r.form_param("csrf") == Some("mock_bili_jct_csrf")
            }),
            "expected POST {path} with csrf in {reqs:?}"
        );
    }

    let st = server.state().lock().unwrap();
    assert!(st.fav_folders.is_empty());
    assert!(st.toview.is_empty());
    assert!(st.history.is_empty());
}

#[test]
fn t3_09_fake_ffmpeg_video_download_with_all_sidecars() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let out_mp4 = env.work_dir().join("vid.mp4");
    let out_mp4_str = out_mp4.to_str().expect("utf8 out_mp4 path");

    let out = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "--danmaku",
        "--subtitle",
        "--cover",
        "-o",
        out_mp4_str,
    ]);
    assert!(out.stderr.trim().is_empty());
    out.assert_json_ok();

    let mp4_bytes = fs::read(&out_mp4).expect("muxed vid.mp4 exists");
    assert!(
        mp4_bytes.starts_with(b"\x00\x00\x00\x14ftypisom"),
        "expected fake ffmpeg MP4 header in {out_mp4:?}"
    );

    let files = collect_files_recursive(env.work_dir());
    let cover_file = files
        .iter()
        .find(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("jpg" | "jpeg")))
        .expect("cover jpg sidecar created");
    assert_eq!(fs::read(cover_file).unwrap(), MOCK_JPEG_BYTES);

    let sub_file = files
        .iter()
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("srt"))
        .expect("subtitle srt sidecar created");
    let sub_text = fs::read_to_string(sub_file).unwrap();
    assert!(sub_text.contains("Hello Bilibili"));

    let dm_file = files
        .iter()
        .find(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("xml" | "ass")))
        .expect("danmaku sidecar created");
    let dm_text = fs::read_to_string(dm_file).unwrap();
    assert!(dm_text.contains("测试弹幕一条"));

    let ff_log = env.read_ffmpeg_log();
    assert!(
        ff_log.iter().any(|l| l == "-version"),
        "expected -version probe in {ff_log:?}"
    );
    assert!(
        ff_log
            .iter()
            .any(|l| l.contains("-i") && l.ends_with(out_mp4_str)),
        "expected mux invocation ending with {out_mp4_str} in {ff_log:?}"
    );

    let reqs = server.requests();
    for expected_prefix in [
        "/x/web-interface/wbi/view",
        "/x/player/",
        "/mock/subtitle/",
        "/mock/cover/",
        "/mock/media/",
    ] {
        assert!(
            reqs.iter().any(|r| r.path.starts_with(expected_prefix)),
            "expected request matching {expected_prefix} in {reqs:?}"
        );
    }
    assert!(
        reqs.iter()
            .any(|r| r.path.starts_with("/x/v1/dm/list.so") || r.path.ends_with(".xml")),
        "expected danmaku request (/x/v1/dm/list.so or *.xml) in {reqs:?}"
    );
}

#[test]
fn t3_10_missing_ffmpeg_guard_vs_skip_mux() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).without_ffmpeg();

    let muxed = env.work_dir().join("muxed.mp4");
    let muxed_str = muxed.to_str().expect("utf8 muxed path");
    let fail_out = env.run_bt(&["--json", "video", "download", "BV1uJ411r7hL", "-o", muxed_str]);
    assert!(fail_out.stderr.trim().is_empty());
    let err = fail_out.assert_json_err(Some("ExternalTool"));
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("ffmpeg"),
        "expected actionable ffmpeg message: {err}"
    );
    assert!(!muxed.exists());

    let raw = env.work_dir().join("raw.mp4");
    let raw_str = raw.to_str().expect("utf8 raw path");
    let ok_out = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "--skip-mux",
        "-o",
        raw_str,
    ]);
    assert!(ok_out.stderr.trim().is_empty());
    ok_out.assert_json_ok();

    let stream_files: Vec<PathBuf> = collect_files_recursive(env.work_dir())
        .into_iter()
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("m4s" | "mp4" | "m4a")
            )
        })
        .collect();
    assert!(
        !stream_files.is_empty(),
        "expected raw downloaded stream files in work_dir"
    );
    assert!(
        stream_files
            .iter()
            .any(|p| fs::read(p).is_ok_and(|b| b == MOCK_MP4_BYTES)),
        "expected at least one raw stream matching MOCK_MP4_BYTES"
    );
    assert!(env.read_ffmpeg_log().is_empty());
}

#[test]
fn t3_11_base_url_overrides_and_wbi_cryptographic_signing() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let search_out = env.run_bt(&["--json", "search", "video", "世 界!*()"]);
    search_out.assert_json_ok();

    let user_vid_out = env.run_bt(&[
        "--json",
        "user",
        "videos",
        "293793435",
        "--keyword",
        "Rust",
    ]);
    user_vid_out.assert_json_ok();

    let reqs = server.requests();
    let search_req = reqs
        .iter()
        .find(|r| r.path == "/x/web-interface/wbi/search/type")
        .expect("WBI search/type request recorded");
    assert!(
        search_req.raw_query.contains("keyword=%E4%B8%96%20%E7%95%8C"),
        "expected uppercase percent-encoded & filtered keyword in raw_query: {}",
        search_req.raw_query
    );
    assert!(!search_req.raw_query.contains('!'));
    assert!(!search_req.raw_query.contains('*'));
    assert!(!search_req.raw_query.contains('+'));
    assert_valid_wbi_signature(search_req, MOCK_WBI_MIXIN_KEY);

    let arc_req = reqs
        .iter()
        .find(|r| r.path == "/x/space/wbi/arc/search")
        .expect("WBI space/arc/search request recorded");
    assert_eq!(arc_req.query_param("mid"), Some("293793435"));
    assert_eq!(arc_req.query_param("keyword"), Some("Rust"));
    assert_valid_wbi_signature(arc_req, MOCK_WBI_MIXIN_KEY);
}

#[test]
fn t3_12_no_color_flag_and_env_var_suppress_ansi() {
    let server = MockBiliServer::start();

    let env_flag = TestEnv::new()
        .with_server(&server)
        .with_valid_cookies()
        .with_no_color(false);
    let live_out = env_flag.run_bt_env(
        &["--no-color", "live", "status"],
        &[("TERM", "xterm-256color")],
    );
    live_out
        .assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("31686846");

    let env_var = TestEnv::new()
        .with_server(&server)
        .with_valid_cookies()
        .with_no_color(true);
    let vid_out = env_var.run_bt_env(
        &["video", "info", "BV1uJ411r7hL"],
        &[("TERM", "xterm-256color")],
    );
    vid_out
        .assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("BV1uJ411r7hL");
}

#[test]
fn t3_13_auth_login_cookie_into_auth_daily() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let c_path = env.alt_cookie_path("c.json");
    let c_str = c_path.to_str().expect("utf8 c.json path");

    let login_out = env.run_bt(&[
        "-u",
        c_str,
        "--json",
        "auth",
        "login",
        "--method",
        "cookie",
        "--sessdata",
        "s_daily",
        "--bili-jct",
        "j_daily",
    ]);
    let login_json = login_out.assert_json_ok();
    assert_eq!(login_json["success"], true);
    assert_eq!(login_json["uid"], "293793435");
    assert_eq!(login_json["room_id"], 31686846);
    assert_mode_0600(&c_path);
    assert!(!env.default_cookie_path().exists());

    let saved = env.read_cookie_file(&c_path);
    assert_eq!(saved.room_id, 31686846);
    assert!(!saved.buvid3.is_empty());
    assert!(!saved.buvid4.is_empty());

    server.reset_requests();
    let daily_out = env.run_bt(&["-u", c_str, "--json", "auth", "daily"]);
    let daily_json = daily_out.assert_json_ok();
    assert_eq!(daily_json["exp_reward"]["login"], true);
    assert_eq!(daily_json["exp_reward"]["coins"], 30);
    assert_eq!(daily_json["live_sign"]["signed"], true);
    let had_days = daily_json["live_sign"]["had_sign_days"]
        .as_i64()
        .or_else(|| daily_json["live_sign"]["hadSignDays"].as_i64());
    assert_eq!(had_days, Some(1));

    let reqs = server.requests();
    for path in ["/x/member/web/exp/reward", "/xlive/web-ucenter/v1/sign/DoSign"] {
        assert!(
            reqs.iter().any(|r| {
                r.path == path
                    && r.header("cookie")
                        .is_some_and(|c| c.contains("SESSDATA=s_daily"))
            }),
            "expected {path} with SESSDATA=s_daily in {reqs:?}"
        );
    }
}

#[test]
fn t3_14_live_record_with_fake_ffmpeg_and_url_input() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let live_flv = env.work_dir().join("live.flv");
    let live_flv_str = live_flv.to_str().expect("utf8 live.flv path");

    let out = env.run_bt(&[
        "--json",
        "live",
        "record",
        "https://live.bilibili.com/14073662",
        "--quality",
        "10000",
        "-d",
        "2s",
        "-o",
        live_flv_str,
    ]);
    assert!(out.stderr.trim().is_empty());
    out.assert_json_ok();

    let flv_bytes = fs::read(&live_flv).expect("live.flv created");
    assert!(
        flv_bytes.starts_with(b"FLV\x01"),
        "expected FLV header in {live_flv:?}"
    );

    let ff_log = env.read_ffmpeg_log();
    assert!(ff_log.iter().any(|l| l == "-version"));
    let expected_url = format!("{}/mock/live/14073662.flv", server.base_url());
    assert!(
        ff_log
            .iter()
            .any(|l| l.contains(&expected_url) && l.contains("-t 2") && l.ends_with(live_flv_str)),
        "expected ffmpeg record line with {expected_url}, -t 2, {live_flv_str} in {ff_log:?}"
    );

    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path == "/room/v1/Room/get_info" || r.path == "/room/v1/Room/room_init")
    );
    assert!(reqs.iter().any(|r| {
        r.path == "/xlive/web-room/v2/index/getRoomPlayInfo"
            || r.path == "/room/v1/Room/playUrl"
    }));
}

#[test]
fn t3_15_page_query_param_propagates_to_danmaku_and_subtitle() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let p2_txt = env.work_dir().join("p2.txt");
    let p2_srt = env.work_dir().join("p2.srt");
    let p2_txt_str = p2_txt.to_str().expect("utf8 p2.txt");
    let p2_srt_str = p2_srt.to_str().expect("utf8 p2.srt");

    env.run_bt(&[
        "video",
        "danmaku",
        "https://www.bilibili.com/video/BV1uJ411r7hL?p=2",
        "--format",
        "txt",
        "-o",
        p2_txt_str,
    ])
    .assert_exit(0);

    let dm_txt = fs::read_to_string(&p2_txt).expect("p2.txt exists");
    assert!(dm_txt.contains("测试弹幕一条"));
    assert!(dm_txt.contains("顶部红字弹幕"));

    let dm_reqs = server.requests();
    assert!(
        dm_reqs.iter().any(|r| {
            (r.path == "/x/v1/dm/list.so" && r.query_param("oid") == Some("146044694"))
                || r.path == "/146044694.xml"
        }),
        "expected page 2 cid 146044694 in danmaku requests: {dm_reqs:?}"
    );
    assert!(
        !dm_reqs.iter().any(|r| {
            (r.path == "/x/v1/dm/list.so" && r.query_param("oid") == Some("146044693"))
                || r.path == "/146044693.xml"
        }),
        "should not request page 1 cid 146044693: {dm_reqs:?}"
    );

    server.reset_requests();
    env.run_bt(&[
        "video",
        "subtitle",
        "https://www.bilibili.com/video/BV1uJ411r7hL?p=2",
        "--lang",
        "zh-Hans",
        "--format",
        "srt",
        "-o",
        p2_srt_str,
    ])
    .assert_exit(0);

    let sub_srt = fs::read_to_string(&p2_srt).expect("p2.srt exists");
    assert!(sub_srt.contains("Hello Bilibili"));
    assert!(sub_srt.contains("00:00:00,500 --> 00:00:02,500"));

    let sub_reqs = server.requests();
    assert!(
        sub_reqs.iter().any(|r| {
            (r.path == "/x/player/wbi/v2" || r.path == "/x/player/v2")
                && r.query_param("cid") == Some("146044694")
        }),
        "expected page 2 cid 146044694 in player/v2 requests: {sub_reqs:?}"
    );
}

#[test]
fn t3_16_cookie_file_json_stateful_comment_post_then_list() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_alt_cookie_file("c.json", CookieFixture::valid());
    let c_path = env.alt_cookie_path("c.json");
    let c_str = c_path.to_str().expect("utf8 c.json path");

    let post_out = env.run_bt(&[
        "-u",
        c_str,
        "--json",
        "video",
        "comment",
        "post",
        "https://www.bilibili.com/video/BV1uJ411r7hL",
        "hello",
    ]);
    assert!(post_out.stderr.trim().is_empty());
    let post_json = post_out.assert_json_ok();
    assert!(
        json_contains_i64(&post_json, 104192624481),
        "expected new rpid 104192624481 in {post_json}"
    );

    let reqs = server.requests();
    let add_req = reqs
        .iter()
        .find(|r| r.method == "POST" && r.path == "/x/v2/reply/add")
        .expect("POST /x/v2/reply/add recorded");
    assert_eq!(add_req.form_param("oid"), Some("79677524"));
    assert_eq!(add_req.form_param("type"), Some("1"));
    assert_eq!(add_req.form_param("message"), Some("hello"));
    assert_eq!(add_req.form_param("csrf"), Some("mock_bili_jct_csrf"));
    assert!(
        add_req
            .header("cookie")
            .is_some_and(|c| c.contains("SESSDATA=mock_sessdata_token"))
    );

    server.reset_requests();
    let list_out = env.run_bt(&[
        "--json",
        "video",
        "comments",
        "BV1uJ411r7hL",
        "--sort",
        "time",
    ]);
    assert!(list_out.stderr.trim().is_empty());
    let list_json = list_out.assert_json_ok();
    assert!(json_contains_str(&list_json, "hello"));
    assert!(json_contains_i64(&list_json, 104192624481));

    let list_reqs = server.requests();
    assert!(list_reqs.iter().any(|r| {
        r.method == "GET"
            && r.query_param("oid") == Some("79677524")
            && ((r.path == "/x/v2/reply" && r.query_param("sort") == Some("0"))
                || (r.path == "/x/v2/reply/wbi/main"
                    && (r.query_param("mode") == Some("2")
                        || r.query_param("sort") == Some("0"))))
    }));
}

#[test]
fn t3_17_quiet_mode_live_start_show_and_stop_countdown() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let start_out = env.run_bt(&[
        "-q",
        "live",
        "start",
        "-a",
        "86",
        "-t",
        "QuietLive",
        "-s",
        "-y",
    ]);
    start_out.assert_exit(0).assert_stdout_empty().assert_no_ansi();
    assert!(
        start_out.stderr.trim().is_empty(),
        "expected empty stderr on quiet live start, got: {}",
        start_out.stderr
    );

    let stream_info = env
        .default_cookie_path()
        .parent()
        .expect("cookie parent dir")
        .join("stream_info.txt");
    assert!(
        stream_info.exists(),
        "expected stream_info.txt at {stream_info:?}"
    );
    assert_mode_0600(&stream_info);
    let stream_text = fs::read_to_string(&stream_info).expect("read stream_info.txt");
    assert!(stream_text.contains("rtmp://live-push.bilivideo.com/live-bvc/"));
    {
        let st = server.state().lock().unwrap();
        assert_eq!(st.rooms[&31686846].live_status, 1);
    }

    let stop_out = env.run_bt(&["-q", "live", "stop", "-d", "1s"]);
    stop_out.assert_exit(0).assert_stdout_empty().assert_no_ansi();
    assert!(
        stop_out.stderr.trim().is_empty(),
        "expected empty stderr on quiet live stop, got: {}",
        stop_out.stderr
    );
    {
        let st = server.state().lock().unwrap();
        assert_eq!(st.rooms[&31686846].live_status, 0);
    }
}

#[test]
fn t3_18_completions_quiet_json_install_and_12_module_sync() {
    let env = TestEnv::new();

    let bash_out = env.run_bt(&["-q", "completions", "bash"]);
    bash_out.assert_exit(0).assert_no_ansi();
    assert!(bash_out.stderr.trim().is_empty());
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
            bash_out.stdout.contains(module),
            "expected module '{module}' in bash completions"
        );
    }

    let fish_out = env.run_bt(&["--json", "completions", "fish", "--install"]);
    assert!(fish_out.stderr.trim().is_empty());
    let fish_json = fish_out.assert_json_ok();
    assert_eq!(fish_json["installed"], true);
    assert_eq!(fish_json["shell"], "fish");

    let expected_fish = env.home_dir().join(".config/fish/completions/bit.fish");
    assert_eq!(
        fish_json["path"].as_str(),
        Some(expected_fish.to_str().expect("utf8 fish path"))
    );
    let fish_script = fs::read_to_string(&expected_fish).expect("installed bt.fish");
    assert!(fish_script.contains("complete -c bit"));
    for module in ["resolve", "user", "feed", "rank"] {
        assert!(
            fish_script.contains(module),
            "expected module '{module}' in fish completions"
        );
    }
}
