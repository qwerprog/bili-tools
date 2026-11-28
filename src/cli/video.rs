use crate::cli::args::{
    DanmakuFormat, DownloadDanmakuFormat, SubtitleFormat, VideoCommands, VideoCommentCommands,
    VideoStudioCommands,
};
use crate::cli::output::{is_json, is_quiet, print_json, print_spacer};
use crate::error::{BiliLiveError, Result};
use crate::ui::components::{Cell, Column, InfoCard, print_section_header, print_table};
use crate::ui::theme::{BILI_CYAN, color_enabled, paint_stderr, terminal_width};
use crate::utils::string::{
    format_duration, format_num, format_timestamp, sanitize_filename, truncate_to_width,
};
use crate::{user_info, user_success, user_warning};
use crossterm::style::Color;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

const MAX_EXPLICIT_PAGE_SPAN: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum PageSpec {
    All,
    Pages(Vec<u32>),
}

/// 解析 `-p/--pages` 语法（`1`、`1,3`、`2-4`、`ALL`），在发起媒体请求/调用 ffmpeg 前校验格式
fn parse_page_spec_syntax(spec: &str) -> Result<PageSpec> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(BiliLiveError::Input("分P参数不能为空".to_string()));
    }
    if trimmed.eq_ignore_ascii_case("all") {
        return Ok(PageSpec::All);
    }

    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    for segment in trimmed.split(',') {
        let seg = segment.trim();
        if seg.is_empty() {
            return Err(BiliLiveError::Input(format!("无效的分P参数: {spec}")));
        }
        if let Some((start_s, end_s)) = seg.split_once('-') {
            if end_s.contains('-') {
                return Err(BiliLiveError::Input(format!("无效的分P范围: {seg}")));
            }
            let start: u32 = start_s
                .trim()
                .parse()
                .map_err(|_| BiliLiveError::Input(format!("无效的分P起始编号: {seg}")))?;
            let end: u32 = end_s
                .trim()
                .parse()
                .map_err(|_| BiliLiveError::Input(format!("无效的分P结束编号: {seg}")))?;
            if start == 0 || end == 0 || start > end {
                return Err(BiliLiveError::Input(format!(
                    "无效的分P范围 '{seg}'：起始页码必须 >= 1 且不大于结束页码"
                )));
            }
            if end - start >= MAX_EXPLICIT_PAGE_SPAN {
                return Err(BiliLiveError::Input(format!(
                    "分P范围 '{seg}' 过大（单段最多支持 {MAX_EXPLICIT_PAGE_SPAN} P），全部分P请使用 ALL"
                )));
            }
            for p in start..=end {
                if seen.insert(p) {
                    if pages.len() >= MAX_EXPLICIT_PAGE_SPAN as usize {
                        return Err(BiliLiveError::Input(format!(
                            "分P数量过多（最多支持 {MAX_EXPLICIT_PAGE_SPAN} P），全部分P请使用 ALL"
                        )));
                    }
                    pages.push(p);
                }
            }
        } else {
            let p: u32 = seg
                .parse()
                .map_err(|_| BiliLiveError::Input(format!("无效的分P编号: {seg}")))?;
            if p == 0 {
                return Err(BiliLiveError::Input("分P编号必须大于等于 1".to_string()));
            }
            if seen.insert(p) {
                if pages.len() >= MAX_EXPLICIT_PAGE_SPAN as usize {
                    return Err(BiliLiveError::Input(format!(
                        "分P数量过多（最多支持 {MAX_EXPLICIT_PAGE_SPAN} P），全部分P请使用 ALL"
                    )));
                }
                pages.push(p);
            }
        }
    }

    if pages.is_empty() {
        return Err(BiliLiveError::Input(format!("未解析到有效分P: {spec}")));
    }
    Ok(PageSpec::Pages(pages))
}

fn resolve_page_list(
    info: &Value,
    target_page: Option<u32>,
    spec: Option<&PageSpec>,
) -> Result<Vec<crate::api::video::ResolvedVideoPage>> {
    let total = info["pages"]
        .as_array()
        .map(|a| a.len())
        .filter(|&n| n > 0)
        .unwrap_or(1);

    let nums: Vec<u32> = match spec {
        Some(PageSpec::All) => (1..=total as u32).collect(),
        Some(PageSpec::Pages(list)) => list.clone(),
        None => vec![target_page.unwrap_or(1)],
    };

    let mut resolved = Vec::with_capacity(nums.len());
    for p in nums {
        resolved.push(crate::api::video::extract_page_info(info, p)?);
    }
    Ok(resolved)
}

/// 格式化 SRT 时间戳：`HH:MM:SS,mmm`（先四舍五入至整数毫秒，防止 59.9999 出现 `60,000`）
pub fn format_srt_timestamp(secs: f64) -> String {
    let total_ms = (secs.max(0.0) * 1000.0).round() as u64;
    let h = total_ms / 3_600_000;
    let m = (total_ms / 60_000) % 60;
    let s = (total_ms / 1_000) % 60;
    let ms = total_ms % 1_000;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

fn render_subtitle_srt(body: &[Value]) -> String {
    let mut out = String::new();
    for (idx, cue) in body.iter().enumerate() {
        let from = cue["from"].as_f64().unwrap_or(0.0);
        let to = cue["to"].as_f64().unwrap_or(from);
        let content = cue["content"].as_str().unwrap_or("");
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            idx + 1,
            format_srt_timestamp(from),
            format_srt_timestamp(to),
            content
        ));
    }
    out
}

fn render_subtitle_txt(body: &[Value]) -> String {
    let mut out = String::new();
    for cue in body {
        let content = cue["content"].as_str().unwrap_or("");
        out.push_str(content);
        out.push('\n');
    }
    out
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn write_stdout_broken_pipe_safe(content: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = stdout.write_all(content.as_bytes())
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(e.into());
    }
    if !content.ends_with('\n')
        && let Err(e) = writeln!(stdout)
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(e.into());
    }
    Ok(())
}

/// 上传后回查 `author/list` 的最长等待时间。默认 90 秒（实测新条目约 40 秒才可见）；
/// 仅测试通过 `BT_SUBTITLE_VERIFY_TIMEOUT_SECS` 覆盖，避免失败用例白等。
fn subtitle_verify_timeout_secs() -> u64 {
    std::env::var("BT_SUBTITLE_VERIFY_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(90)
}

/// 回查轮询每轮请求的条数。列表按 `mtime` 倒序，新版本必然在第 1 页内，
/// 因此只查第 1 页，条数取接口上限 50 即可。
const SUBTITLE_VERIFY_PAGE_SIZE: u32 = 50;

fn select_subtitle_track<'a>(tracks: &'a [Value], lang: Option<&str>) -> Result<&'a Value> {
    if tracks.is_empty() {
        return Err(BiliLiveError::Input("该视频分P暂无可用字幕".to_string()));
    }
    let Some(wanted) = lang.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(&tracks[0]);
    };
    let wanted_lower = wanted.to_ascii_lowercase();
    if let Some(exact) = tracks.iter().find(|t| {
        t["lan"]
            .as_str()
            .is_some_and(|l| l.to_ascii_lowercase() == wanted_lower)
    }) {
        return Ok(exact);
    }
    if let Some(prefix) = tracks.iter().find(|t| {
        t["lan"]
            .as_str()
            .is_some_and(|l| l.to_ascii_lowercase().starts_with(&wanted_lower))
    }) {
        return Ok(prefix);
    }
    Err(BiliLiveError::Input(format!(
        "未找到匹配语言 '{wanted}' 的字幕"
    )))
}

const KNOWN_MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "flv", "m4a", "m4s", "ts", "mov", "webm", "mp3", "aac",
];

fn is_known_media_ext(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    KNOWN_MEDIA_EXTENSIONS.contains(&lower.as_str())
}

fn expand_output_template(
    template: Option<&str>,
    resolved: &crate::api::video::ResolvedVideoPage,
    quality: u32,
    codec: &str,
    default_ext: &str,
    multi_page: bool,
) -> PathBuf {
    let clean_title = sanitize_filename(&resolved.title);
    let clean_bvid = sanitize_filename(if resolved.bvid.is_empty() {
        "video"
    } else {
        &resolved.bvid
    });
    let clean_part = sanitize_filename(&resolved.part);
    let page_str = resolved.page.to_string();
    let aid_str = resolved.aid.to_string();
    let qn_str = quality.to_string();

    let raw_tpl = match template {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            if multi_page {
                format!("{{title}}_{{bvid}}_p{{page}}.{default_ext}")
            } else {
                format!("{{title}}_{{bvid}}.{default_ext}")
            }
        }
    };

    let has_page_placeholder = raw_tpl.contains("{p}") || raw_tpl.contains("{page}");
    let tpl_has_ext = Path::new(&raw_tpl)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| !e.contains('{') && !e.contains('}') && is_known_media_ext(e));
    let is_dir_hint =
        raw_tpl.ends_with('/') || raw_tpl.ends_with('\\') || Path::new(&raw_tpl).is_dir();

    let expanded = raw_tpl
        .replace("{title}", &clean_title)
        .replace("{bvid}", &clean_bvid)
        .replace("{aid}", &aid_str)
        .replace("{p_title}", &clean_part)
        .replace("{part}", &clean_part)
        .replace("{page}", &page_str)
        .replace("{p}", &page_str)
        .replace("{quality}", &qn_str)
        .replace("{codec}", codec);

    let mut path = PathBuf::from(&expanded);
    if is_dir_hint {
        let fname = if multi_page {
            format!("{clean_title}_{clean_bvid}_p{page_str}.{default_ext}")
        } else {
            format!("{clean_title}_{clean_bvid}.{default_ext}")
        };
        path.push(fname);
        return path;
    }

    if multi_page && !has_page_placeholder {
        if tpl_has_ext {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("video")
                .to_string();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or(default_ext)
                .to_string();
            path.set_file_name(format!("{stem}_p{page_str}.{ext}"));
        } else {
            let fname = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("video")
                .to_string();
            path.set_file_name(format!("{fname}_p{page_str}.{default_ext}"));
        }
    } else if !tpl_has_ext {
        let fname = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("video")
            .to_string();
        path.set_file_name(format!("{fname}.{default_ext}"));
    }

    path
}

fn stem_sibling_path(base: &Path, suffix: &str) -> PathBuf {
    let has_known_ext = base
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(is_known_media_ext);
    let stem = if has_known_ext {
        base.file_stem().and_then(|s| s.to_str()).unwrap_or("video")
    } else {
        base.file_name().and_then(|s| s.to_str()).unwrap_or("video")
    };
    let mut out = base.to_path_buf();
    out.set_file_name(format!("{stem}{suffix}"));
    out
}

pub fn handle_video(cmd: VideoCommands) -> Result<()> {
    match cmd {
        VideoCommands::Info { video } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let data = crate::api::video::get_video_info(&video, cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let title = data["title"].as_str().unwrap_or("未知标题");
            let bvid = data["bvid"].as_str().unwrap_or("-");
            let aid = data["aid"].as_i64().unwrap_or(0);
            let owner = data["owner"]["name"].as_str().unwrap_or("未知");
            let mid = data["owner"]["mid"].as_i64().unwrap_or(0);
            let duration = data["duration"].as_i64().unwrap_or(0);
            let pubdate = data["pubdate"].as_i64().unwrap_or(0);

            let view = format_num(&data["stat"]["view"]);
            let danmaku = format_num(&data["stat"]["danmaku"]);
            let like = format_num(&data["stat"]["like"]);
            let coin = format_num(&data["stat"]["coin"]);
            let fav = format_num(&data["stat"]["favorite"]);
            let share = format_num(&data["stat"]["share"]);

            let desc = data["desc"].as_str().unwrap_or("").replace('\n', " ");
            let desc = desc.trim();

            let mut card = InfoCard::new("📺 视频详情")
                .with_badge(bvid, BILI_CYAN)
                .add_row(
                    "标题",
                    Cell::bold_colored(title, Color::White),
                )
                .add_plain_row("AV号", format!("av{aid}"))
                .add_plain_row("UP主", format!("{owner} (UID: {mid})"))
                .add_plain_row(
                    "发布",
                    format!(
                        "{} · 时长 {}",
                        format_timestamp(pubdate),
                        format_duration(duration)
                    ),
                )
                .add_row(
                    "数据",
                    Cell::pink(format!(
                        "▶ {view}  ·  弹幕 {danmaku}  ·  赞 {like}  ·  币 {coin}  ·  藏 {fav}  ·  享 {share}"
                    )),
                );

            if !desc.is_empty() {
                card = card.add_row("简介", Cell::muted(desc));
            }

            print_spacer();
            card.print();
            print_spacer();

            if let Some(pages) = data["pages"].as_array()
                && !pages.is_empty()
            {
                print_section_header("分P列表", Some(&format!("共 {} P", pages.len())));
                let cols = [
                    Column::right("P", 5),
                    Column::left("分P标题", 38),
                    Column::right("时长", 10),
                    Column::right("CID", 14),
                ];
                let rows: Vec<Vec<Cell>> = pages
                    .iter()
                    .map(|p| {
                        let p_num = p["page"].as_u64().unwrap_or(0);
                        let part = p["part"].as_str().unwrap_or("-");
                        let dur = format_duration(p["duration"].as_i64().unwrap_or(0));
                        let cid = p["cid"].as_i64().unwrap_or(0);
                        vec![
                            Cell::cyan(format!("P{p_num}")),
                            Cell::plain(truncate_to_width(part, 38)),
                            Cell::plain(dur),
                            Cell::muted(cid.to_string()),
                        ]
                    })
                    .collect();
                print_table(&cols, &rows);
                print_spacer();
            }
        }

        VideoCommands::Streams { video, page } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let (target_page, info) =
                crate::api::video::resolve_video_context(&video, cookies.as_ref())?;
            let page_num = page.or(target_page).unwrap_or(1);
            let resolved = crate::api::video::extract_page_info(&info, page_num)?;
            let is_pgc = info["is_pgc"].as_bool() == Some(true);
            let playurl = if is_pgc {
                crate::api::video::get_pgc_playurl(
                    &resolved.bvid,
                    resolved.aid,
                    resolved.cid,
                    127,
                    cookies.as_ref(),
                )?
            } else {
                crate::api::video::get_video_playurl(
                    &resolved.bvid,
                    resolved.aid,
                    resolved.cid,
                    127,
                    cookies.as_ref(),
                )?
            };
            let (video_streams, audio_streams) = crate::api::video::parse_dash_streams(&playurl);
            let durl = crate::api::video::parse_durl_segments(&playurl);

            if is_json() {
                print_json(&json!({
                    "bvid": resolved.bvid,
                    "aid": resolved.aid,
                    "cid": resolved.cid,
                    "page": resolved.page,
                    "part": resolved.part,
                    "quality": playurl["quality"],
                    "format": playurl["format"],
                    "support_formats": playurl["support_formats"],
                    "dash": playurl["dash"],
                    "durl": playurl["durl"],
                    "video_streams": video_streams,
                    "audio_streams": audio_streams,
                    "durl_segments": durl,
                }))?;
                return Ok(());
            }

            print_spacer();
            print_section_header(
                "可用视频流",
                Some(&format!(
                    "{} · P{} · cid={}",
                    resolved.bvid, resolved.page, resolved.cid
                )),
            );
            if !video_streams.is_empty() {
                let cols = [
                    Column::right("QN", 5),
                    Column::left("编码", 18),
                    Column::left("分辨率", 13),
                    Column::right("帧率", 8),
                    Column::right("码率(kbps)", 12),
                ];
                let rows: Vec<Vec<Cell>> = video_streams
                    .iter()
                    .map(|s| {
                        vec![
                            Cell::cyan(s.id.to_string()),
                            Cell::plain(format!(
                                "{} ({})",
                                crate::api::video::codec_label(s),
                                truncate_to_width(&s.codecs, 11)
                            )),
                            Cell::plain(format!("{}x{}", s.width, s.height)),
                            Cell::plain(if s.frame_rate.is_empty() {
                                "-"
                            } else {
                                &s.frame_rate
                            }),
                            Cell::muted((s.bandwidth / 1000).to_string()),
                        ]
                    })
                    .collect();
                print_table(&cols, &rows);
                print_spacer();
            }

            if !audio_streams.is_empty() {
                print_section_header(
                    "可用音频流",
                    Some(&format!("共 {} 条", audio_streams.len())),
                );
                let cols = [
                    Column::right("ID", 7),
                    Column::left("编码", 16),
                    Column::left("MIME", 14),
                    Column::right("码率(kbps)", 12),
                ];
                let rows: Vec<Vec<Cell>> = audio_streams
                    .iter()
                    .map(|s| {
                        vec![
                            Cell::cyan(s.id.to_string()),
                            Cell::plain(if s.codecs.is_empty() { "-" } else { &s.codecs }),
                            Cell::plain(if s.mime_type.is_empty() {
                                "-"
                            } else {
                                &s.mime_type
                            }),
                            Cell::muted((s.bandwidth / 1000).to_string()),
                        ]
                    })
                    .collect();
                print_table(&cols, &rows);
                print_spacer();
            }

            if !durl.is_empty() {
                print_section_header("MP4/FLV 分段流", Some(&format!("共 {} 段", durl.len())));
                let cols = [
                    Column::right("分段", 6),
                    Column::right("大小(MB)", 10),
                    Column::right("时长", 10),
                ];
                let rows: Vec<Vec<Cell>> = durl
                    .iter()
                    .map(|d| {
                        vec![
                            Cell::cyan(d.order.to_string()),
                            Cell::plain(format!("{:.2}", d.size as f64 / (1024.0 * 1024.0))),
                            Cell::plain(format_duration((d.length / 1000) as i64)),
                        ]
                    })
                    .collect();
                print_table(&cols, &rows);
                if !is_quiet() {
                    print_spacer();
                    let mut stdout = std::io::stdout().lock();
                    for d in &durl {
                        if writeln!(stdout, "  [分段 {}] {}", d.order, d.url).is_err() {
                            break;
                        }
                    }
                }
                print_spacer();
            } else if (!video_streams.is_empty() || !audio_streams.is_empty()) && !is_quiet() {
                // 展示首选 DASH 视频与音频流的完整未截断直链，方便外部播放器或调试工具直接使用
                print_section_header(
                    "首选直链地址 (未截断)",
                    Some("需携带 Referer: https://www.bilibili.com/"),
                );
                let mut stdout = std::io::stdout().lock();
                if let Some(best_v) =
                    crate::api::video::select_dash_video(&video_streams, 127, "avc")
                    && !best_v.base_url.is_empty()
                {
                    let _ = writeln!(
                        stdout,
                        "  [视频 qn={} {} {}x{}]\n  {}",
                        best_v.id,
                        crate::api::video::codec_label(&best_v),
                        best_v.width,
                        best_v.height,
                        best_v.base_url
                    );
                }
                if let Some(best_a) =
                    crate::api::video::select_dash_audio_with_quality(&audio_streams, None)
                    && !best_a.base_url.is_empty()
                {
                    let _ = writeln!(
                        stdout,
                        "  [音频 id={} {}]\n  {}",
                        best_a.id, best_a.codecs, best_a.base_url
                    );
                }
                print_spacer();
            }

            if video_streams.is_empty() && audio_streams.is_empty() && durl.is_empty() {
                user_info!("未获取到可用的视频或音频流");
            }
        }

        VideoCommands::Subtitle {
            video,
            page,
            lang,
            format,
            output,
        } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let (target_page, info) =
                crate::api::video::resolve_video_context(&video, cookies.as_ref())?;
            let page_num = page.or(target_page).unwrap_or(1);
            let resolved = crate::api::video::extract_page_info(&info, page_num)?;

            let v2 = crate::api::video::get_player_v2(
                &resolved.bvid,
                resolved.aid,
                resolved.cid,
                cookies.as_ref(),
            )?;
            let tracks = v2["subtitle"]["subtitles"]
                .as_array()
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            if tracks.is_empty() {
                let need_login = v2["need_login_subtitle"].as_bool().unwrap_or(false);
                if need_login && cookies.is_none() {
                    return Err(BiliLiveError::Auth(
                        "该视频字幕需要登录后才能获取，请先执行 `bit auth login`".to_string(),
                    ));
                }
                return Err(BiliLiveError::Input("该视频分P暂无可用字幕".to_string()));
            }

            let track = select_subtitle_track(tracks, lang.as_deref())?;
            let sub_url = track["subtitle_url"].as_str().unwrap_or("");
            if sub_url.trim().is_empty() {
                return Err(BiliLiveError::Api("所选字幕的下载地址为空".to_string()));
            }
            let lan = track["lan"].as_str().unwrap_or("");
            let lan_doc = track["lan_doc"].as_str().unwrap_or(lan);

            let sub_json = crate::api::video::fetch_subtitle_json(sub_url)?;
            let body = sub_json["body"].as_array().cloned().unwrap_or_default();

            let content = match format {
                SubtitleFormat::Srt => render_subtitle_srt(&body),
                SubtitleFormat::Txt => render_subtitle_txt(&body),
                SubtitleFormat::Json => serde_json::to_string_pretty(&sub_json)?,
            };

            if let Some(path) = output {
                ensure_parent_dir(&path)?;
                std::fs::write(&path, content.as_bytes())?;
                if is_json() {
                    print_json(&json!({
                        "bvid": resolved.bvid,
                        "aid": resolved.aid,
                        "cid": resolved.cid,
                        "page": resolved.page,
                        "lan": lan,
                        "lan_doc": lan_doc,
                        "count": body.len(),
                        "format": format.as_str(),
                        "file": path.display().to_string(),
                    }))?;
                } else {
                    user_success!(
                        "字幕已保存 ({}, {} 条): {}",
                        lan_doc,
                        body.len(),
                        path.display()
                    );
                }
            } else if is_json() {
                match format {
                    SubtitleFormat::Json => {
                        print_json(&json!({
                            "bvid": resolved.bvid,
                            "aid": resolved.aid,
                            "cid": resolved.cid,
                            "page": resolved.page,
                            "lan": lan,
                            "lan_doc": lan_doc,
                            "count": body.len(),
                            "body": body,
                        }))?;
                    }
                    SubtitleFormat::Srt | SubtitleFormat::Txt => {
                        print_json(&json!({
                            "bvid": resolved.bvid,
                            "aid": resolved.aid,
                            "cid": resolved.cid,
                            "page": resolved.page,
                            "lan": lan,
                            "lan_doc": lan_doc,
                            "count": body.len(),
                            "format": format.as_str(),
                            "content": content,
                        }))?;
                    }
                }
            } else {
                write_stdout_broken_pipe_safe(&content)?;
            }
        }

        VideoCommands::SubtitleUpload {
            video,
            file,
            lang,
            page,
            draft,
            sign,
            no_verify,
            dry_run,
        } => {
            // 语言码优先归一化：非法值必须在任何网络请求之前失败
            let lang_input = lang.clone();
            let normalized_lang = crate::api::subtitle::normalize_lan(&lang)?;

            let is_srt = file
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("srt"));
            if !is_srt {
                return Err(BiliLiveError::Input(format!(
                    "当前仅支持 .srt 字幕文件: {}",
                    file.display()
                )));
            }
            let text = std::fs::read_to_string(&file).map_err(BiliLiveError::Io)?;
            let (bcc, stats) = crate::api::subtitle::srt_to_bcc(&text)?;
            let entries = bcc.body.len();
            let first_from = bcc.body.first().map(|cue| cue.from).unwrap_or(0.0);
            let last_to = bcc.body.last().map(|cue| cue.to).unwrap_or(0.0);

            if dry_run {
                // 离线验收路径：只做解析与转换，不发起任何请求，也不要求登录
                let parsed = crate::utils::id::parse_input(&video, true)?;
                let (bvid, aid, url_page) = match parsed.kind {
                    crate::utils::id::IdKind::Video => {
                        (parsed.bvid.clone(), parsed.aid, parsed.page)
                    }
                    crate::utils::id::IdKind::Numeric => {
                        (None, parsed.id.parse::<i64>().ok(), None)
                    }
                    crate::utils::id::IdKind::ShortLink => {
                        return Err(BiliLiveError::Input(
                            "--dry-run 不联网，无法解析 b23.tv 短链接；请改用 BV 号或完整视频 URL"
                                .to_string(),
                        ));
                    }
                    _ => {
                        return Err(BiliLiveError::Input(format!(
                            "字幕上传仅支持普通投稿视频，不接受番剧/直播等标识: {video}"
                        )));
                    }
                };
                let page_num = page.or(url_page).unwrap_or(1);

                if is_json() {
                    print_json(&json!({
                        "dry_run": true,
                        "bvid": bvid,
                        "aid": aid,
                        "cid": Value::Null,
                        "page": page_num,
                        "lang": normalized_lang,
                        "lang_input": lang_input,
                        "submit": !draft,
                        "draft": draft,
                        "sign": sign,
                        "entries": entries,
                        "parsed": stats.parsed,
                        "kept": stats.kept,
                        "skipped": stats.skipped,
                        "first_from": first_from,
                        "last_to": last_to,
                        "bcc": bcc,
                    }))?;
                } else {
                    user_success!("SRT→BCC 转换完成（--dry-run 不联网、不提交）");
                    user_info!(
                        "条目: 解析 {} / 保留 {} / 丢弃 {}",
                        stats.parsed,
                        stats.kept,
                        stats.skipped
                    );
                    user_info!("时间轴: {:.2}s .. {:.2}s", first_from, last_to);
                    user_info!(
                        "目标: bvid={} page=P{} lang={}（输入 {}）",
                        bvid.as_deref().unwrap_or("-"),
                        page_num,
                        normalized_lang,
                        lang_input
                    );
                    user_info!("正式提交时会联网解析该分P的 cid 并上传；dry-run 不校验稿件是否存在");
                    write_stdout_broken_pipe_safe(&serde_json::to_string(&bcc)?)?;
                }
                return Ok(());
            }

            let cookies = crate::auth::ensure_csrf()?;
            let (target_page, info) =
                crate::api::video::resolve_video_context(&video, Some(&cookies))?;
            if info["is_pgc"].as_bool() == Some(true) {
                return Err(BiliLiveError::Input(
                    "字幕上传仅支持普通投稿视频稿件，番剧/影视稿件暂不支持".to_string(),
                ));
            }
            let page_num = page.or(target_page).unwrap_or(1);
            let resolved = crate::api::video::extract_page_info(&info, page_num)?;

            // draft/save 的响应体不可信：先记录已有版本 ID，提交后回查 author/list 确认新版本真的落库。
            // `--no-verify` 供大批量上传使用：跳过回查（每条会扫描全账号字幕列表），由调用方统一复核。
            let before_ids: HashSet<String> = if no_verify {
                HashSet::new()
            } else {
                let (before, _) = crate::api::subtitle::list_all_subtitles(&cookies, 50)?;
                before
                    .iter()
                    .filter_map(|item| item["id_str"].as_str())
                    .map(str::to_string)
                    .collect()
            };

            let response = crate::api::subtitle::submit_subtitle(
                &cookies,
                resolved.cid,
                &resolved.bvid,
                &normalized_lang,
                &bcc,
                !draft,
                sign,
            )?;

            let verified: Option<Value> = if no_verify {
                if !is_json() {
                    user_warning!(
                        "已按 --no-verify 跳过字幕列表回查；请稍后用 `bit video subtitle-list` 复核是否落库"
                    );
                }
                None
            } else {
                // author/list 是搜索索引，新建字幕通常要几十秒才可见（实测约 40 秒），
                // 所以这里做有上限的轮询，既不能立即误报失败，也不能无限等待。
                // 列表按 mtime 倒序返回，新版本必然排在首页，因此每轮只查第 1 页：
                // 账号字幕较多时，全量翻页的轮询会把请求量放大一个数量级。
                let verify_timeout = subtitle_verify_timeout_secs();
                let poll_interval = std::time::Duration::from_secs(3);
                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_secs(verify_timeout);
                if !is_json() {
                    user_info!(
                        "正在回查字幕列表确认新版本已落库（服务端索引通常有几十秒延迟，最多等待 {} 秒）",
                        verify_timeout
                    );
                }
                let mut verified: Option<Value> = None;
                let mut last_error: Option<BiliLiveError> = None;
                let mut polls = 0usize;
                loop {
                    // 用真实时间窗口而非「次数 × 间隔」控制超时：单次请求耗时也算在预算内
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    match crate::api::subtitle::list_my_subtitles(
                        &cookies,
                        1,
                        SUBTITLE_VERIFY_PAGE_SIZE,
                        None,
                    ) {
                        Ok(data) => {
                            polls += 1;
                            verified = data["subtitles"]
                                .as_array()
                                .map(|v| v.as_slice())
                                .unwrap_or(&[])
                                .iter()
                                .find(|item| {
                                    item["oid"].as_i64() == Some(resolved.cid)
                                        && item["lan"].as_str() == Some(normalized_lang.as_str())
                                        && item["id_str"]
                                            .as_str()
                                            .is_some_and(|id| !before_ids.contains(id))
                                })
                                .cloned();
                            if verified.is_some() {
                                break;
                            }
                        }
                        // 轮询期间的瞬时网络/接口抖动不该让整次上传失败：记录最近一次错误后继续轮询
                        Err(err) => last_error = Some(err),
                    }
                    crate::utils::interrupt::sleep_interruptible(poll_interval)?;
                }
                let Some(verified) = verified else {
                    // 全程一次都没查成功时直接暴露底层错误，不把网络问题伪装成「没落库」
                    if polls == 0
                        && let Some(err) = last_error
                    {
                        return Err(err);
                    }
                    let mut message = format!(
                        "提交接口返回成功，但 {timeout} 秒内未在字幕列表中确认到 {bvid} P{page}（cid={cid}, lan={lan}）的新条目；\
                         为避免误报成功，本次结果按失败处理，请用 `bit video subtitle-list {bvid} -p {page}` 复核",
                        timeout = verify_timeout,
                        bvid = resolved.bvid,
                        page = resolved.page,
                        cid = resolved.cid,
                        lan = normalized_lang,
                    );
                    if let Some(err) = last_error {
                        message.push_str(&format!("；轮询期间最近一次查询错误：{err}"));
                    }
                    return Err(BiliLiveError::Api(message));
                };
                Some(verified)
            };
            let verified_id = verified
                .as_ref()
                .and_then(|v| v["id_str"].as_str())
                .unwrap_or("")
                .to_string();
            let verified_status = verified
                .as_ref()
                .and_then(|v| v["status"].as_i64())
                .unwrap_or(0);
            let verified_ok = verified.is_some();

            if is_json() {
                print_json(&json!({
                    "dry_run": false,
                    "bvid": resolved.bvid,
                    "aid": resolved.aid,
                    "cid": resolved.cid,
                    "page": resolved.page,
                    "part": resolved.part,
                    "lang": normalized_lang,
                    "lang_input": lang_input,
                    "submit": !draft,
                    "draft": draft,
                    "sign": sign,
                    "entries": entries,
                    "parsed": stats.parsed,
                    "kept": stats.kept,
                    "skipped": stats.skipped,
                    "first_from": first_from,
                    "last_to": last_to,
                    "verified": verified_ok,
                    "verified_id": if verified_ok { Value::String(verified_id.clone()) } else { Value::Null },
                    "verified_status": if verified_ok { json!(verified_status) } else { Value::Null },
                    "verified_status_text": if verified_ok {
                        Value::String(crate::api::subtitle::status_text(verified_status).to_string())
                    } else {
                        Value::Null
                    },
                    "response": response,
                }))?;
            } else {
                if draft {
                    user_success!(
                        "字幕草稿已保存 (bvid={}, P{} cid={}, {} 条, 语言 {})",
                        resolved.bvid,
                        resolved.page,
                        resolved.cid,
                        entries,
                        normalized_lang
                    );
                } else {
                    user_success!(
                        "字幕已提交审核 (bvid={}, P{} cid={}, {} 条, 语言 {})",
                        resolved.bvid,
                        resolved.page,
                        resolved.cid,
                        entries,
                        normalized_lang
                    );
                }
                user_info!(
                    "条目: 解析 {} / 保留 {} / 丢弃 {}，时间轴 {:.2}s .. {:.2}s",
                    stats.parsed,
                    stats.kept,
                    stats.skipped,
                    first_from,
                    last_to
                );
                if verified_ok {
                    user_info!(
                        "语言: 输入 {} → 归一化为 {}；新版本 ID {}（当前状态：{}）",
                        lang_input,
                        normalized_lang,
                        verified_id,
                        crate::api::subtitle::status_text(verified_status)
                    );
                } else {
                    user_info!(
                        "语言: 输入 {} → 归一化为 {}；已跳过回查，落库状态未知",
                        lang_input,
                        normalized_lang
                    );
                }
                user_info!("这是新增一个版本，同一分P同语言的旧版本仍保留在字幕列表中");
                user_info!("审核通常需要几分钟：登录态可立即看到；匿名播放器要等审核通过后才出现，暂时看不到不是失败");
            }
        }

        VideoCommands::SubtitleList {
            video,
            page,
            status,
            page_num,
            page_size,
        } => {
            let cookies = crate::auth::ensure_login()?;

            let (scope, cid, bvid, items, matched_total, account_total) = match video {
                Some(video) => {
                    let (target_page, info) =
                        crate::api::video::resolve_video_context(&video, Some(&cookies))?;
                    let page_num = page.or(target_page).unwrap_or(1);
                    let resolved = crate::api::video::extract_page_info(&info, page_num)?;

                    let (all, account_total) =
                        crate::api::subtitle::list_all_subtitles(&cookies, page_size)?;
                    let matched: Vec<Value> = all
                        .iter()
                        .filter(|item| item["oid"].as_i64() == Some(resolved.cid))
                        .filter(|item| {
                            status.matches(item["status"].as_i64().unwrap_or(0))
                        })
                        .cloned()
                        .collect();
                    let matched_total = matched.len() as i64;
                    (
                        "page",
                        Some(resolved.cid),
                        Some(resolved.bvid),
                        matched,
                        matched_total,
                        account_total,
                    )
                }
                None => {
                    // 账号范围：先翻完所有分页再按状态过滤，最后在过滤结果上分页。
                    // 只取 `--list-page` 指定的一页再本地过滤，会漏掉后续分页里的草稿/已删除记录，
                    // 查询结果因此不完整；这里用「按需翻页过滤」保证结果集完整。
                    let (all, account_total) =
                        crate::api::subtitle::list_all_subtitles(&cookies, page_size)?;
                    let matched: Vec<Value> = all
                        .iter()
                        .filter(|item| status.matches(item["status"].as_i64().unwrap_or(0)))
                        .cloned()
                        .collect();
                    let matched_total = matched.len() as i64;
                    let offset = (page_num.saturating_sub(1) as usize) * page_size as usize;
                    let page_items: Vec<Value> = matched
                        .into_iter()
                        .skip(offset)
                        .take(page_size as usize)
                        .collect();
                    (
                        "account",
                        None,
                        None,
                        page_items,
                        matched_total,
                        account_total,
                    )
                }
            };

            let subtitles: Vec<Value> = items
                .iter()
                .map(|item| {
                    let item_status = item["status"].as_i64().unwrap_or(0);
                    json!({
                        "id_str": item["id_str"],
                        "oid": item["oid"],
                        "lan": item["lan"],
                        "lan_doc": item["lan_doc"],
                        "status": item_status,
                        "status_text": crate::api::subtitle::status_text(item_status),
                        "mtime": item["mtime"],
                        "bvid": item["bvid"],
                        "archive_name": item["archive_name"],
                        "video_name": item["video_name"],
                    })
                })
                .collect();

            if is_json() {
                print_json(&json!({
                    "scope": scope,
                    "cid": cid,
                    "bvid": bvid,
                    "total": subtitles.len(),
                    "matched_total": matched_total,
                    "account_total": account_total,
                    "subtitles": subtitles,
                }))?;
                return Ok(());
            }

            if subtitles.is_empty() {
                user_info!("未查询到符合条件的字幕");
                return Ok(());
            }

            let subtitle_label = match scope {
                "page" => format!(
                    "分P cid={}",
                    cid.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string())
                ),
                _ => format!("全账号 · 第 {page_num} 页"),
            };
            print_spacer();
            print_section_header(
                "字幕列表",
                Some(&format!("{subtitle_label} · {} 条", subtitles.len())),
            );
            let columns = [
                Column::left("字幕ID", 19),
                Column::left("语言", 12),
                Column::left("状态", 8),
                Column::left("更新时间", 19),
                Column::left("稿件 / 分P视频", 36),
            ];
            let rows: Vec<Vec<Cell>> = items
                .iter()
                .map(|item| {
                    let item_status = item["status"].as_i64().unwrap_or(0);
                    let lan = item["lan"].as_str().unwrap_or("-");
                    let lan_doc = item["lan_doc"].as_str().unwrap_or(lan);
                    let id = item["id_str"].as_str().unwrap_or("-");
                    let mtime = item["mtime"].as_i64().unwrap_or(0);
                    let title = item["archive_name"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| item["video_name"].as_str())
                        .unwrap_or("-");
                    let status_cell = match item_status {
                        5 => Cell::pink(crate::api::subtitle::status_text(item_status)),
                        1 => Cell::cyan(crate::api::subtitle::status_text(item_status)),
                        _ => Cell::muted(crate::api::subtitle::status_text(item_status)),
                    };
                    vec![
                        Cell::plain(id),
                        Cell::plain(format!("{lan} ({lan_doc})")),
                        status_cell,
                        Cell::muted(format_timestamp(mtime)),
                        Cell::muted(truncate_to_width(title, 36)),
                    ]
                })
                .collect();
            print_table(&columns, &rows);
            print_spacer();
            if scope == "page" {
                user_info!(
                    "账号字幕总数 {}；同一 (分P, 语言) 的多条记录是不同版本，删除只是软删除（status=4）",
                    account_total
                );
            } else {
                user_info!(
                    "账号字幕总数 {}；当前过滤条件匹配 {} 条，本页显示 {} 条",
                    account_total,
                    matched_total,
                    subtitles.len()
                );
                user_info!("同一 (分P, 语言) 的多条记录是不同版本，删除只是软删除（status=4）");
            }
        }

        VideoCommands::SubtitleDelete { video, ids, page } => {
            // 非交互模式必须在任何网络请求之前完成确认：--json 或非 TTY 下必须显式 -y
            let stdin_is_tty = std::io::stdin().is_terminal();
            if is_json() || !stdin_is_tty {
                crate::cli::output::confirm_action_with_tty(
                    "确认删除这些字幕？(软删除，status 将变为 4)",
                    stdin_is_tty,
                )?;
            }

            let mut requested: Vec<String> = Vec::new();
            for id in &ids {
                let trimmed = id.trim();
                if trimmed.is_empty() {
                    return Err(BiliLiveError::Input("字幕 ID 不能为空".to_string()));
                }
                if !requested.iter().any(|existing| existing == trimmed) {
                    requested.push(trimmed.to_string());
                }
            }

            let cookies = crate::auth::ensure_csrf()?;
            let (target_page, info) =
                crate::api::video::resolve_video_context(&video, Some(&cookies))?;
            if info["is_pgc"].as_bool() == Some(true) {
                return Err(BiliLiveError::Input(
                    "字幕删除仅支持普通投稿视频稿件，番剧/影视稿件暂不支持".to_string(),
                ));
            }
            let page_num = page.or(target_page).unwrap_or(1);
            let resolved = crate::api::video::extract_page_info(&info, page_num)?;

            let (all, _) = crate::api::subtitle::list_all_subtitles(&cookies, 50)?;
            let page_entries: Vec<&Value> = all
                .iter()
                .filter(|item| item["oid"].as_i64() == Some(resolved.cid))
                .collect();

            let mut targets: Vec<&Value> = Vec::new();
            for id in &requested {
                let Some(entry) = page_entries
                    .iter()
                    .find(|item| item["id_str"].as_str() == Some(id.as_str()))
                else {
                    return Err(BiliLiveError::Input(format!(
                        "字幕 {id} 不属于 {} P{}（cid={}），未执行任何删除",
                        resolved.bvid, resolved.page, resolved.cid
                    )));
                };
                targets.push(entry);
            }

            if !is_json() && !crate::cli::output::is_yes() {
                print_spacer();
                print_section_header(
                    "即将删除的字幕",
                    Some(&format!(
                        "{} P{} · cid={}",
                        resolved.bvid, resolved.page, resolved.cid
                    )),
                );
                let columns = [
                    Column::left("字幕ID", 19),
                    Column::left("语言", 12),
                    Column::left("状态", 8),
                ];
                let rows: Vec<Vec<Cell>> = targets
                    .iter()
                    .map(|item| {
                        let item_status = item["status"].as_i64().unwrap_or(0);
                        let lan = item["lan"].as_str().unwrap_or("-");
                        let lan_doc = item["lan_doc"].as_str().unwrap_or(lan);
                        vec![
                            Cell::plain(item["id_str"].as_str().unwrap_or("-")),
                            Cell::plain(format!("{lan} ({lan_doc})")),
                            Cell::muted(crate::api::subtitle::status_text(item_status)),
                        ]
                    })
                    .collect();
                print_table(&columns, &rows);
                print_spacer();
                let confirmed = crate::cli::output::confirm_action(&format!(
                    "确认删除以上 {} 条字幕？（软删除，status 将变为 4）",
                    targets.len()
                ))?;
                if !confirmed {
                    user_warning!("已取消删除");
                    return Ok(());
                }
            }

            let mut deleted: Vec<String> = Vec::new();
            for (idx, id) in requested.iter().enumerate() {
                // 连发会被服务端静默吞掉，逐个删除之间至少间隔 1 秒。
                // 用可中断睡眠：Ctrl+C 时立刻返回 Interrupted 而不是等满 1 秒。
                if idx > 0 {
                    crate::utils::interrupt::sleep_interruptible(
                        std::time::Duration::from_millis(1000),
                    )?;
                }
                crate::api::subtitle::delete_subtitle(&cookies, resolved.cid, id)?;
                deleted.push(id.clone());
            }

            if is_json() {
                print_json(&json!({
                    "cid": resolved.cid,
                    "bvid": resolved.bvid,
                    "page": resolved.page,
                    "deleted": deleted,
                    "soft_delete": true,
                }))?;
            } else {
                user_success!(
                    "已删除 {} 条字幕 (bvid={}, P{}, cid={})",
                    deleted.len(),
                    resolved.bvid,
                    resolved.page,
                    resolved.cid
                );
                user_warning!(
                    "这是软删除：条目 status 先变为 4，短时间内仍会出现在 `subtitle-list` 中，之后由服务端从列表移除"
                );
                user_info!(
                    "可用 `bit video subtitle-list {} -p {} --status deleted` 在删除后的一段时间内复核",
                    resolved.bvid,
                    resolved.page
                );
            }
        }

        VideoCommands::Summary { video, page } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let (target_page, info) =
                crate::api::video::resolve_video_context(&video, cookies.as_ref())?;
            let page_num = page.or(target_page).unwrap_or(1);
            let resolved = crate::api::video::extract_page_info(&info, page_num)?;

            let data = crate::api::video::get_video_summary(
                &resolved.bvid,
                resolved.aid,
                resolved.cid,
                resolved.up_mid,
                cookies.as_ref(),
            )?;

            let inner_code = data["code"].as_i64().unwrap_or(-1);
            let model_result = &data["model_result"];
            let available = inner_code == 0 && !model_result.is_null();

            if is_json() {
                print_json(&json!({
                    "bvid": resolved.bvid,
                    "aid": resolved.aid,
                    "cid": resolved.cid,
                    "page": resolved.page,
                    "available": available,
                    "code": data["code"],
                    "stid": data["stid"],
                    "status": data["status"],
                    "like_num": data["like_num"],
                    "dislike_num": data["dislike_num"],
                    "model_result": model_result,
                }))?;
                return Ok(());
            }

            if !available {
                let stid = data["stid"].as_str().unwrap_or("");
                let reason = match inner_code {
                    1 if stid == "0" => "AI 总结正在生成队列中，请稍后再试",
                    1 => "该视频无语音内容或暂未生成 AI 总结",
                    _ => "该视频不支持或暂无 AI 总结",
                };
                user_info!("{reason} (code={inner_code})");
                return Ok(());
            }

            let summary_text = model_result["summary"].as_str().unwrap_or("");
            let card = InfoCard::new("🤖 视频 AI 总结")
                .with_badge(&resolved.bvid, BILI_CYAN)
                .add_plain_row("分P", format!("P{} · {}", resolved.page, resolved.part))
                .add_row(
                    "摘要",
                    Cell::plain(if summary_text.is_empty() {
                        "-"
                    } else {
                        summary_text
                    }),
                );

            print_spacer();
            card.print();
            print_spacer();

            if let Some(outline) = model_result["outline"].as_array()
                && !outline.is_empty()
                && !crate::cli::output::is_quiet()
            {
                print_section_header("内容大纲", Some(&format!("共 {} 节", outline.len())));
                print_spacer();
                let color = crate::ui::theme::color_enabled();
                let wrap_w = crate::ui::theme::terminal_width()
                    .saturating_sub(14)
                    .max(36);
                for (idx, sec) in outline.iter().enumerate() {
                    let sec_ts = format_duration(sec["timestamp"].as_i64().unwrap_or(0));
                    let sec_title = sec["title"].as_str().unwrap_or("-");
                    if color {
                        use crossterm::style::Stylize;
                        println!(
                            "  {} {} {}",
                            format!("{}.", idx + 1)
                                .with(crate::ui::theme::BILI_PINK)
                                .bold(),
                            format!("[{sec_ts}]")
                                .with(crate::ui::theme::BILI_CYAN)
                                .bold(),
                            sec_title.bold()
                        );
                    } else {
                        println!("  {}. [{sec_ts}] {sec_title}", idx + 1);
                    }
                    if let Some(arr) = sec["part_outline"].as_array() {
                        for p in arr {
                            let pt_ts = format_duration(p["timestamp"].as_i64().unwrap_or(0));
                            let pt_content = p["content"].as_str().unwrap_or("").trim();
                            if pt_content.is_empty() {
                                continue;
                            }
                            let lines = crate::ui::components::wrap_to_width(pt_content, wrap_w);
                            for (li, line) in lines.iter().enumerate() {
                                if li == 0 {
                                    if color {
                                        use crossterm::style::Stylize;
                                        println!(
                                            "     {} {}  {}",
                                            "•".dark_grey(),
                                            pt_ts.as_str().dark_grey(),
                                            line
                                        );
                                    } else {
                                        println!("     • {pt_ts}  {line}");
                                    }
                                } else {
                                    println!("              {line}");
                                }
                            }
                        }
                    }
                    if idx + 1 < outline.len() {
                        print_spacer();
                    }
                }
                print_spacer();
            }
        }

        VideoCommands::Tags { video } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let tags = crate::api::video::get_video_tags(&video, cookies.as_ref())?;

            if is_json() {
                print_json(&tags)?;
                return Ok(());
            }

            if tags.is_empty() {
                user_info!("该视频暂无标签");
                return Ok(());
            }

            print_spacer();
            print_section_header("视频标签", Some(&format!("共 {} 个", tags.len())));
            let cols = [
                Column::right("ID", 10),
                Column::left("标签名称", 26),
                Column::left("类型", 14),
                Column::right("订阅/赞", 10),
            ];
            let rows: Vec<Vec<Cell>> = tags
                .iter()
                .map(|t| {
                    let id = t["tag_id"].as_i64().unwrap_or(0);
                    let name = t["tag_name"].as_str().unwrap_or("-");
                    let kind = t["tag_type"].as_str().unwrap_or("-");
                    let stat = t["subscribed_count"]
                        .as_i64()
                        .or_else(|| t["count"]["atten"].as_i64())
                        .or_else(|| t["likes"].as_i64())
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    vec![
                        Cell::cyan(id.to_string()),
                        Cell::plain(truncate_to_width(name, 26)),
                        Cell::muted(truncate_to_width(kind, 14)),
                        Cell::plain(stat),
                    ]
                })
                .collect();
            print_table(&cols, &rows);
            print_spacer();
        }

        VideoCommands::Related { video } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let related = crate::api::video::get_video_related(&video, cookies.as_ref())?;

            if is_json() {
                print_json(&related)?;
                return Ok(());
            }

            if related.is_empty() {
                user_info!("暂无相关推荐视频");
                return Ok(());
            }

            print_spacer();
            print_section_header("相关推荐", Some(&format!("共 {} 条", related.len())));
            let cols = [
                Column::left("BV/AV号", 14),
                Column::left("标题", 36),
                Column::left("UP主", 16),
                Column::right("播放", 10),
                Column::right("时长", 8),
            ];
            let rows: Vec<Vec<Cell>> = related
                .iter()
                .map(|v| {
                    let aid = v["aid"].as_i64().unwrap_or(0);
                    let bvid = v["bvid"].as_str().unwrap_or("");
                    let vid_id = if bvid.is_empty() {
                        format!("av{aid}")
                    } else {
                        bvid.to_string()
                    };
                    let title = v["title"].as_str().unwrap_or("-");
                    let owner = v["owner"]["name"].as_str().unwrap_or("-");
                    let views = format_num(&v["stat"]["view"]);
                    let dur = format_duration(v["duration"].as_i64().unwrap_or(0));
                    vec![
                        Cell::cyan(truncate_to_width(&vid_id, 14)),
                        Cell::plain(truncate_to_width(title, 36)),
                        Cell::muted(truncate_to_width(owner, 16)),
                        Cell::plain(views),
                        Cell::plain(dur),
                    ]
                })
                .collect();
            print_table(&cols, &rows);
            print_spacer();
        }

        VideoCommands::Danmaku {
            video,
            page,
            format,
            output,
        } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let (target_page, info) =
                crate::api::video::resolve_video_context(&video, cookies.as_ref())?;
            let page_num = page.or(target_page).unwrap_or(1);
            let resolved = crate::api::video::extract_page_info(&info, page_num)?;

            let xml = crate::api::danmaku::fetch_danmaku_xml(resolved.cid, cookies.as_ref())?;
            let items = crate::api::danmaku::parse_danmaku_xml(&xml)?;

            let content = match format {
                DanmakuFormat::Xml => xml,
                DanmakuFormat::Ass => crate::api::danmaku::to_ass(
                    &items,
                    &format!("{} - {}", resolved.title, resolved.part),
                ),
                DanmakuFormat::Txt => crate::api::danmaku::to_txt(&items),
                DanmakuFormat::Json => serde_json::to_string_pretty(&json!({
                    "bvid": resolved.bvid,
                    "aid": resolved.aid,
                    "cid": resolved.cid,
                    "page": resolved.page,
                    "count": items.len(),
                    "danmaku": items,
                }))?,
            };

            if let Some(path) = output {
                ensure_parent_dir(&path)?;
                std::fs::write(&path, content.as_bytes())?;
                if is_json() {
                    print_json(&json!({
                        "bvid": resolved.bvid,
                        "aid": resolved.aid,
                        "cid": resolved.cid,
                        "page": resolved.page,
                        "count": items.len(),
                        "format": format.as_str(),
                        "file": path.display().to_string(),
                    }))?;
                } else {
                    user_success!(
                        "弹幕已导出 ({} 条, {}): {}",
                        items.len(),
                        format.as_str(),
                        path.display()
                    );
                }
            } else if is_json() {
                match format {
                    DanmakuFormat::Json => {
                        print_json(&json!({
                            "bvid": resolved.bvid,
                            "aid": resolved.aid,
                            "cid": resolved.cid,
                            "page": resolved.page,
                            "count": items.len(),
                            "danmaku": items,
                        }))?;
                    }
                    DanmakuFormat::Xml | DanmakuFormat::Ass | DanmakuFormat::Txt => {
                        print_json(&json!({
                            "bvid": resolved.bvid,
                            "aid": resolved.aid,
                            "cid": resolved.cid,
                            "page": resolved.page,
                            "count": items.len(),
                            "format": format.as_str(),
                            "content": content,
                        }))?;
                    }
                }
            } else {
                write_stdout_broken_pipe_safe(&content)?;
            }
        }

        VideoCommands::Comments {
            video,
            page,
            sort,
            root,
            full,
            compact,
            interactive,
            raw,
        } => {
            let _ = full;
            if interactive {
                use std::io::IsTerminal;
                if is_json() {
                    return Err(BiliLiveError::Input(
                        "交互式评论浏览器不支持 --json 模式；请移除 --json 或改用默认输出"
                            .to_string(),
                    ));
                }
                if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                    return Err(BiliLiveError::Input(
                        "交互式评论浏览器需要交互式终端 (TTY)；管道环境请移除 -i".to_string(),
                    ));
                }
            }
            if raw && !is_json() {
                return Err(BiliLiveError::Input(
                    "--raw 仅支持配合 --json 使用".to_string(),
                ));
            }

            let cookies = crate::auth::cookies::read_cookies().ok();
            if interactive {
                let (aid, video_label, video_title) =
                    match crate::api::video::resolve_video_context(&video, cookies.as_ref()) {
                        Ok((_, info)) => {
                            let aid = info["aid"].as_i64().unwrap_or(0);
                            let aid = if aid > 0 {
                                aid
                            } else {
                                crate::api::video::resolve_video_aid(&video, cookies.as_ref())?
                            };
                            let bvid = info["bvid"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .unwrap_or(&video)
                                .to_string();
                            let title = info["title"].as_str().unwrap_or("").to_string();
                            (aid, bvid, title)
                        }
                        Err(_) => {
                            let aid =
                                crate::api::video::resolve_video_aid(&video, cookies.as_ref())?;
                            (aid, video.clone(), String::new())
                        }
                    };
                return crate::ui::comment_tui::run_comment_tui(
                    crate::ui::comment_tui::CommentTuiContext {
                        aid,
                        video_label,
                        video_title,
                        initial_sort: sort,
                        initial_page: page,
                        initial_root: root,
                        cookies,
                    },
                );
            }

            let aid = crate::api::video::resolve_video_aid(&video, cookies.as_ref())?;

            if let Some(root_rpid) = root {
                let sub =
                    crate::api::comment::get_sub_replies(aid, root_rpid, page, cookies.as_ref())?;
                if is_json() {
                    if raw {
                        print_json(&sub.raw)?;
                    } else {
                        print_json(&sub)?;
                    }
                    return Ok(());
                }

                print_spacer();
                if let Some(ref root_item) = sub.root
                    && root_item.rpid > 0
                {
                    let card = crate::ui::comment_fmt::build_root_info_card(
                        root_item,
                        sub.upper_mid(),
                        sub.page.count,
                    );
                    card.print();
                    print_spacer();
                }

                if sub.replies.is_empty() {
                    user_info!("该评论下暂无楼中楼回复");
                    return Ok(());
                }

                let total_pages = sub.page.count.div_ceil(sub.page.size.max(20)).max(1);
                print_section_header(
                    &format!("楼中楼回复 (root={root_rpid})"),
                    Some(&format!(
                        "第 {page}/{total_pages} 页 · 共 {} 条",
                        sub.page.count
                    )),
                );
                if !is_quiet() {
                    let rendered = crate::ui::comment_fmt::render_sub_reply_list(
                        &sub,
                        root_rpid,
                        &video,
                        page,
                        terminal_width(),
                        color_enabled(),
                    );
                    write_stdout_broken_pipe_safe(&rendered)?;
                }
                return Ok(());
            }

            let data =
                crate::api::comment::get_comments(aid, page, sort.as_api_str(), cookies.as_ref())?;

            if is_json() {
                if raw {
                    print_json(&data.raw)?;
                } else {
                    print_json(&data)?;
                }
                return Ok(());
            }

            if data.replies.is_empty() && !(page == 1 && data.pinned_reply().is_some()) {
                user_info!("暂无评论");
                return Ok(());
            }

            let sort_label = match sort {
                crate::cli::args::CommentSort::Hot => "按热度",
                crate::cli::args::CommentSort::Time => "按时间",
            };
            let total_display = data.page.acount.max(data.page.count);
            let total_pages = data.page.count.div_ceil(data.page.size.max(1)).max(1);

            print_spacer();
            print_section_header(
                "视频评论",
                Some(&format!(
                    "{sort_label} · 第 {page}/{total_pages} 页 · 共 {total_display} 条"
                )),
            );

            if compact {
                crate::ui::comment_fmt::print_comments_compact_table(&data);
                print_spacer();
                return Ok(());
            }

            if !is_quiet() {
                let rendered = crate::ui::comment_fmt::render_comment_list(
                    &data,
                    &video,
                    page,
                    sort_label,
                    terminal_width(),
                    color_enabled(),
                );
                write_stdout_broken_pipe_safe(&rendered)?;
            }
        }

        VideoCommands::Comment(sub) => match sub {
            VideoCommentCommands::Post {
                video,
                message,
                root,
                parent,
            } => {
                if message.trim().is_empty() {
                    return Err(BiliLiveError::Input("评论内容不能为空".to_string()));
                }
                let cookies = crate::auth::ensure_csrf()?;
                let aid = crate::api::video::resolve_video_aid(&video, Some(&cookies))?;
                let data =
                    crate::api::comment::post_comment(&cookies, aid, &message, root, parent)?;
                let rpid = data["rpid"].as_i64().unwrap_or(0);

                if is_json() {
                    print_json(&json!({
                        "success": true,
                        "aid": aid,
                        "rpid": rpid,
                        "message": message,
                        "data": data,
                    }))?;
                } else {
                    user_success!("评论发表成功 (rpid={})", rpid);
                }
            }

            VideoCommentCommands::Like {
                video,
                rpid,
                cancel,
            } => {
                let cookies = crate::auth::ensure_csrf()?;
                let aid = crate::api::video::resolve_video_aid(&video, Some(&cookies))?;
                crate::api::comment::like_comment(&cookies, aid, rpid, cancel)?;

                if is_json() {
                    print_json(&json!({
                        "success": true,
                        "aid": aid,
                        "rpid": rpid,
                        "liked": !cancel,
                    }))?;
                } else {
                    user_success!(
                        "{}评论成功 (rpid={})",
                        if cancel { "取消点赞" } else { "点赞" },
                        rpid
                    );
                }
            }

            VideoCommentCommands::Delete { video, rpid } => {
                let cookies = crate::auth::ensure_csrf()?;
                let aid = crate::api::video::resolve_video_aid(&video, Some(&cookies))?;
                crate::api::comment::delete_comment(&cookies, aid, rpid)?;

                if is_json() {
                    print_json(&json!({
                        "success": true,
                        "aid": aid,
                        "rpid": rpid,
                        "deleted": true,
                    }))?;
                } else {
                    user_success!("评论已删除 (rpid={})", rpid);
                }
            }
        },

        VideoCommands::Like { video, cancel } => {
            let cookies = crate::auth::ensure_csrf()?;
            crate::api::video::like_video(&cookies, &video, cancel)?;

            if is_json() {
                print_json(&json!({
                    "video": video,
                    "liked": !cancel,
                    "success": true
                }))?;
            } else {
                user_success!(
                    "{}成功: {}",
                    if cancel { "取消点赞" } else { "点赞" },
                    video
                );
            }
        }

        VideoCommands::Coin {
            video,
            num,
            also_like,
        } => {
            let cookies = crate::auth::ensure_csrf()?;
            let liked = crate::api::video::coin_video(&cookies, &video, num, also_like)?;

            if is_json() {
                print_json(&json!({
                    "video": video,
                    "coins": num,
                    "also_like": also_like,
                    "success": true,
                    "liked": liked
                }))?;
            } else {
                user_success!(
                    "成功为 {} 投币 {} 枚{}",
                    video,
                    num,
                    if also_like {
                        "，并已同步点赞"
                    } else {
                        ""
                    }
                );
            }
        }

        VideoCommands::Triple { video } => {
            let cookies = crate::auth::ensure_csrf()?;
            let res = crate::api::video::triple_video(&cookies, &video)?;

            if is_json() {
                print_json(&res)?;
            } else {
                let like = res["like"].as_bool().unwrap_or(false);
                let coin = res["coin"].as_bool().unwrap_or(false);
                let fav = res["fav"].as_bool().unwrap_or(false);

                user_success!("视频一键三连完成: {}", video);
                user_info!(
                    "点赞: {} | 投币: {} | 收藏: {}",
                    if like {
                        paint_stderr("成功", Color::Green)
                    } else {
                        paint_stderr("跳过/已点赞", Color::DarkGrey)
                    },
                    if coin {
                        paint_stderr("成功", Color::Green)
                    } else {
                        paint_stderr("跳过/已投币", Color::DarkGrey)
                    },
                    if fav {
                        paint_stderr("成功", Color::Green)
                    } else {
                        paint_stderr("跳过/已收藏", Color::DarkGrey)
                    },
                );
            }
        }

        VideoCommands::Download {
            video,
            pages,
            quality,
            audio_quality,
            codec,
            output,
            audio_only,
            video_only,
            skip_mux,
            danmaku,
            danmaku_format,
            subtitle,
            cover,
        } => {
            // 1. 先解析分P参数语法，避免无效输入触发媒体下载或 ffmpeg
            let page_spec = pages.as_deref().map(parse_page_spec_syntax).transpose()?;

            let cookies = crate::auth::cookies::read_cookies().ok();
            let (target_page, info) =
                crate::api::video::resolve_video_context(&video, cookies.as_ref())?;
            let is_pgc = info["is_pgc"].as_bool() == Some(true);
            let selected_pages = resolve_page_list(&info, target_page, page_spec.as_ref())?;
            let multi_page = selected_pages.len() > 1;

            let mut all_files: Vec<String> = Vec::new();
            let mut page_reports: Vec<Value> = Vec::new();

            for resolved in &selected_pages {
                crate::utils::interrupt::check_interrupted()?;
                let playurl = if is_pgc {
                    crate::api::video::get_pgc_playurl(
                        &resolved.bvid,
                        resolved.aid,
                        resolved.cid,
                        quality,
                        cookies.as_ref(),
                    )?
                } else {
                    crate::api::video::get_video_playurl(
                        &resolved.bvid,
                        resolved.aid,
                        resolved.cid,
                        quality,
                        cookies.as_ref(),
                    )?
                };

                let (dash_videos, dash_audios) = crate::api::video::parse_dash_streams(&playurl);
                let durls = crate::api::video::parse_durl_segments(&playurl);

                let has_dash = !dash_videos.is_empty() || !dash_audios.is_empty();
                let referer = if !resolved.bvid.is_empty() {
                    format!("https://www.bilibili.com/video/{}", resolved.bvid)
                } else {
                    "https://www.bilibili.com/".to_string()
                };

                let mut chosen_qn = quality;
                let mut chosen_codec = codec.as_str().to_string();
                let mut page_files: Vec<String> = Vec::new();

                let primary_path: PathBuf;

                if has_dash {
                    let v_stream = if !audio_only {
                        crate::api::video::select_dash_video(&dash_videos, quality, codec.as_str())
                    } else {
                        None
                    };
                    let a_stream = if !video_only {
                        crate::api::video::select_dash_audio_with_quality(
                            &dash_audios,
                            audio_quality,
                        )
                    } else {
                        None
                    };

                    if !audio_only && v_stream.is_none() {
                        return Err(BiliLiveError::Api("未找到匹配的 DASH 视频流".to_string()));
                    }
                    if audio_only && a_stream.is_none() {
                        return Err(BiliLiveError::Api("未找到可用的 DASH 音频流".to_string()));
                    }

                    if let Some(ref vs) = v_stream {
                        chosen_qn = vs.id;
                        chosen_codec = crate::api::video::codec_label(vs).to_string();
                    } else if let Some(ref aus) = a_stream {
                        chosen_qn = aus.id;
                        chosen_codec = "audio".to_string();
                    }

                    let need_mux = !audio_only
                        && !video_only
                        && !skip_mux
                        && v_stream.is_some()
                        && a_stream.is_some();

                    // 在下载任何 DASH 流之前校验 ffmpeg 是否可用
                    let ffmpeg_bin = if need_mux {
                        Some(crate::utils::ffmpeg::ensure_ffmpeg()?)
                    } else {
                        None
                    };

                    let audio_ext = a_stream
                        .as_ref()
                        .map(crate::api::video::audio_stream_extension)
                        .unwrap_or("m4a");

                    let default_ext = if audio_only {
                        audio_ext
                    } else if video_only || skip_mux {
                        "m4s"
                    } else {
                        "mp4"
                    };

                    let out_path = expand_output_template(
                        output.as_deref(),
                        resolved,
                        chosen_qn,
                        &chosen_codec,
                        default_ext,
                        multi_page,
                    );
                    ensure_parent_dir(&out_path)?;
                    primary_path = out_path.clone();

                    match (v_stream, a_stream, ffmpeg_bin) {
                        (Some(vs), Some(aus), Some(ff)) => {
                            let parent_dir = out_path
                                .parent()
                                .filter(|p| !p.as_os_str().is_empty())
                                .unwrap_or_else(|| Path::new("."));
                            let v_tmp = tempfile::NamedTempFile::new_in(parent_dir)?;
                            let a_tmp = tempfile::NamedTempFile::new_in(parent_dir)?;
                            struct TmpRegGuard(PathBuf, PathBuf);
                            impl Drop for TmpRegGuard {
                                fn drop(&mut self) {
                                    crate::utils::interrupt::unregister_temp_path(&self.0);
                                    crate::utils::interrupt::unregister_temp_path(&self.1);
                                }
                            }
                            crate::utils::interrupt::register_temp_path(v_tmp.path().to_path_buf());
                            crate::utils::interrupt::register_temp_path(a_tmp.path().to_path_buf());
                            let _tmp_reg =
                                TmpRegGuard(v_tmp.path().to_path_buf(), a_tmp.path().to_path_buf());

                            user_info!(
                                "下载 P{} 视频流 (qn={}, codec={})...",
                                resolved.page,
                                vs.id,
                                chosen_codec
                            );
                            crate::api::video::download_binary_to_path(
                                &vs.all_urls(),
                                &referer,
                                cookies.as_ref(),
                                v_tmp.path(),
                            )?;

                            user_info!("下载 P{} 音频流 (id={})...", resolved.page, aus.id);
                            crate::api::video::download_binary_to_path(
                                &aus.all_urls(),
                                &referer,
                                cookies.as_ref(),
                                a_tmp.path(),
                            )?;

                            crate::utils::interrupt::check_interrupted()?;
                            user_info!("使用 ffmpeg 混流 -> {}", out_path.display());
                            crate::api::video::mux_dash_streams(
                                &ff,
                                v_tmp.path(),
                                a_tmp.path(),
                                &out_path,
                            )?;
                            page_files.push(out_path.display().to_string());
                        }
                        (Some(vs), Some(aus), None) => {
                            // --skip-mux：分别保留原始视频流与音频流文件
                            let v_out = stem_sibling_path(&out_path, ".video.m4s");
                            let a_out =
                                stem_sibling_path(&out_path, &format!(".audio.{audio_ext}"));
                            ensure_parent_dir(&v_out)?;

                            user_info!("下载 P{} 视频流 -> {}", resolved.page, v_out.display());
                            crate::api::video::download_binary_to_path(
                                &vs.all_urls(),
                                &referer,
                                cookies.as_ref(),
                                &v_out,
                            )?;
                            user_info!("下载 P{} 音频流 -> {}", resolved.page, a_out.display());
                            crate::api::video::download_binary_to_path(
                                &aus.all_urls(),
                                &referer,
                                cookies.as_ref(),
                                &a_out,
                            )?;

                            page_files.push(v_out.display().to_string());
                            page_files.push(a_out.display().to_string());
                        }
                        (Some(vs), None, _) => {
                            user_info!("下载 P{} 视频流 -> {}", resolved.page, out_path.display());
                            crate::api::video::download_binary_to_path(
                                &vs.all_urls(),
                                &referer,
                                cookies.as_ref(),
                                &out_path,
                            )?;
                            page_files.push(out_path.display().to_string());
                        }
                        (None, Some(aus), _) => {
                            user_info!("下载 P{} 音频流 -> {}", resolved.page, out_path.display());
                            crate::api::video::download_binary_to_path(
                                &aus.all_urls(),
                                &referer,
                                cookies.as_ref(),
                                &out_path,
                            )?;
                            page_files.push(out_path.display().to_string());
                        }
                        (None, None, _) => {
                            return Err(BiliLiveError::Api(
                                "未找到可下载的 DASH 媒体流".to_string(),
                            ));
                        }
                    }
                } else if !durls.is_empty() {
                    if audio_only {
                        return Err(BiliLiveError::Input(
                            "当前视频仅提供合并 MP4/FLV 流，不支持 --audio-only".to_string(),
                        ));
                    }
                    chosen_qn = playurl["quality"].as_u64().unwrap_or(quality as u64) as u32;
                    chosen_codec = "durl".to_string();

                    let out_path = expand_output_template(
                        output.as_deref(),
                        resolved,
                        chosen_qn,
                        &chosen_codec,
                        "mp4",
                        multi_page,
                    );
                    ensure_parent_dir(&out_path)?;
                    primary_path = out_path.clone();

                    if durls.len() == 1 {
                        user_info!("下载 P{} 媒体文件 -> {}", resolved.page, out_path.display());
                        crate::api::video::download_binary_to_path(
                            &durls[0].all_urls(),
                            &referer,
                            cookies.as_ref(),
                            &out_path,
                        )?;
                        page_files.push(out_path.display().to_string());
                    } else {
                        for (idx, seg) in durls.iter().enumerate() {
                            let seg_path =
                                stem_sibling_path(&out_path, &format!("_part{}.mp4", idx + 1));
                            crate::api::video::download_binary_to_path(
                                &seg.all_urls(),
                                &referer,
                                cookies.as_ref(),
                                &seg_path,
                            )?;
                            page_files.push(seg_path.display().to_string());
                        }
                    }
                } else {
                    return Err(BiliLiveError::Api("未获取到可用的媒体流地址".to_string()));
                }

                // 下载附属资源（封面 / 字幕 / 弹幕）
                if cover && !resolved.pic.trim().is_empty() {
                    let cover_ext = if resolved
                        .pic
                        .split('?')
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .ends_with(".png")
                    {
                        ".png"
                    } else {
                        ".jpg"
                    };
                    let cover_path = stem_sibling_path(&primary_path, cover_ext);
                    ensure_parent_dir(&cover_path)?;
                    crate::api::video::download_binary_to_path(
                        std::slice::from_ref(&resolved.pic),
                        &referer,
                        cookies.as_ref(),
                        &cover_path,
                    )?;
                    page_files.push(cover_path.display().to_string());
                }

                if subtitle {
                    match crate::api::video::get_player_v2(
                        &resolved.bvid,
                        resolved.aid,
                        resolved.cid,
                        cookies.as_ref(),
                    ) {
                        Ok(v2) => {
                            if let Some(tracks) = v2["subtitle"]["subtitles"].as_array()
                                && let Some(track) = tracks.iter().find(|t| {
                                    !t["subtitle_url"].as_str().unwrap_or("").trim().is_empty()
                                })
                            {
                                let sub_url = track["subtitle_url"].as_str().unwrap_or("");
                                match crate::api::video::fetch_subtitle_json(sub_url) {
                                    Ok(sub_json) => {
                                        let body = sub_json["body"]
                                            .as_array()
                                            .cloned()
                                            .unwrap_or_default();
                                        let srt = render_subtitle_srt(&body);
                                        let sub_path = stem_sibling_path(&primary_path, ".srt");
                                        if let Err(e) =
                                            ensure_parent_dir(&sub_path).and_then(|()| {
                                                std::fs::write(&sub_path, srt.as_bytes())
                                                    .map_err(Into::into)
                                            })
                                        {
                                            user_warning!(
                                                "保存 P{} 字幕失败，已跳过: {}",
                                                resolved.page,
                                                e
                                            );
                                        } else {
                                            page_files.push(sub_path.display().to_string());
                                        }
                                    }
                                    Err(e) => {
                                        user_warning!(
                                            "下载 P{} 字幕失败，已跳过: {}",
                                            resolved.page,
                                            e
                                        );
                                    }
                                }
                            } else {
                                user_warning!("P{} 无可用字幕，已跳过字幕下载", resolved.page);
                            }
                        }
                        Err(e) => {
                            user_warning!("获取 P{} 字幕失败，已跳过: {}", resolved.page, e);
                        }
                    }
                }

                if danmaku {
                    let xml =
                        crate::api::danmaku::fetch_danmaku_xml(resolved.cid, cookies.as_ref())?;
                    match danmaku_format {
                        DownloadDanmakuFormat::Ass => {
                            let dm_path = stem_sibling_path(&primary_path, ".ass");
                            ensure_parent_dir(&dm_path)?;
                            let items = crate::api::danmaku::parse_danmaku_xml(&xml)?;
                            let ass = crate::api::danmaku::to_ass(
                                &items,
                                &format!("{} - {}", resolved.title, resolved.part),
                            );
                            std::fs::write(&dm_path, ass.as_bytes())?;
                            page_files.push(dm_path.display().to_string());
                        }
                        DownloadDanmakuFormat::Xml => {
                            let dm_path = stem_sibling_path(&primary_path, ".xml");
                            ensure_parent_dir(&dm_path)?;
                            std::fs::write(&dm_path, xml.as_bytes())?;
                            page_files.push(dm_path.display().to_string());
                        }
                    }
                }

                all_files.extend(page_files.iter().cloned());
                page_reports.push(json!({
                    "page": resolved.page,
                    "cid": resolved.cid,
                    "part": resolved.part,
                    "quality": chosen_qn,
                    "codec": chosen_codec,
                    "files": page_files,
                }));
            }

            let primary_title = selected_pages
                .first()
                .map(|p| p.title.as_str())
                .unwrap_or("");
            let primary_bvid = selected_pages
                .first()
                .map(|p| p.bvid.as_str())
                .unwrap_or("");
            let primary_aid = selected_pages.first().map(|p| p.aid).unwrap_or(0);

            if is_json() {
                print_json(&json!({
                    "success": true,
                    "bvid": primary_bvid,
                    "aid": primary_aid,
                    "title": primary_title,
                    "files": all_files,
                    "outputs": all_files,
                    "pages": page_reports,
                }))?;
            } else {
                user_success!(
                    "下载完成 ({} 个文件): {}",
                    all_files.len(),
                    all_files.join(", ")
                );
            }
        }

        VideoCommands::Upload(args) => {
            let crate::cli::args::VideoUploadArgs {
                files,
                title,
                tid,
                tag,
                topic,
                topic_id,
                mission_id,
                desc,
                copyright,
                source,
                cover,
                cover43,
                dynamic,
                dtime,
                no_reprint,
                line,
                limit,
            } = *args;
            // 1. 所有本地参数与文件存在性前置校验（避免大文件上传完才因标题/转载来源缺失失败）
            for f in &files {
                let m = std::fs::metadata(f).map_err(|e| {
                    BiliLiveError::Input(format!("无法读取视频文件 '{}': {e}", f.display()))
                })?;
                if !m.is_file() || m.len() == 0 {
                    return Err(BiliLiveError::Input(format!(
                        "无效或空的视频文件: {}",
                        f.display()
                    )));
                }
            }
            if copyright == 2 && source.trim().is_empty() {
                return Err(BiliLiveError::Input(
                    "转载稿件 (--copyright 2) 必须通过 --source 指定转载来源网址".to_string(),
                ));
            }

            let default_title = files[0]
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("未命名稿件");
            let effective_title = title
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(default_title)
                .to_string();
            if effective_title.chars().count() > 80 {
                return Err(BiliLiveError::Input(
                    "稿件标题不能超过 80 个字符".to_string(),
                ));
            }

            let mut effective_tag = match tag {
                Some(ref raw) => crate::api::upload::normalize_tags(raw)?,
                None => "视频,日常".to_string(),
            };

            let parsed_dtime = dtime.as_deref().map(parse_schedule_dtime).transpose()?;

            let cookies = crate::auth::ensure_csrf()?;

            let resolved_topic = match topic {
                Some(ref q) if !q.trim().is_empty() => {
                    user_info!("正在解析活动/话题「{}」...", q.trim());
                    Some(crate::api::upload::resolve_studio_topic(
                        &cookies,
                        q,
                        Some(tid),
                    )?)
                }
                _ => None,
            };
            let effective_topic_id = topic_id
                .or_else(|| resolved_topic.as_ref().map(|t| t.topic_id))
                .filter(|&id| id > 0);
            let effective_mission_id = mission_id
                .or_else(|| {
                    resolved_topic
                        .as_ref()
                        .map(|t| t.mission_id)
                        .filter(|&id| id > 0)
                })
                .filter(|&id| id > 0);
            if let Some(ref rt) = resolved_topic {
                effective_tag =
                    crate::api::upload::ensure_topic_tag(&effective_tag, &rt.topic_name)?;
            }

            let cover_url = match cover {
                Some(ref c) if !c.trim().is_empty() => {
                    user_info!("正在处理稿件封面...");
                    crate::api::upload::upload_cover_image(&cookies, c)?
                }
                _ => String::new(),
            };
            let cover43_url = match cover43 {
                Some(ref c) if !c.trim().is_empty() => {
                    user_info!("正在处理 4:3 推荐封面...");
                    crate::api::upload::upload_cover_image(&cookies, c)?
                }
                _ => String::new(),
            };

            let line_query = crate::api::upload::resolve_line_query(line);
            let mut uploaded_parts = Vec::with_capacity(files.len());
            for (idx, f) in files.iter().enumerate() {
                user_info!(
                    "开始上传分P [{}/{}]: {} (线路={}, 并发={})...",
                    idx + 1,
                    files.len(),
                    f.display(),
                    line.as_str(),
                    limit
                );
                let part = crate::api::upload::upload_single_video_file(
                    &cookies,
                    f,
                    None,
                    &line_query,
                    limit as usize,
                )?;
                user_success!(
                    "分P [{}/{}] 上传完成 (filename={}, cid={})",
                    idx + 1,
                    files.len(),
                    part.filename,
                    part.cid
                );
                uploaded_parts.push(part);
            }

            let submit_params = crate::api::upload::ArchiveSubmitParams {
                videos: uploaded_parts.clone(),
                title: effective_title.clone(),
                tid,
                tag: effective_tag.clone(),
                desc,
                copyright,
                source,
                cover: cover_url.clone(),
                cover43: cover43_url.clone(),
                dynamic,
                dtime: parsed_dtime,
                no_reprint,
                topic_id: effective_topic_id,
                mission_id: effective_mission_id,
            };
            let res = crate::api::upload::submit_archive(&cookies, &submit_params)?;
            let bvid = res["bvid"].as_str().unwrap_or("").to_string();
            let aid = res["aid"].as_i64().unwrap_or(0);

            if is_json() {
                print_json(&json!({
                    "success": true,
                    "bvid": bvid,
                    "aid": aid,
                    "title": effective_title,
                    "tid": tid,
                    "tag": effective_tag,
                    "topic_id": effective_topic_id.unwrap_or(0),
                    "mission_id": effective_mission_id.unwrap_or(0),
                    "cover": cover_url,
                    "cover43": cover43_url,
                    "parts": uploaded_parts,
                    "data": res,
                }))?;
            } else {
                let mut card = InfoCard::new("🎉 视频稿件投递成功")
                    .with_badge(
                        if bvid.is_empty() {
                            format!("av{aid}")
                        } else {
                            bvid.clone()
                        },
                        Color::Green,
                    )
                    .add_row(
                        "BV号",
                        Cell::cyan(if bvid.is_empty() { "-" } else { &bvid }),
                    )
                    .add_plain_row("AV号", format!("av{aid}"))
                    .add_row("标题", Cell::bold_colored(&effective_title, Color::White))
                    .add_plain_row("分P数量", uploaded_parts.len().to_string())
                    .add_plain_row("分区 TID", tid.to_string())
                    .add_plain_row("标签", &effective_tag);
                if let Some(tid_val) = effective_topic_id {
                    let topic_disp = match (&resolved_topic, effective_mission_id) {
                        (Some(rt), Some(mid)) => {
                            format!("{} (topic_id={}, mission_id={})", rt.topic_name, tid_val, mid)
                        }
                        (Some(rt), None) => format!("{} (topic_id={})", rt.topic_name, tid_val),
                        (None, Some(mid)) => format!("topic_id={}, mission_id={}", tid_val, mid),
                        (None, None) => format!("topic_id={}", tid_val),
                    };
                    card = card.add_plain_row("活动/话题", topic_disp);
                } else if let Some(mid) = effective_mission_id {
                    card = card.add_plain_row("征稿活动", format!("mission_id={mid}"));
                }
                print_spacer();
                card.print();
                print_spacer();
            }
        }

        VideoCommands::Append {
            video,
            files,
            line,
            limit,
        } => {
            for f in &files {
                let m = std::fs::metadata(f).map_err(|e| {
                    BiliLiveError::Input(format!("无法读取待追加视频文件 '{}': {e}", f.display()))
                })?;
                if !m.is_file() || m.len() == 0 {
                    return Err(BiliLiveError::Input(format!(
                        "无效或空的视频文件: {}",
                        f.display()
                    )));
                }
            }

            let cookies = crate::auth::ensure_csrf()?;
            // 先校验目标稿件在创作中心可访问，避免上传数 GB 后才发现无权限编辑
            let existing = crate::api::upload::get_studio_archive_view(&cookies, &video)?;
            let existing_title = existing["archive"]["title"]
                .as_str()
                .unwrap_or(&video)
                .to_string();

            let line_query = crate::api::upload::resolve_line_query(line);
            let mut new_parts = Vec::with_capacity(files.len());
            for (idx, f) in files.iter().enumerate() {
                user_info!(
                    "上传待追加分P [{}/{}] -> 《{}》: {}...",
                    idx + 1,
                    files.len(),
                    existing_title,
                    f.display()
                );
                let part = crate::api::upload::upload_single_video_file(
                    &cookies,
                    f,
                    None,
                    &line_query,
                    limit as usize,
                )?;
                new_parts.push(part);
            }

            let (res, total_parts) =
                crate::api::upload::append_archive_parts(&cookies, &video, &new_parts)?;
            let bvid = res["bvid"]
                .as_str()
                .or_else(|| existing["archive"]["bvid"].as_str())
                .unwrap_or("")
                .to_string();
            let aid = res["aid"]
                .as_i64()
                .or_else(|| existing["archive"]["aid"].as_i64())
                .unwrap_or(0);

            if is_json() {
                print_json(&json!({
                    "success": true,
                    "bvid": bvid,
                    "aid": aid,
                    "title": existing_title,
                    "appended_count": new_parts.len(),
                    "total_parts": total_parts,
                    "appended_parts": new_parts,
                    "data": res,
                }))?;
            } else {
                user_success!(
                    "成功向稿件《{}》({}) 追加 {} 个分P（当前共 {} P）",
                    existing_title,
                    if bvid.is_empty() {
                        format!("av{aid}")
                    } else {
                        bvid
                    },
                    new_parts.len(),
                    total_parts
                );
            }
        }

        VideoCommands::Replace {
            video,
            file,
            part,
            title,
            line,
            limit,
        } => {
            let m = std::fs::metadata(&file).map_err(|e| {
                BiliLiveError::Input(format!("无法读取替换视频文件 '{}': {e}", file.display()))
            })?;
            if !m.is_file() || m.len() == 0 {
                return Err(BiliLiveError::Input(format!(
                    "无效或空的视频文件: {}",
                    file.display()
                )));
            }
            let new_title = title
                .as_deref()
                .map(crate::api::upload::validate_archive_title)
                .transpose()?;

            let cookies = crate::auth::ensure_csrf()?;
            // 上传前先校验稿件可编辑、允许替换且分P存在，避免上传完才失败
            let existing = crate::api::upload::get_studio_archive_view(&cookies, &video)?;
            crate::api::upload::ensure_replace_allowed(&existing)?;
            let arc = &existing["archive"];
            let existing_title = arc["title"].as_str().unwrap_or(&video).to_string();
            let bvid = arc["bvid"].as_str().unwrap_or(&video).to_string();
            let parts = existing["videos"].as_array().map(|a| a.len()).unwrap_or(0);
            let part_idx = part as usize;
            if part_idx > parts {
                return Err(BiliLiveError::Input(format!(
                    "分P序号 {part} 超出范围（稿件共 {parts} P）"
                )));
            }
            let part_title = existing["videos"][part_idx - 1]["title"]
                .as_str()
                .unwrap_or("")
                .to_string();

            let mut prompt = format!(
                "将《{existing_title}》({bvid}) 的 P{part}「{part_title}」替换为 {}",
                file.display()
            );
            if let Some(t) = &new_title {
                prompt.push_str(&format!("，并将标题改为《{t}》"));
            }
            prompt.push_str("。提交后稿件需重新审核，确认继续？");
            if !crate::cli::output::confirm_action(&prompt)? {
                user_warning!("已取消替换");
                return Ok(());
            }

            let line_query = crate::api::upload::resolve_line_query(line);
            user_info!(
                "上传替换视频 -> 《{}》P{}: {}...",
                existing_title,
                part,
                file.display()
            );
            let uploaded = crate::api::upload::upload_single_video_file(
                &cookies,
                &file,
                Some(&part_title),
                &line_query,
                limit as usize,
            )?;

            let outcome = crate::api::upload::replace_archive_part(
                &cookies,
                &video,
                part_idx,
                &uploaded,
                new_title.as_deref(),
            )?;

            if is_json() {
                print_json(&json!({
                    "success": true,
                    "bvid": bvid,
                    "aid": arc["aid"].as_i64().unwrap_or(0),
                    "title": outcome.title,
                    "title_changed": new_title.is_some(),
                    "part": outcome.part,
                    "total_parts": outcome.total_parts,
                    "part_title": outcome.part_title,
                    "old_filename": outcome.old_filename,
                    "new_part": uploaded,
                    "data": outcome.data,
                }))?;
            } else {
                user_success!(
                    "已替换稿件《{}》({}) 的 P{}，稿件将重新进入审核",
                    outcome.title,
                    bvid,
                    outcome.part
                );
            }
        }

        VideoCommands::Edit(args) => {
            let crate::cli::args::VideoEditArgs {
                video,
                title,
                tid,
                tag,
                topic,
                topic_id,
                mission_id,
                desc,
                copyright,
                source,
                cover,
                cover43,
                dynamic,
                no_reprint,
            } = *args;
            // 1. 前置校验：至少指定一个修改项，且本地字段格式合法（0 网络请求快速失败）
            if title.is_none()
                && tid.is_none()
                && tag.is_none()
                && topic.is_none()
                && topic_id.is_none()
                && mission_id.is_none()
                && desc.is_none()
                && copyright.is_none()
                && source.is_none()
                && cover.is_none()
                && cover43.is_none()
                && dynamic.is_none()
                && no_reprint.is_none()
            {
                return Err(BiliLiveError::Input(
                    "至少需要指定一个要修改的字段（如 --title / --tid / --tag / --topic / --desc / --cover / --cover43 等）"
                        .to_string(),
                ));
            }

            let validated_title = title
                .as_deref()
                .map(crate::api::upload::validate_archive_title)
                .transpose()?;
            let mut validated_tag = tag
                .as_deref()
                .map(crate::api::upload::normalize_tags)
                .transpose()?;

            let mut changed_labels: Vec<&str> = Vec::new();
            if validated_title.is_some() {
                changed_labels.push("标题");
            }
            if tid.is_some() {
                changed_labels.push("分区");
            }
            if validated_tag.is_some() {
                changed_labels.push("标签");
            }
            if topic.is_some() || topic_id.is_some() || mission_id.is_some() {
                changed_labels.push("活动/话题");
            }
            if desc.is_some() {
                changed_labels.push("简介");
            }
            if copyright.is_some() {
                changed_labels.push("版权类型");
            }
            if source.is_some() {
                changed_labels.push("转载来源");
            }
            if cover.is_some() {
                changed_labels.push("16:9封面");
            }
            if cover43.is_some() {
                changed_labels.push("4:3封面");
            }
            if dynamic.is_some() {
                changed_labels.push("动态文案");
            }
            if no_reprint.is_some() {
                changed_labels.push("禁转声明");
            }

            let cookies = crate::auth::ensure_csrf()?;
            let existing = crate::api::upload::get_studio_archive_view(&cookies, &video)?;
            let arc = &existing["archive"];
            let existing_title = arc["title"].as_str().unwrap_or(&video).to_string();
            let bvid = arc["bvid"].as_str().unwrap_or(&video).to_string();

            let effective_copyright = copyright
                .map(u64::from)
                .unwrap_or_else(|| arc["copyright"].as_u64().unwrap_or(1));
            let effective_source = source
                .as_deref()
                .unwrap_or_else(|| arc["source"].as_str().unwrap_or(""));
            if effective_copyright == 2 && effective_source.trim().is_empty() {
                return Err(BiliLiveError::Input(
                    "转载稿件 (--copyright 2) 必须通过 --source 指定转载来源网址".to_string(),
                ));
            }

            let effective_tid_for_topic =
                tid.or_else(|| arc["tid"].as_u64().and_then(|v| u32::try_from(v).ok()));
            let resolved_topic = match topic {
                Some(ref q) if !q.trim().is_empty() => {
                    user_info!("正在解析活动/话题「{}」...", q.trim());
                    Some(crate::api::upload::resolve_studio_topic(
                        &cookies,
                        q,
                        effective_tid_for_topic,
                    )?)
                }
                _ => None,
            };
            let patch_topic_id = topic_id.or_else(|| resolved_topic.as_ref().map(|t| t.topic_id));
            let patch_mission_id = mission_id.or_else(|| {
                resolved_topic.as_ref().map(|t| t.mission_id)
            });
            if let Some(ref rt) = resolved_topic {
                let base_tags = validated_tag
                    .as_deref()
                    .unwrap_or_else(|| arc["tag"].as_str().unwrap_or(""));
                validated_tag = Some(crate::api::upload::ensure_topic_tag(
                    base_tags,
                    &rt.topic_name,
                )?);
                if !changed_labels.contains(&"标签") {
                    changed_labels.push("标签");
                }
            }

            let mut prompt = format!(
                "将修改稿件《{existing_title}》({bvid}) 的 {}",
                changed_labels.join("、")
            );
            if let Some(ref rt) = resolved_topic {
                prompt.push_str(&format!(
                    "（活动/话题：{}，topic_id={}，mission_id={}）",
                    rt.topic_name,
                    patch_topic_id.unwrap_or(rt.topic_id),
                    patch_mission_id.unwrap_or(rt.mission_id)
                ));
            }
            prompt.push_str("，确认提交？");
            if !crate::cli::output::confirm_action(&prompt)? {
                user_warning!("已取消编辑");
                return Ok(());
            }

            let uploaded_cover = match cover {
                Some(ref c) => {
                    if c.trim().is_empty() {
                        Some(String::new())
                    } else {
                        user_info!("正在处理 16:9 主封面...");
                        Some(crate::api::upload::upload_cover_image(&cookies, c)?)
                    }
                }
                None => None,
            };
            let uploaded_cover43 = match cover43 {
                Some(ref c) => {
                    if c.trim().is_empty() {
                        Some(String::new())
                    } else {
                        user_info!("正在处理 4:3 推荐封面...");
                        Some(crate::api::upload::upload_cover_image(&cookies, c)?)
                    }
                }
                None => None,
            };

            let patch = crate::api::upload::ArchiveEditPatch {
                title: validated_title,
                tid,
                tag: validated_tag,
                desc,
                copyright,
                source: source.map(|s| s.trim().to_string()),
                cover: uploaded_cover,
                cover43: uploaded_cover43,
                dynamic,
                no_reprint,
                topic_id: patch_topic_id,
                mission_id: patch_mission_id,
            };
            let outcome =
                crate::api::upload::edit_archive_metadata(&cookies, &existing, &patch)?;

            if is_json() {
                print_json(&json!({
                    "success": true,
                    "bvid": outcome.bvid,
                    "aid": outcome.aid,
                    "updated_fields": changed_labels,
                    "title": outcome.title,
                    "tid": outcome.tid,
                    "tag": outcome.tag,
                    "topic_id": outcome.topic_id,
                    "mission_id": outcome.mission_id,
                    "desc": outcome.desc,
                    "copyright": outcome.copyright,
                    "source": outcome.source,
                    "cover": outcome.cover,
                    "cover43": outcome.cover43,
                    "dynamic": outcome.dynamic,
                    "no_reprint": outcome.no_reprint,
                    "data": outcome.data,
                }))?;
            } else {
                user_success!(
                    "已更新稿件《{}》({})（修改项：{}）",
                    outcome.title,
                    outcome.bvid,
                    changed_labels.join("、")
                );
            }
        }

        VideoCommands::Topics {
            keyword,
            tid,
            activity_only,
            page,
            page_size,
        } => {
            let cookies = crate::auth::ensure_login()?;
            let mut topics = match keyword.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(kw) => {
                    let offset = page.saturating_sub(1).saturating_mul(page_size);
                    crate::api::upload::search_studio_topics(&cookies, kw, page_size, offset)?
                }
                None => crate::api::upload::list_studio_topics(&cookies, tid, page, page_size)?,
            };
            if activity_only {
                topics.retain(|t| t.is_activity || t.mission_id > 0);
            }

            if is_json() {
                print_json(&json!({
                    "count": topics.len(),
                    "topics": topics,
                }))?;
                return Ok(());
            }

            let subtitle = match (&keyword, tid) {
                (Some(kw), _) => format!("关键词「{}」· 共 {} 条", kw.trim(), topics.len()),
                (None, Some(t)) => format!("分区 TID {t} · 共 {} 条", topics.len()),
                (None, None) => format!("共 {} 条", topics.len()),
            };
            print_spacer();
            print_section_header("创作中心投稿活动与话题", Some(&subtitle));
            print_spacer();

            if topics.is_empty() {
                user_info!("未找到匹配的投稿活动或话题");
                print_spacer();
                return Ok(());
            }

            let cols = [
                Column::left("topic_id", 10),
                Column::left("话题/活动标签", 24),
                Column::left("mission_id", 12),
                Column::left("活动说明", 36),
            ];
            let rows: Vec<Vec<Cell>> = topics
                .iter()
                .map(|t| {
                    let mission_cell = if t.mission_id > 0 {
                        Cell::colored(t.mission_id.to_string(), Color::Green)
                    } else {
                        Cell::muted("-")
                    };
                    let info = if !t.mission_name.is_empty() {
                        t.mission_name.as_str()
                    } else if !t.activity_text.is_empty() {
                        t.activity_text.as_str()
                    } else if !t.description.is_empty() {
                        t.description.as_str()
                    } else {
                        "-"
                    };
                    vec![
                        Cell::cyan(t.topic_id.to_string()),
                        Cell::bold_colored(&t.topic_name, Color::White),
                        mission_cell,
                        Cell::muted(info),
                    ]
                })
                .collect();
            print_table(&cols, &rows);
            print_spacer();
        }

        VideoCommands::Studio(sub) => match sub {
            VideoStudioCommands::List {
                status,
                page,
                page_size,
            } => {
                let cookies = crate::auth::ensure_login()?;
                let data =
                    crate::api::upload::list_studio_archives(&cookies, status, page, page_size)?;
                if is_json() {
                    print_json(&data)?;
                    return Ok(());
                }

                let total = data["page"]["count"].as_i64().unwrap_or(0);
                let audits = data["arc_audits"].as_array();
                print_spacer();
                print_section_header(
                    "创作中心稿件管理",
                    Some(&format!("共 {total} 个稿件 · 第 {page} 页")),
                );
                print_spacer();

                match audits {
                    Some(list) if !list.is_empty() => {
                        let cols = [
                            Column::left("BV号", 14),
                            Column::left("标题", 32),
                            Column::left("状态", 14),
                            Column::right("播放", 8),
                            Column::left("提交时间", 16),
                        ];
                        let rows: Vec<Vec<Cell>> = list
                            .iter()
                            .map(|item| {
                                let arc = &item["Archive"];
                                let bvid = arc["bvid"]
                                    .as_str()
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                                    .unwrap_or_else(|| {
                                        format!("av{}", arc["aid"].as_i64().unwrap_or(0))
                                    });
                                let title = arc["title"].as_str().unwrap_or("-");
                                let state = arc["state"].as_i64().unwrap_or(0);
                                let state_desc = arc["state_desc"]
                                    .as_str()
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or(if state >= 0 {
                                        "已通过"
                                    } else {
                                        "审核/退回"
                                    });
                                let state_cell = if state >= 0 {
                                    Cell::colored(state_desc, Color::Green)
                                } else if state == -30 || state == -6 {
                                    Cell::colored(state_desc, Color::Yellow)
                                } else {
                                    Cell::pink(state_desc)
                                };
                                let view = format_num(&item["stat"]["view"]);
                                let ts = arc["ptime"]
                                    .as_i64()
                                    .filter(|&t| t > 0)
                                    .or_else(|| arc["ctime"].as_i64())
                                    .unwrap_or(0);
                                vec![
                                    Cell::cyan(bvid),
                                    Cell::plain(title),
                                    state_cell,
                                    Cell::plain(view),
                                    Cell::muted(format_timestamp(ts)),
                                ]
                            })
                            .collect();
                        print_table(&cols, &rows);
                        print_spacer();
                    }
                    _ => {
                        user_info!("当前筛选条件下暂无创作中心稿件");
                    }
                }
            }

            VideoStudioCommands::Show { video } => {
                let cookies = crate::auth::ensure_login()?;
                let data = crate::api::upload::get_studio_archive_view(&cookies, &video)?;
                if is_json() {
                    print_json(&data)?;
                    return Ok(());
                }

                let arc = &data["archive"];
                let bvid = arc["bvid"].as_str().unwrap_or("-");
                let aid = arc["aid"].as_i64().unwrap_or(0);
                let title = arc["title"].as_str().unwrap_or("-");
                let state = arc["state"].as_i64().unwrap_or(0);
                let state_desc = arc["state_desc"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(if state >= 0 {
                        "正常开放"
                    } else {
                        "处理中/受限"
                    });
                let reject = arc["reject_reason"].as_str().unwrap_or("").trim();
                let tid = arc["tid"].as_i64().unwrap_or(0);
                let tag = arc["tag"].as_str().unwrap_or("-");

                let badge_col = if state >= 0 {
                    Color::Green
                } else {
                    Color::Yellow
                };
                let mut card = InfoCard::new("🎬 创作中心稿件详情")
                    .with_badge(state_desc, badge_col)
                    .add_row("BV / AV", Cell::cyan(format!("{bvid}  (av{aid})")))
                    .add_row("标题", Cell::bold_colored(title, Color::White))
                    .add_plain_row("分区 / 标签", format!("TID {tid}  ·  {tag}"));
                if !reject.is_empty() {
                    card = card.add_row("退回原因", Cell::pink(reject));
                }
                print_spacer();
                card.print();
                print_spacer();

                if let Some(videos) = data["videos"].as_array()
                    && !videos.is_empty()
                {
                    print_section_header(
                        "分P转码与审核明细",
                        Some(&format!("共 {} P", videos.len())),
                    );
                    let cols = [
                        Column::right("P", 4),
                        Column::left("分P标题", 26),
                        Column::right("CID", 14),
                        Column::left("文件名", 22),
                        Column::left("状态", 16),
                    ];
                    let rows: Vec<Vec<Cell>> = videos
                        .iter()
                        .enumerate()
                        .map(|(idx, v)| {
                            let st = v["status"].as_i64().unwrap_or(0);
                            let s_desc = v["status_desc"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .or_else(|| v["fail_desc"].as_str().filter(|s| !s.is_empty()))
                                .unwrap_or(if st == 0 { "转码通过" } else { "处理中" });
                            vec![
                                Cell::cyan((idx + 1).to_string()),
                                Cell::plain(v["title"].as_str().unwrap_or("-")),
                                Cell::muted(v["cid"].as_i64().unwrap_or(0).to_string()),
                                Cell::muted(v["filename"].as_str().unwrap_or("-")),
                                if st >= 0 {
                                    Cell::colored(s_desc, Color::Green)
                                } else {
                                    Cell::pink(s_desc)
                                },
                            ]
                        })
                        .collect();
                    print_table(&cols, &rows);
                    print_spacer();
                }
            }
        },
    }

    Ok(())
}

fn parse_schedule_dtime(raw: &str) -> Result<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(BiliLiveError::Input("--dtime 不能为空".to_string()));
    }
    let now_ts = chrono::Utc::now().timestamp();

    let target_ts = if let Some(rel) = trimmed.strip_prefix('+') {
        let offset = parse_relative_secs(rel)?;
        now_ts + offset as i64
    } else if trimmed.chars().all(|c| c.is_ascii_digit()) {
        trimmed
            .parse::<i64>()
            .map_err(|e| BiliLiveError::Input(format!("无效的 Unix 时间戳 '{trimmed}': {e}")))?
    } else {
        let offset = parse_relative_secs(trimmed)?;
        now_ts + offset as i64
    };

    if target_ts < now_ts + 7200 {
        return Err(BiliLiveError::Input(
            "B 站定时发布时间 (--dtime) 需至少晚于当前时间 2 小时以上".to_string(),
        ));
    }
    Ok(target_ts)
}

fn parse_relative_secs(s: &str) -> Result<u64> {
    let mut total: u64 = 0;
    let mut num_buf = String::new();
    for ch in s.trim().chars() {
        if ch.is_ascii_digit() {
            num_buf.push(ch);
        } else {
            let n: u64 = num_buf
                .parse()
                .map_err(|_| BiliLiveError::Input(format!("无效的相对时长 '{s}'")))?;
            num_buf.clear();
            let mult = match ch.to_ascii_lowercase() {
                'd' => 86_400,
                'h' => 3_600,
                'm' => 60,
                's' => 1,
                _ => {
                    return Err(BiliLiveError::Input(format!(
                        "不支持的时长单位 '{ch}' (支持 d/h/m/s)"
                    )));
                }
            };
            total = total.saturating_add(n.saturating_mul(mult));
        }
    }
    if !num_buf.is_empty() || total == 0 {
        return Err(BiliLiveError::Input(format!(
            "无效的定时偏移 '{s}'（示例: +4h 或 +2h30m）"
        )));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_page_spec_syntax_valid_and_invalid() {
        assert_eq!(
            parse_page_spec_syntax("1").unwrap(),
            PageSpec::Pages(vec![1])
        );
        assert_eq!(
            parse_page_spec_syntax("1,3,2-4").unwrap(),
            PageSpec::Pages(vec![1, 3, 2, 4])
        );
        assert_eq!(parse_page_spec_syntax("ALL").unwrap(), PageSpec::All);
        assert_eq!(parse_page_spec_syntax("all").unwrap(), PageSpec::All);

        assert!(parse_page_spec_syntax("").is_err());
        assert!(parse_page_spec_syntax("0").is_err());
        assert!(parse_page_spec_syntax("5-1").is_err());
        assert!(parse_page_spec_syntax("abc").is_err());
        assert!(parse_page_spec_syntax("1-2-3").is_err());
        assert!(parse_page_spec_syntax("1,").is_err());
        assert!(matches!(
            parse_page_spec_syntax("1-100000000").unwrap_err(),
            BiliLiveError::Input(_)
        ));
    }

    #[test]
    fn test_format_srt_timestamp_rounding_and_boundaries() {
        assert_eq!(format_srt_timestamp(0.5), "00:00:00,500");
        assert_eq!(format_srt_timestamp(3.0), "00:00:03,000");
        assert_eq!(format_srt_timestamp(59.9999), "00:01:00,000");
        assert_eq!(format_srt_timestamp(3661.25), "01:01:01,250");
        assert_eq!(format_srt_timestamp(-1.0), "00:00:00,000");
    }

    #[test]
    fn test_expand_output_template_sanitizes_tokens_only() {
        let resolved = crate::api::video::ResolvedVideoPage {
            bvid: "BV1uJ411r7hL".to_string(),
            aid: 79677524,
            up_mid: 42,
            title: "A/B:C*?<>|\"test".to_string(),
            pic: String::new(),
            page: 2,
            cid: 146044694,
            part: "P2:进阶/测试".to_string(),
            duration: 100,
            total_pages: 3,
        };

        let out = expand_output_template(
            Some("/tmp/dl/{title}_p{page}.mp4"),
            &resolved,
            80,
            "hevc",
            "mp4",
            true,
        );
        assert_eq!(out, PathBuf::from("/tmp/dl/A_B_C______test_p2.mp4"));

        // Multi-page fixed template auto-appends _p{page}
        let fixed_multi =
            expand_output_template(Some("/tmp/dl/out.mp4"), &resolved, 80, "avc", "mp4", true);
        assert_eq!(fixed_multi, PathBuf::from("/tmp/dl/out_p2.mp4"));

        // Extensionless template with dots inside {title} preserves title intact
        let mut dotted_resolved = resolved.clone();
        dotted_resolved.title = "Part 1.5".to_string();
        let dotted_multi = expand_output_template(
            Some("/tmp/dl/{title}"),
            &dotted_resolved,
            80,
            "avc",
            "mp4",
            true,
        );
        assert_eq!(dotted_multi, PathBuf::from("/tmp/dl/Part 1.5_p2.mp4"));

        // Single-page extensionless template with dots and {page}/{codec} templates
        dotted_resolved.title = "v1.0 release".to_string();
        dotted_resolved.page = 1;
        assert_eq!(
            expand_output_template(Some("{title}"), &dotted_resolved, 80, "avc", "mp4", false),
            PathBuf::from("v1.0 release.mp4")
        );
        assert_eq!(
            expand_output_template(
                Some("{title}_p{page}"),
                &dotted_resolved,
                80,
                "avc",
                "mp4",
                true
            ),
            PathBuf::from("v1.0 release_p1.mp4")
        );
        assert_eq!(
            expand_output_template(
                Some("sub/my.video"),
                &dotted_resolved,
                80,
                "avc",
                "mp4",
                true
            ),
            PathBuf::from("sub/my.video_p1.mp4")
        );
        assert_eq!(
            expand_output_template(
                Some("{title}_{codec}.mkv"),
                &dotted_resolved,
                80,
                "hevc",
                "mp4",
                false
            ),
            PathBuf::from("v1.0 release_hevc.mkv")
        );
        assert_eq!(
            expand_output_template(
                Some("{title}_{codec}"),
                &dotted_resolved,
                80,
                "hevc",
                "mp4",
                false
            ),
            PathBuf::from("v1.0 release_hevc.mp4")
        );
        assert_eq!(
            stem_sibling_path(Path::new("v1.0 release.mp4"), ".png"),
            PathBuf::from("v1.0 release.png")
        );
        assert_eq!(
            stem_sibling_path(Path::new("v1.0 release"), ".png"),
            PathBuf::from("v1.0 release.png")
        );
    }

    #[test]
    fn test_select_subtitle_track_exact_prefix_and_missing() {
        let tracks = vec![
            json!({"lan": "zh-Hans", "lan_doc": "中文", "subtitle_url": "//a"}),
            json!({"lan": "en-US", "lan_doc": "English", "subtitle_url": "//b"}),
        ];
        assert_eq!(
            select_subtitle_track(&tracks, None).unwrap()["lan"],
            "zh-Hans"
        );
        assert_eq!(
            select_subtitle_track(&tracks, Some("ZH-HANS")).unwrap()["lan"],
            "zh-Hans"
        );
        assert_eq!(
            select_subtitle_track(&tracks, Some("en")).unwrap()["lan"],
            "en-US"
        );
        assert!(matches!(
            select_subtitle_track(&tracks, Some("ja-JP")).unwrap_err(),
            BiliLiveError::Input(_)
        ));
        assert!(matches!(
            select_subtitle_track(&[], Some("zh-Hans")).unwrap_err(),
            BiliLiveError::Input(_)
        ));
    }
}
