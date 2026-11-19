mod common;

use common::{CookieFixture, DoSignMode, FaultConfig, MockBiliServer, QrMockStatus, TestEnv};
use serde_json::json;
use std::net::TcpListener;
use std::time::{Duration, Instant};

// ============================================================================
// Feature 01 — Global CLI & Output Contract (6 Tests)
// ============================================================================

#[test]
fn t2_f01_01() {
    let env = TestEnv::new();

    let human = env.run_bt(&["invalidcmd"]);
    human.assert_exit(2).assert_stdout_empty();
    assert!(!human.stderr.trim().is_empty());
    assert!(human.stderr.contains("invalidcmd") || human.stderr.contains("Usage"));

    let json_out = env.run_bt(&["--json", "invalidcmd"]);
    json_out.assert_json_err(Some("Input"));
    assert!(json_out.stderr.trim().is_empty());
}

#[test]
fn t2_f01_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let missing = env.alt_cookie_path("missing.json");
    let missing_str = missing.to_string_lossy().into_owned();

    let out = env.run_bt(&[
        "--json",
        "-u",
        &missing_str,
        "video",
        "like",
        "BV1uJ411r7hL",
    ]);
    out.assert_json_err(Some("Auth"));
    assert!(out.stderr.trim().is_empty());
    assert!(server.requests().is_empty());
    assert!(!missing.exists());
}

#[test]
fn t2_f01_03() {
    let env = TestEnv::new();

    let human = env.run_bt(&["auth", "login", "--method", "invalid"]);
    human.assert_exit(2).assert_stdout_empty();
    assert!(!human.stderr.trim().is_empty());

    let json_out = env.run_bt(&["--json", "auth", "login", "--method", "invalid"]);
    json_out.assert_json_err(Some("Input"));
    assert!(json_out.stderr.trim().is_empty());
}

#[test]
fn t2_f01_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["-q", "--json", "search", "hot"]);
    let v = out.assert_json_ok();
    assert!(v.is_array() && !v.as_array().unwrap().is_empty());
    assert!(out.stderr.trim().is_empty());
    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path == "/x/web-interface/wbi/search/square")
    );
    assert!(reqs.iter().all(|r| r.path != "/main/hotword"));
}

#[test]
fn t2_f01_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let missing_nested = env.temp_path().join("nonexistent_dir/sub/cookies.json");
    let missing_nested_str = missing_nested.to_string_lossy().into_owned();
    let out_a = env.run_bt(&["--json", "-u", &missing_nested_str, "auth", "status"]);
    let v_a = out_a.assert_json_ok();
    assert_eq!(v_a["is_login"], false);
    assert!(!missing_nested.exists());
    assert!(!missing_nested.parent().unwrap().exists());

    let blocker = env.write_work_file("blocker_file", b"x");
    let bad_cookie_path = blocker.join("bad/c.json");
    let bad_cookie_str = bad_cookie_path.to_string_lossy().into_owned();
    let out_b = env.run_bt(&[
        "--json",
        "-u",
        &bad_cookie_str,
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
    out_b.assert_json_err(Some("Io"));
    assert!(out_b.stderr.trim().is_empty());
}

#[test]
fn t2_f01_06() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_valid_cookies()
        .with_no_color(false);

    let out_a = env.run_bt_env(&["--no-color", "--json"], &[("TERM", "xterm-256color")]);
    out_a.assert_json_ok();
    out_a.assert_no_ansi();

    let out_b = env.run_bt_env(
        &["live", "status"],
        &[("NO_COLOR", "1"), ("TERM", "xterm-256color")],
    );
    out_b
        .assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("31686846");
}

// ============================================================================
// Feature 02 — Universal ID / URL / b23.tv Resolver (6 Tests)
// ============================================================================

#[test]
fn t2_f02_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&[
        "--json",
        "resolve",
        "https://b23.tv/pigt3PQ",
        "--no-network",
    ]);
    let v = out.assert_json_ok();
    assert_eq!(v["kind"], "short_link");
    assert_eq!(v["id"], "pigt3PQ");
    assert_eq!(v["network_used"], false);
    assert!(server.requests().is_empty());
}

#[test]
fn t2_f02_02() {
    let env = TestEnv::new();

    let out_a = env.run_bt(&["--json", "resolve", "not_a_valid_bilibili_id_!!!"]);
    out_a.assert_json_err(Some("Input"));
    assert!(out_a.stderr.trim().is_empty());

    let out_b = env.run_bt(&[
        "--json",
        "resolve",
        "https://example.com/video/BV1uJ411r7hL",
    ]);
    out_b.assert_json_err(Some("Input"));
    assert!(out_b.stderr.trim().is_empty());
}

#[test]
fn t2_f02_03() {
    let env = TestEnv::new();

    let out_a = env.run_bt(&["--json", "resolve", "bv1uJ411r7hL"]);
    let v_a = out_a.assert_json_ok();
    assert_eq!(v_a["kind"], "video");
    assert_eq!(v_a["id"], "BV1uJ411r7hL");
    assert_eq!(v_a["bvid"], "BV1uJ411r7hL");

    let out_b = env.run_bt(&["--json", "resolve", "AV170001"]);
    let v_b = out_b.assert_json_ok();
    assert_eq!(v_b["kind"], "video");
    assert_eq!(v_b["id"], "av170001");
    assert_eq!(v_b["aid"], 170001);
}

#[test]
fn t2_f02_04() {
    let env = TestEnv::new();

    let out_a = env.run_bt(&[
        "--json",
        "resolve",
        "https://www.bilibili.com/bangumi/play/ss6789?from=search",
    ]);
    let v_a = out_a.assert_json_ok();
    assert_eq!(v_a["kind"], "bangumi_season");
    assert_eq!(v_a["season_id"], 6789);
    assert_eq!(
        v_a["resolved_url"],
        "https://www.bilibili.com/bangumi/play/ss6789"
    );

    let out_b = env.run_bt(&["--json", "resolve", "md4321"]);
    let v_b = out_b.assert_json_ok();
    assert_eq!(v_b["kind"], "bangumi_media");
    assert_eq!(v_b["media_id"], 4321);
}

#[test]
fn t2_f02_05() {
    let server = MockBiliServer::start();
    server.set_fault("/noloc", FaultConfig::missing_location());
    let env = TestEnv::new().with_server(&server);

    let out_a = env.run_bt(&["--json", "resolve", "https://b23.tv/loop"]);
    out_a.assert_json_err(Some("Api"));
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.path == "/loop")
            .count(),
        3
    );

    let out_b = env.run_bt(&["--json", "resolve", "https://b23.tv/missing404"]);
    out_b.assert_json_err(Some("Api"));

    let out_c = env.run_bt(&["--json", "resolve", "https://b23.tv/noloc"]);
    out_c.assert_json_err(Some("Parse"));
}

#[test]
fn t2_f02_06() {
    let env = TestEnv::new();

    let out_a = env.run_bt(&["--json", "resolve", ""]);
    out_a.assert_json_err(Some("Input"));
    assert!(out_a.stderr.trim().is_empty());

    let out_b = env.run_bt(&["--json", "resolve", "   "]);
    out_b.assert_json_err(Some("Input"));
    assert!(out_b.stderr.trim().is_empty());
}

// ============================================================================
// Feature 03 — External Binary (ffmpeg) Guard (5 Tests)
// ============================================================================

#[test]
fn t2_f03_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).without_ffmpeg();
    let out_mp4 = env.work_dir().join("missing_ffmpeg.mp4");
    let out_mp4_str = out_mp4.to_string_lossy().into_owned();

    let out = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "-o",
        &out_mp4_str,
    ]);
    let v = out.assert_json_err(Some("ExternalTool"));
    assert!(v["error"]["message"].as_str().unwrap().contains("ffmpeg"));
    assert!(out.stderr.trim().is_empty());
    assert!(!out_mp4.exists());
}

#[test]
fn t2_f03_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).without_ffmpeg();
    let rec_flv = env.work_dir().join("live_missing_ffmpeg.flv");
    let rec_flv_str = rec_flv.to_string_lossy().into_owned();

    let started = Instant::now();
    let out = env.run_bt(&["--json", "live", "record", "14073662", "-o", &rec_flv_str]);
    let v = out.assert_json_err(Some("ExternalTool"));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(v["error"]["message"].as_str().unwrap().contains("ffmpeg"));
    assert!(out.stderr.trim().is_empty());
    assert!(!rec_flv.exists());
    assert!(
        server
            .requests()
            .iter()
            .all(|r| !r.path.starts_with("/mock/live/"))
    );
}

#[test]
fn t2_f03_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_failing_ffmpeg_exec();
    let out_mp4 = env.work_dir().join("failing_exec.mp4");
    let out_mp4_str = out_mp4.to_string_lossy().into_owned();

    let out = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "-o",
        &out_mp4_str,
    ]);
    let v = out.assert_json_err(None);
    assert!(matches!(
        v["error"]["kind"].as_str(),
        Some("ExternalTool" | "Io")
    ));
    assert!(!env.read_ffmpeg_log().is_empty());
    assert!(out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("panicked at"));
}

#[test]
fn t2_f03_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();

    let blocker = env.write_work_file("blocker_file", b"x");
    let bad_out = blocker.join("sub/out.mp4");
    let bad_out_str = bad_out.to_string_lossy().into_owned();
    let out = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "-o",
        &bad_out_str,
    ]);
    let v = out.assert_json_err(None);
    assert!(matches!(
        v["error"]["kind"].as_str(),
        Some("Io" | "Input" | "ExternalTool")
    ));
    assert!(
        !v["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("unrecognized subcommand")
    );
    assert!(out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("panicked at"));
}

#[test]
fn t2_f03_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).without_ffmpeg();
    let out_mp4 = env.work_dir().join("human_missing_ffmpeg.mp4");
    let out_mp4_str = out_mp4.to_string_lossy().into_owned();

    let out = env.run_bt(&["video", "download", "BV1uJ411r7hL", "-o", &out_mp4_str]);
    out.assert_exit(1)
        .assert_stdout_empty()
        .assert_stderr_contains("ffmpeg");
    assert!(!out.stderr.contains("panicked at"));
    assert!(!out_mp4.exists());
}

// ============================================================================
// Feature 04 — API Base URL Testability Hook (5 Tests)
// ============================================================================

#[test]
fn t2_f04_01() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let closed_url = format!("http://127.0.0.1:{port}");
    let env = TestEnv::new();

    let start_a = Instant::now();
    let out_a = env.run_bt_env(
        &["--json", "search", "hot"],
        &[
            ("BT_API_BASE_URL", &closed_url),
            ("BT_SEARCH_BASE_URL", &closed_url),
        ],
    );
    out_a.assert_json_err(Some("Network"));
    assert!(out_a.stderr.trim().is_empty());
    assert!(start_a.elapsed() < Duration::from_secs(5));

    let start_b = Instant::now();
    let out_b = env.run_bt_env(
        &["--json", "video", "info", "BV1uJ411r7hL"],
        &[("BT_API_BASE_URL", &closed_url)],
    );
    out_b.assert_json_err(Some("Network"));
    assert!(out_b.stderr.trim().is_empty());
    assert!(start_b.elapsed() < Duration::from_secs(5));
}

#[test]
fn t2_f04_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let trailing = format!("{}/", server.base_url());

    let out = env.run_bt_env(
        &["--json", "search", "hot"],
        &[
            ("BT_API_BASE_URL", &trailing),
            ("BT_SEARCH_BASE_URL", &trailing),
        ],
    );
    let v = out.assert_json_ok();
    assert!(v.is_array());
    let reqs = server.requests();
    assert!(
        reqs.iter()
            .any(|r| r.path == "/x/web-interface/wbi/search/square")
    );
    assert!(reqs.iter().all(|r| r.path != "/main/hotword"));
    assert!(reqs.iter().all(|r| !r.path.starts_with("//")));
}

#[test]
fn t2_f04_03() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/nav",
        FaultConfig::HttpStatus {
            status: 500,
            content_type: "text/html; charset=utf-8".to_string(),
            body: b"<html>500 Internal Server Error</html>".to_vec(),
            headers: vec![],
        },
    );
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out = env.run_bt(&["--json", "auth", "status"]);
    out.assert_json_err(Some("Json"));
    assert!(out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("panicked at"));
}

#[test]
fn t2_f04_04() {
    let server = MockBiliServer::start();
    server.set_fault("/room/v1/Area/getList", FaultConfig::EmptyBody);
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["--json", "live", "area"]);
    out.assert_json_err(Some("Json"));
    assert!(out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("panicked at"));
}

#[test]
fn t2_f04_05() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/wbi/view",
        FaultConfig::api_code(-352, "风控校验失败"),
    );
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["--json", "video", "info", "BV1uJ411r7hL"]);
    let v = out.assert_json_err(Some("Api"));
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("风控校验失败")
    );
    assert!(out.stderr.trim().is_empty());
}

// ============================================================================
// Feature 05 — Auth Hardening & auth daily (6 Tests)
// ============================================================================

#[test]
fn t2_f05_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_cookie_fixture(CookieFixture::without_csrf());

    let out = env.run_bt(&["--json", "video", "like", "BV1uJ411r7hL"]);
    out.assert_json_err(Some("Auth"));
    assert!(out.stderr.trim().is_empty());
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.path != "/x/web-interface/archive/like")
    );
}

#[test]
fn t2_f05_02() {
    let server = MockBiliServer::start();
    server.state().lock().unwrap().web_qr_status = QrMockStatus::Expired;
    let env = TestEnv::new().with_server(&server);

    let out_a = env.run_bt(&["--json", "auth", "login", "--method", "qr"]);
    out_a.assert_json_err(Some("Input"));
    assert!(server.requests().is_empty());
    assert!(
        !env.default_cookie_path()
            .parent()
            .unwrap()
            .join("qrcode.png")
            .exists()
    );

    let out_b = env.run_bt(&["auth", "login", "--method", "qr"]);
    out_b.assert_exit(1).assert_stderr_contains("过期");
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/passport-login/web/qrcode/poll")
    );
    assert!(
        !env.default_cookie_path()
            .parent()
            .unwrap()
            .join("qrcode.png")
            .exists()
    );
    assert!(!env.work_dir().join("qrcode.png").exists());
}

#[test]
fn t2_f05_03() {
    let env = TestEnv::new();

    let out = env.run_bt(&["--json", "auth", "login", "--method", "cookie"]);
    out.assert_json_err(Some("Input"));
    assert!(out.stderr.trim().is_empty());
    assert!(!env.default_cookie_path().exists());
}

#[test]
fn t2_f05_04() {
    let server_a = MockBiliServer::start();
    let env_a = TestEnv::new().with_server(&server_a);
    let out_a = env_a.run_bt(&["--json", "auth", "status"]);
    let v_a = out_a.assert_json_ok();
    assert_eq!(v_a["is_login"], false);
    assert!(server_a.requests().is_empty());

    let server_b = MockBiliServer::start();
    server_b.state().lock().unwrap().auth_logged_in = false;
    let env_b = TestEnv::new().with_server(&server_b).with_valid_cookies();
    let out_b = env_b.run_bt(&["--json", "auth", "status"]);
    let v_b = out_b.assert_json_ok();
    assert_eq!(v_b["is_login"], false);
    assert!(
        server_b
            .requests()
            .iter()
            .any(|r| r.path == "/x/web-interface/nav")
    );

    server_b.set_fault(
        "/x/web-interface/nav",
        FaultConfig::api_code(-500, "服务异常"),
    );
    let out_c = env_b.run_bt(&["--json", "auth", "status"]);
    out_c.assert_json_err(Some("Api"));
}

#[test]
fn t2_f05_05() {
    let server = MockBiliServer::start();
    server.state().lock().unwrap().need_refresh = true;
    let env = TestEnv::new()
        .with_server(&server)
        .with_cookie_fixture(CookieFixture::without_refresh_token());

    let out = env.run_bt(&["--json", "auth", "refresh"]);
    let v = out.assert_json_err(Some("Api"));
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("refresh_token")
    );
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.path != "/x/passport-login/web/cookie/refresh")
    );
}

#[test]
fn t2_f05_06() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    server.state().lock().unwrap().dosign_mode = DoSignMode::AlreadySigned;
    let out_a = env.run_bt(&["--json", "auth", "daily"]);
    let v_a = out_a.assert_json_ok();
    assert_eq!(v_a["live_sign"]["already_signed"], true);
    assert_eq!(v_a["live_sign"]["signed"], false);

    server.state().lock().unwrap().dosign_mode = DoSignMode::Retired;
    let out_b = env.run_bt(&["--json", "auth", "daily"]);
    let v_b = out_b.assert_json_ok();
    assert_eq!(v_b["live_sign"]["offline"], true);
    assert_eq!(v_b["live_sign"]["signed"], false);
}

// ============================================================================
// Feature 06 — live Refactor & Extensions (6 Tests)
// ============================================================================

#[test]
fn t2_f06_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    for delay in ["invalid_delay", "0s", "30"] {
        let started = Instant::now();
        let out = env.run_bt(&["--json", "live", "stop", "-d", delay]);
        let v = out.assert_json_err(None);
        assert!(matches!(
            v["error"]["kind"].as_str(),
            Some("Parse" | "Input")
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            server
                .requests()
                .iter()
                .all(|r| r.path != "/room/v1/Room/stopLive")
        );
    }
}

#[test]
fn t2_f06_02() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let started = Instant::now();
    let out = env.run_bt(&["--json", "live", "stop", "-d", "5m"]);
    assert!(
        out.code == 0 || out.code == 1,
        "unexpected exit code {}: stdout={}, stderr={}",
        out.code,
        out.stdout,
        out.stderr
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "must pre-validate live_status before sleeping 5m"
    );
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.path != "/room/v1/Room/stopLive")
    );
    if out.code == 0 {
        let v = out.assert_json_ok();
        assert_eq!(v["stopped"], false);
    } else {
        out.assert_json_err(None);
    }
}

#[test]
fn t2_f06_03() {
    let server = MockBiliServer::start();
    server.state().lock().unwrap().own_room_id = 0;
    let env = TestEnv::new()
        .with_server(&server)
        .with_cookie_fixture(CookieFixture::without_room_id());

    let out = env.run_bt(&["--json", "live", "start", "-a", "86", "-y"]);
    out.assert_json_err(Some("Api"));
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.path != "/room/v1/Room/startLive")
    );
}

#[test]
fn t2_f06_04() {
    let env = TestEnv::new().with_valid_cookies();

    let out_a = env.run_bt(&["live", "following", "--page-size", "0"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());

    let out_b = env.run_bt(&["live", "following", "--page-size", "21"]);
    out_b.assert_exit(2).assert_stdout_empty();
    assert!(!out_b.stderr.trim().is_empty());

    let out_c = env.run_bt(&["--json", "live", "following", "--page-size", "21"]);
    out_c.assert_json_err(Some("Input"));
}

#[test]
fn t2_f06_05() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/xlive/web-ucenter/user/following",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "data": { "count": 0, "list": null }
        })),
    );
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out_a = env.run_bt(&["live", "danmaku", "14073662", "--limit", "51"]);
    out_a.assert_exit(2).assert_stdout_empty();

    let out_b = env.run_bt(&["--json", "live", "following"]);
    out_b.assert_json_ok();
    assert!(!out_b.stderr.contains("panicked at"));

    let out_c = env.run_bt(&["live", "following"]);
    out_c.assert_exit(0);
    assert!(!out_c.stderr.contains("panicked at"));
}

#[test]
fn t2_f06_06() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out = env.run_bt(&["--json", "live", "send", "14073662", ""]);
    let v = out.assert_json_err(None);
    assert!(matches!(v["error"]["kind"].as_str(), Some("Input" | "Api")));
    assert!(server.requests().iter().all(|r| r.path != "/msg/send"));
}

// ============================================================================
// Feature 07 — search Refactor (5 Tests)
// ============================================================================

#[test]
fn t2_f07_01() {
    let env = TestEnv::new();

    let out = env.run_bt(&["search", "video", "Rust", "--order", "bogus_order"]);
    out.assert_exit(2).assert_stdout_empty();
    assert!(!out.stderr.trim().is_empty());
}

#[test]
fn t2_f07_02() {
    let env = TestEnv::new();

    let out = env.run_bt(&["search", "user", "UP", "--order", "bogus"]);
    out.assert_exit(2).assert_stdout_empty();
    assert!(!out.stderr.trim().is_empty());
}

#[test]
fn t2_f07_03() {
    let env = TestEnv::new();

    let out_a = env.run_bt(&["search", "hot", "--limit", "0"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());

    let out_b = env.run_bt(&["search", "hot", "--limit", "51"]);
    out_b.assert_exit(2).assert_stdout_empty();
    assert!(!out_b.stderr.trim().is_empty());
}

#[test]
fn t2_f07_04() {
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
                "numResults": 0,
                "numPages": 0,
                "result": null
            }
        })),
    );
    let env = TestEnv::new().with_server(&server);

    let out_a = env.run_bt(&["--json", "search", "video", "nonexistent_xyz"]);
    let v_a = out_a.assert_json_ok();
    assert!(v_a["result"].is_null());

    let out_b = env.run_bt(&["search", "video", "nonexistent_xyz"]);
    out_b
        .assert_exit(0)
        .assert_stderr_contains("未找到相关视频");
    assert!(!out_b.stderr.contains("panicked at"));
}

#[test]
fn t2_f07_05() {
    let env = TestEnv::new();

    let out = env.run_bt(&["search", "video", "Rust", "--page", "0"]);
    out.assert_exit(2).assert_stdout_empty();
    assert!(!out.stderr.trim().is_empty());
}

// ============================================================================
// Feature 08 — fav Refactor & CRUD Symmetry (5 Tests)
// ============================================================================

#[test]
fn t2_f08_01() {
    let env = TestEnv::new();

    let out = env.run_bt(&["fav", "export", "1052622027", "--format", "yaml"]);
    out.assert_exit(2).assert_stdout_empty();
    assert!(!out.stderr.trim().is_empty());
}

#[test]
fn t2_f08_02() {
    let env = TestEnv::new();

    let out_a = env.run_bt(&["fav", "show", "1052622027", "--page-size", "0"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());

    let out_b = env.run_bt(&["fav", "show", "1052622027", "--page-size", "21"]);
    out_b.assert_exit(2).assert_stdout_empty();
    assert!(!out_b.stderr.trim().is_empty());
}

#[test]
fn t2_f08_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["--json", "fav", "list"]);
    let v = out.assert_json_err(None);
    assert!(matches!(
        v["error"]["kind"].as_str(),
        Some("Input" | "Auth")
    ));
    assert!(server.requests().is_empty());
}

#[test]
fn t2_f08_04() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/v3/fav/resource/list",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "info": {
                    "id": 1052622027,
                    "fid": 10526220,
                    "mid": 293793435,
                    "attr": 0,
                    "title": "空收藏夹",
                    "media_count": 0
                },
                "medias": null,
                "has_more": false
            }
        })),
    );
    server.set_fault(
        "/x/v3/fav/folder/created/list-all",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": { "count": 0, "list": null }
        })),
    );
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out_show_json = env.run_bt(&["--json", "fav", "show", "1052622027"]);
    out_show_json.assert_json_ok();
    assert!(!out_show_json.stderr.contains("panicked at"));

    let out_show_human = env.run_bt(&["fav", "show", "1052622027"]);
    out_show_human.assert_exit(0);
    assert!(!out_show_human.stderr.contains("panicked at"));
    assert!(!out_show_human.stderr.contains("无法解析"));

    let out_list_json = env.run_bt(&["--json", "fav", "list", "--mid", "293793435"]);
    out_list_json.assert_json_ok();
    assert!(!out_list_json.stderr.contains("panicked at"));

    let out_list_human = env.run_bt(&["fav", "list", "--mid", "293793435"]);
    out_list_human.assert_exit(0);
    assert!(!out_list_human.stderr.contains("panicked at"));
    assert!(!out_list_human.stderr.contains("无法解析"));
}

#[test]
fn t2_f08_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out_a = env.run_bt(&["--json", "fav", "clean", "1052622027"]);
    out_a.assert_json_err(Some("Input"));

    let out_b = env.run_bt(&["--json", "fav", "delete", "1052622027"]);
    out_b.assert_json_err(Some("Input"));

    assert!(
        server
            .requests()
            .iter()
            .all(|r| { r.path != "/x/v3/fav/resource/clean" && r.path != "/x/v3/fav/folder/del" })
    );
}

// ============================================================================
// Feature 09 — later & history Refactor & CRUD (5 Tests)
// ============================================================================

#[test]
fn t2_f09_01() {
    let env = TestEnv::new().with_valid_cookies();

    let out_a = env.run_bt(&["history", "list", "--limit", "0"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());

    let out_b = env.run_bt(&["history", "list", "--limit", "31"]);
    out_b.assert_exit(2).assert_stdout_empty();
    assert!(!out_b.stderr.trim().is_empty());
}

#[test]
fn t2_f09_02() {
    let env = TestEnv::new().with_valid_cookies();

    let out_a = env.run_bt(&["history", "list", "-c", "bogus"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());

    let out_b = env.run_bt(&["history", "list", "-t", "archive"]);
    out_b.assert_exit(2).assert_stdout_empty();
    assert!(!out_b.stderr.trim().is_empty());
}

#[test]
fn t2_f09_03() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/history/cursor",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "cursor": { "max": 0, "view_at": 0, "business": "", "ps": 20 },
                "list": null
            }
        })),
    );
    server.set_fault(
        "/x/v2/history/toview",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": { "count": 0, "list": null }
        })),
    );
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out_a = env.run_bt(&["--json", "history", "list"]);
    out_a.assert_json_ok();
    assert!(!out_a.stderr.contains("panicked at"));

    let out_b = env.run_bt(&["history", "list"]);
    out_b.assert_exit(0);
    assert!(!out_b.stderr.contains("无法解析"));
    assert!(!out_b.stderr.contains("panicked at"));

    let out_c = env.run_bt(&["--json", "later", "list"]);
    out_c.assert_json_ok();
    assert!(!out_c.stderr.contains("panicked at"));

    let out_d = env.run_bt(&["later", "list"]);
    out_d.assert_exit(0);
    assert!(!out_d.stderr.contains("无法解析"));
    assert!(!out_d.stderr.contains("panicked at"));
}

#[test]
fn t2_f09_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out_a = env.run_bt(&["--json", "later", "clear"]);
    out_a.assert_json_err(Some("Input"));

    let out_b = env.run_bt(&["--json", "history", "clear"]);
    out_b.assert_json_err(Some("Input"));

    assert!(
        server
            .requests()
            .iter()
            .all(|r| { r.path != "/x/v2/history/toview/clear" && r.path != "/x/v2/history/clear" })
    );
}

#[test]
fn t2_f09_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out_a = env.run_bt(&["--json", "later", "add", "invalid_id"]);
    out_a.assert_json_err(Some("Input"));

    let out_b = env.run_bt(&["--json", "history", "delete", "invalid_id"]);
    out_b.assert_json_err(Some("Input"));

    assert!(server.requests().is_empty());
}

// ============================================================================
// Feature 10 — video Inspection & Interactions (5 Tests)
// ============================================================================

#[test]
fn t2_f10_01() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out_a = env.run_bt(&["video", "coin", "BV1uJ411r7hL", "-n", "0"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());

    let out_b = env.run_bt(&["video", "coin", "BV1uJ411r7hL", "-n", "3"]);
    out_b.assert_exit(2).assert_stdout_empty();
    assert!(!out_b.stderr.trim().is_empty());

    assert!(server.requests().is_empty());
}

#[test]
fn t2_f10_02() {
    let server = MockBiliServer::start();
    let empty_sub = FaultConfig::json(json!({
        "code": 0,
        "message": "0",
        "ttl": 1,
        "data": {
            "aid": 79677524,
            "bvid": "BV1uJ411r7hL",
            "cid": 146044693,
            "subtitle": { "allow_submit": false, "subtitles": [] }
        }
    }));
    server.set_fault("/x/player/wbi/v2", empty_sub.clone());
    server.set_fault("/x/player/v2", empty_sub);
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&[
        "--json",
        "video",
        "subtitle",
        "BV1uJ411r7hL",
        "--lang",
        "ja-JP",
    ]);
    assert!(out.code == 0 || out.code == 1);
    assert!(out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("panicked at"));
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/player/wbi/v2" || r.path == "/x/player/v2")
    );
    if out.code == 1 {
        out.assert_json_err(None);
    } else {
        out.assert_json_ok();
    }
}

#[test]
fn t2_f10_03() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/view/conclusion/get",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": { "code": -1, "stid": "", "status": 0, "model_result": null }
        })),
    );
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["--json", "video", "summary", "BV1uJ411r7hL"]);
    assert!(out.code == 0 || out.code == 1);
    assert!(out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("panicked at"));
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/web-interface/view/conclusion/get")
    );
    if out.code == 1 {
        out.assert_json_err(None);
    } else {
        out.assert_json_ok();
    }
}

#[test]
fn t2_f10_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["--json", "video", "streams", "BV1uJ411r7hL", "-p", "99"]);
    let v = out.assert_json_err(None);
    assert!(matches!(
        v["error"]["kind"].as_str(),
        Some("Input" | "Parse")
    ));
    let reqs = server.requests();
    assert!(reqs.iter().any(|r| r.path == "/x/web-interface/wbi/view"));
    assert!(reqs.iter().all(|r| !r.path.contains("playurl")));
}

#[test]
fn t2_f10_05() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/coin/add",
        FaultConfig::api_code(34005, "超过投币上限"),
    );
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out = env.run_bt(&["--json", "video", "coin", "BV1uJ411r7hL", "-n", "1"]);
    let v = out.assert_json_err(Some("Api"));
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("超过投币上限")
    );
    assert!(out.stderr.trim().is_empty());
}

// ============================================================================
// Feature 11 — video Danmaku & Comments (5 Tests)
// ============================================================================

#[test]
fn t2_f11_01() {
    let env = TestEnv::new();

    let out = env.run_bt(&["video", "danmaku", "BV1uJ411r7hL", "--format", "invalid"]);
    out.assert_exit(2).assert_stdout_empty();
    assert!(!out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("unrecognized subcommand"));
    assert!(out.stderr.contains("--format") || out.stderr.contains("invalid"));
}

#[test]
fn t2_f11_02() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/v2/reply*",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "page": { "num": 1, "size": 20, "count": 0, "acount": 0 },
                "cursor": { "is_begin": true, "is_end": true, "all_count": 0 },
                "replies": null,
                "hots": null
            }
        })),
    );
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["--json", "video", "comments", "BV1uJ411r7hL"]);
    out.assert_json_ok();
    assert!(!out.stderr.contains("panicked at"));
    assert!(out.stderr.trim().is_empty());
}

#[test]
fn t2_f11_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out = env.run_bt(&["--json", "video", "comment", "post", "BV1uJ411r7hL", ""]);
    let v = out.assert_json_err(Some("Input"));
    assert!(
        !v["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("unrecognized subcommand")
    );
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.path != "/x/v2/reply/add")
    );
}

#[test]
fn t2_f11_04() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/v2/reply/del",
        FaultConfig::api_code(12022, "已经被删除了"),
    );
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let out = env.run_bt(&[
        "--json",
        "video",
        "comment",
        "delete",
        "BV1uJ411r7hL",
        "104192624480",
    ]);
    let v = out.assert_json_err(Some("Api"));
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("已经被删除了")
    );
    assert!(out.stderr.trim().is_empty());
}

#[test]
fn t2_f11_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let blocker = env.write_work_file("blocker_file", b"x");
    let bad_dm = blocker.join("sub/dm.ass");
    let bad_dm_str = bad_dm.to_string_lossy().into_owned();
    let out = env.run_bt(&[
        "--json",
        "video",
        "danmaku",
        "BV1uJ411r7hL",
        "-o",
        &bad_dm_str,
    ]);
    out.assert_json_err(Some("Io"));
    assert!(out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("panicked at"));
}

// ============================================================================
// Feature 12 — video download (ffmpeg Muxing) (5 Tests)
// ============================================================================

#[test]
fn t2_f12_01() {
    let env = TestEnv::new();

    let out = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "--audio-only",
        "--video-only",
    ]);
    out.assert_exit(2).assert_stdout_empty();
    assert!(!out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("unrecognized subcommand"));
    assert!(out.stderr.contains("--audio-only") || out.stderr.contains("--video-only"));
}

#[test]
fn t2_f12_02() {
    let env = TestEnv::new();

    let out = env.run_bt(&["video", "download", "BV1uJ411r7hL", "-e", "vp9"]);
    out.assert_exit(2).assert_stdout_empty();
    assert!(!out.stderr.trim().is_empty());
    assert!(!out.stderr.contains("unrecognized subcommand"));
    assert!(
        out.stderr.contains("-e") || out.stderr.contains("--codec") || out.stderr.contains("vp9")
    );
}

#[test]
fn t2_f12_03() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();

    for spec in ["5-1", "abc", "99"] {
        let out = env.run_bt(&["--json", "video", "download", "BV1uJ411r7hL", "-p", spec]);
        let v = out.assert_json_err(None);
        assert!(matches!(
            v["error"]["kind"].as_str(),
            Some("Input" | "Parse" | "ParseInt")
        ));
        assert!(
            !v["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("unrecognized subcommand")
        );
        assert!(
            env.read_ffmpeg_log()
                .iter()
                .all(|line| line.trim() == "-version"),
            "unexpected ffmpeg mux invocation: {:?}",
            env.read_ffmpeg_log()
        );
        assert!(
            server
                .requests()
                .iter()
                .all(|r| !r.path.starts_with("/mock/media/"))
        );
        if spec == "99" {
            assert!(
                server
                    .requests()
                    .iter()
                    .any(|r| r.path == "/x/web-interface/wbi/view")
            );
        }
    }
}

#[test]
fn t2_f12_04() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/wbi/view",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": {
                "bvid": "BV1uJ411r7hL",
                "aid": 79677524,
                "videos": 1,
                "title": "A/B:C*?<>|\"test",
                "duration": 120,
                "owner": { "mid": 293793435, "name": "UP" },
                "stat": {
                    "view": 1,
                    "danmaku": 1,
                    "reply": 1,
                    "favorite": 1,
                    "coin": 1,
                    "share": 1,
                    "like": 1
                },
                "cid": 146044693,
                "pages": [
                    { "cid": 146044693, "page": 1, "from": "vupload", "part": "P1", "duration": 120 }
                ]
            }
        })),
    );
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let tpl = format!("{}/{{title}}.mp4", env.work_dir().display());

    let out = env.run_bt(&["--json", "video", "download", "BV1uJ411r7hL", "-o", &tpl]);
    out.assert_json_ok();
    assert!(!env.work_dir().join("A").exists());

    let has_mp4 = std::fs::read_dir(env.work_dir())
        .expect("read work dir")
        .filter_map(|e| e.ok())
        .any(|e| {
            e.path().is_file()
                && e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
        });
    assert!(has_mp4, "expected sanitized .mp4 output in work dir");
}

#[test]
fn t2_f12_05() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let out_mp4 = env.work_dir().join("fallback_quality.mp4");
    let out_mp4_str = out_mp4.to_string_lossy().into_owned();

    let out = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "--quality",
        "120",
        "-o",
        &out_mp4_str,
    ]);
    out.assert_json_ok();
    assert!(out_mp4.exists());
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path.contains("/mock/media/video_80_"))
    );
    assert!(!env.read_ffmpeg_log().is_empty());
}

// ============================================================================
// Feature 13 — user, feed & rank Modules (5 Tests)
// ============================================================================

#[test]
fn t2_f13_01() {
    let env = TestEnv::new();

    let out_a = env.run_bt(&["user", "videos", "293793435", "--page-size", "0"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());
    assert!(!out_a.stderr.contains("unrecognized subcommand"));
    assert!(out_a.stderr.contains("--page-size"));

    let out_b = env.run_bt(&["user", "videos", "293793435", "--page-size", "51"]);
    out_b.assert_exit(2).assert_stdout_empty();
    assert!(!out_b.stderr.trim().is_empty());
    assert!(!out_b.stderr.contains("unrecognized subcommand"));
    assert!(out_b.stderr.contains("--page-size"));
}

#[test]
fn t2_f13_02() {
    let env = TestEnv::new().with_valid_cookies();

    let out_a = env.run_bt(&["feed", "--type", "bogus"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());
    assert!(!out_a.stderr.contains("unrecognized subcommand"));
    assert!(out_a.stderr.contains("--type") || out_a.stderr.contains("bogus"));

    let out_b = env.run_bt(&["rank", "video", "--type", "bogus"]);
    out_b.assert_exit(2).assert_stdout_empty();
    assert!(!out_b.stderr.trim().is_empty());
    assert!(!out_b.stderr.contains("unrecognized subcommand"));
    assert!(out_b.stderr.contains("--type") || out_b.stderr.contains("bogus"));
}

#[test]
fn t2_f13_03() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/space/wbi/acc/info",
        FaultConfig::api_code(-404, "啥都木有"),
    );
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["--json", "user", "info", "999999999"]);
    let v = out.assert_json_err(Some("Api"));
    assert!(v["error"]["message"].as_str().unwrap().contains("啥都木有"));
    assert!(out.stderr.trim().is_empty());
}

#[test]
fn t2_f13_04() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    let out = env.run_bt(&["--json", "feed"]);
    out.assert_json_err(Some("Auth"));
    assert!(server.requests().is_empty());
    assert!(out.stderr.trim().is_empty());
}

#[test]
fn t2_f13_05() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/polymer/web-dynamic/v1/feed/space",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "ttl": 1,
            "data": { "has_more": false, "offset": "", "items": null }
        })),
    );
    let env = TestEnv::new().with_server(&server);

    let out_a = env.run_bt(&["rank", "popular", "--page-size", "51"]);
    out_a.assert_exit(2).assert_stdout_empty();
    assert!(!out_a.stderr.trim().is_empty());
    assert!(!out_a.stderr.contains("unrecognized subcommand"));
    assert!(out_a.stderr.contains("--page-size"));

    let out_b = env.run_bt(&["--json", "user", "dynamics", "293793435"]);
    out_b.assert_json_ok();
    assert!(!out_b.stderr.contains("panicked at"));
    assert!(out_b.stderr.trim().is_empty());
}

// ============================================================================
// Feature 14 — Help, Banner, README.md & Audit Sync (5 Tests)
// ============================================================================

#[test]
fn t2_f14_01() {
    let env = TestEnv::new();

    for legacy in ["start", "stop", "status"] {
        let out = env.run_bt(&[legacy]);
        out.assert_exit(2).assert_stdout_empty();
        assert!(!out.stderr.trim().is_empty());
    }
}

#[test]
fn t2_f14_02() {
    let env = TestEnv::new();

    for flag in ["--help", "-h"] {
        let out = env.run_bt(&["-q", flag]);
        out.assert_exit(0)
            .assert_stdout_contains("bt")
            .assert_stdout_contains("resolve");
        assert!(out.stderr.trim().is_empty());
    }
}

#[test]
fn t2_f14_03() {
    let env = TestEnv::new().with_no_color(false);

    let out = env.run_bt_env(&["--no-color", "--help"], &[("TERM", "xterm-256color")]);
    out.assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("resolve");
}

#[test]
fn t2_f14_04() {
    let env = TestEnv::new();
    let missing = env.temp_path().join("never_created/c.json");
    let missing_str = missing.to_string_lossy().into_owned();
    let eq_flag = format!("--cookie-file={missing_str}");

    let out_a = env.run_bt(&["-u", &missing_str, "--help"]);
    out_a.assert_exit(0).assert_stdout_contains("bt");
    assert!(!missing.exists());
    assert!(!missing.parent().unwrap().exists());

    let out_b = env.run_bt(&[&eq_flag, "--help"]);
    out_b.assert_exit(0).assert_stdout_contains("bt");
    assert!(!missing.exists());
    assert!(!missing.parent().unwrap().exists());
}

#[test]
fn t2_f14_05() {
    let env = TestEnv::new();
    let modules = [
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

    for cmd in modules {
        let out = env.run_bt(&[cmd, "--help"]);
        out.assert_exit(0).assert_stdout_contains("Usage:");
        let combined = format!("{}\n{}", out.stdout, out.stderr).to_ascii_lowercase();
        for forbidden in ["todo", "placeholder", "unimplemented", "fixme"] {
            assert!(
                !combined.contains(forbidden),
                "module `{cmd}` help contains `{forbidden}`"
            );
        }
    }
}
