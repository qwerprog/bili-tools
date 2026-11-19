mod common;

use common::{
    CookieFixture, FaultConfig, MOCK_JPEG_BYTES, MOCK_MP4_BYTES, MockBiliServer, MockFavItem,
    MockHistoryItem, TestEnv, assert_mode_0600,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// ── Feature #1: Global CLI & Output Contract (t1_f01_01..t1_f01_06) ──────────

#[test]
fn t1_f01_01() {
    let env = TestEnv::new();
    let out = env.run_bt(&["--json"]);
    let v = out.assert_json_ok();
    assert_eq!(v["name"], "bt");
    assert_eq!(v["package"], "bili-tools");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    let mods: Vec<&str> = v["modules"]
        .as_array()
        .expect("modules array")
        .iter()
        .filter_map(|m| m.as_str())
        .collect();
    for expected in [
        "resolve",
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
            mods.contains(&expected),
            "missing module {expected} in {mods:?}"
        );
    }
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);
}

#[test]
fn t1_f01_02() {
    let env = TestEnv::new();
    let out = env.run_bt(&["--json", "--help"]);
    let v = out.assert_json_ok();
    assert_eq!(v["name"], "bt");
    assert!(v["modules"].is_array());
    out.assert_no_ansi();
    assert!(
        !out.stdout.contains("核心模块"),
        "help banner leaked into --json --help output: {}",
        out.stdout
    );
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);
}

#[test]
fn t1_f01_03() {
    let env = TestEnv::new().with_no_color(false);
    let out = env.run_bt_env(&["--no-color", "-V"], &[("TERM", "xterm-256color")]);
    out.assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("bt")
        .assert_stdout_contains(env!("CARGO_PKG_VERSION"));
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);
}

#[test]
fn t1_f01_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["-q", "search", "hot"]);
    out.assert_exit(0).assert_stdout_empty();
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/web-interface/wbi/search/square")
    );
}

#[test]
fn t1_f01_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_alt_cookie_file("alt.json", CookieFixture::valid());
    let alt = env.alt_cookie_path("alt.json");
    let out = env.run_bt(&["-u", alt.to_str().unwrap(), "auth", "status"]);
    out.assert_exit(0)
        .assert_stdout_contains("测试用户")
        .assert_stdout_contains("293793435");
    assert!(
        !env.default_cookie_path().exists(),
        "default cookie path should not be created when -u is used"
    );
}

#[test]
fn t1_f01_06() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/wbi/view",
        FaultConfig::api_code(-404, "啥都木有"),
    );
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "video", "info", "BV1uJ411r7hL"]);
    let err = out.assert_json_err(Some("Api"));
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("啥都木有"),
        "expected message to contain 啥都木有: {err}"
    );
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);
}

// ── Feature #2: Universal ID / URL / b23.tv Resolver (t1_f02_01..t1_f02_06) ─

#[test]
fn t1_f02_01() {
    let env = TestEnv::new();
    let out = env.run_bt(&["--json", "resolve", "BV1uJ411r7hL", "--no-network"]);
    let v = out.assert_json_ok();
    assert_eq!(v["kind"], "video");
    assert_eq!(v["id"], "BV1uJ411r7hL");
    assert_eq!(v["bvid"], "BV1uJ411r7hL");
    assert_eq!(v["network_used"], false);
}

#[test]
fn t1_f02_02() {
    let env = TestEnv::new();
    let out = env.run_bt(&[
        "--json",
        "resolve",
        "https://www.bilibili.com/video/BV1uJ411r7hL/?p=3&spm_id_from=333.999",
    ]);
    let v = out.assert_json_ok();
    assert_eq!(v["kind"], "video");
    assert_eq!(v["bvid"], "BV1uJ411r7hL");
    assert_eq!(v["page"], 3);
    assert_eq!(
        v["resolved_url"],
        "https://www.bilibili.com/video/BV1uJ411r7hL?p=3"
    );
    assert!(
        !v["resolved_url"]
            .as_str()
            .unwrap()
            .contains("spm_id_from")
    );
}

#[test]
fn t1_f02_03() {
    let env = TestEnv::new();
    let out = env.run_bt(&["--json", "resolve", "av170001"]);
    let v = out.assert_json_ok();
    assert_eq!(v["kind"], "video");
    assert_eq!(v["id"], "av170001");
    assert_eq!(v["aid"], 170001);
}

#[test]
fn t1_f02_04() {
    let env = TestEnv::new();

    let ep = env
        .run_bt(&["--json", "resolve", "ep12345"])
        .assert_json_ok();
    assert_eq!(ep["kind"], "bangumi_episode");
    assert_eq!(ep["id"], "ep12345");
    assert_eq!(ep["episode_id"], 12345);

    let ss = env
        .run_bt(&["--json", "resolve", "ss678"])
        .assert_json_ok();
    assert_eq!(ss["kind"], "bangumi_season");
    assert_eq!(ss["id"], "ss678");
    assert_eq!(ss["season_id"], 678);

    let md = env
        .run_bt(&["--json", "resolve", "md99"])
        .assert_json_ok();
    assert_eq!(md["kind"], "bangumi_media");
    assert_eq!(md["id"], "md99");
    assert_eq!(md["media_id"], 99);
}

#[test]
fn t1_f02_05() {
    let server = MockBiliServer::start();
    server.state().lock().unwrap().short_links.insert(
        "mock123".to_string(),
        "https://www.bilibili.com/video/BV1xx411c7mD?p=2&vd_source=abc".to_string(),
    );
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "resolve", "https://b23.tv/mock123"]);
    let v = out.assert_json_ok();
    assert_eq!(v["kind"], "video");
    assert_eq!(v["bvid"], "BV1xx411c7mD");
    assert_eq!(v["page"], 2);
    assert_eq!(v["short_url"], "https://b23.tv/mock123");
    assert_eq!(
        v["resolved_url"],
        "https://www.bilibili.com/video/BV1xx411c7mD?p=2"
    );
    assert_eq!(v["network_used"], true);
}

#[test]
fn t1_f02_06() {
    let env = TestEnv::new();

    let space = env
        .run_bt(&["--json", "resolve", "https://space.bilibili.com/123456"])
        .assert_json_ok();
    assert_eq!(space["kind"], "user");
    assert_eq!(space["mid"], 123456);

    let live = env
        .run_bt(&["--json", "resolve", "https://live.bilibili.com/21452505"])
        .assert_json_ok();
    assert_eq!(live["kind"], "live_room");
    assert_eq!(live["room_id"], 21452505);

    let fav = env
        .run_bt(&[
            "--json",
            "resolve",
            "https://space.bilibili.com/123456/favlist?fid=777",
        ])
        .assert_json_ok();
    assert_eq!(fav["kind"], "fav");
    assert_eq!(fav["fav_id"], 777);
    assert_eq!(fav["mid"], 123456);
}

// ── Feature #3: External Binary (ffmpeg) Guard (t1_f03_01..t1_f03_05) ────────

#[test]
fn t1_f03_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let out_mp4 = env.work_dir().join("out.mp4");
    let out = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "-o",
        out_mp4.to_str().unwrap(),
    ]);
    out.assert_exit(0);
    let log = env.read_ffmpeg_log();
    assert!(!log.is_empty(), "expected ffmpeg invocations in log");
    assert!(
        log.iter().any(|l| l.contains("out.mp4")),
        "expected out.mp4 in ffmpeg log: {log:?}"
    );
    assert!(
        out_mp4.exists() && std::fs::metadata(&out_mp4).unwrap().len() > 0,
        "expected non-empty out.mp4"
    );
}

#[test]
fn t1_f03_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let rec_flv = env.work_dir().join("rec.flv");
    let out = env.run_bt(&[
        "live",
        "record",
        "14073662",
        "-d",
        "1s",
        "-o",
        rec_flv.to_str().unwrap(),
    ]);
    out.assert_exit(0);
    let log = env.read_ffmpeg_log().join("\n");
    assert!(
        log.contains("14073662") && log.contains("rec.flv"),
        "expected room stream URL and rec.flv in ffmpeg log: {log}"
    );
    assert!(rec_flv.exists(), "expected rec.flv to be created");
}

#[test]
fn t1_f03_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).without_ffmpeg();
    let out = env.run_bt(&["search", "hot"]);
    out.assert_exit(0).assert_stdout_contains("热搜词1");
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);
}

#[test]
fn t1_f03_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).without_ffmpeg();
    let out = env.run_bt(&["video", "info", "BV1uJ411r7hL"]);
    out.assert_exit(0)
        .assert_stdout_contains("BV1uJ411r7hL")
        .assert_stdout_contains("Mock Video BV1uJ411r7hL");
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);
}

#[test]
fn t1_f03_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).without_ffmpeg();
    let out_base = env.work_dir().join("raw");
    let out = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "--skip-mux",
        "-o",
        out_base.to_str().unwrap(),
    ]);
    out.assert_exit(0);
    assert!(
        !env.ffmpeg_log_path().exists(),
        "ffmpeg must not be invoked with --skip-mux"
    );
    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path.starts_with("/mock/media/video_"))
    );
    assert!(
        reqs.iter()
            .any(|r| r.path.starts_with("/mock/media/audio_"))
    );
    let raw_files: Vec<_> = std::fs::read_dir(env.work_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name().to_string_lossy().contains("raw")
                && e.metadata().map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
        })
        .collect();
    assert!(
        raw_files.len() >= 2,
        "expected separate video and audio files in work_dir, found: {:?}",
        raw_files
            .iter()
            .map(|e| e.file_name())
            .collect::<Vec<_>>()
    );
}

// ── Feature #4: API Base URL Testability Hook (t1_f04_01..t1_f04_05) ─────────

#[test]
fn t1_f04_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "search", "hot"]);
    let v = out.assert_json_ok();
    assert_eq!(v[0]["keyword"], "热搜词1");
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/web-interface/wbi/search/square")
    );
}

#[test]
fn t1_f04_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "live", "area", "游戏"]);
    let v = out.assert_json_ok();
    let arr = v.as_array().expect("area array");
    assert!(!arr.is_empty());
    assert!(arr.iter().any(|a| a["id"] == "86" || a["id"] == "235"));
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/room/v1/Area/getList")
    );
}

#[test]
fn t1_f04_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "auth", "status"]);
    let v = out.assert_json_ok();
    assert_eq!(v["is_login"], true);
    assert_eq!(v["uname"], "测试用户");
    assert_eq!(v["mid"], 293793435);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/web-interface/nav")
    );
}

#[test]
fn t1_f04_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let dm_xml = env.work_dir().join("dm.xml");
    let out = env.run_bt(&[
        "video",
        "danmaku",
        "BV1uJ411r7hL",
        "--format",
        "xml",
        "-o",
        dm_xml.to_str().unwrap(),
    ]);
    out.assert_exit(0);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path.ends_with(".xml") || r.path == "/x/v1/dm/list.so")
    );
    let xml = std::fs::read_to_string(&dm_xml).expect("read dm.xml");
    assert!(
        xml.contains("<d p=") && xml.contains("测试弹幕一条"),
        "unexpected danmaku XML: {xml}"
    );
}

#[test]
fn t1_f04_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "resolve", "https://b23.tv/pigt3PQ"]);
    let v = out.assert_json_ok();
    assert_eq!(v["kind"], "video");
    assert_eq!(v["bvid"], "BV1uJ411r7hL");
    assert_eq!(v["short_url"], "https://b23.tv/pigt3PQ");
    assert_eq!(v["network_used"], true);
    assert!(server.requests().iter().any(|r| r.path == "/pigt3PQ"));
}

// ── Feature #5: Auth Hardening, auth daily & completions (t1_f05_01..t1_f05_06)

#[test]
fn t1_f05_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&[
        "--json",
        "auth",
        "login",
        "--method",
        "cookie",
        "--sessdata",
        "s1",
        "--bili-jct",
        "j1",
        "--dede-user-id",
        "293793435",
    ]);
    let v = out.assert_json_ok();
    assert_eq!(v["success"], true);
    assert_eq!(v["uid"], "293793435");
    assert_eq!(v["room_id"], 31686846);
    assert_mode_0600(&env.default_cookie_path());
    let saved = env.read_cookie_file(&env.default_cookie_path());
    assert_eq!(saved.sessdata, "s1");
    assert_eq!(saved.bili_jct, "j1");
    assert_eq!(saved.dede_user_id, "293793435");
    assert_eq!(saved.room_id, 31686846);
    assert!(!saved.buvid3.is_empty());
    assert!(!saved.buvid4.is_empty());
}

#[test]
fn t1_f05_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_cookie_fixture(CookieFixture::without_room_id().with_dede_user_id(""));
    let out = env.run_bt(&["--json", "auth", "status"]);
    let v = out.assert_json_ok();
    assert_eq!(v["is_login"], true);
    assert_eq!(v["uid"], "293793435");
    assert_eq!(v["mid"], 293793435);
    assert_eq!(v["room_id"], 31686846);
    let saved = env.read_cookie_file(&env.default_cookie_path());
    assert_eq!(saved.dede_user_id, "293793435");
    assert_eq!(saved.room_id, 31686846);
}

#[test]
fn t1_f05_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "auth", "daily"]);
    let v = out.assert_json_ok();
    assert_eq!(v["exp_reward"]["login"], true);
    assert_eq!(v["exp_reward"]["watch"], true);
    assert_eq!(v["exp_reward"]["coins"], 30);
    assert_eq!(v["exp_reward"]["share"], false);
    assert_eq!(v["exp_reward"]["today_exp"], 40);
    assert_eq!(v["live_sign"]["signed"], true);
    assert_eq!(v["live_sign"]["already_signed"], false);
    assert_eq!(v["live_sign"]["offline"], false);
    assert_eq!(v["live_sign"]["text"], "3000点用户经验");
    assert_eq!(v["live_sign"]["had_sign_days"], 1);
    assert_eq!(v["live_sign"]["all_days"], 30);
}

#[test]
fn t1_f05_04() {
    let server = MockBiliServer::start();
    server.state().lock().unwrap().need_refresh = true;
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "auth", "refresh"]);
    let v = out.assert_json_ok();
    assert_eq!(v["need_refresh"], true);
    assert_eq!(v["success"], true);
    let saved = env.read_cookie_file(&env.default_cookie_path());
    assert_eq!(saved.sessdata, "mock_sessdata_token_refreshed");
    assert_eq!(saved.bili_jct, "mock_bili_jct_csrf_refreshed");
    assert_eq!(saved.refresh_token, "mock_refresh_token_abc_refreshed");
}

#[test]
fn t1_f05_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "auth", "logout"]);
    let v = out.assert_json_ok();
    assert_eq!(v["success"], true);
    assert!(!env.default_cookie_path().exists());
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.method == "POST" && r.path == "/login/exit/v2")
    );
}

#[test]
fn t1_f05_06() {
    let env = TestEnv::new();

    let ps = env.run_bt(&["completions", "powershell"]);
    ps.assert_exit(0)
        .assert_stdout_contains("bt")
        .assert_stdout_contains("Register-ArgumentCompleter");
    assert!(ps.stderr.is_empty());

    let elv = env.run_bt(&["completions", "elvish"]);
    elv.assert_exit(0)
        .assert_stdout_contains("bt")
        .assert_stdout_contains("edit:completion:arg-completer");
    assert!(elv.stderr.is_empty());

    let zsh = env.run_bt(&["--json", "completions", "zsh", "--install"]);
    let v = zsh.assert_json_ok();
    assert_eq!(v["installed"], true);
    assert_eq!(v["shell"], "zsh");
    let installed_path = env.home_dir().join(".zsh/completions/_bt");
    assert_eq!(v["path"], installed_path.display().to_string());
    assert!(
        installed_path.exists()
            && !std::fs::read_to_string(&installed_path)
                .unwrap()
                .trim()
                .is_empty()
    );
}

// ── Feature #6: live Refactor & Extensions (t1_f06_01..t1_f06_07) ────────────

#[test]
fn t1_f06_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "live", "start", "-a", "235", "-t", "测试标题", "-y"]);
    let v = out.assert_json_ok();
    assert_eq!(v["success"], true);
    assert_eq!(v["room_id"], 31686846);
    assert_eq!(v["area_id"], "235");
    assert_eq!(v["rtmp_addr"], "rtmp://live-push.bilivideo.com/live-bvc/");
    assert!(
        v["rtmp_code_masked"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "expected non-empty rtmp_code_masked in JSON: {v}"
    );
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);

    let reqs = server.requests();
    assert!(reqs.iter().any(|r| {
        r.method == "POST"
            && r.path == "/room/v1/Room/update"
            && r.form_param("room_id") == Some("31686846")
            && r.form_param("title") == Some("测试标题")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(reqs.iter().any(|r| {
        r.method == "POST"
            && r.path == "/room/v1/Room/startLive"
            && r.form_param("room_id") == Some("31686846")
            && r.form_param("area_v2") == Some("235")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));

    let stream_path = env
        .default_cookie_path()
        .parent()
        .unwrap()
        .join("stream_info.txt");
    assert!(stream_path.exists(), "expected stream_info.txt");
    assert_mode_0600(&stream_path);
    let stream_txt = std::fs::read_to_string(&stream_path).unwrap();
    assert!(stream_txt.contains("rtmp://live-push.bilivideo.com/live-bvc/"));
    assert!(stream_txt.contains("live_mock_stream"));
}

#[test]
fn t1_f06_02() {
    let server = MockBiliServer::start();
    server
        .state()
        .lock()
        .unwrap()
        .rooms
        .get_mut(&31686846)
        .unwrap()
        .live_status = 1;
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "live", "start", "-y"]);
    assert!(
        out.code == 0 || out.code == 1,
        "unexpected exit code {}: stdout={} stderr={}",
        out.code,
        out.stdout,
        out.stderr
    );
    let v = out.json();
    if out.code == 1 {
        assert_eq!(v["ok"], false);
    }

    let reqs = server.requests();
    assert_eq!(
        reqs.iter()
            .filter(|r| r.method == "POST" && r.path == "/room/v1/Room/stopLive")
            .count(),
        0,
        "live start -y must NEVER call stopLive when already live"
    );
    assert_eq!(
        reqs.iter()
            .filter(|r| r.method == "POST" && r.path == "/room/v1/Room/startLive")
            .count(),
        0
    );
    assert!(reqs.iter().any(|r| r.path == "/room/v1/Room/get_info"));
    assert_eq!(
        server.state().lock().unwrap().rooms[&31686846].live_status,
        1
    );
}

#[test]
fn t1_f06_03() {
    let server = MockBiliServer::start();
    server
        .state()
        .lock()
        .unwrap()
        .rooms
        .get_mut(&31686846)
        .unwrap()
        .live_status = 1;
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "live", "stop"]);
    let v = out.assert_json_ok();
    assert_eq!(v["success"], true);
    assert_eq!(v["room_id"], 31686846);
    assert!(out.stderr.is_empty(), "expected empty stderr: {}", out.stderr);

    let stop_calls = server
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST" && r.path == "/room/v1/Room/stopLive")
        .collect::<Vec<_>>();
    assert_eq!(stop_calls.len(), 1);
    assert_eq!(stop_calls[0].form_param("room_id"), Some("31686846"));
    assert_eq!(
        stop_calls[0].form_param("csrf"),
        Some("mock_bili_jct_csrf")
    );
    assert_eq!(
        server.state().lock().unwrap().rooms[&31686846].live_status,
        0
    );
}

#[test]
fn t1_f06_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "live", "title", "新直播标题"]);
    let v = out.assert_json_ok();
    assert_eq!(v["success"], true);
    assert_eq!(v["room_id"], 31686846);
    assert_eq!(v["title"], "新直播标题");
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/room/v1/Room/update"
            && r.form_param("room_id") == Some("31686846")
            && r.form_param("title") == Some("新直播标题")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert_eq!(
        server.state().lock().unwrap().rooms[&31686846].title,
        "新直播标题"
    );
}

#[test]
fn t1_f06_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&[
        "--json",
        "live",
        "info",
        "https://live.bilibili.com/14073662",
    ]);
    let v = out.assert_json_ok();
    let info = if v.get("room_info").is_some() && v.get("room_id").is_none() {
        &v["room_info"]
    } else {
        &v
    };
    assert_eq!(info["room_id"], 14073662);
    assert_eq!(info["title"], "官方测试直播");
    assert_eq!(info["live_status"], 1);
    assert!(server.requests().iter().any(|r| {
        (r.path == "/room/v1/Room/get_info" || r.path == "/room/v1/Room/room_init")
            && (r.query_param("room_id") == Some("14073662")
                || r.query_param("id") == Some("14073662"))
    }));
}

#[test]
fn t1_f06_06() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&[
        "--json",
        "live",
        "stream",
        "14073662",
        "--quality",
        "10000",
    ]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(
        serialized.contains("/mock/live/14073662.flv"),
        "expected stream URL in JSON: {serialized}"
    );
    assert!(
        serialized.contains("10000"),
        "expected quality 10000 in JSON: {serialized}"
    );
    assert!(server.requests().iter().any(|r| {
        (r.path == "/xlive/web-room/v2/index/getRoomPlayInfo"
            && r.query_param("room_id") == Some("14073662"))
            || (r.path == "/room/v1/Room/playUrl" && r.query_param("cid") == Some("14073662"))
    }));
}

#[test]
fn t1_f06_07() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let send_out = env.run_bt(&["--json", "live", "send", "14073662", "晚上好"]);
    let send_v = send_out.assert_json_ok();
    assert!(
        send_v.get("success") == Some(&json!(true)) || send_v.get("code") == Some(&json!(0)),
        "unexpected live send JSON: {send_v}"
    );
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/msg/send"
            && r.form_param("msg") == Some("晚上好")
            && r.form_param("roomid") == Some("14073662")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));

    let dm_out = env.run_bt(&[
        "--json",
        "live",
        "danmaku",
        "14073662",
        "--limit",
        "10",
    ]);
    let dm_v = dm_out.assert_json_ok();
    let serialized = serde_json::to_string(&dm_v).unwrap();
    assert!(serialized.contains("欢迎来到直播间"));
    assert!(serialized.contains("晚上好"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/xlive/web-room/v1/dM/gethistory"
            && r.query_param("roomid") == Some("14073662")
    }));
}

// ── Feature #7: search Refactor (t1_f07_01..t1_f07_05) ───────────────────────

#[test]
fn t1_f07_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&[
        "--json",
        "search",
        "video",
        "Rust",
        "--page",
        "2",
        "--order",
        "pubdate",
    ]);
    let v = out.assert_json_ok();
    assert_eq!(v["page"], 2);
    assert_eq!(v["result"][0]["bvid"], "BV1uJ411r7hL");
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/wbi/search/type"
            && r.query_param("search_type") == Some("video")
            && r.query_param("keyword") == Some("Rust")
            && r.query_param("page") == Some("2")
            && r.query_param("order") == Some("pubdate")
            && r.has_wbi_signature()
    }));
}

#[test]
fn t1_f07_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&[
        "--json",
        "search",
        "user",
        "测试UP主",
        "--order",
        "fans",
    ]);
    let v = out.assert_json_ok();
    assert_eq!(v["result"][0]["mid"], 293793435);
    assert_eq!(v["result"][0]["uname"], "测试UP主");
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/wbi/search/type"
            && r.query_param("search_type") == Some("bili_user")
            && r.query_param("keyword") == Some("测试UP主")
            && r.query_param("order") == Some("fans")
            && r.has_wbi_signature()
    }));
}

#[test]
fn t1_f07_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "search", "live", "测试主播", "--page", "1"]);
    let v = out.assert_json_ok();
    assert_eq!(v["result"]["live_room"][0]["roomid"], 14073662);
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/wbi/search/type"
            && r.query_param("search_type") == Some("live")
            && r.query_param("keyword") == Some("测试主播")
            && r.query_param("page") == Some("1")
            && r.has_wbi_signature()
    }));
}

#[test]
fn t1_f07_04() {
    let server = MockBiliServer::start();
    let items: Vec<_> = (1..=10)
        .map(|i| json!({ "keyword": format!("热搜词{i}"), "show_name": format!("热搜词{i}"), "icon_name": "" }))
        .collect();
    server.set_fault(
        "/x/web-interface/wbi/search/square",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": { "trending": { "title": "bilibili热搜", "list": items } }
        })),
    );
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "search", "hot", "--limit", "5"]);
    let v = out.assert_json_ok();
    let arr = v.as_array().expect("hot array");
    assert_eq!(arr.len(), 5);
    assert_eq!(arr[0]["keyword"], "热搜词1");
    assert_eq!(arr[4]["keyword"], "热搜词5");
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/wbi/search/square"
            && r.query_param("limit") == Some("5")
            && r.has_wbi_signature()
    }));
}

#[test]
fn t1_f07_05() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/wbi/search/type",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "page": 1,
                "pagesize": 20,
                "numResults": 1,
                "numPages": 1,
                "result": [{
                    "bvid": "BV1uJ411r7hL",
                    "aid": 79677524,
                    "title": "<em class=\"keyword\">Rust</em> &amp; &lt;C++&gt;",
                    "author": "测试UP主",
                    "mid": 293793435,
                    "play": 12345,
                    "duration": "03:45",
                    "pubdate": 1700000000
                }]
            }
        })),
    );
    let env = TestEnv::new().with_server(&server).with_no_color(true);
    let out = env.run_bt(&["search", "video", "Rust & <C++>"]);
    out.assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("Rust & <C++>");
    assert!(!out.stdout.contains("<em"));
    assert!(!out.stdout.contains("&amp;"));
    assert!(!out.stdout.contains("&lt;"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/wbi/search/type"
            && r.query_param("keyword") == Some("Rust & <C++>")
            && r.has_wbi_signature()
    }));
}

// ── Feature #8: fav Refactor & CRUD Symmetry (t1_f08_01..t1_f08_06) ──────────

#[test]
fn t1_f08_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out1 = env.run_bt(&["--json", "fav", "list"]);
    let v1 = out1.assert_json_ok();
    assert_eq!(v1["count"], 1);
    assert_eq!(v1["list"][0]["id"], 1052622027);
    assert_eq!(v1["list"][0]["title"], "默认收藏夹");
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/v3/fav/folder/created/list-all"
            && r.query_param("up_mid") == Some("293793435")
    }));

    server.reset_requests();
    let out2 = env.run_bt(&["--json", "fav", "list", "--mid", "999"]);
    let v2 = out2.assert_json_ok();
    assert_eq!(v2["count"], 1);
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/v3/fav/folder/created/list-all" && r.query_param("up_mid") == Some("999")
    }));
}

#[test]
fn t1_f08_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out1 = env.run_bt(&[
        "--json",
        "fav",
        "show",
        "https://www.bilibili.com/medialist/detail/ml1052622027",
    ]);
    let v1 = out1.assert_json_ok();
    assert_eq!(v1["info"]["id"], 1052622027);
    assert_eq!(v1["medias"][0]["bvid"], "BV1uJ411r7hL");
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/v3/fav/resource/list" && r.query_param("media_id") == Some("1052622027")
    }));

    server.reset_requests();
    let out2 = env.run_bt(&[
        "--json",
        "fav",
        "show",
        "https://space.bilibili.com/293793435/favlist?fid=1052622027",
    ]);
    let v2 = out2.assert_json_ok();
    assert_eq!(v2["info"]["id"], 1052622027);
    assert_eq!(v2["medias"][0]["bvid"], "BV1uJ411r7hL");
}

#[test]
fn t1_f08_03() {
    let server = MockBiliServer::start();
    {
        let mut st = server.state().lock().unwrap();
        st.fav_folders[0].items = (1..=25)
            .map(|i| MockFavItem {
                aid: 1000 + i,
                bvid: format!("BV1test{i:05}"),
                title: format!("收藏视频 {i}"),
                duration: 120,
                upper_mid: 293793435,
                upper_name: "测试UP主".to_string(),
                valid: true,
            })
            .collect();
    }
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let csv_path = env.work_dir().join("fav.csv");
    let out = env.run_bt(&[
        "fav",
        "export",
        "1052622027",
        "--format",
        "csv",
        "-o",
        csv_path.to_str().unwrap(),
    ]);
    out.assert_exit(0);

    let list_reqs: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|r| {
            r.path == "/x/v3/fav/resource/list" && r.query_param("media_id") == Some("1052622027")
        })
        .collect();
    assert!(list_reqs.iter().any(|r| r.query_param("pn") == Some("1")));
    assert!(list_reqs.iter().any(|r| r.query_param("pn") == Some("2")));

    let csv_text = std::fs::read_to_string(&csv_path).expect("read fav.csv");
    let lines: Vec<_> = csv_text.lines().collect();
    assert_eq!(lines.len(), 26);
    assert_eq!(lines[0], "bvid,aid,title,upper_name,play,duration,link");
    assert!(csv_text.contains("BV1test00001"));
    assert!(csv_text.contains("BV1test00025"));
}

#[test]
fn t1_f08_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "fav", "create", "我的收藏", "--private"]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("2000000001"));
    assert!(serialized.contains("我的收藏"));
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v3/fav/folder/add"
            && r.form_param("title") == Some("我的收藏")
            && r.form_param("privacy") == Some("1")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    let st = server.state().lock().unwrap();
    assert_eq!(st.fav_folders.len(), 2);
    assert_eq!(st.fav_folders[1].attr, 1);
}

#[test]
fn t1_f08_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let add_out = env.run_bt(&["--json", "fav", "add", "1052622027", "BV1GJ411x7h7"]);
    add_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v3/fav/resource/deal"
            && r.form_param("rid") == Some("170001")
            && r.form_param("add_media_ids") == Some("1052622027")
            && r.form_param("type") == Some("2")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert_eq!(server.state().lock().unwrap().fav_folders[0].items.len(), 2);

    server.reset_requests();
    let rm_out = env.run_bt(&["--json", "fav", "remove", "1052622027", "BV1GJ411x7h7"]);
    rm_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v3/fav/resource/deal"
            && r.form_param("rid") == Some("170001")
            && r.form_param("del_media_ids") == Some("1052622027")
            && r.form_param("type") == Some("2")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert_eq!(server.state().lock().unwrap().fav_folders[0].items.len(), 1);
}

#[test]
fn t1_f08_06() {
    let server = MockBiliServer::start();
    server.state().lock().unwrap().fav_folders[0]
        .items
        .push(MockFavItem {
            aid: 999999,
            bvid: "BV1dead00001".to_string(),
            title: "已失效视频".to_string(),
            duration: 10,
            upper_mid: 293793435,
            upper_name: "测试UP主".to_string(),
            valid: false,
        });
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let clean_out = env.run_bt(&["-y", "--json", "fav", "clean", "1052622027"]);
    clean_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v3/fav/resource/clean"
            && r.form_param("media_id") == Some("1052622027")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    {
        let st = server.state().lock().unwrap();
        assert_eq!(st.fav_folders[0].items.len(), 1);
        assert_eq!(st.fav_folders[0].items[0].aid, 79677524);
    }

    let del_out = env.run_bt(&["-y", "--json", "fav", "delete", "1052622027"]);
    del_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v3/fav/folder/del"
            && r.form_param("media_ids") == Some("1052622027")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(server.state().lock().unwrap().fav_folders.is_empty());
}

// ── Feature #9: later & history Refactor & CRUD (t1_f09_01..t1_f09_05) ────────

#[test]
fn t1_f09_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let list_out = env.run_bt(&["--json", "later", "list"]);
    let list_v = list_out.assert_json_ok();
    assert_eq!(list_v["count"], 2);
    assert_eq!(list_v["list"][0]["bvid"], "BV1uJ411r7hL");
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/v2/history/toview")
    );

    let add_out = env.run_bt(&["--json", "later", "add", "BV1uJ411r7hL"]);
    let add_v = add_out.assert_json_ok();
    assert_eq!(add_v["success"], true);
    assert_eq!(add_v["video"], "BV1uJ411r7hL");
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/history/toview/add"
            && r.form_param("bvid") == Some("BV1uJ411r7hL")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
}

#[test]
fn t1_f09_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "later", "remove", "BV1GJ411x7h7"]);
    out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/history/toview/del"
            && r.form_param("aid") == Some("170001")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    let st = server.state().lock().unwrap();
    assert_eq!(st.toview.len(), 1);
    assert!(!st.toview.iter().any(|i| i.aid == 170001));
}

#[test]
fn t1_f09_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["-y", "--json", "later", "clear", "--viewed-only"]);
    let v = out.assert_json_ok();
    assert_eq!(v["success"], true);
    assert_eq!(v["viewed_only"], true);
    let reqs = server.requests();
    assert!(reqs.iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/history/toview/del"
            && r.form_param("viewed") == Some("true")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(
        !reqs
            .iter()
            .any(|r| r.path == "/x/v2/history/toview/clear")
    );
    let st = server.state().lock().unwrap();
    assert_eq!(st.toview.len(), 1);
    assert_eq!(st.toview[0].aid, 79677524);
}

#[test]
fn t1_f09_04() {
    let server = MockBiliServer::start();
    {
        let mut st = server.state().lock().unwrap();
        for i in 1..=11 {
            st.history.push(MockHistoryItem {
                oid: 800_000 + i,
                bvid: format!("BV1hist{i:05}"),
                title: format!("历史视频 {i}"),
                author_name: "测试UP主".to_string(),
                business: "archive".to_string(),
                cid: 900_000 + i,
                progress: 10,
                duration: 100,
                view_at: 1_700_000_000 - i,
            });
        }
    }
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "history", "list", "--limit", "5", "-c", "archive"]);
    let v = out.assert_json_ok();
    let arr = v.as_array().expect("history list array");
    assert_eq!(arr.len(), 5);
    for item in arr {
        assert_eq!(item["history"]["business"], "archive");
    }
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/history/cursor"
            && r.query_param("ps") == Some("5")
            && r.query_param("type") == Some("archive")
    }));
}

#[test]
fn t1_f09_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let del_out = env.run_bt(&["--json", "history", "delete", "BV1uJ411r7hL"]);
    del_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/history/delete"
            && r.form_param("kid") == Some("archive_79677524")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert_eq!(server.state().lock().unwrap().history.len(), 1);

    let clear_out = env.run_bt(&["-y", "--json", "history", "clear"]);
    clear_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/history/clear"
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(server.state().lock().unwrap().history.is_empty());
}

// ── Feature #10: video Inspection & Interactions (t1_f10_01..t1_f10_07) ──────

#[test]
fn t1_f10_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let json_out = env.run_bt(&[
        "--json",
        "video",
        "info",
        "https://www.bilibili.com/video/BV1uJ411r7hL",
    ]);
    let v = json_out.assert_json_ok();
    assert_eq!(v["bvid"], "BV1uJ411r7hL");
    assert_eq!(v["title"], "Mock Video BV1uJ411r7hL");
    let pages = v["pages"].as_array().expect("pages array");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0]["part"], "P1 开篇");
    assert_eq!(pages[1]["part"], "P2 进阶");

    let human_out = env.run_bt(&[
        "video",
        "info",
        "https://www.bilibili.com/video/BV1uJ411r7hL",
    ]);
    human_out
        .assert_exit(0)
        .assert_stdout_contains("分P列表")
        .assert_stdout_contains("P1 开篇")
        .assert_stdout_contains("P2 进阶");
}

#[test]
fn t1_f10_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "video", "streams", "BV1uJ411r7hL", "-p", "2"]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("80"));
    assert!(serialized.contains("avc1.640028"));
    assert!(serialized.contains("hev1.1.6.L120.90"));
    assert!(serialized.contains("30280"));
    assert!(server.requests().iter().any(|r| {
        (r.path == "/x/player/wbi/playurl" || r.path == "/x/player/playurl")
            && r.query_param("cid") == Some("146044694")
    }));
}

#[test]
fn t1_f10_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out_srt = env.work_dir().join("sub.srt");
    let out = env.run_bt(&[
        "video",
        "subtitle",
        "BV1uJ411r7hL",
        "--lang",
        "zh-Hans",
        "--format",
        "srt",
        "-o",
        out_srt.to_str().unwrap(),
    ]);
    out.assert_exit(0);
    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path == "/x/player/wbi/v2" || r.path == "/x/player/v2")
    );
    assert!(
        reqs.iter()
            .any(|r| r.path == "/mock/subtitle/zh-Hans.json")
    );
    let srt = std::fs::read_to_string(&out_srt).expect("read sub.srt");
    assert!(srt.contains("1\n"));
    assert!(srt.contains("00:00:00,500 --> 00:00:02,500"));
    assert!(srt.contains("Hello Bilibili"));
    assert!(srt.contains("2\n"));
    assert!(srt.contains("00:00:03,000 --> 00:00:05,000"));
    assert!(srt.contains("Mock Subtitle Line 2"));
}

#[test]
fn t1_f10_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "video", "summary", "BV1uJ411r7hL"]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("这是一个测试视频的 AI 总结。"));
    assert!(serialized.contains("核心内容") || serialized.contains("介绍基础概念"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/view/conclusion/get" && r.has_wbi_signature()
    }));
}

#[test]
fn t1_f10_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let tags_out = env.run_bt(&["--json", "video", "tags", "BV1uJ411r7hL"]);
    let tags_v = tags_out.assert_json_ok();
    let tags_str = serde_json::to_string(&tags_v).unwrap();
    assert!(tags_str.contains("Rust"));
    assert!(tags_str.contains("CLI"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/view/detail/tag" || r.path == "/x/tag/archive/tags"
    }));

    server.reset_requests();
    let rel_out = env.run_bt(&["--json", "video", "related", "BV1uJ411r7hL"]);
    let rel_v = rel_out.assert_json_ok();
    let rel_str = serde_json::to_string(&rel_v).unwrap();
    assert!(rel_str.contains("BV17x411w7KC"));
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/web-interface/archive/related")
    );
}

#[test]
fn t1_f10_06() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let like_out = env.run_bt(&["--json", "video", "like", "BV1uJ411r7hL"]);
    let like_v = like_out.assert_json_ok();
    assert_eq!(like_v["video"], "BV1uJ411r7hL");
    assert_eq!(like_v["liked"], true);
    assert_eq!(like_v["success"], true);
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/web-interface/archive/like"
            && r.form_param("aid") == Some("79677524")
            && r.form_param("like") == Some("1")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(
        server
            .state()
            .lock()
            .unwrap()
            .liked_videos
            .contains(&79677524)
    );

    server.reset_requests();
    let cancel_out = env.run_bt(&["--json", "video", "like", "BV1uJ411r7hL", "--cancel"]);
    let cancel_v = cancel_out.assert_json_ok();
    assert_eq!(cancel_v["liked"], false);
    assert_eq!(cancel_v["success"], true);
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/web-interface/archive/like"
            && r.form_param("like") == Some("2")
    }));
    assert!(
        !server
            .state()
            .lock()
            .unwrap()
            .liked_videos
            .contains(&79677524)
    );
}

#[test]
fn t1_f10_07() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let coin_out = env.run_bt(&[
        "--json",
        "video",
        "coin",
        "BV1uJ411r7hL",
        "-n",
        "2",
        "--also-like",
    ]);
    let coin_v = coin_out.assert_json_ok();
    assert_eq!(coin_v["coins"], 2);
    assert_eq!(coin_v["also_like"], true);
    assert_eq!(coin_v["liked"], true);
    assert_eq!(coin_v["success"], true);
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/web-interface/coin/add"
            && r.form_param("multiply") == Some("2")
            && r.form_param("select_like") == Some("1")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));

    let triple_out = env.run_bt(&["--json", "video", "triple", "BV1uJ411r7hL"]);
    let triple_v = triple_out.assert_json_ok();
    assert_eq!(triple_v["like"], true);
    assert_eq!(triple_v["coin"], true);
    assert_eq!(triple_v["fav"], true);
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/web-interface/archive/like/triple"
            && r.form_param("aid") == Some("79677524")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
}

// ── Feature #11: video Danmaku & Comments (t1_f11_01..t1_f11_05) ─────────────

#[test]
fn t1_f11_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out_ass = env.work_dir().join("dm.ass");
    let out = env.run_bt(&[
        "video",
        "danmaku",
        "BV1uJ411r7hL",
        "--format",
        "ass",
        "-o",
        out_ass.to_str().unwrap(),
    ]);
    out.assert_exit(0);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/v1/dm/list.so" || r.path == "/146044693.xml")
    );
    let ass = std::fs::read_to_string(&out_ass).expect("read dm.ass");
    assert!(ass.contains("[Script Info]"));
    assert!(ass.contains("[V4+ Styles]"));
    assert!(ass.contains("[Events]"));
    assert!(ass.contains("Dialogue:"));
    assert!(ass.contains("测试弹幕一条"));
    assert!(ass.contains("顶部红字弹幕"));
}

#[test]
fn t1_f11_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&["--json", "video", "danmaku", "BV1uJ411r7hL", "--format", "json"]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("测试弹幕一条"));
    assert!(serialized.contains("顶部红字弹幕"));
    assert!(serialized.contains("12.345"));
    assert!(serialized.contains("16777215") || serialized.contains("#FFFFFF"));
}

#[test]
fn t1_f11_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&[
        "--json",
        "video",
        "comments",
        "BV1uJ411r7hL",
        "--page",
        "1",
        "--sort",
        "hot",
    ]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("104192624480"));
    assert!(serialized.contains("第一条测试评论"));
    assert!(serialized.contains("测试评论用户"));
    assert!(server.requests().iter().any(|r| {
        (r.path == "/x/v2/reply" || r.path == "/x/v2/reply/wbi/main" || r.path == "/x/v2/reply/hot")
            && r.query_param("oid") == Some("79677524")
    }));
}

#[test]
fn t1_f11_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&[
        "--json",
        "video",
        "comment",
        "post",
        "BV1uJ411r7hL",
        "写得很好",
    ]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("104192624481"));
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/reply/add"
            && r.form_param("type") == Some("1")
            && r.form_param("oid") == Some("79677524")
            && r.form_param("message") == Some("写得很好")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(
        server
            .state()
            .lock()
            .unwrap()
            .comments
            .iter()
            .any(|c| c.message == "写得很好" && c.rpid == 104192624481)
    );
}

#[test]
fn t1_f11_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let like_out = env.run_bt(&[
        "--json",
        "video",
        "comment",
        "like",
        "BV1uJ411r7hL",
        "104192624480",
    ]);
    like_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/reply/action"
            && r.form_param("type") == Some("1")
            && r.form_param("oid") == Some("79677524")
            && r.form_param("rpid") == Some("104192624480")
            && r.form_param("action") == Some("1")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert_eq!(server.state().lock().unwrap().comments[0].like, 6);

    let del_out = env.run_bt(&[
        "--json",
        "video",
        "comment",
        "delete",
        "BV1uJ411r7hL",
        "104192624480",
    ]);
    del_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/reply/del"
            && r.form_param("type") == Some("1")
            && r.form_param("oid") == Some("79677524")
            && r.form_param("rpid") == Some("104192624480")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(server.state().lock().unwrap().comments.is_empty());
}

// ── Feature #12: video download (ffmpeg Muxing) (t1_f12_01..t1_f12_05) ────────

#[test]
fn t1_f12_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let tmpl = env.work_dir().join("{bvid}_{title}.mp4");
    let out = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "--quality",
        "80",
        "-e",
        "hevc",
        "-o",
        tmpl.to_str().unwrap(),
    ]);
    out.assert_exit(0);

    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path == "/mock/media/video_80_hevc.m4s")
    );
    assert!(
        reqs.iter()
            .any(|r| r.path == "/mock/media/audio_30280.m4s")
    );
    assert!(
        !reqs
            .iter()
            .any(|r| r.path == "/mock/media/video_80_avc.m4s"
                || r.path == "/mock/media/video_64_avc.m4s")
    );

    let mp4_files: Vec<_> = std::fs::read_dir(env.work_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s.starts_with("BV1uJ411r7hL_")
                && s.contains("Mock")
                && s.ends_with(".mp4")
                && e.metadata().map(|m| m.len() > 0).unwrap_or(false)
        })
        .collect();
    assert_eq!(mp4_files.len(), 1, "expected 1 templated .mp4 output file");

    let log = env.read_ffmpeg_log();
    assert!(log.iter().any(|l| l.contains("-version")));
    assert!(
        log.iter()
            .any(|l| l.contains("-i") && l.contains("BV1uJ411r7hL_") && l.contains(".mp4"))
    );
}

#[test]
fn t1_f12_02() {
    let server = MockBiliServer::start();
    let port = server.port();
    server.set_fault(
        "/x/web-interface/wbi/view",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "bvid": "BV1uJ411r7hL",
                "aid": 79677524,
                "videos": 3,
                "tid": 171,
                "tname": "电子竞技",
                "pic": format!("http://127.0.0.1:{port}/mock/cover/BV1uJ411r7hL.jpg"),
                "title": "Mock Video BV1uJ411r7hL",
                "pubdate": 1700000000,
                "ctime": 1700000000,
                "desc": "Mock video description",
                "duration": 330,
                "owner": { "mid": 293793435, "name": "测试UP主", "face": "" },
                "stat": { "view": 10000, "danmaku": 120, "reply": 45, "favorite": 300, "coin": 200, "share": 50, "like": 800 },
                "cid": 146044693,
                "pages": [
                    { "cid": 146044693, "page": 1, "from": "vupload", "part": "P1 开篇", "duration": 120 },
                    { "cid": 146044694, "page": 2, "from": "vupload", "part": "P2 进阶", "duration": 105 },
                    { "cid": 146044695, "page": 3, "from": "vupload", "part": "P3 尾声", "duration": 105 }
                ]
            }
        })),
    );
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let tmpl = env.work_dir().join("{bvid}_p{page}.mp4");
    let out = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "-p",
        "1-2",
        "-o",
        tmpl.to_str().unwrap(),
    ]);
    out.assert_exit(0);

    let p1 = env.work_dir().join("BV1uJ411r7hL_p1.mp4");
    let p2 = env.work_dir().join("BV1uJ411r7hL_p2.mp4");
    let p3 = env.work_dir().join("BV1uJ411r7hL_p3.mp4");
    assert!(p1.exists() && std::fs::metadata(&p1).unwrap().len() > 0);
    assert!(p2.exists() && std::fs::metadata(&p2).unwrap().len() > 0);
    assert!(!p3.exists(), "page 3 must not be downloaded");

    let reqs = server.requests();
    assert!(reqs.iter().any(|r| {
        (r.path == "/x/player/wbi/playurl" || r.path == "/x/player/playurl")
            && r.query_param("cid") == Some("146044693")
    }));
    assert!(reqs.iter().any(|r| {
        (r.path == "/x/player/wbi/playurl" || r.path == "/x/player/playurl")
            && r.query_param("cid") == Some("146044694")
    }));
    assert!(!reqs.iter().any(|r| r.query_param("cid") == Some("146044695")));

    let log = env.read_ffmpeg_log();
    assert!(log.iter().any(|l| l.contains("BV1uJ411r7hL_p1.mp4")));
    assert!(log.iter().any(|l| l.contains("BV1uJ411r7hL_p2.mp4")));
}

#[test]
fn t1_f12_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let out_audio = env.work_dir().join("audio.m4a");
    let out = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "--audio-only",
        "-o",
        out_audio.to_str().unwrap(),
    ]);
    out.assert_exit(0);

    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path == "/mock/media/audio_30280.m4s")
    );
    assert!(
        !reqs
            .iter()
            .any(|r| r.path.starts_with("/mock/media/video_"))
    );
    assert!(out_audio.exists() && std::fs::metadata(&out_audio).unwrap().len() > 0);
}

#[test]
fn t1_f12_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).without_ffmpeg();
    let out_vid = env.work_dir().join("vid.m4s");
    let out = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "--video-only",
        "--skip-mux",
        "-o",
        out_vid.to_str().unwrap(),
    ]);
    out.assert_exit(0);
    assert_eq!(std::fs::read(&out_vid).unwrap(), MOCK_MP4_BYTES);

    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path.starts_with("/mock/media/video_80_"))
    );
    assert!(
        !reqs
            .iter()
            .any(|r| r.path.starts_with("/mock/media/audio_"))
    );
}

#[test]
fn t1_f12_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let out_mp4 = env.work_dir().join("full.mp4");
    let out = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "--danmaku",
        "--subtitle",
        "--cover",
        "-o",
        out_mp4.to_str().unwrap(),
    ]);
    let v = out.assert_json_ok();
    assert!(serde_json::to_string(&v).unwrap().contains("full.mp4"));
    assert!(out_mp4.exists() && std::fs::metadata(&out_mp4).unwrap().len() > 0);
    assert!(
        env.read_ffmpeg_log()
            .iter()
            .any(|l| l.contains("full.mp4"))
    );

    let entries: Vec<_> = std::fs::read_dir(env.work_dir())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();

    let cover = entries
        .iter()
        .find(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name.starts_with("full") && (name.ends_with(".jpg") || name.ends_with(".jpeg"))
        })
        .expect("expected cover .jpg sidecar");
    assert_eq!(std::fs::read(cover).unwrap(), MOCK_JPEG_BYTES);

    let sub = entries
        .iter()
        .find(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name.starts_with("full") && name.ends_with(".srt")
        })
        .expect("expected subtitle .srt sidecar");
    assert!(std::fs::read_to_string(sub).unwrap().contains("Hello Bilibili"));

    let dm = entries
        .iter()
        .find(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name.starts_with("full") && (name.ends_with(".xml") || name.ends_with(".ass"))
        })
        .expect("expected danmaku .xml/.ass sidecar");
    assert!(std::fs::read_to_string(dm).unwrap().contains("测试弹幕一条"));

    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path == "/mock/cover/BV1uJ411r7hL.jpg")
    );
    assert!(
        reqs.iter()
            .any(|r| r.path == "/x/player/wbi/v2" || r.path == "/x/player/v2")
    );
    assert!(
        reqs.iter()
            .any(|r| r.path.starts_with("/mock/subtitle/"))
    );
    assert!(
        reqs.iter()
            .any(|r| r.path == "/x/v1/dm/list.so" || r.path.ends_with(".xml"))
    );
}

// ── Feature #13: user, feed & rank Modules (t1_f13_01..t1_f13_05) ────────────

#[test]
fn t1_f13_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&[
        "--json",
        "user",
        "info",
        "https://space.bilibili.com/123456",
    ]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("123456"));
    assert!(serialized.contains("用户123456"));
    assert!(serialized.contains("50000"));
    assert!(serialized.contains("120"));
    let reqs = server.requests();
    assert!(reqs.iter().any(|r| {
        r.path == "/x/space/wbi/acc/info"
            && r.query_param("mid") == Some("123456")
            && r.has_wbi_signature()
    }));
    assert!(reqs.iter().any(|r| {
        r.path == "/x/relation/stat" && r.query_param("vmid") == Some("123456")
    }));
}

#[test]
fn t1_f13_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let out = env.run_bt(&[
        "--json",
        "user",
        "videos",
        "123456",
        "--page",
        "1",
        "--page-size",
        "10",
        "--order",
        "click",
        "--keyword",
        "Rust",
    ]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("BV1uJ411r7hL"));
    assert!(serialized.contains("Mock Space Video Rust"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/space/wbi/arc/search"
            && r.query_param("mid") == Some("123456")
            && r.query_param("ps") == Some("10")
            && r.query_param("order") == Some("click")
            && r.query_param("keyword") == Some("Rust")
            && r.has_wbi_signature()
    }));
}

#[test]
fn t1_f13_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let dyn_out = env.run_bt(&["--json", "user", "dynamics", "123456"]);
    let dyn_v = dyn_out.assert_json_ok();
    let dyn_str = serde_json::to_string(&dyn_v).unwrap();
    assert!(dyn_str.contains("9000000001") && dyn_str.contains("Mock Dynamic Video"));
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/polymer/web-dynamic/v1/feed/space")
    );

    let follow_out = env.run_bt(&["--json", "user", "follow", "123456"]);
    follow_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/relation/modify"
            && r.form_param("fid") == Some("123456")
            && r.form_param("act") == Some("1")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(
        server
            .state()
            .lock()
            .unwrap()
            .followed_mids
            .contains(&123456)
    );

    server.reset_requests();
    let unfollow_out = env.run_bt(&["--json", "user", "unfollow", "123456"]);
    unfollow_out.assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/relation/modify"
            && r.form_param("fid") == Some("123456")
            && r.form_param("act") == Some("2")
            && r.form_param("csrf") == Some("mock_bili_jct_csrf")
    }));
    assert!(
        !server
            .state()
            .lock()
            .unwrap()
            .followed_mids
            .contains(&123456)
    );
}

#[test]
fn t1_f13_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();
    let out = env.run_bt(&["--json", "feed", "--type", "video"]);
    let v = out.assert_json_ok();
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(serialized.contains("9000000002"));
    assert!(serialized.contains("Mock Feed All Video"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/polymer/web-dynamic/v1/feed/all"
            && r.query_param("type") == Some("video")
            && r.header("cookie")
                .is_some_and(|c| c.contains("SESSDATA="))
    }));
}

#[test]
fn t1_f13_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let rank_out = env.run_bt(&["--json", "rank", "video", "--rid", "0", "--type", "all"]);
    let rank_v = rank_out.assert_json_ok();
    let rank_str = serde_json::to_string(&rank_v).unwrap();
    assert!(rank_str.contains("BV1uJ411r7hL"));
    assert!(rank_str.contains("Mock Video BV1uJ411r7hL"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/ranking/v2"
            && r.query_param("rid") == Some("0")
            && r.query_param("type") == Some("all")
    }));

    let pop_out = env.run_bt(&[
        "--json",
        "rank",
        "popular",
        "--page",
        "1",
        "--page-size",
        "10",
    ]);
    let pop_v = pop_out.assert_json_ok();
    assert!(serde_json::to_string(&pop_v).unwrap().contains("BV1uJ411r7hL"));
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/popular"
            && r.query_param("pn") == Some("1")
            && r.query_param("ps") == Some("10")
    }));
}

// ── Feature #14: Help, Banner, README.md & Audit Sync (t1_f14_01..t1_f14_05) ─

#[test]
fn t1_f14_01() {
    let env = TestEnv::new();
    let expected_modules = [
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
    ];
    for args in [&[][..], &["--help"][..]] {
        let out = env.run_bt(args);
        out.assert_exit(0).assert_no_ansi();
        assert!(
            out.stdout.contains("Bilibili CLI Toolbox") || out.stdout.contains("╭────┴─────┴────╮"),
            "missing banner in stdout for {args:?}: {}",
            out.stdout
        );
        for m in expected_modules {
            assert!(
                out.stdout.contains(m),
                "missing module {m} in {args:?} stdout: {}",
                out.stdout
            );
        }
    }
}

#[test]
fn t1_f14_02() {
    let env = TestEnv::new();
    let top_level = [
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
    ];
    for cmd in top_level {
        env.run_bt(&[cmd, "--help"])
            .assert_exit(0)
            .assert_stdout_contains("Usage:");
    }

    let nested: &[&[&str]] = &[
        &["auth", "login"],
        &["auth", "status"],
        &["auth", "logout"],
        &["auth", "refresh"],
        &["auth", "daily"],
        &["live", "start"],
        &["live", "stop"],
        &["live", "status"],
        &["live", "info"],
        &["live", "title"],
        &["live", "area"],
        &["live", "following"],
        &["live", "stream"],
        &["live", "danmaku"],
        &["live", "chat"],
        &["live", "send"],
        &["live", "record"],
        &["search", "video"],
        &["search", "user"],
        &["search", "live"],
        &["search", "hot"],
        &["video", "info"],
        &["video", "streams"],
        &["video", "subtitle"],
        &["video", "summary"],
        &["video", "tags"],
        &["video", "related"],
        &["video", "danmaku"],
        &["video", "comments"],
        &["video", "comment"],
        &["video", "comment", "post"],
        &["video", "comment", "like"],
        &["video", "comment", "delete"],
        &["video", "like"],
        &["video", "coin"],
        &["video", "triple"],
        &["video", "download"],
        &["user", "info"],
        &["user", "videos"],
        &["user", "dynamics"],
        &["user", "follow"],
        &["user", "unfollow"],
        &["rank", "video"],
        &["rank", "popular"],
        &["fav", "list"],
        &["fav", "show"],
        &["fav", "export"],
        &["fav", "add"],
        &["fav", "remove"],
        &["fav", "clean"],
        &["fav", "create"],
        &["fav", "delete"],
        &["later", "list"],
        &["later", "add"],
        &["later", "remove"],
        &["later", "clear"],
        &["history", "list"],
        &["history", "delete"],
        &["history", "clear"],
    ];

    for path in nested {
        let mut args = path.to_vec();
        args.push("--help");
        env.run_bt(&args)
            .assert_exit(0)
            .assert_stdout_contains("Usage:");
    }
}

#[test]
fn t1_f14_03() {
    let env = TestEnv::new();
    let v = env.run_bt(&["--json"]).assert_json_ok();
    let actual: HashSet<&str> = v["modules"]
        .as_array()
        .expect("modules array")
        .iter()
        .filter_map(|m| m.as_str())
        .collect();
    let expected: HashSet<&str> = [
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
    ]
    .into_iter()
    .collect();
    assert_eq!(actual, expected);
}

#[test]
fn t1_f14_04() {
    let env = TestEnv::new().with_no_color(false);
    let out = env.run_bt_env(&["--no-color"], &[("TERM", "xterm-256color")]);
    out.assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("Bilibili CLI Toolbox")
        .assert_stdout_contains("auth");
}

#[test]
fn t1_f14_05() {
    let env = TestEnv::new();
    let readme_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let readme = std::fs::read_to_string(&readme_path).expect("read README.md");

    for flag in ["--json", "--quiet", "--yes", "--no-color", "--cookie-file"] {
        assert!(
            readme.contains(flag),
            "README.md missing documented global flag {flag}"
        );
    }

    let arch_section = readme
        .split("### 命令架构")
        .nth(1)
        .expect("README.md missing ### 命令架构 section");
    let commands_block = arch_section
        .split("Commands:")
        .nth(1)
        .and_then(|s| s.split("Options:").next())
        .expect("README.md missing Commands: .. Options: block");

    let mut readme_cmds: HashMap<String, Vec<String>> = HashMap::new();
    for line in commands_block.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("```") {
            continue;
        }
        let Some(cmd_name) = trimmed.split_whitespace().next() else {
            continue;
        };
        if !cmd_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            continue;
        }
        let mut subcmds = Vec::new();
        if let Some(open) = trimmed.rfind('(')
            && let Some(close) = trimmed[open + 1..].find(')')
        {
            let inner = &trimmed[open + 1..open + 1 + close];
            for item in inner.split(',') {
                let s = item.trim();
                if !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    subcmds.push(s.to_string());
                }
            }
        }
        readme_cmds.insert(cmd_name.to_string(), subcmds);
    }
    assert!(
        !readme_cmds.is_empty(),
        "parsed zero commands from README.md Commands: block"
    );

    let meta = env.run_bt(&["--json"]).assert_json_ok();
    let cli_modules: HashSet<String> = meta["modules"]
        .as_array()
        .expect("modules array")
        .iter()
        .filter_map(|m| m.as_str().map(str::to_string))
        .collect();
    let doc_modules: HashSet<String> = readme_cmds.keys().cloned().collect();
    assert_eq!(
        doc_modules, cli_modules,
        "README.md Commands: top-level modules differ from bt --json modules"
    );

    let mut has_subcommands: HashMap<String, bool> = HashMap::new();
    for (cmd, subs) in &readme_cmds {
        let help = env.run_bt(&[cmd, "--help"]);
        help.assert_exit(0).assert_stdout_contains("Usage:");
        let cmd_has_subs = help.stdout.contains("Commands:");
        has_subcommands.insert(cmd.clone(), cmd_has_subs);
        if cmd_has_subs {
            for sub in subs {
                assert!(
                    help.stdout.contains(sub),
                    "bt {cmd} --help missing documented subcommand {sub}"
                );
                env.run_bt(&[cmd, sub, "--help"])
                    .assert_exit(0)
                    .assert_stdout_contains("Usage:");
            }
        }
    }

    let quick_section = readme
        .split("## 快速使用")
        .nth(1)
        .and_then(|s| s.split("## ").next())
        .expect("README.md missing ## 快速使用 section");
    let mut in_bash = false;
    for line in quick_section.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```bash") {
            in_bash = true;
            continue;
        }
        if in_bash && trimmed.starts_with("```") {
            in_bash = false;
            continue;
        }
        if !in_bash {
            continue;
        }
        let code = trimmed.split('#').next().unwrap_or("").trim();
        let Some(rest) = code.strip_prefix("bt ") else {
            continue;
        };
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let mut idx = 0;
        while idx < tokens.len() {
            let tok = tokens[idx];
            if matches!(
                tok,
                "--json" | "-q" | "--quiet" | "-y" | "--yes" | "--no-color"
            ) {
                idx += 1;
            } else if matches!(tok, "-u" | "--cookie-file") {
                idx += 2;
            } else if tok.starts_with("--cookie-file=") {
                idx += 1;
            } else {
                break;
            }
        }
        let Some(&cmd) = tokens.get(idx) else {
            continue;
        };
        if cmd.starts_with('-') {
            continue;
        }
        env.run_bt(&[cmd, "--help"])
            .assert_exit(0)
            .assert_stdout_contains("Usage:");
        if has_subcommands.get(cmd).copied().unwrap_or(false)
            && let Some(&sub) = tokens.get(idx + 1)
            && !sub.starts_with('<')
            && !sub.starts_with('-')
            && !sub.starts_with('"')
        {
            env.run_bt(&[cmd, sub, "--help"])
                .assert_exit(0)
                .assert_stdout_contains("Usage:");
        }
    }
}
