use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};
use base64::{Engine as _, engine::general_purpose};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::File;
use std::io::{IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const UPOS_PROFILE: &str = "ugcfx/bup";
const DEFAULT_CHUNK_SIZE: u64 = 10 * 1024 * 1024; // 10 MiB
const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024; // 20 MiB

/// B 站 UPOS 投稿上传线路（对标 biliup-rs）
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UploadLine {
    /// 自动探测延迟最低的线路
    #[default]
    Auto,
    /// 百度云 BOS (bda2)
    Bda2,
    /// 网宿 (ws)
    Ws,
    /// 七牛云 (qn)
    Qn,
    /// 腾讯云 COS (tx / cos)
    #[value(alias = "tx")]
    Cos,
    /// 腾讯云海外 (txa)
    Txa,
    /// 阿里云海外 (alia)
    Alia,
    /// B 站自建节点 (bldsa)
    Bldsa,
}

impl UploadLine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Bda2 => "bda2",
            Self::Ws => "ws",
            Self::Qn => "qn",
            Self::Cos => "cos",
            Self::Txa => "txa",
            Self::Alia => "alia",
            Self::Bldsa => "bldsa",
        }
    }

    fn explicit_query(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Bda2 => Some("probe_version=20221109&upcdn=bda2&zone=cs"),
            Self::Ws => Some("probe_version=20221109&upcdn=ws&zone=cs"),
            Self::Qn => Some("probe_version=20221109&upcdn=qn&zone=cs"),
            Self::Cos => Some("probe_version=20221109&upcdn=tx&zone=cs"),
            Self::Txa => Some("probe_version=20221109&upcdn=txa&zone=cs"),
            Self::Alia => Some("probe_version=20221109&upcdn=alia&zone=cs"),
            Self::Bldsa => Some("probe_version=20221109&upcdn=bldsa&zone=cs"),
        }
    }
}

/// 创作中心后台稿件状态过滤（对标 biliup list）
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StudioArchiveStatus {
    /// 全部状态（已通过 + 进行中 + 未通过）
    #[default]
    All,
    /// 已通过发布 (pubed)
    Pubed,
    /// 审核/转码进行中 (is_pubing)
    #[value(name = "pubing", alias = "is_pubing")]
    IsPubing,
    /// 未通过/退回锁定 (not_pubed)
    #[value(name = "rejected", alias = "not_pubed")]
    NotPubed,
}

impl StudioArchiveStatus {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::All => "is_pubing,pubed,not_pubed",
            Self::Pubed => "pubed",
            Self::IsPubing => "is_pubing",
            Self::NotPubed => "not_pubed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedVideoPart {
    pub filename: String,
    pub title: String,
    pub desc: String,
    pub cid: i64,
}

#[derive(Debug, Clone)]
pub struct ArchiveSubmitParams {
    pub videos: Vec<UploadedVideoPart>,
    pub title: String,
    pub tid: u32,
    pub tag: String,
    pub desc: String,
    pub copyright: u8,
    pub source: String,
    pub cover: String,
    pub dynamic: String,
    pub dtime: Option<i64>,
    pub no_reprint: bool,
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn normalize_upos_endpoint(endpoint: &str, upos_uri: &str) -> Result<(String, String)> {
    let ep = endpoint.trim().trim_end_matches('/');
    if ep.is_empty() {
        return Err(BiliLiveError::Parse("preupload 缺少 endpoint 字段".to_string()));
    }
    let scheme_host = if ep.starts_with("http://") || ep.starts_with("https://") {
        ep.to_string()
    } else if ep.starts_with("//") {
        format!("https:{ep}")
    } else {
        format!("https://{ep}")
    };

    let raw_uri = upos_uri.trim();
    let path = raw_uri
        .strip_prefix("upos://")
        .unwrap_or(raw_uri)
        .trim_start_matches('/');
    if path.is_empty() {
        return Err(BiliLiveError::Parse("preupload 缺少有效的 upos_uri".to_string()));
    }

    let last_seg = path.rsplit('/').next().unwrap_or(path);
    let server_filename = match last_seg.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => last_seg.to_string(),
    };

    Ok((format!("{scheme_host}/{path}"), server_filename))
}

fn normalize_probe_url(raw: &str) -> String {
    let t = raw.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else if t.starts_with("//") {
        format!("https:{t}")
    } else {
        format!("https://{t}")
    }
}

/// 自动探测最快上传线路 query 参数；若探测失败则安全回退为空（由服务端自动分配）
pub fn resolve_line_query(line: UploadLine) -> String {
    if let Some(q) = line.explicit_query() {
        return q.to_string();
    }

    let probe_api = format!("{}/preupload?r=probe", crate::api::client::member_api_base_url());
    let Ok(resp) = minreq::get(&probe_api)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_timeout(4)
        .send()
    else {
        return String::new();
    };
    if resp.status_code >= 400 {
        return String::new();
    }
    let Ok(text) = resp.as_str() else {
        return String::new();
    };
    let Ok(json) = serde_json::from_str::<Value>(text) else {
        return String::new();
    };
    let Some(lines) = json["lines"].as_array() else {
        return String::new();
    };

    let mut best: Option<(Duration, String)> = None;
    for item in lines {
        let q = item["query"].as_str().unwrap_or("").trim();
        let purl = item["probe_url"].as_str().unwrap_or("").trim();
        if q.is_empty() || purl.is_empty() {
            continue;
        }
        let target = normalize_probe_url(purl);
        let t0 = Instant::now();
        if let Ok(r) = minreq::get(&target)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_timeout(2)
            .send()
            && r.status_code < 400
        {
            let elapsed = t0.elapsed();
            if best.as_ref().is_none_or(|(prev, _)| elapsed < *prev) {
                best = Some((elapsed, q.to_string()));
            }
        }
    }

    best.map(|(_, q)| q)
        .or_else(|| {
            lines
                .first()
                .and_then(|l| l["query"].as_str())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default()
}

fn format_bytes_human(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    if bytes as f64 >= MIB {
        format!("{:.1} MB", bytes as f64 / MIB)
    } else if bytes as f64 >= KIB {
        format!("{:.0} KB", bytes as f64 / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn render_upload_progress(uploaded: u64, total: u64, bps: f64, enable_color: bool) -> String {
    let ratio = if total == 0 {
        1.0
    } else {
        (uploaded as f64 / total as f64).clamp(0.0, 1.0)
    };
    let bar_w = 18usize;
    let filled = (ratio * bar_w as f64).round() as usize;
    let empty = bar_w.saturating_sub(filled);
    let pct = (ratio * 100.0).round() as u32;
    let f_bar = "━".repeat(filled);
    let e_bar = "─".repeat(empty);
    let size_str = format!(
        "{} / {}",
        format_bytes_human(uploaded),
        format_bytes_human(total)
    );
    let speed_str = format!("{}/s", format_bytes_human(bps.max(0.0) as u64));

    if enable_color {
        use crossterm::style::Stylize;
        format!(
            "  {} {}{} {:>3}%  {}  {}",
            "⬆".with(crate::ui::theme::BILI_PINK),
            f_bar.with(crate::ui::theme::BILI_PINK),
            e_bar.dark_grey(),
            pct,
            size_str.with(crate::ui::theme::BILI_CYAN),
            speed_str.dark_grey()
        )
    } else {
        format!("  ⬆ [{f_bar}{e_bar}] {pct:>3}%  {size_str}  {speed_str}")
    }
}

/// 通过 UPOS 多线程分块协议上传单个视频文件，返回可用于投稿/追加分P的 `UploadedVideoPart`
pub fn upload_single_video_file(
    cookies: &Cookies,
    file_path: &Path,
    part_title_override: Option<&str>,
    line_query: &str,
    concurrency_limit: usize,
) -> Result<UploadedVideoPart> {
    crate::utils::interrupt::check_interrupted()?;

    let meta = std::fs::metadata(file_path).map_err(|e| {
        BiliLiveError::Input(format!("无法读取待上传视频文件 '{}': {e}", file_path.display()))
    })?;
    if !meta.is_file() {
        return Err(BiliLiveError::Input(format!(
            "路径不是普通文件: {}",
            file_path.display()
        )));
    }
    let total_size = meta.len();
    if total_size == 0 {
        return Err(BiliLiveError::Input(format!(
            "待上传视频文件大小不能为 0: {}",
            file_path.display()
        )));
    }

    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("video.mp4");
    let default_stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Part");
    let part_title: String = part_title_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_stem)
        .chars()
        .take(80)
        .collect();

    // Step 1: preupload 获取上传节点、auth 与 chunk_size
    let base_q = serde_urlencoded::to_string([
        ("name", file_name),
        ("size", &total_size.to_string()),
        ("r", "upos"),
        ("profile", UPOS_PROFILE),
        ("ssl", "0"),
        ("version", "2.14.0.0"),
        ("build", "2140000"),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let full_pre_q = if line_query.trim().is_empty() {
        base_q
    } else {
        format!("{base_q}&{}", line_query.trim().trim_start_matches('&'))
    };
    let pre_url = format!(
        "{}/preupload?{full_pre_q}",
        crate::api::client::member_api_base_url()
    );
    let pre_resp = crate::api::client::get(&pre_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://member.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;
    let pre_json: Value = crate::api::client::parse_json_response(&pre_resp, "预上传请求失败")?;
    if pre_json["OK"].as_i64() != Some(1) {
        return Err(BiliLiveError::Api(format!(
            "UPOS 预上传失败: {}",
            pre_json
        )));
    }

    let auth = pre_json["auth"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| BiliLiveError::Parse("preupload 响应缺少 auth 凭证".to_string()))?
        .to_string();
    let biz_id = pre_json["biz_id"]
        .as_i64()
        .filter(|&id| id > 0)
        .ok_or_else(|| BiliLiveError::Parse("preupload 响应缺少有效 biz_id".to_string()))?;
    let chunk_size = pre_json["chunk_size"]
        .as_u64()
        .filter(|&c| c > 0)
        .unwrap_or(DEFAULT_CHUNK_SIZE);
    let endpoint = pre_json["endpoint"].as_str().unwrap_or("");
    let upos_uri = pre_json["upos_uri"].as_str().unwrap_or("");
    let (target_url, server_filename) = normalize_upos_endpoint(endpoint, upos_uri)?;

    // Step 2: POST ?uploads= 初始化分片上传并获取 upload_id
    let init_q = serde_urlencoded::to_string([
        ("uploads", ""),
        ("output", "json"),
        ("profile", UPOS_PROFILE),
        ("filesize", &total_size.to_string()),
        ("partsize", &chunk_size.to_string()),
        ("biz_id", &biz_id.to_string()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let init_url = format!("{target_url}?{init_q}");
    let init_resp = crate::api::client::post(&init_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("X-Upos-Auth", &auth)
        .send()?;
    let init_json: Value =
        crate::api::client::parse_json_response(&init_resp, "初始化 UPOS 分片失败")?;
    if init_json["OK"].as_i64() != Some(1) {
        return Err(BiliLiveError::Api(format!(
            "初始化 UPOS 分片返回异常: {init_json}"
        )));
    }
    let upload_id = init_json["upload_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| BiliLiveError::Parse("UPOS 响应缺少 upload_id".to_string()))?
        .to_string();

    // Step 3: 并发分块 PUT 上传（支持自动重试、Ctrl+C 中断与实时进度条）
    let total_chunks = total_size.div_ceil(chunk_size) as usize;
    let worker_count = concurrency_limit.clamp(1, 16).min(total_chunks.max(1));
    let next_chunk = AtomicUsize::new(0);
    let uploaded_bytes = AtomicU64::new(0);
    let failed = AtomicBool::new(false);
    let first_err: Mutex<Option<BiliLiveError>> = Mutex::new(None);
    let draw_lock: Mutex<Instant> = Mutex::new(
        Instant::now()
            .checked_sub(Duration::from_millis(200))
            .unwrap_or_else(Instant::now),
    );
    let started = Instant::now();

    let show_progress = !crate::cli::output::is_json()
        && !crate::cli::output::is_quiet()
        && std::io::stderr().is_terminal();
    let use_ansi = show_progress && crate::ui::theme::stderr_color_enabled();
    if show_progress && use_ansi {
        let mut stderr = std::io::stderr();
        let _ = crossterm::execute!(stderr, crossterm::cursor::Hide);
        crate::utils::interrupt::set_stderr_cursor_hidden(true);
    }

    std::thread::scope(|s| {
        for _ in 0..worker_count {
            s.spawn(|| {
                let mut fh = match File::open(file_path) {
                    Ok(f) => f,
                    Err(e) => {
                        failed.store(true, Ordering::SeqCst);
                        let mut g = first_err.lock().unwrap_or_else(|p| p.into_inner());
                        if g.is_none() {
                            *g = Some(BiliLiveError::Io(e));
                        }
                        return;
                    }
                };

                loop {
                    if failed.load(Ordering::SeqCst) || crate::utils::interrupt::is_interrupted() {
                        if crate::utils::interrupt::is_interrupted() {
                            failed.store(true, Ordering::SeqCst);
                            let mut g = first_err.lock().unwrap_or_else(|p| p.into_inner());
                            if g.is_none() {
                                *g = Some(BiliLiveError::Interrupted);
                            }
                        }
                        break;
                    }

                    let chunk_idx = next_chunk.fetch_add(1, Ordering::SeqCst);
                    if chunk_idx >= total_chunks {
                        break;
                    }

                    let start_offset = (chunk_idx as u64) * chunk_size;
                    let end_offset = (start_offset + chunk_size).min(total_size);
                    let part_len = (end_offset - start_offset) as usize;
                    let mut buf = vec![0u8; part_len];

                    if let Err(e) = fh
                        .seek(SeekFrom::Start(start_offset))
                        .and_then(|_| fh.read_exact(&mut buf))
                    {
                        failed.store(true, Ordering::SeqCst);
                        let mut g = first_err.lock().unwrap_or_else(|p| p.into_inner());
                        if g.is_none() {
                            *g = Some(BiliLiveError::Io(e));
                        }
                        break;
                    }

                    let put_q = match serde_urlencoded::to_string([
                        ("partNumber", &(chunk_idx + 1).to_string()),
                        ("uploadId", &upload_id),
                        ("chunk", &chunk_idx.to_string()),
                        ("chunks", &total_chunks.to_string()),
                        ("size", &part_len.to_string()),
                        ("start", &start_offset.to_string()),
                        ("end", &end_offset.to_string()),
                        ("total", &total_size.to_string()),
                    ]) {
                        Ok(q) => q,
                        Err(e) => {
                            failed.store(true, Ordering::SeqCst);
                            let mut g = first_err.lock().unwrap_or_else(|p| p.into_inner());
                            if g.is_none() {
                                *g = Some(BiliLiveError::Parse(e.to_string()));
                            }
                            break;
                        }
                    };
                    let put_url = format!("{target_url}?{put_q}");

                    let mut ok = false;
                    let mut attempt_err: Option<BiliLiveError> = None;
                    for attempt in 0..3 {
                        if crate::utils::interrupt::is_interrupted() {
                            attempt_err = Some(BiliLiveError::Interrupted);
                            break;
                        }
                        let resp = crate::api::client::put_with_timeout(&put_url, 180)
                            .with_header("User-Agent", DEFAULT_USER_AGENT)
                            .with_header("X-Upos-Auth", &auth)
                            .with_header("Content-Type", "application/octet-stream")
                            .with_body(buf.clone())
                            .send();
                        match resp {
                            Ok(r) if r.status_code < 400 => {
                                ok = true;
                                break;
                            }
                            Ok(r) => {
                                attempt_err = Some(BiliLiveError::Api(format!(
                                    "分块 {}/{} 上传失败: HTTP {} {}",
                                    chunk_idx + 1,
                                    total_chunks,
                                    r.status_code,
                                    r.reason_phrase
                                )));
                            }
                            Err(e) => {
                                attempt_err = Some(BiliLiveError::Network(e));
                            }
                        }
                        if attempt < 2 {
                            let _ = crate::utils::interrupt::sleep_interruptible(
                                Duration::from_millis(500 * (attempt as u64 + 1)),
                            );
                        }
                    }

                    if !ok {
                        failed.store(true, Ordering::SeqCst);
                        let mut g = first_err.lock().unwrap_or_else(|p| p.into_inner());
                        if g.is_none() {
                            *g = Some(attempt_err.unwrap_or_else(|| {
                                BiliLiveError::Api("分块上传失败".to_string())
                            }));
                        }
                        break;
                    }

                    let done = uploaded_bytes.fetch_add(part_len as u64, Ordering::SeqCst)
                        + part_len as u64;
                    if show_progress
                        && let Ok(mut last) = draw_lock.try_lock()
                        && (done >= total_size || last.elapsed() >= Duration::from_millis(80))
                    {
                        let secs = started.elapsed().as_secs_f64().max(0.001);
                        let line = render_upload_progress(done, total_size, done as f64 / secs, use_ansi);
                        if use_ansi {
                            eprint!("\r\x1b[K{line}");
                        } else {
                            eprint!("\r{line}");
                        }
                        let _ = std::io::stderr().flush();
                        *last = Instant::now();
                    }
                }
            });
        }
    });

    if show_progress {
        let mut stderr = std::io::stderr();
        if use_ansi {
            let _ = crossterm::execute!(stderr, crossterm::cursor::Show);
            crate::utils::interrupt::set_stderr_cursor_hidden(false);
            eprint!("\r\x1b[K");
        } else {
            eprintln!();
        }
        let _ = stderr.flush();
    }

    if let Some(err) = first_err.into_inner().unwrap_or_else(|p| p.into_inner()) {
        return Err(err);
    }
    crate::utils::interrupt::check_interrupted()?;

    // Step 4: POST 合并所有分片并结束上传
    let finish_q = serde_urlencoded::to_string([
        ("output", "json"),
        ("name", file_name),
        ("profile", UPOS_PROFILE),
        ("uploadId", &upload_id),
        ("biz_id", &biz_id.to_string()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let parts: Vec<Value> = (1..=total_chunks)
        .map(|i| json!({ "partNumber": i, "eTag": "etag" }))
        .collect();
    let finish_body = serde_json::to_string(&json!({ "parts": parts }))?;
    let finish_url = format!("{target_url}?{finish_q}");
    let finish_resp = crate::api::client::post(&finish_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("X-Upos-Auth", &auth)
        .with_header("Content-Type", "application/json; charset=UTF-8")
        .with_body(finish_body)
        .send()?;
    let finish_json: Value =
        crate::api::client::parse_json_response(&finish_resp, "结束 UPOS 上传失败")?;
    if finish_json["OK"].as_i64() != Some(1) {
        return Err(BiliLiveError::Api(format!(
            "合并 UPOS 分片失败: {finish_json}"
        )));
    }

    Ok(UploadedVideoPart {
        filename: server_filename,
        title: part_title,
        desc: String::new(),
        cid: biz_id,
    })
}

/// 上传本地图片作为视频封面（若传入已是 http/https URL 则原样返回）
pub fn upload_cover_image(cookies: &Cookies, cover_arg: &str) -> Result<String> {
    let trimmed = cover_arg.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(trimmed.to_string());
    }

    let path = Path::new(trimmed);
    let meta = std::fs::metadata(path).map_err(|e| {
        BiliLiveError::Input(format!("无法读取封面图片 '{}': {e}", path.display()))
    })?;
    if !meta.is_file() || meta.len() == 0 {
        return Err(BiliLiveError::Input(format!(
            "封面图片无效或为空文件: {}",
            path.display()
        )));
    }
    if meta.len() > MAX_COVER_BYTES {
        return Err(BiliLiveError::Input(format!(
            "封面图片超过 20MB 上限: {}",
            path.display()
        )));
    }

    let bytes = std::fs::read(path)?;
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    };
    let data_uri = format!("data:{mime};base64,{}", general_purpose::STANDARD.encode(&bytes));
    let form = serde_urlencoded::to_string([
        ("csrf", cookies.bili_jct.as_str()),
        ("cover", data_uri.as_str()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/vu/web/cover/up?ts={}",
        crate::api::client::member_api_base_url(),
        now_millis()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://member.bilibili.com/")
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(form)
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&resp, "上传视频封面失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "上传视频封面失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    let cover_url = json["data"]["url"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| BiliLiveError::Parse("上传封面响应缺少 url 字段".to_string()))?;
    Ok(cover_url.to_string())
}

/// 校验并归一化逗号分隔的投稿标签（最多 10 个）
pub fn normalize_tags(raw: &str) -> Result<String> {
    let tags: Vec<&str> = raw
        .split([',', '，'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if tags.is_empty() {
        return Err(BiliLiveError::Input("至少需要提供 1 个非空视频标签 (--tag)".to_string()));
    }
    if tags.len() > 10 {
        return Err(BiliLiveError::Input(format!(
            "B 站单个稿件最多支持 10 个标签，当前提供了 {} 个",
            tags.len()
        )));
    }
    Ok(tags.join(","))
}

/// 提交新视频稿件 (`POST /x/vu/web/add/v3`)
pub fn submit_archive(cookies: &Cookies, params: &ArchiveSubmitParams) -> Result<Value> {
    if params.videos.is_empty() {
        return Err(BiliLiveError::Input("投稿分P列表不能为空".to_string()));
    }
    let title_trimmed = params.title.trim();
    if title_trimmed.is_empty() || title_trimmed.chars().count() > 80 {
        return Err(BiliLiveError::Input(
            "稿件标题长度需在 1～80 个字符之间".to_string(),
        ));
    }
    if !matches!(params.copyright, 1 | 2) {
        return Err(BiliLiveError::Input(
            "--copyright 仅支持 1 (自制) 或 2 (转载)".to_string(),
        ));
    }
    if params.copyright == 2 && params.source.trim().is_empty() {
        return Err(BiliLiveError::Input(
            "转载稿件 (--copyright 2) 必须通过 --source 指定转载来源网址".to_string(),
        ));
    }

    let mut payload = json!({
        "videos": params.videos,
        "cover": params.cover,
        "cover43": "",
        "title": title_trimmed,
        "copyright": params.copyright,
        "source": params.source.trim(),
        "tid": params.tid,
        "tag": params.tag,
        "desc_format_id": 9999,
        "desc": params.desc,
        "recreate": -1,
        "dynamic": params.dynamic,
        "interactive": 0,
        "act_reserve_create": 0,
        "no_disturbance": 0,
        "no_reprint": i32::from(params.copyright == 1 && params.no_reprint),
        "subtitle": {
            "open": 0,
            "lan": ""
        },
        "dolby": 0,
        "lossless_music": 0,
        "up_selection_reply": false,
        "up_close_reply": false,
        "up_close_danmu": false,
        "web_os": 3,
        "csrf": cookies.bili_jct,
    });
    if let Some(dt) = params.dtime {
        payload["dtime"] = json!(dt);
    }

    let body = serde_json::to_string(&payload)?;
    let url = format!(
        "{}/x/vu/web/add/v3?ts={}&csrf={}",
        crate::api::client::member_api_base_url(),
        now_millis(),
        cookies.bili_jct
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://member.bilibili.com/")
        .with_header("Content-Type", "application/json; charset=UTF-8")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&resp, "投递视频稿件失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "投递视频稿件失败 ({}): {}",
            json["code"].as_i64().unwrap_or(-1),
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(json["data"].clone())
}

/// 查询创作中心单个稿件后台详情与各分P转码/审核状态 (`GET /x/vupre/web/archive/view`，对标 `biliup show`)
pub fn get_studio_archive_view(cookies: &Cookies, video_input: &str) -> Result<Value> {
    let target = crate::utils::id::resolve_video(video_input, false)?;
    let query = if let Some(ref bvid) = target.bvid {
        serde_urlencoded::to_string([("bvid", bvid.as_str())])
            .map_err(|e| BiliLiveError::Parse(e.to_string()))?
    } else if let Some(aid) = target.aid {
        serde_urlencoded::to_string([("aid", aid.to_string())])
            .map_err(|e| BiliLiveError::Parse(e.to_string()))?
    } else {
        return Err(BiliLiveError::Input(format!("无效的稿件标识: {video_input}")));
    };

    let url = format!(
        "{}/x/vupre/web/archive/view?{query}",
        crate::api::client::member_api_base_url()
    );
    let resp = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://member.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&resp, "获取创作中心稿件详情失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取创作中心稿件详情失败 ({}): {}",
            json["code"].as_i64().unwrap_or(-1),
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(json["data"].clone())
}

/// 向已有稿件追加新视频分P并提交编辑 (`POST /x/vu/web/edit`，对标 `biliup append`)
pub fn append_archive_parts(
    cookies: &Cookies,
    video_input: &str,
    new_parts: &[UploadedVideoPart],
) -> Result<(Value, usize)> {
    if new_parts.is_empty() {
        return Err(BiliLiveError::Input("待追加的视频分P列表不能为空".to_string()));
    }

    let view = get_studio_archive_view(cookies, video_input)?;
    let arc = &view["archive"];
    let aid = arc["aid"]
        .as_i64()
        .filter(|&a| a > 0)
        .ok_or_else(|| BiliLiveError::Parse("原稿件详情缺少有效 aid".to_string()))?;

    let mut merged_videos: Vec<Value> = view["videos"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    json!({
                        "filename": v["filename"].as_str().unwrap_or(""),
                        "title": v["title"].as_str().unwrap_or(""),
                        "desc": v["desc"].as_str().unwrap_or(""),
                        "cid": v["cid"].as_i64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    for p in new_parts {
        merged_videos.push(json!({
            "filename": p.filename,
            "title": p.title,
            "desc": p.desc,
            "cid": p.cid,
        }));
    }
    let total_parts = merged_videos.len();

    let payload = json!({
        "aid": aid,
        "videos": merged_videos,
        "title": arc["title"].as_str().unwrap_or(""),
        "cover": arc["cover"].as_str().unwrap_or(""),
        "cover43": arc["cover43"].as_str().unwrap_or(""),
        "copyright": arc["copyright"].as_u64().unwrap_or(1),
        "source": arc["source"].as_str().unwrap_or(""),
        "tid": arc["tid"].as_u64().unwrap_or(171),
        "tag": arc["tag"].as_str().unwrap_or(""),
        "desc_format_id": arc["desc_format_id"].as_i64().unwrap_or(9999),
        "desc": arc["desc"].as_str().unwrap_or(""),
        "recreate": arc["recreate"].as_i64().unwrap_or(-1),
        "dynamic": arc["dynamic"].as_str().unwrap_or(""),
        "interactive": arc["interactive"].as_i64().unwrap_or(0),
        "act_reserve_create": 0,
        "no_disturbance": 0,
        "no_reprint": arc["no_reprint"].as_i64().unwrap_or(1),
        "subtitle": {
            "open": 0,
            "lan": ""
        },
        "web_os": 1,
        "csrf": cookies.bili_jct,
    });

    let body = serde_json::to_string(&payload)?;
    let url = format!(
        "{}/x/vu/web/edit?t={}&csrf={}",
        crate::api::client::member_api_base_url(),
        now_millis(),
        cookies.bili_jct
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://member.bilibili.com/")
        .with_header("Content-Type", "application/json; charset=UTF-8")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&resp, "追加稿件分P失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "追加稿件分P失败 ({}): {}",
            json["code"].as_i64().unwrap_or(-1),
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok((json["data"].clone(), total_parts))
}

/// 查询创作中心后台稿件列表 (`GET /x/web/archives`，对标 `biliup list`)
pub fn list_studio_archives(
    cookies: &Cookies,
    status: StudioArchiveStatus,
    page: u32,
    page_size: u32,
) -> Result<Value> {
    let query = serde_urlencoded::to_string([
        ("status", status.as_api_str()),
        ("pn", &page.to_string()),
        ("ps", &page_size.clamp(1, 50).to_string()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let url = format!(
        "{}/x/web/archives?{query}",
        crate::api::client::member_api_base_url()
    );
    let resp = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://member.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&resp, "获取创作中心稿件列表失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取创作中心稿件列表失败 ({}): {}",
            json["code"].as_i64().unwrap_or(-1),
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let mut data = json["data"].clone();
    if data.is_null() {
        data = json!({ "arc_audits": [], "page": { "pn": page, "ps": page_size, "count": 0 } });
    } else if data["arc_audits"].is_null() {
        data["arc_audits"] = json!([]);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_upos_endpoint_handles_protocol_relative_and_strips_ext() {
        let (url, fname) = normalize_upos_endpoint(
            "//upos-cs-upcdntxa.bilivideo.com",
            "upos://ugcfx2lf/n240728ad33h52yqhxbtw51cb06sq9gx.mp4",
        )
        .unwrap();
        assert_eq!(
            url,
            "https://upos-cs-upcdntxa.bilivideo.com/ugcfx2lf/n240728ad33h52yqhxbtw51cb06sq9gx.mp4"
        );
        assert_eq!(fname, "n240728ad33h52yqhxbtw51cb06sq9gx");

        let (local_url, fname2) = normalize_upos_endpoint(
            "http://127.0.0.1:9999/",
            "upos://ugcfx/mock_part_01.flv",
        )
        .unwrap();
        assert_eq!(local_url, "http://127.0.0.1:9999/ugcfx/mock_part_01.flv");
        assert_eq!(fname2, "mock_part_01");
    }

    #[test]
    fn normalize_tags_trims_splits_chinese_commas_and_enforces_ten_limit() {
        assert_eq!(normalize_tags(" Rust , 直播 ，编程 ").unwrap(), "Rust,直播,编程");
        assert!(normalize_tags(" , ， ").is_err());
        assert!(normalize_tags("1,2,3,4,5,6,7,8,9,10,11").is_err());
    }
}
