mod common;

use common::{
    CookieFixture, FaultConfig, MOCK_MP4_BYTES, MockBiliServer, MockHistoryItem, MockLiveDanmaku,
    TestEnv, assert_mode_0600,
};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};

// ── 1. Global CLI, Flag Clustering, Precedence & BrokenPipe (Challenger 1) ────

#[test]
fn t5_cli_json_version_help_and_missing_subcommand() {
    let env = TestEnv::new();

    for ver_flag in ["-V", "--version"] {
        let out = env.run_bt(&["--json", ver_flag]);
        let v = out.assert_json_ok();
        assert_eq!(v["name"], "bit");
        assert_eq!(v["package"], "bili-tools");
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
        assert!(out.stderr.trim().is_empty());
    }

    let help_alias = env.run_bt(&["--json", "help"]);
    let v_help = help_alias.assert_json_ok();
    assert_eq!(v_help["name"], "bit");
    assert!(v_help["modules"].is_array());
    assert!(help_alias.stderr.trim().is_empty());

    let quiet_bare = env.run_bt(&["-q"]);
    quiet_bare.assert_exit(0).assert_stdout_empty();
    assert!(quiet_bare.stderr.trim().is_empty());

    let quiet_ver = env.run_bt(&["-q", "-V"]);
    quiet_ver
        .assert_exit(0)
        .assert_stdout_contains("bit")
        .assert_stdout_contains(env!("CARGO_PKG_VERSION"));

    let missing_sub = env.run_bt(&["--json", "auth"]);
    missing_sub.assert_json_err(Some("Input"));
    assert!(missing_sub.stderr.trim().is_empty());

    let missing_u_val = env.run_bt(&["--json", "-u", "--help"]);
    missing_u_val.assert_json_err(Some("Input"));
    assert!(missing_u_val.stderr.trim().is_empty());
}

#[test]
fn t5_cli_flag_clustering_relative_cookie_and_non_tty_human_confirm() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    // Clustered -yqh shows welcome help cleanly
    let yqh = env.run_bt(&["-yqh"]);
    yqh.assert_exit(0)
        .assert_no_ansi()
        .assert_stdout_contains("resolve");

    // Relative -u cookies.json (single filename without directory) + implicit cookie login + -qy
    let login = env.run_bt(&[
        "-u",
        "rel_cookies.json",
        "-qy",
        "--json",
        "auth",
        "login",
        "--sessdata",
        "s_rel",
        "--bili-jct",
        "j_rel",
    ]);
    let v_login = login.assert_json_ok();
    assert_eq!(v_login["success"], true);
    let rel_path = env.work_dir().join("rel_cookies.json");
    assert!(rel_path.exists());
    assert_mode_0600(&rel_path);

    // Non-TTY human destructive command without -y and without --json fails cleanly before HTTP
    server.reset_requests();
    let non_tty_human = env.run_bt(&["-u", "rel_cookies.json", "fav", "clean", "1052622027"]);
    non_tty_human
        .assert_exit(1)
        .assert_stdout_empty()
        .assert_stderr_contains("Confirmation required");
    assert!(server.requests().is_empty());

    // Clustered -qy executes destructive op in human mode cleanly
    let clustered_clean = env.run_bt(&[
        "-u",
        "rel_cookies.json",
        "-qy",
        "fav",
        "clean",
        "1052622027",
    ]);
    clustered_clean.assert_exit(0).assert_stdout_empty();
    assert!(clustered_clean.stderr.trim().is_empty());
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/v3/fav/resource/clean")
    );
}

#[test]
fn t5_stdout_broken_pipe_exits_cleanly_without_panic() {
    let env = TestEnv::new();
    for args in [
        vec!["completions", "bash"],
        vec!["--json"],
        vec!["--help"],
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bit"))
            .args(&args)
            .current_dir(env.work_dir())
            .env("HOME", env.home_dir())
            .env("XDG_CONFIG_HOME", env.xdg_dir())
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn bt for broken pipe probe");

        let mut buf = [0u8; 5];
        {
            let mut stdout = child.stdout.take().expect("piped stdout");
            let _ = stdout.read(&mut buf);
        } // drop read end early to induce BrokenPipe

        let mut stderr_str = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut stderr_str);
        }
        let status = child.wait().expect("wait bt");
        assert!(
            status.success(),
            "BrokenPipe on {args:?} failed with {status:?}, stderr={stderr_str}"
        );
        assert!(
            !stderr_str.contains("panicked at"),
            "BrokenPipe panicked on {args:?}: {stderr_str}"
        );
    }
}

// ── 2. Auth, Cookies, Permissions & Buvid Backfill (Challenger 1) ─────────────

#[test]
fn t5_auth_corrupted_cookie_file_handling() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let cookie_path = env.default_cookie_path();
    std::fs::create_dir_all(cookie_path.parent().unwrap()).unwrap();
    std::fs::write(&cookie_path, "{not valid json").unwrap();

    // auth status & refresh report Json error cleanly without panic
    let st = env.run_bt(&["--json", "auth", "status"]);
    st.assert_json_err(Some("Json"));
    assert!(!st.stderr.contains("panicked at"));

    let rf = env.run_bt(&["--json", "auth", "refresh"]);
    rf.assert_json_err(Some("Json"));

    // Optional-auth command ignores corrupted cookies and succeeds
    let hot = env.run_bt(&["--json", "search", "hot", "--limit", "3"]);
    hot.assert_json_ok();

    // auth logout deletes corrupted cookie file cleanly
    let lo = env.run_bt(&["--json", "auth", "logout"]);
    lo.assert_json_ok();
    assert!(!cookie_path.exists());
}

#[test]
fn t5_auth_refresh_edge_cases_and_login_validation() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    // Missing cookie file -> Auth error (requires B1 fix)
    let missing_rf = env.run_bt(&["--json", "auth", "refresh"]);
    missing_rf.assert_json_err(Some("Auth"));

    // Empty bili_jct -> Auth error
    let no_jct_env = TestEnv::new()
        .with_server(&server)
        .with_cookie_fixture(CookieFixture::without_csrf());
    let no_jct_rf = no_jct_env.run_bt(&["--json", "auth", "refresh"]);
    no_jct_rf.assert_json_err(Some("Auth"));

    // need_refresh == false -> clean success JSON
    let valid_env = TestEnv::new().with_server(&server).with_valid_cookies();
    server.state().lock().unwrap().need_refresh = false;
    let no_need = valid_env.run_bt(&["--json", "auth", "refresh"]);
    let v_no_need = no_need.assert_json_ok();
    assert_eq!(v_no_need["need_refresh"], false);

    // Whitespace-only --sessdata rejected
    let ws_login = env.run_bt(&[
        "--json",
        "auth",
        "login",
        "--method",
        "cookie",
        "--sessdata",
        "   ",
    ]);
    ws_login.assert_json_err(Some("Input"));

    // Missing --bili-jct warns in human mode and blocks subsequent CSRF write
    let partial = env.run_bt(&[
        "auth",
        "login",
        "--method",
        "cookie",
        "--sessdata",
        "s_only",
    ]);
    partial.assert_exit(0).assert_stderr_contains("--bili-jct");
    let like_blocked = env.run_bt(&["--json", "video", "like", "BV1uJ411r7hL"]);
    like_blocked.assert_json_err(Some("Auth"));
}

#[test]
fn t5_auth_buvid_backfill_legacy_aliases_and_permission_tightening() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);
    let cookie_path = env.default_cookie_path();
    let dir = cookie_path.parent().unwrap();
    std::fs::create_dir_all(dir).unwrap();

    // Legacy uppercase/csrf_token aliases without buvid3/4 + loose 0o644/0o755 perms
    std::fs::write(
        &cookie_path,
        r#"{"SESSDATA":"mock_sessdata_token","csrf_token":"mock_bili_jct_csrf","DedeUserID":"293793435","room_id":31686846}"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&cookie_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    // Running later list triggers prepare_data_dir (tightens perms) + ensure_buvid (/spi backfill)
    let out = env.run_bt(&["--json", "later", "list"]);
    out.assert_json_ok();
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/x/frontend/finger/spi")
    );
    assert_mode_0600(&cookie_path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let updated = env.read_cookie_file(&cookie_path);
    assert!(!updated.buvid3.is_empty());
    assert!(!updated.buvid4.is_empty());
}

// ── 3. ffmpeg Discovery & Error Paths (Challenger 1) ──────────────────────────

#[test]
fn t5_ffmpeg_non_exec_override_and_execution_failures() {
    let server = MockBiliServer::start();

    // 1. Non-executable ffmpeg (0o644) in PATH must be ignored -> ExternalTool (Unix permission model)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let env_non_exec = TestEnv::new().with_server(&server).with_fake_ffmpeg();
        let ff = env_non_exec.bin_dir().join("ffmpeg");
        std::fs::set_permissions(&ff, std::fs::Permissions::from_mode(0o644)).unwrap();
        let out_non_exec = env_non_exec.run_bt(&["--json", "live", "record", "14073662"]);
        out_non_exec.assert_json_err(Some("ExternalTool"));
        assert!(server.requests().is_empty());
    }

    // 2. Nonexistent BT_FFMPEG_PATH must NOT fall back to valid ffmpeg in PATH
    let env_ok = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let bad_ff = env_ok.temp_path().join("no_such_dir/ffmpeg");
    let out_bad_override = env_ok.run_bt_env(
        &["--json", "live", "record", "14073662"],
        &[("BT_FFMPEG_PATH", bad_ff.to_str().unwrap())],
    );
    out_bad_override.assert_json_err(Some("ExternalTool"));
    assert!(server.requests().is_empty());

    // 3. Failing `ffmpeg -version` (`with_failing_ffmpeg`) on live record
    let env_fail_ver = TestEnv::new().with_server(&server).with_failing_ffmpeg();
    let out_fail_ver = env_fail_ver.run_bt(&["--json", "live", "record", "14073662"]);
    out_fail_ver.assert_json_err(Some("ExternalTool"));
    assert!(server.requests().is_empty());

    // 4. Failing `ffmpeg` execution (`with_failing_ffmpeg_exec`) on live record
    let env_fail_exec = TestEnv::new()
        .with_server(&server)
        .with_failing_ffmpeg_exec();
    let out_fail_exec = env_fail_exec.run_bt(&[
        "--json",
        "live",
        "record",
        "14073662",
        "-o",
        "rec_fail.flv",
    ]);
    out_fail_exec.assert_json_err(Some("ExternalTool"));
}

// ── 4. Universal Resolver Edge Cases (Challenger 1) ───────────────────────────

#[test]
fn t5_resolve_multihop_schemeless_variants_and_adversarial_inputs() {
    let server = MockBiliServer::start();
    server.state().lock().unwrap().short_links.insert(
        "evil".to_string(),
        "https://example.com/phish".to_string(),
    );
    let env = TestEnv::new().with_server(&server);

    // Multi-hop short link: hop1 -> pigt3PQ -> BV1uJ411r7hL
    let hop = env
        .run_bt(&["--json", "resolve", "https://b23.tv/hop1"])
        .assert_json_ok();
    assert_eq!(hop["kind"], "video");
    assert_eq!(hop["bvid"], "BV1uJ411r7hL");
    assert_eq!(hop["network_used"], true);

    // External non-Bilibili redirect must fail
    env.run_bt(&["--json", "resolve", "https://b23.tv/evil"])
        .assert_json_err(Some("Input"));

    // Scheme-less & URL variants
    let schemeless_short = env
        .run_bt(&["--json", "resolve", "b23.tv/pigt3PQ", "--no-network"])
        .assert_json_ok();
    assert_eq!(schemeless_short["kind"], "short_link");
    assert_eq!(schemeless_short["id"], "pigt3PQ");

    let s_video = env
        .run_bt(&[
            "--json",
            "resolve",
            "https://www.bilibili.com/s/video/BV1uJ411r7hL?p=0",
        ])
        .assert_json_ok();
    assert_eq!(s_video["kind"], "video");
    assert_eq!(s_video["bvid"], "BV1uJ411r7hL");
    assert!(s_video["page"].is_null());

    let m_space = env
        .run_bt(&["--json", "resolve", "https://m.bilibili.com/space/293793435"])
        .assert_json_ok();
    assert_eq!(m_space["kind"], "user");
    assert_eq!(m_space["mid"], 293793435);

    let blanc_live = env
        .run_bt(&["--json", "resolve", "https://live.bilibili.com/blanc/76"])
        .assert_json_ok();
    assert_eq!(blanc_live["kind"], "live_room");
    assert_eq!(blanc_live["room_id"], 76);

    let ml_fav = env
        .run_bt(&["--json", "resolve", "ml1052622027"])
        .assert_json_ok();
    assert_eq!(ml_fav["kind"], "fav");
    assert_eq!(ml_fav["fav_id"], 1052622027);

    // Boundary & malformed inputs (zero panics)
    for bad in [
        "https://b23.tv/",
        "av0",
        "0",
        "99999999999999999999999",
        "BV1",
        "bv_123",
        "中文测试BV1",
        "https://live.bilibili.com/76/extra",
    ] {
        let out = env.run_bt(&["--json", "resolve", bad, "--no-network"]);
        out.assert_json_err(Some("Input"));
        assert!(!out.stderr.contains("panicked at"), "panicked on {bad}");
    }
}

// ── 5. Live Edge Cases (Challenger 1) ─────────────────────────────────────────

#[test]
fn t5_live_start_show_relogin_whitespace_and_stats() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_cookie_fixture(CookieFixture::valid().with_live_key(Some(608336837537435443)));

    // --relogin in --json mode fails Auth WITHOUT deleting cookies
    let relogin = env.run_bt(&["--json", "live", "start", "--relogin"]);
    relogin.assert_json_err(Some("Auth"));
    assert!(env.default_cookie_path().exists());

    // Whitespace-only --title rejected pre-start (B4 fix)
    let empty_title = env.run_bt(&["--json", "live", "start", "-a", "86", "-t", "   ", "-y"]);
    empty_title.assert_json_err(Some("Input"));

    // -s/--show includes full rtmp_code in JSON
    let start_show = env.run_bt(&["--json", "live", "start", "-a", "86", "-s", "-y"]);
    let v_start = start_show.assert_json_ok();
    assert_eq!(
        v_start["rtmp_code"],
        "?streamname=live_mock_stream&key=mock_secret_key_12345"
    );

    // Already-live human mode warns cleanly and never calls stopLive
    server.reset_requests();
    let already_human = env.run_bt(&["live", "start", "-y"]);
    already_human
        .assert_exit(0)
        .assert_stderr_contains("当前已在直播中");
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.path != "/room/v1/Room/stopLive")
    );

    // Human live status with live_key fetches StopLiveData stats; -q suppresses it
    server.reset_requests();
    let status_human = env.run_bt(&["live", "status"]);
    status_human
        .assert_exit(0)
        .assert_stdout_contains("本场直播统计")
        .assert_stdout_contains("512");
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/xlive/app-blink/v1/live/StopLiveData")
    );

    server.reset_requests();
    let status_quiet = env.run_bt(&["-q", "live", "status"]);
    status_quiet.assert_exit(0).assert_stdout_empty();
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.path != "/xlive/app-blink/v1/live/StopLiveData")
    );

    // live stop -d 1s while LIVE (human mode) waits countdown, stops live, and prints stats
    let stop_delay = env.run_bt(&["live", "stop", "-d", "1s"]);
    stop_delay
        .assert_exit(0)
        .assert_stdout_contains("本场直播统计");
    assert_eq!(
        server.state().lock().unwrap().rooms[&31686846].live_status,
        0
    );
}

#[test]
fn t5_live_info_area_stream_record_danmaku_and_send_edge_cases() {
    let server = MockBiliServer::start();
    let env_unauth = TestEnv::new().with_server(&server);

    // live info without ROOM fails Auth when logged out
    env_unauth
        .run_bt(&["--json", "live", "info"])
        .assert_json_err(Some("Auth"));

    // live info with short ID 76 works when anchor endpoint fails (graceful fallback)
    server.set_fault(
        "/live_user/v1/UserInfo/get_anchor_in_room",
        FaultConfig::api_code(-404, "missing anchor"),
    );
    let info_short = env_unauth
        .run_bt(&["--json", "live", "info", "76"])
        .assert_json_ok();
    assert_eq!(info_short["room_id"], 14073662);
    assert!(info_short["uname"].is_null());
    server.clear_fault("/live_user/v1/UserInfo/get_anchor_in_room");

    // live area exact numeric ID, no matches (JSON & human), and API error
    let area_id = env_unauth
        .run_bt(&["--json", "live", "area", "86"])
        .assert_json_ok();
    assert_eq!(area_id.as_array().unwrap().len(), 1);
    assert_eq!(area_id[0]["id"], "86");

    let area_none_json = env_unauth
        .run_bt(&["--json", "live", "area", "no_such_area_xyz"])
        .assert_json_ok();
    assert_eq!(area_none_json.as_array().unwrap().len(), 0);

    let area_none_human = env_unauth.run_bt(&["live", "area", "no_such_area_xyz"]);
    area_none_human
        .assert_exit(0)
        .assert_stderr_contains("未找到匹配的分区");

    // Offline live stream -> exit 0, streams: []
    server.set_fault(
        "/xlive/web-room/v2/index/getRoomPlayInfo",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "data": { "room_id": 31686846, "live_status": 0, "playurl_info": null }
        })),
    );
    let off_stream = env_unauth
        .run_bt(&["--json", "live", "stream", "31686846"])
        .assert_json_ok();
    assert_eq!(off_stream["is_live"], false);
    assert_eq!(off_stream["streams"].as_array().unwrap().len(), 0);

    // Offline live record -> Api error
    let env_ff = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    env_ff
        .run_bt(&["--json", "live", "record", "31686846"])
        .assert_json_err(Some("Api"));
    server.clear_fault("/xlive/web-room/v2/index/getRoomPlayInfo");

    // live record empty -o rejected pre-network (B5 fix) & invalid -d rejected pre-network
    server.reset_requests();
    env_ff
        .run_bt(&["--json", "live", "record", "14073662", "-o", ""])
        .assert_json_err(Some("Input"));
    env_ff
        .run_bt(&["--json", "live", "record", "14073662", "-d", "bad_dur"])
        .assert_json_err(Some("Parse"));
    assert!(server.requests().is_empty());

    // live record without -o generates live_14073662_*.flv in cwd
    let rec_default = env_ff
        .run_bt(&["--json", "live", "record", "14073662", "-d", "1s"])
        .assert_json_ok();
    let gen_name = rec_default["output"].as_str().unwrap();
    assert!(gen_name.starts_with("live_14073662_") && gen_name.ends_with(".flv"));
    assert!(env_ff.work_dir().join(gen_name).exists());

    // live danmaku merges admin + room sorted by timeline and keeps latest --limit items
    {
        let mut st = server.state().lock().unwrap();
        st.live_danmaku.push(MockLiveDanmaku {
            text: "第二条".into(),
            uid: 2,
            nickname: "U2".into(),
            timeline: "2026-01-01 12:02:00".into(),
            isadmin: 0,
        });
        st.live_danmaku.push(MockLiveDanmaku {
            text: "最新一条".into(),
            uid: 3,
            nickname: "U3".into(),
            timeline: "2026-01-01 12:03:00".into(),
            isadmin: 0,
        });
    }
    let dm_lim = env_unauth
        .run_bt(&["--json", "live", "danmaku", "76", "--limit", "2"])
        .assert_json_ok();
    assert_eq!(dm_lim["count"], 2);
    assert_eq!(dm_lim["danmaku"][0]["text"], "第二条");
    assert_eq!(dm_lim["danmaku"][1]["text"], "最新一条");

    // live send whitespace rejected pre-network, silent filter msg="f" fails Api
    let env_auth = TestEnv::new().with_server(&server).with_valid_cookies();
    server.reset_requests();
    env_auth
        .run_bt(&["--json", "live", "send", "76", "   "])
        .assert_json_err(Some("Input"));
    assert!(server.requests().is_empty());

    server.set_fault(
        "/msg/send",
        FaultConfig::json(json!({ "code": 0, "message": "", "msg": "f" })),
    );
    env_auth
        .run_bt(&["--json", "live", "send", "76", "filtered"])
        .assert_json_err(Some("Api"));
}

// ── 6. Search, Fav, Later, History & Completions Edge Cases (Challenger 1) ────

#[test]
fn t5_search_hot_fallback_and_null_user_live_results() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    // WBI search/square failure triggers BT_SEARCH_BASE_URL /main/hotword fallback
    server.set_fault(
        "/x/web-interface/wbi/search/square",
        FaultConfig::api_code(-400, "wbi fail"),
    );
    let fb = env
        .run_bt(&["--json", "search", "hot", "--limit", "1"])
        .assert_json_ok();
    let arr = fb.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["keyword"], "热搜词1");
    assert!(server.requests().iter().any(|r| r.path == "/main/hotword"));

    // search user / live null results in human mode
    server.set_fault(
        "/x/web-interface/wbi/search/type",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "data": { "numResults": 0, "result": null }
        })),
    );
    env.run_bt(&["search", "user", "nobody"])
        .assert_exit(0)
        .assert_stderr_contains("未找到相关用户");
    env.run_bt(&["search", "live", "nobody"])
        .assert_exit(0)
        .assert_stderr_contains("未找到相关直播间");
}

#[test]
fn t5_fav_text_export_csv_escaping_create_validation_and_mid_backfill() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_cookie_fixture(CookieFixture::valid().with_dede_user_id(""));

    // fav create whitespace title rejected pre-network
    server.reset_requests();
    env.run_bt(&["--json", "fav", "create", "   "])
        .assert_json_err(Some("Input"));
    assert!(server.requests().is_empty());

    // fav list backfills empty dede_user_id via /x/web-interface/nav
    let list_v = env.run_bt(&["--json", "fav", "list"]).assert_json_ok();
    assert_eq!(list_v["count"], 1);
    assert_eq!(
        env.read_cookie_file(&env.default_cookie_path()).dede_user_id,
        "293793435"
    );

    // CSV escaping with quotes, commas, and CJK + text/txt export variants
    server.state().lock().unwrap().fav_folders[0].items[0].title =
        "测试, \"引号\" 标题".to_string();

    let csv_out = env.run_bt(&["-q", "fav", "export", "1052622027", "--format", "csv"]);
    csv_out
        .assert_exit(0)
        .assert_stdout_contains("\"测试, \"\"引号\"\" 标题\"");

    let txt_stdout = env.run_bt(&["fav", "export", "1052622027", "--format", "txt"]);
    txt_stdout
        .assert_exit(0)
        .assert_stdout_contains("# 收藏夹: 默认收藏夹")
        .assert_stdout_contains("BV1uJ411r7hL");

    let txt_json = env
        .run_bt(&["--json", "fav", "export", "1052622027", "--format", "text"])
        .assert_json_ok();
    assert_eq!(txt_json["format"], "text");
    assert!(
        txt_json["content"]
            .as_str()
            .unwrap()
            .contains("BV1uJ411r7hL")
    );
}

#[test]
fn t5_later_and_history_human_rendering_and_completions_install_all_shells() {
    let server = MockBiliServer::start();
    {
        let mut st = server.state().lock().unwrap();
        st.toview[0].progress = 112; // ~50%
        st.history.push(MockHistoryItem {
            oid: 555,
            bvid: String::new(),
            title: "测试专栏文章".into(),
            author_name: "专栏作者".into(),
            business: "article".into(),
            cid: 0,
            progress: 0,
            duration: 0,
            view_at: 1700000010,
        });
    }
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    // Human later list shows partial %, 已看完
    let later_human = env.run_bt(&["later", "list"]);
    later_human
        .assert_exit(0)
        .assert_stdout_contains("50%")
        .assert_stdout_contains("已看完");

    // Human history list shows archive, live (room:14073662), and article (cv555)
    let hist_human = env.run_bt(&["history", "list"]);
    hist_human
        .assert_exit(0)
        .assert_stdout_contains("BV1uJ411r7hL")
        .assert_stdout_contains("room:14073662")
        .assert_stdout_contains("cv555");

    // Completions --install for bash, powershell, elvish
    for (shell, rel_path) in [
        ("bash", ".local/share/bash-completion/completions/bit"),
        ("powershell", ".config/powershell/completions/bit.ps1"),
        ("elvish", ".config/elvish/lib/bit.elv"),
    ] {
        let out = env.run_bt(&["--json", "completions", shell, "--install"]);
        let v = out.assert_json_ok();
        assert_eq!(v["installed"], true);
        assert_eq!(v["shell"], shell);
        let full = env.home_dir().join(rel_path);
        assert!(full.exists(), "missing installed completion at {full:?}");
        assert!(!std::fs::read_to_string(full).unwrap().trim().is_empty());
    }
}

// ── 7. Regression Tests for C2 Bugs #3, #4, #5 & README/Audit Parity ──────────

#[test]
fn t5_c2_01_download_extensionless_and_dotted_templates_preserve_ext_and_sidecar() {
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
                "videos": 2,
                "title": "v1.0 release",
                "pic": format!("http://127.0.0.1:{port}/mock/cover/c.png?x=1"),
                "duration": 200,
                "owner": { "mid": 42, "name": "UP" },
                "cid": 146044693,
                "pages": [
                    { "cid": 146044693, "page": 1, "part": "P1", "duration": 100 },
                    { "cid": 146044694, "page": 2, "part": "P2", "duration": 100 }
                ]
            }
        })),
    );
    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();

    // Case A: single-page extensionless {title} with dot + --cover
    let out_a = env
        .run_bt(&[
            "--json",
            "video",
            "download",
            "BV1uJ411r7hL",
            "-o",
            "{title}",
            "--cover",
        ])
        .assert_json_ok();
    let files_a = out_a["files"].as_array().unwrap();
    assert!(
        files_a.iter().any(|v| v.as_str() == Some("v1.0 release.mp4")),
        "expected v1.0 release.mp4 in {files_a:?}"
    );
    assert!(
        files_a.iter().any(|v| v.as_str() == Some("v1.0 release.png")),
        "expected v1.0 release.png (not v1.png) in {files_a:?}"
    );
    assert!(env.work_dir().join("v1.0 release.mp4").exists());
    assert!(env.work_dir().join("v1.0 release.png").exists());

    // Case B: multi-page extensionless template containing {page}
    let out_b = env
        .run_bt(&[
            "--json",
            "video",
            "download",
            "BV1uJ411r7hL",
            "-p",
            "1-2",
            "-o",
            "{title}_p{page}",
        ])
        .assert_json_ok();
    let files_b = out_b["files"].as_array().unwrap();
    assert!(files_b.iter().any(|v| v.as_str() == Some("v1.0 release_p1.mp4")));
    assert!(files_b.iter().any(|v| v.as_str() == Some("v1.0 release_p2.mp4")));
    assert!(env.work_dir().join("v1.0 release_p1.mp4").exists());
    assert!(env.work_dir().join("v1.0 release_p2.mp4").exists());

    // Case C: multi-page template with dot in filename (no {page}, no container ext)
    let out_c = env
        .run_bt(&[
            "--json",
            "video",
            "download",
            "BV1uJ411r7hL",
            "-p",
            "1,2",
            "-o",
            "sub/my.video",
        ])
        .assert_json_ok();
    let files_c = out_c["files"].as_array().unwrap();
    assert!(
        files_c
            .iter()
            .any(|v| v.as_str().map(|s| s.replace('\\', "/")).as_deref() == Some("sub/my.video_p1.mp4")),
        "expected sub/my.video_p1.mp4 in {files_c:?}"
    );
    assert!(
        files_c
            .iter()
            .any(|v| v.as_str().map(|s| s.replace('\\', "/")).as_deref() == Some("sub/my.video_p2.mp4")),
        "expected sub/my.video_p2.mp4 in {files_c:?}"
    );
}

#[test]
fn t5_c2_02_readme_contains_cli_design_audit_record_and_docs_sync() {
    let readme_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let readme = std::fs::read_to_string(&readme_path).expect("read README.md");

    // Must explicitly reference all 4 benchmark projects from ORIGINAL_REQUEST.md
    for ref_proj in ["biliup", "BBDown", "yutto", "bilibili-cli"] {
        assert!(
            readme.contains(ref_proj),
            "README.md missing CLI Design Audit reference project: {ref_proj}"
        );
    }
    // Must document key architecture files
    assert!(readme.contains("error.rs"), "README.md missing src/error.rs");
    assert!(readme.contains("area.rs"), "README.md missing src/api/area.rs");
}

#[test]
fn t5_c2_03_http_5xx_html_responses_surface_as_api_error_not_json_parse() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    let html_502 = FaultConfig::HttpStatus {
        status: 502,
        content_type: "text/html; charset=utf-8".to_string(),
        body: b"<html>502 Bad Gateway</html>".to_vec(),
        headers: Vec::new(),
    };

    for (route, args) in [
        (
            "/x/web-interface/archive/like",
            vec!["--json", "video", "like", "BV1uJ411r7hL"],
        ),
        (
            "/x/web-interface/coin/add",
            vec!["--json", "video", "coin", "BV1uJ411r7hL", "-n", "1"],
        ),
        (
            "/x/web-interface/archive/like/triple",
            vec!["--json", "video", "triple", "BV1uJ411r7hL"],
        ),
        (
            "/x/v3/fav/resource/deal",
            vec!["--json", "fav", "add", "1052622027", "BV1uJ411r7hL"],
        ),
        (
            "/x/v2/history/delete",
            vec!["--json", "history", "delete", "av79677524"],
        ),
    ] {
        server.clear_all_faults();
        server.set_fault(route, html_502.clone());
        let out = env.run_bt(&args);
        let err = out.assert_json_err(Some("Api"));
        assert!(
            err["error"]["message"].as_str().unwrap_or("").contains("502"),
            "expected HTTP 502 in Api error for {args:?}: {err}"
        );
    }
}

// ── 8. Video Download Edge Cases (Challenger 2) ───────────────────────────────

#[test]
fn t5_c2_04_download_truncated_stream_falls_back_to_backup_url() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind media listener");
    let media_addr = listener.local_addr().unwrap();

    let media_handle = std::thread::spawn(move || {
        // 1. Primary video URL: advertise 1000 bytes, close after 7 bytes -> UnexpectedEof
        // 2. Backup video URL: valid MOCK_MP4_BYTES
        // 3. Audio URL: valid MOCK_MP4_BYTES
        for idx in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            if idx == 0 {
                assert!(first.contains("/v_trunc.m4s"), "unexpected first req: {first}");
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\npartial",
                );
            } else {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    MOCK_MP4_BYTES.len()
                );
                let _ = stream.write_all(hdr.as_bytes());
                let _ = stream.write_all(MOCK_MP4_BYTES);
            }
        }
    });

    let server = MockBiliServer::start();
    server.set_fault(
        "/x/player/wbi/playurl",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "data": {
                "quality": 80,
                "dash": {
                    "video": [{
                        "id": 80,
                        "codecid": 7,
                        "codecs": "avc1.640028",
                        "bandwidth": 2000000,
                        "baseUrl": format!("http://{media_addr}/v_trunc.m4s"),
                        "backupUrl": [format!("http://{media_addr}/v_backup.m4s")]
                    }],
                    "audio": [{
                        "id": 30280,
                        "codecs": "mp4a.40.2",
                        "bandwidth": 192000,
                        "baseUrl": format!("http://{media_addr}/audio.m4s"),
                        "backupUrl": []
                    }]
                }
            }
        })),
    );

    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let out_mp4 = env.work_dir().join("recovered.mp4");
    let out = env
        .run_bt(&[
            "--json",
            "video",
            "download",
            "BV1uJ411r7hL",
            "-o",
            out_mp4.to_str().unwrap(),
        ])
        .assert_json_ok();
    media_handle.join().unwrap();
    assert_eq!(out["success"], true);
    assert!(out_mp4.exists());
}

#[test]
fn t5_c2_05_download_durl_only_multi_segment_and_audio_only_rejection() {
    let server = MockBiliServer::start();
    let port = server.port();
    server.set_fault(
        "/x/player/wbi/playurl",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "data": {
                "quality": 64,
                "format": "flv720",
                "durl": [
                    {
                        "order": 1,
                        "length": 100000,
                        "size": MOCK_MP4_BYTES.len(),
                        "url": format!("http://127.0.0.1:{port}/mock/media/seg1.mp4"),
                        "backup_url": []
                    },
                    {
                        "order": 2,
                        "length": 125000,
                        "size": MOCK_MP4_BYTES.len(),
                        "url": format!("http://127.0.0.1:{port}/mock/media/seg2.mp4"),
                        "backup_url": []
                    }
                ],
                "dash": null
            }
        })),
    );

    // Without ffmpeg: durl download must still succeed without invoking ffmpeg
    let env = TestEnv::new().with_server(&server).without_ffmpeg();

    let err_audio = env
        .run_bt(&["--json", "video", "download", "BV1uJ411r7hL", "--audio-only"])
        .assert_json_err(Some("Input"));
    assert!(
        err_audio["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("--audio-only")
    );

    let out_file = env.work_dir().join("durl_multi.mp4");
    let ok = env
        .run_bt(&[
            "--json",
            "video",
            "download",
            "BV1uJ411r7hL",
            "-o",
            out_file.to_str().unwrap(),
        ])
        .assert_json_ok();
    let files = ok["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(
        std::fs::read(env.work_dir().join("durl_multi_part1.mp4")).unwrap(),
        MOCK_MP4_BYTES
    );
    assert_eq!(
        std::fs::read(env.work_dir().join("durl_multi_part2.mp4")).unwrap(),
        MOCK_MP4_BYTES
    );
}

#[test]
fn t5_c2_06_download_flac_audio_all_pages_dir_hint_and_subtitle_missing_warning() {
    let server = MockBiliServer::start();
    let port = server.port();
    server.set_fault(
        "/x/player/wbi/playurl",
        FaultConfig::json(json!({
            "code": 0,
            "data": {
                "quality": 80,
                "dash": {
                    "video": [{
                        "id": 80,
                        "codecid": 7,
                        "codecs": "avc1.640028",
                        "bandwidth": 2000000,
                        "base_url": format!("http://127.0.0.1:{port}/mock/media/v80.m4s"),
                        "backup_url": []
                    }],
                    "audio": null,
                    "dolby": { "type": 0, "audio": null },
                    "flac": {
                        "display": true,
                        "audio": {
                            "id": 30251,
                            "codecs": "fLaC",
                            "bandwidth": 320000,
                            "baseUrl": format!("http://127.0.0.1:{port}/mock/media/flac.m4s")
                        }
                    }
                }
            }
        })),
    );
    server.set_fault(
        "/x/player/wbi/v2",
        FaultConfig::json(json!({
            "code": 0,
            "data": { "subtitle": { "subtitles": [] } }
        })),
    );

    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let dl_dir = env.work_dir().join("dl_dir");
    std::fs::create_dir_all(&dl_dir).unwrap();
    let dir_arg = format!("{}/", dl_dir.display());

    let res = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "-p",
        "ALL",
        "--subtitle",
        "-o",
        &dir_arg,
    ]);
    let out = res.assert_json_ok();
    assert_eq!(out["pages"].as_array().unwrap().len(), 2);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/mock/media/flac.m4s")
    );
    // Missing subtitle in --json mode succeeds without .srt; human mode warns on stderr and succeeds
    assert!(
        !out["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap_or("").ends_with(".srt"))
    );
    let mp4_count = std::fs::read_dir(&dl_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("mp4"))
        .count();
    assert_eq!(mp4_count, 2);

    let human_sub_warn = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "-p",
        "1",
        "--subtitle",
        "-o",
        &dir_arg,
    ]);
    human_sub_warn
        .assert_exit(0)
        .assert_stderr_contains("无可用字幕");
}

#[test]
fn t5_c2_07_ffmpeg_version_failure_exec_cleanup_and_bt_ffmpeg_path_override() {
    let server = MockBiliServer::start();

    // 1. Failing -version must abort BEFORE downloading any media
    let env_ver_fail = TestEnv::new().with_server(&server).with_failing_ffmpeg();
    let out_a = env_ver_fail.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "-o",
        "never.mp4",
    ]);
    out_a.assert_json_err(Some("ExternalTool"));
    assert!(
        server
            .requests()
            .iter()
            .all(|r| !r.path.starts_with("/mock/media/")),
        "media downloaded despite ffmpeg -version failure"
    );

    // 2. Failing mux execution must clean up partial output AND tempfile temp files
    server.reset_requests();
    let env_exec_fail = TestEnv::new().with_server(&server).with_failing_ffmpeg_exec();
    let out_b = env_exec_fail.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "-o",
        "mux_fail.mp4",
    ]);
    out_b.assert_json_err(Some("ExternalTool"));
    assert!(!env_exec_fail.work_dir().join("mux_fail.mp4").exists());
    let leftover: Vec<_> = std::fs::read_dir(env_exec_fail.work_dir())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(
        leftover.is_empty(),
        "expected empty work_dir after mux failure, found: {leftover:?}"
    );

    // 3. Empty PATH + BT_FFMPEG_PATH override succeeds
    let env_override = TestEnv::new().with_server(&server).without_ffmpeg();
    env_override.install_fake_ffmpeg();
    let ff_path = env_override.fake_ffmpeg_path();
    let out_c = env_override.run_bt_env(
        &[
            "--json",
            "video",
            "download",
            "BV1uJ411r7hL",
            "-o",
            "override.mp4",
        ],
        &[("BT_FFMPEG_PATH", ff_path.to_str().unwrap())],
    );
    out_c.assert_json_ok();
    assert!(env_override.work_dir().join("override.mp4").exists());
}

#[test]
fn t5_c2_14_download_subtitle_fetch_error_warns_and_continues() {
    let server = MockBiliServer::start();
    let port = server.port();
    server.set_fault(
        "/x/player/wbi/v2",
        FaultConfig::json(json!({
            "code": 0,
            "data": {
                "subtitle": {
                    "subtitles": [{
                        "lan": "zh-Hans",
                        "lan_doc": "中文",
                        "subtitle_url": format!("http://127.0.0.1:{port}/mock/subtitle/missing.json")
                    }]
                }
            }
        })),
    );
    server.set_fault(
        "/mock/subtitle/missing.json",
        FaultConfig::http(502, b"<html>502 Bad Gateway</html>".to_vec()),
    );

    let env = TestEnv::new().with_server(&server).with_fake_ffmpeg();
    let out_mp4 = env.work_dir().join("sub_warn.mp4");
    let res = env.run_bt(&[
        "--json",
        "video",
        "download",
        "BV1uJ411r7hL",
        "--subtitle",
        "-o",
        out_mp4.to_str().unwrap(),
    ]);
    let ok = res.assert_json_ok();
    assert_eq!(ok["success"], true);
    assert!(out_mp4.exists());
    assert!(
        !ok["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap_or("").ends_with(".srt"))
    );

    // Human mode emits user_warning! on stderr and still completes download
    let human_res = env.run_bt(&[
        "video",
        "download",
        "BV1uJ411r7hL",
        "--subtitle",
        "-o",
        out_mp4.to_str().unwrap(),
    ]);
    human_res
        .assert_exit(0)
        .assert_stderr_contains("已跳过");
}

// ── 9. Video Danmaku, Subtitle, Summary, Tags, Related, Streams (Challenger 2)

#[test]
fn t5_c2_08_danmaku_primary_404_fallback_txt_xml_formats_and_corrupt_bytes() {
    let server = MockBiliServer::start();
    // Primary /{cid}.xml returns 404 -> falls back to /x/v1/dm/list.so
    server.set_fault("/146044693.xml", FaultConfig::http(404, b"not found".to_vec()));
    let env = TestEnv::new().with_server(&server);

    let txt_out = env.run_bt(&["video", "danmaku", "BV1uJ411r7hL", "--format", "txt"]);
    txt_out
        .assert_exit(0)
        .assert_stdout_contains("[00:12.35] 测试弹幕一条")
        .assert_stdout_contains("[00:25.50] 顶部红字弹幕");
    let reqs = server.requests();
    assert!(reqs.iter().any(|r| r.path == "/146044693.xml"));
    assert!(reqs.iter().any(|r| r.path == "/x/v1/dm/list.so"));

    // XML + --json returns content field
    let xml_json = env
        .run_bt(&["--json", "video", "danmaku", "BV1uJ411r7hL", "--format", "xml"])
        .assert_json_ok();
    assert_eq!(xml_json["format"], "xml");
    assert!(xml_json["content"].as_str().unwrap().contains("<d p="));

    // Corrupt non-XML bytes on both primary and fallback -> Parse error
    server.set_fault("/146044693.xml", FaultConfig::http(200, b"NOT_DEFLATE_OR_XML".to_vec()));
    server.set_fault("/x/v1/dm/list.so", FaultConfig::http(200, b"ALSO_BAD".to_vec()));
    env.run_bt(&["--json", "video", "danmaku", "BV1uJ411r7hL"])
        .assert_json_err(Some("Parse"));

    // Out-of-range page -> Input error before danmaku fetch
    server.clear_all_faults();
    server.reset_requests();
    env.run_bt(&["--json", "video", "danmaku", "BV1uJ411r7hL", "-p", "99"])
        .assert_json_err(Some("Input"));
    assert!(
        server
            .requests()
            .iter()
            .all(|r| !r.path.ends_with(".xml") && r.path != "/x/v1/dm/list.so")
    );
}

#[test]
fn t5_c2_09_subtitle_need_login_lang_prefix_txt_json_and_empty_url() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    // 1. Prefix language match (--lang en -> en-US) and --format txt
    let txt = env.run_bt(&[
        "video",
        "subtitle",
        "BV1uJ411r7hL",
        "--lang",
        "en",
        "--format",
        "txt",
    ]);
    txt.assert_exit(0)
        .assert_stdout_contains("Hello Bilibili")
        .assert_stdout_contains("Mock Subtitle Line 2");

    // 2. --format json without --json vs with --json
    let raw_json = env
        .run_bt(&["video", "subtitle", "BV1uJ411r7hL", "--format", "json"])
        .assert_json_ok();
    assert!(raw_json["body"].is_array());
    assert!(raw_json.get("bvid").is_none());

    let wrapped_json = env
        .run_bt(&["--json", "video", "subtitle", "BV1uJ411r7hL", "--format", "json"])
        .assert_json_ok();
    assert_eq!(wrapped_json["bvid"], "BV1uJ411r7hL");
    assert_eq!(wrapped_json["count"], 2);

    // 3. need_login_subtitle: true without cookies -> Auth error
    server.set_fault(
        "/x/player/wbi/v2",
        FaultConfig::json(json!({
            "code": 0,
            "data": { "need_login_subtitle": true, "subtitle": { "subtitles": [] } }
        })),
    );
    env.run_bt(&["--json", "video", "subtitle", "BV1uJ411r7hL"])
        .assert_json_err(Some("Auth"));

    // 4. Empty subtitle_url -> Api error
    server.set_fault(
        "/x/player/wbi/v2",
        FaultConfig::json(json!({
            "code": 0,
            "data": { "subtitle": { "subtitles": [{ "lan": "zh-Hans", "subtitle_url": "" }] } }
        })),
    );
    env.run_bt(&["--json", "video", "subtitle", "BV1uJ411r7hL"])
        .assert_json_err(Some("Api"));
}

#[test]
fn t5_c2_10_summary_tags_related_streams_human_and_null_edge_cases() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server);

    // Human mode streams, summary (with outline), tags, related
    env.run_bt(&["video", "streams", "BV1uJ411r7hL"])
        .assert_exit(0)
        .assert_stdout_contains("可用视频流")
        .assert_stdout_contains("可用音频流")
        .assert_stdout_contains("MP4/FLV 分段流");
    env.run_bt(&["video", "summary", "BV1uJ411r7hL"])
        .assert_exit(0)
        .assert_stdout_contains("视频 AI 总结")
        .assert_stdout_contains("内容大纲")
        .assert_stdout_contains("介绍基础概念");

    // Summary queue state (code=1, stid="0") in human mode
    server.set_fault(
        "/x/web-interface/view/conclusion/get",
        FaultConfig::json(json!({
            "code": 0,
            "data": { "code": 1, "stid": "0", "status": 0, "model_result": null }
        })),
    );
    env.run_bt(&["video", "summary", "BV1uJ411r7hL"])
        .assert_exit(0)
        .assert_stderr_contains("生成队列");

    // Null tags & related -> empty JSON arrays & friendly human messages
    server.set_fault("/x/web-interface/view/detail/tag", FaultConfig::data_null());
    server.set_fault("/x/web-interface/archive/related", FaultConfig::data_null());
    let tags_v = env
        .run_bt(&["--json", "video", "tags", "BV1uJ411r7hL"])
        .assert_json_ok();
    assert_eq!(tags_v, json!([]));
    let rel_v = env
        .run_bt(&["--json", "video", "related", "BV1uJ411r7hL"])
        .assert_json_ok();
    assert_eq!(rel_v, json!([]));
    env.run_bt(&["video", "tags", "BV1uJ411r7hL"])
        .assert_exit(0)
        .assert_stderr_contains("暂无标签");

    // Strict WBI failure on summary when /nav lacks wbi_img
    server.set_fault("/x/web-interface/nav", FaultConfig::data_null());
    env.run_bt(&["--json", "video", "summary", "BV1uJ411r7hL"])
        .assert_json_err(Some("Parse"));
}

// ── 10. Video Comments / Comment Subcommands & Error Mapping (Challenger 2) ───

#[test]
fn t5_c2_11_comments_human_sort_time_reply_root_parent_cancel_and_error_mapping() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    // 1. comments --sort time --page 2 -> sort=0, pn=2
    server.reset_requests();
    env.run_bt(&[
        "--json",
        "video",
        "comments",
        "BV1uJ411r7hL",
        "--page",
        "2",
        "--sort",
        "time",
    ])
    .assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/v2/reply"
            && r.query_param("pn") == Some("2")
            && r.query_param("sort") == Some("0")
    }));

    // 2. comment post with --root and --parent + comment like --cancel
    server.reset_requests();
    env.run_bt(&[
        "--json",
        "video",
        "comment",
        "post",
        "BV1uJ411r7hL",
        "二级回复",
        "--root",
        "104192624480",
        "--parent",
        "104192624480",
    ])
    .assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/reply/add"
            && r.form_param("root") == Some("104192624480")
            && r.form_param("parent") == Some("104192624480")
    }));

    server.reset_requests();
    let unlike = env
        .run_bt(&[
            "--json",
            "video",
            "comment",
            "like",
            "BV1uJ411r7hL",
            "104192624480",
            "--cancel",
        ])
        .assert_json_ok();
    assert_eq!(unlike["liked"], false);
    assert!(server.requests().iter().any(|r| {
        r.method == "POST"
            && r.path == "/x/v2/reply/action"
            && r.form_param("action") == Some("0")
    }));

    // 3. rpid=0 rejected by clap (exit 2)
    env.run_bt(&["video", "comment", "like", "BV1uJ411r7hL", "0"])
        .assert_exit(2);
    env.run_bt(&["video", "comment", "delete", "BV1uJ411r7hL", "0"])
        .assert_exit(2);

    // 4. Human mode with member: null -> "匿名用户", and empty replies -> "暂无评论"
    server.set_fault(
        "/x/v2/reply",
        FaultConfig::json(json!({
            "code": 0,
            "data": {
                "page": { "num": 1, "size": 20, "count": 1, "acount": 1 },
                "replies": [{
                    "rpid": 555,
                    "oid": 79677524,
                    "mid": 0,
                    "rcount": 0,
                    "like": 1,
                    "action": 0,
                    "ctime": 1700000000,
                    "member": null,
                    "content": { "message": "第一行\n第二行" },
                    "replies": null
                }],
                "hots": null
            }
        })),
    );
    env.run_bt(&["video", "comments", "BV1uJ411r7hL"])
        .assert_exit(0)
        .assert_stdout_contains("匿名用户")
        .assert_stdout_contains("第一行 第二行");

    server.set_fault("/x/v2/reply", FaultConfig::data_null());
    env.run_bt(&["video", "comments", "BV1uJ411r7hL"])
        .assert_exit(0)
        .assert_stderr_contains("暂无评论");

    // 5. Auth (-101/-111) vs Api (12002) error mapping
    server.set_fault("/x/v2/reply/action", FaultConfig::api_code(-111, "csrf 校验失败"));
    env.run_bt(&["--json", "video", "comment", "like", "BV1uJ411r7hL", "555"])
        .assert_json_err(Some("Auth"));

    server.set_fault("/x/v2/reply/del", FaultConfig::api_code(12002, "评论区已关闭"));
    env.run_bt(&["--json", "video", "comment", "delete", "BV1uJ411r7hL", "555"])
        .assert_json_err(Some("Api"));
}

// ── 11. User, Feed, Rank & Search Hot Fallback (Challenger 2) ─────────────────

#[test]
fn t5_c2_12_user_feed_rank_human_modes_offsets_types_and_voucher_errors() {
    let server = MockBiliServer::start();
    let env = TestEnv::new().with_server(&server).with_valid_cookies();

    // 1. user info human mode with /x/relation/stat 500 degradation
    server.set_fault("/x/relation/stat", FaultConfig::http(500, b"err".to_vec()));
    env.run_bt(&["user", "info", "293793435"])
        .assert_exit(0)
        .assert_stdout_contains("用户主页")
        .assert_stdout_contains("- 粉丝");

    // 2. user videos human mode (with av<aid> fallback when bvid is empty) & null vlist
    server.set_fault(
        "/x/space/wbi/arc/search",
        FaultConfig::json(json!({
            "code": 0,
            "data": {
                "list": { "vlist": [{ "aid": 4242, "bvid": "", "title": "AV稿件", "play": "--", "length": "01:00", "created": 1700000000 }] },
                "page": { "pn": 1, "ps": 20, "count": 1 }
            }
        })),
    );
    env.run_bt(&["user", "videos", "293793435"])
        .assert_exit(0)
        .assert_stdout_contains("av4242");

    // 3. user dynamics --offset & FORWARD+OPUS+pub_ts+has_more human rendering
    server.set_fault(
        "/x/polymer/web-dynamic/v1/feed/space",
        FaultConfig::json(json!({
            "code": 0,
            "data": {
                "has_more": true,
                "offset": "cursor_next_2",
                "items": [{
                    "id_str": "9901",
                    "type": "DYNAMIC_TYPE_FORWARD",
                    "modules": {
                        "module_author": { "name": "转发者", "pub_ts": 1700000000 },
                        "module_dynamic": { "desc": { "text": "转发评语" } }
                    },
                    "orig": {
                        "type": "DYNAMIC_TYPE_WORD",
                        "modules": {
                            "module_author": { "name": "原作者" },
                            "module_dynamic": {
                                "major": {
                                    "type": "MAJOR_TYPE_OPUS",
                                    "opus": { "title": "图文标题", "summary": { "text": "正文" } }
                                }
                            }
                        }
                    }
                }]
            }
        })),
    );
    server.reset_requests();
    env.run_bt(&["user", "dynamics", "293793435", "--offset", "cursor_1"])
        .assert_exit(0)
        .assert_stdout_contains("更多游标: cursor_next_2")
        .assert_stdout_contains("转发评语 // @原作者: 图文标题: 正文");
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/polymer/web-dynamic/v1/feed/space"
            && r.query_param("offset") == Some("cursor_1")
    }));

    // 4. feed --type pgc / article --offset & human mode
    for ftype in ["all", "pgc", "article"] {
        server.reset_requests();
        env.run_bt(&["feed", "--type", ftype, "--offset", "feed_cur"])
            .assert_exit(0)
            .assert_stdout_contains("关注动态");
        assert!(server.requests().iter().any(|r| {
            r.path == "/x/polymer/web-dynamic/v1/feed/all"
                && r.query_param("type") == Some(ftype)
                && r.query_param("offset") == Some("feed_cur")
        }));
    }

    // 5. rank video --rid 36 --type rookie/origin & rank popular human mode + empty list: null
    for rtype in ["rookie", "origin"] {
        server.reset_requests();
        env.run_bt(&["rank", "video", "--rid", "36", "--type", rtype])
            .assert_exit(0)
            .assert_stdout_contains("视频排行榜");
        assert!(server.requests().iter().any(|r| {
            r.path == "/x/web-interface/ranking/v2"
                && r.query_param("rid") == Some("36")
                && r.query_param("type") == Some(rtype)
        }));
    }
    server.set_fault("/x/web-interface/popular", FaultConfig::data_null());
    env.run_bt(&["rank", "popular"])
        .assert_exit(0)
        .assert_stderr_contains("暂无排行榜视频数据");

    // 6. v_voucher risk control & unauth user follow
    server.set_fault(
        "/x/space/wbi/acc/info",
        FaultConfig::json(json!({ "code": 0, "data": { "v_voucher": "voucher_123" } })),
    );
    let voucher_err = env
        .run_bt(&["--json", "user", "info", "293793435"])
        .assert_json_err(Some("Api"));
    assert!(voucher_err["error"]["message"].as_str().unwrap().contains("v_voucher"));

    let no_auth_env = TestEnv::new()
        .with_server(&server)
        .with_cookie_fixture(CookieFixture::without_csrf());
    no_auth_env
        .run_bt(&["--json", "user", "follow", "293793435"])
        .assert_json_err(Some("Auth"));
    no_auth_env
        .run_bt(&["--json", "user", "unfollow", "293793435"])
        .assert_json_err(Some("Auth"));
}

#[test]
fn t5_c2_13_search_hot_fallback_to_search_base_url_hotword() {
    let server = MockBiliServer::start();
    server.set_fault(
        "/x/web-interface/wbi/search/square",
        FaultConfig::api_code(-400, "square unavailable"),
    );
    let env = TestEnv::new().with_server(&server);

    let out = env
        .run_bt(&["--json", "search", "hot", "--limit", "1"])
        .assert_json_ok();
    let arr = out.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["keyword"], "热搜词1");
    assert!(server.requests().iter().any(|r| r.path == "/main/hotword"));
}

#[test]
fn t5_phase1_phase2_cli_ergonomics_pgc_srt_and_untruncated_urls() {
    let server = MockBiliServer::start();
    let port = server.port();

    // 1. P0-1: 240+ char signed live & video stream URLs must NOT be truncated in human stdout
    let long_sig = format!("{}tail_marker_end_98765", "a".repeat(200));
    server.set_fault(
        "/xlive/web-room/v2/index/getRoomPlayInfo",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "data": {
                "room_id": 14073662,
                "live_status": 1,
                "playurl_info": {
                    "playurl": {
                        "stream": [{
                            "protocol_name": "http_stream",
                            "format": [{
                                "format_name": "flv",
                                "codec": [{
                                    "codec_name": "avc",
                                    "current_qn": 400,
                                    "base_url": "/live-bvc/14073662/live.flv",
                                    "url_info": [{
                                        "host": format!("http://127.0.0.1:{port}"),
                                        "extra": format!("?expires=1999999999&sign={long_sig}")
                                    }]
                                }]
                            }]
                        }],
                        "g_qn_desc": [{ "qn": 400, "desc": "蓝光" }]
                    }
                }
            }
        })),
    );

    let env = TestEnv::new()
        .with_server(&server)
        .with_valid_cookies()
        .with_fake_ffmpeg();

    let live_st_human = env.run_bt(&["live", "streams", "76", "-Q", "1080p"]);
    live_st_human
        .assert_exit(0)
        .assert_stdout_contains("tail_marker_end_98765");
    assert!(
        server.requests().iter().any(|r| {
            r.path == "/xlive/web-room/v2/index/getRoomPlayInfo"
                && r.query_param("qn") == Some("400")
        }),
        "expected semantic -Q 1080p on live stream to map to qn=400"
    );
    server.clear_fault("/xlive/web-room/v2/index/getRoomPlayInfo");

    // 2. P1-3: live start 60024 face-auth QR URL & SRT protocol extraction
    server.set_fault(
        "/room/v1/Room/startLive",
        FaultConfig::json(json!({
            "code": 60024,
            "message": "need face auth",
            "data": { "qr": "https://www.bilibili.com/blackboard/face_auth_qr_test" }
        })),
    );
    let face_err = env
        .run_bt(&["--json", "live", "start", "-a", "235", "-y"])
        .assert_json_err(Some("Api"));
    assert!(
        face_err["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("face_auth_qr_test")
    );

    server.set_fault(
        "/room/v1/Room/startLive",
        FaultConfig::json(json!({
            "code": 0,
            "message": "0",
            "data": {
                "live_key": "889900",
                "rtmp": {
                    "addr": "rtmp://live-push.bilivideo.com/live-bvc/",
                    "code": "rtmp_secret_123456"
                },
                "protocols": [{
                    "protocol": "srt",
                    "addr": "srt://live-push.bilivideo.com:1935",
                    "code": "?streamid=#!::h=live-push,r=live-bvc/srt_secret_654321"
                }]
            }
        })),
    );
    let srt_ok = env
        .run_bt(&["--json", "live", "start", "-a", "235", "-y"])
        .assert_json_ok();
    assert_eq!(srt_ok["srt_addr"], "srt://live-push.bilivideo.com:1935");
    assert!(srt_ok["srt_code_masked"].as_str().unwrap().contains('*'));
    assert!(srt_ok.get("srt_code").is_none());
    server.clear_fault("/room/v1/Room/startLive");

    // 3. P1-1 & P2-3: semantic -Q 1080p + -e h265 + default .ass danmaku + FLAC .flac extension
    let dl_mp4 = env.work_dir().join("sem.mp4");
    let dl_res = env
        .run_bt(&[
            "--json",
            "video",
            "dl",
            "BV1uJ411r7hL",
            "-Q",
            "1080p",
            "-e",
            "h265",
            "--danmaku",
            "-o",
            dl_mp4.to_str().unwrap(),
        ])
        .assert_json_ok();
    assert!(
        dl_res["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f.as_str().unwrap().ends_with("sem.ass")),
        "expected default .ass danmaku sidecar in {}",
        dl_res
    );

    // 4. P1-4: Bangumi md99 -> ss678 -> ep12345 resolution & pgc playurl
    server.set_fault(
        "/pgc/review/user",
        FaultConfig::json(json!({
            "code": 0,
            "result": { "media": { "season_id": 678 } }
        })),
    );
    server.set_fault(
        "/pgc/view/web/season",
        FaultConfig::json(json!({
            "code": 0,
            "result": {
                "season_id": 678,
                "season_title": "测试番剧第一季",
                "cover": format!("http://127.0.0.1:{port}/mock/cover/BV1uJ411r7hL.jpg"),
                "evaluate": "番剧简介",
                "up_info": { "mid": 293793435, "uname": "番剧出品" },
                "stat": { "views": 99999, "danmakus": 888, "coins": 777, "favorites": 666, "likes": 555 },
                "episodes": [
                    { "id": 12344, "aid": 79677523, "bvid": "BV1uJ411r7hK", "cid": 146044692, "title": "1", "long_title": "序章", "duration": 1440000 },
                    { "id": 12345, "aid": 79677524, "bvid": "BV1uJ411r7hL", "cid": 146044693, "title": "2", "long_title": "觉醒", "duration": 1440000 }
                ]
            }
        })),
    );
    server.set_fault(
        "/pgc/player/web/playurl",
        FaultConfig::json(json!({
            "code": 0,
            "result": {
                "quality": 80,
                "format": "hdflv2",
                "dash": {
                    "video": [{
                        "id": 80,
                        "base_url": format!("http://127.0.0.1:{port}/mock/media/video_80_avc.m4s?tail_pgc_url_ok"),
                        "bandwidth": 2000000,
                        "codecs": "avc1.640028",
                        "width": 1920,
                        "height": 1080,
                        "frame_rate": "23.976",
                        "codecid": 7
                    }],
                    "audio": []
                }
            }
        })),
    );

    let ep_streams = env.run_bt(&["video", "stream", "ep12345"]);
    ep_streams
        .assert_exit(0)
        .assert_stdout_contains("P2")
        .assert_stdout_contains("tail_pgc_url_ok");

    let md_info = env
        .run_bt(&["--json", "video", "info", "md99"])
        .assert_json_ok();
    assert_eq!(md_info["season_id"], 678);
    assert_eq!(md_info["pages"].as_array().unwrap().len(), 2);

    // 5. P1-4 & P1-6 & P2-3: history rm room:/cv + cursor, bare `bit user info`, nested fav export
    server.reset_requests();
    let del_live = env
        .run_bt(&["--json", "history", "rm", "room:76"])
        .assert_json_ok();
    assert_eq!(del_live["kid"], "live_76");
    let del_cv = env
        .run_bt(&["--json", "history", "remove", "cv12345"])
        .assert_json_ok();
    assert_eq!(del_cv["kid"], "article_12345");

    env.run_bt(&[
        "--json",
        "history",
        "ls",
        "--max",
        "888",
        "--view-at",
        "1700000000",
    ])
    .assert_json_ok();
    assert!(server.requests().iter().any(|r| {
        r.path == "/x/web-interface/history/cursor"
            && r.query_param("max") == Some("888")
            && r.query_param("view_at") == Some("1700000000")
    }));

    let self_user = env.run_bt(&["--json", "user", "info"]).assert_json_ok();
    assert_eq!(self_user["mid"], 293793435);

    let fav_list = env.run_bt(&["--json", "fav", "ls"]).assert_json_ok();
    let fid = fav_list["list"][0]["id"].as_i64().unwrap().to_string();
    let nested_csv = env.work_dir().join("sub_a/sub_b/exported.csv");
    env.run_bt(&[
        "fav",
        "export",
        &fid,
        "--format",
        "csv",
        "-o",
        nested_csv.to_str().unwrap(),
    ])
    .assert_exit(0);
    assert!(nested_csv.exists());
}

#[test]
fn t5_biliup_upload_append_studio_live_record_danmaku_and_sub_replies() {
    let server = MockBiliServer::start();
    let env = TestEnv::new()
        .with_server(&server)
        .with_valid_cookies()
        .with_fake_ffmpeg();

    let p1 = env.work_dir().join("part_one.mp4");
    let p2 = env.work_dir().join("part_two.mp4");
    let p3 = env.work_dir().join("part_three.mp4");
    let cover_png = env.work_dir().join("cover.png");
    let cover43_png = env.work_dir().join("cover43.png");
    std::fs::write(&p1, b"0123456789abcdefghij").unwrap(); // 20 bytes -> 3 chunks (chunk_size=8)
    std::fs::write(&p2, b"ABCDEFGHIJ0123456789").unwrap(); // 20 bytes -> 3 chunks
    std::fs::write(&p3, b"third_part_video_raw").unwrap();
    std::fs::write(&cover_png, b"\x89PNG\r\n\x1a\nmockpngdata").unwrap();
    std::fs::write(&cover43_png, b"\x89PNG\r\n\x1a\nmockpng43data").unwrap();

    // 1. Fail-fast pre-upload validations (0 network requests!)
    server.reset_requests();
    env.run_bt(&[
        "--json",
        "video",
        "upload",
        p1.to_str().unwrap(),
        "--copyright",
        "2",
    ])
    .assert_json_err(Some("Input"));
    env.run_bt(&[
        "--json",
        "video",
        "upload",
        p1.to_str().unwrap(),
        "--dtime",
        "+30m",
    ])
    .assert_json_err(Some("Input"));
    assert!(server.requests().is_empty());

    // 2. Multi-P concurrent UPOS chunk upload + cover upload + archive submission
    let up_json = env
        .run_bt(&[
            "--json",
            "video",
            "up",
            p1.to_str().unwrap(),
            p2.to_str().unwrap(),
            "-t",
            "多P并发投稿测试",
            "--tid",
            "122",
            "--tag",
            "Rust, 录播",
            "--cover",
            cover_png.to_str().unwrap(),
            "--cover43",
            cover43_png.to_str().unwrap(),
            "-l",
            "auto",
            "--limit",
            "3",
            "--dtime",
            "+4h",
        ])
        .assert_json_ok();
    assert_eq!(up_json["bvid"], "BV1Up411x7mK");
    assert_eq!(up_json["parts"].as_array().unwrap().len(), 2);
    assert_eq!(
        up_json["cover"],
        "https://archive.biliimg.com/bfs/archive/mock_uploaded_cover.png"
    );
    assert_eq!(
        up_json["cover43"],
        "https://archive.biliimg.com/bfs/archive/mock_uploaded_cover.png"
    );

    let reqs = server.requests();
    assert_eq!(
        reqs.iter()
            .filter(|r| r.method == "POST" && r.path == "/x/vu/web/cover/up")
            .count(),
        2,
        "expected 2 cover uploads (16:9 cover + 4:3 cover43)"
    );
    assert!(
        reqs.iter()
            .filter(|r| r.method == "PUT" && r.path.starts_with("/ugcfx/"))
            .count()
            == 6,
        "expected 6 concurrent UPOS PUT chunks (3 per 20-byte file)"
    );
    let add_req = reqs
        .iter()
        .find(|r| r.method == "POST" && r.path == "/x/vu/web/add/v3")
        .expect("expected POST /x/vu/web/add/v3 request");
    let add_body: serde_json::Value = serde_json::from_slice(&add_req.body).unwrap();
    assert_eq!(
        add_body["cover"],
        "https://archive.biliimg.com/bfs/archive/mock_uploaded_cover.png"
    );
    assert_eq!(
        add_body["cover43"],
        "https://archive.biliimg.com/bfs/archive/mock_uploaded_cover.png"
    );

    // 3. Append new part to existing archive (biliup append parity)
    let app_json = env
        .run_bt(&[
            "--json",
            "video",
            "append",
            "BV1uJ411r7hL",
            p3.to_str().unwrap(),
            "-l",
            "bda2",
        ])
        .assert_json_ok();
    assert_eq!(app_json["bvid"], "BV1uJ411r7hL");
    assert_eq!(app_json["appended_count"], 1);
    assert_eq!(app_json["total_parts"], 3);

    // 4. Creative Center Studio list & show (biliup list / show parity)
    env.run_bt(&["video", "studio", "ls", "--status", "pubing"])
        .assert_exit(0)
        .assert_stdout_contains("创作中心稿件管理")
        .assert_stdout_contains("转码中测试稿件");

    env.run_bt(&["video", "archives", "show", "BV1uJ411r7hL"])
        .assert_exit(0)
        .assert_stdout_contains("创作中心稿件详情")
        .assert_stdout_contains("P1 上半场")
        .assert_stdout_contains("orig_p2");

    // 5. Sub-replies (--root) & full multi-line comments (--full)
    env.run_bt(&["video", "comments", "BV1uJ411r7hL", "--root", "1001"])
        .assert_exit(0)
        .assert_stdout_contains("楼主的一级长评论内容")
        .assert_stdout_contains("楼中楼第一条回复")
        .assert_stdout_contains("楼中楼第二条回复");

    env.run_bt(&["video", "comments", "BV1uJ411r7hL", "--full"])
        .assert_exit(0)
        .assert_stdout_contains("视频评论");

    // 6. Live recording with synchronized .ass & .xml danmaku sidecars + --watch --once
    let rec_flv = env.work_dir().join("rec_dm.flv");
    let rec_res = env
        .run_bt(&[
            "--json",
            "live",
            "record",
            "76",
            "-d",
            "1s",
            "--danmaku",
            "-o",
            rec_flv.to_str().unwrap(),
        ])
        .assert_json_ok();
    assert!(rec_flv.exists());
    let ass_sidecar = env.work_dir().join("rec_dm.ass");
    assert!(ass_sidecar.exists(), "expected synced .ass danmaku sidecar");
    assert!(
        std::fs::read_to_string(&ass_sidecar)
            .unwrap()
            .contains("[Script Info]")
    );
    assert_eq!(
        rec_res["danmaku_file"].as_str(),
        Some(ass_sidecar.to_str().unwrap())
    );

    let watch_flv = env.work_dir().join("watch_once.flv");
    let watch_res = env
        .run_bt(&[
            "--json",
            "live",
            "record",
            "76",
            "-w",
            "--once",
            "-d",
            "1s",
            "--danmaku",
            "--danmaku-format",
            "xml",
            "-o",
            watch_flv.to_str().unwrap(),
        ])
        .assert_json_ok();
    assert_eq!(watch_res["watch"], true);
    assert_eq!(watch_res["segment_count"], 1);
    let xml_sidecar = env.work_dir().join("watch_once.xml");
    assert!(xml_sidecar.exists(), "expected synced .xml danmaku sidecar");
    assert!(
        std::fs::read_to_string(&xml_sidecar)
            .unwrap()
            .contains("<chatserver>chat.bilibili.com</chatserver>")
    );
}
