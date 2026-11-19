use crate::cli::args::{
    DanmakuFormat, DownloadDanmakuFormat, SubtitleFormat, VideoCommands, VideoCommentCommands,
    VideoStudioCommands,
};
use crate::cli::output::{is_json, is_quiet, print_json, print_spacer};
use crate::error::{BiliLiveError, Result};
use crate::ui::components::{
    Cell, Column, InfoCard, print_section_header, print_table, wrap_to_width,
};
use crate::ui::theme::{BILI_CYAN, BILI_PINK, color_enabled, paint_stderr, terminal_width};
use crate::utils::string::{
    format_duration, format_num, format_timestamp, sanitize_filename, truncate_to_width,
    unescape_html_entities,
};
use crate::{user_info, user_success, user_warning};
use crossterm::style::Color;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::Write;
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
            let start: u32 = start_s.trim().parse().map_err(|_| {
                BiliLiveError::Input(format!("无效的分P起始编号: {seg}"))
            })?;
            let end: u32 = end_s.trim().parse().map_err(|_| {
                BiliLiveError::Input(format!("无效的分P结束编号: {seg}"))
            })?;
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
            let p: u32 = seg.parse().map_err(|_| {
                BiliLiveError::Input(format!("无效的分P编号: {seg}"))
            })?;
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
    let is_dir_hint = raw_tpl.ends_with('/')
        || raw_tpl.ends_with('\\')
        || Path::new(&raw_tpl).is_dir();

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
        base.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video")
    } else {
        base.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("video")
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
            let (video_streams, audio_streams) =
                crate::api::video::parse_dash_streams(&playurl);
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
                Some(&format!("{} · P{} · cid={}", resolved.bvid, resolved.page, resolved.cid)),
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
                            Cell::plain(if s.frame_rate.is_empty() { "-" } else { &s.frame_rate }),
                            Cell::muted((s.bandwidth / 1000).to_string()),
                        ]
                    })
                    .collect();
                print_table(&cols, &rows);
                print_spacer();
            }

            if !audio_streams.is_empty() {
                print_section_header("可用音频流", Some(&format!("共 {} 条", audio_streams.len())));
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
                            Cell::plain(if s.mime_type.is_empty() { "-" } else { &s.mime_type }),
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
                print_section_header("首选直链地址 (未截断)", Some("需携带 Referer: https://www.bilibili.com/"));
                let mut stdout = std::io::stdout().lock();
                if let Some(best_v) = crate::api::video::select_dash_video(&video_streams, 127, "avc")
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
                if let Some(best_a) = crate::api::video::select_dash_audio_with_quality(&audio_streams, None)
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
                        "该视频字幕需要登录后才能获取，请先执行 `bt auth login`".to_string(),
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
                    Cell::plain(if summary_text.is_empty() { "-" } else { summary_text }),
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
                let wrap_w = crate::ui::theme::terminal_width().saturating_sub(14).max(36);
                for (idx, sec) in outline.iter().enumerate() {
                    let sec_ts = format_duration(sec["timestamp"].as_i64().unwrap_or(0));
                    let sec_title = sec["title"].as_str().unwrap_or("-");
                    if color {
                        use crossterm::style::Stylize;
                        println!(
                            "  {} {} {}",
                            format!("{}.", idx + 1).with(crate::ui::theme::BILI_PINK).bold(),
                            format!("[{sec_ts}]").with(crate::ui::theme::BILI_CYAN).bold(),
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
                DanmakuFormat::Ass => {
                    crate::api::danmaku::to_ass(&items, &format!("{} - {}", resolved.title, resolved.part))
                }
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
        } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let aid = crate::api::video::resolve_video_aid(&video, cookies.as_ref())?;

            if let Some(root_rpid) = root {
                let sub =
                    crate::api::comment::get_sub_replies(aid, root_rpid, page, cookies.as_ref())?;
                if is_json() {
                    print_json(&sub)?;
                    return Ok(());
                }

                print_spacer();
                if let Some(ref root_item) = sub.root
                    && root_item.rpid > 0
                {
                    let root_uname = root_item
                        .member
                        .as_ref()
                        .map(|m| m.uname.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("匿名用户");
                    let card = InfoCard::new("💬 根评论详情")
                        .with_badge(format!("RPID {}", root_item.rpid), BILI_CYAN)
                        .add_row("作者", Cell::bold_colored(root_uname, Color::White))
                        .add_row(
                            "互动",
                            Cell::pink(format!(
                                "{} 赞  ·  {} 条回复",
                                root_item.like, sub.page.count
                            )),
                        )
                        .add_plain_row("时间", format_timestamp(root_item.ctime))
                        .add_plain_row(
                            "内容",
                            unescape_html_entities(root_item.content.message.trim()),
                        );
                    card.print();
                    print_spacer();
                }

                if sub.replies.is_empty() {
                    user_info!("该评论下暂无楼中楼回复");
                    return Ok(());
                }

                print_section_header(
                    &format!("楼中楼回复 (root={root_rpid})"),
                    Some(&format!("第 {} 页 · 共 {} 条", page, sub.page.count)),
                );
                if !is_quiet() {
                    print_spacer();
                    let use_color = color_enabled();
                    let wrap_w = terminal_width().saturating_sub(6).max(32);
                    let mut stdout = std::io::stdout().lock();
                    for r in &sub.replies {
                        let uname = r
                            .member
                            .as_ref()
                            .map(|m| m.uname.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or("匿名用户");
                        let ts = format_timestamp(r.ctime);
                        let meta_line = if use_color {
                            use crossterm::style::Stylize;
                            format!(
                                "  {} {}  {}  {}",
                                format!("[{}]", r.rpid).with(BILI_CYAN).bold(),
                                uname.bold(),
                                format!("♥ {}", r.like).with(BILI_PINK),
                                ts.as_str().dark_grey()
                            )
                        } else {
                            format!("  [{}] {}  ♥ {}  {}", r.rpid, uname, r.like, ts)
                        };
                        if writeln!(stdout, "{meta_line}").is_err() {
                            break;
                        }
                        let clean_msg = unescape_html_entities(r.content.message.trim());
                        for seg in wrap_to_width(&clean_msg, wrap_w) {
                            if writeln!(stdout, "    {seg}").is_err() {
                                break;
                            }
                        }
                        let _ = writeln!(stdout);
                    }
                }
                return Ok(());
            }

            let data = crate::api::comment::get_comments(
                aid,
                page,
                sort.as_api_str(),
                cookies.as_ref(),
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            if data.replies.is_empty() {
                user_info!("暂无评论");
                return Ok(());
            }

            print_spacer();
            print_section_header(
                "视频评论",
                Some(&format!(
                    "第 {} 页 · 共 {} 条",
                    page, data.page.acount
                )),
            );

            if full {
                if !is_quiet() {
                    print_spacer();
                    let use_color = color_enabled();
                    let wrap_w = terminal_width().saturating_sub(6).max(32);
                    let mut stdout = std::io::stdout().lock();
                    for r in &data.replies {
                        let uname = r
                            .member
                            .as_ref()
                            .map(|m| m.uname.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or("匿名用户");
                        let ts = format_timestamp(r.ctime);
                        let header = if use_color {
                            use crossterm::style::Stylize;
                            format!(
                                "  {} {}  {}  {}  {}",
                                format!("[{}]", r.rpid).with(BILI_CYAN).bold(),
                                uname.bold(),
                                format!("♥ {}", r.like).with(BILI_PINK),
                                format!("💬 {}", r.rcount).dark_grey(),
                                ts.as_str().dark_grey()
                            )
                        } else {
                            format!(
                                "  [{}] {}  ♥ {}  💬 {}  {}",
                                r.rpid, uname, r.like, r.rcount, ts
                            )
                        };
                        if writeln!(stdout, "{header}").is_err() {
                            break;
                        }
                        let clean_msg = unescape_html_entities(r.content.message.trim());
                        for seg in wrap_to_width(&clean_msg, wrap_w) {
                            if writeln!(stdout, "    {seg}").is_err() {
                                break;
                            }
                        }
                        for child in &r.replies {
                            let c_name = child
                                .member
                                .as_ref()
                                .map(|m| m.uname.as_str())
                                .filter(|s| !s.is_empty())
                                .unwrap_or("匿名用户");
                            let c_msg =
                                unescape_html_entities(&child.content.message).replace('\n', " ");
                            let sub_line = format!(
                                "↳ [{}] {}: {}",
                                child.rpid,
                                c_name,
                                truncate_to_width(c_msg.trim(), wrap_w.saturating_sub(6))
                            );
                            if use_color {
                                use crossterm::style::Stylize;
                                let _ = writeln!(stdout, "      {}", sub_line.dark_grey());
                            } else {
                                let _ = writeln!(stdout, "      {sub_line}");
                            }
                        }
                        let _ = writeln!(stdout);
                    }
                }
                return Ok(());
            }

            let cols = [
                Column::right("RPID", 14),
                Column::left("用户", 16),
                Column::right("赞", 6),
                Column::right("回复", 6),
                Column::left("时间", 16),
                Column::left("评论内容", 36),
            ];
            let rows: Vec<Vec<Cell>> = data
                .replies
                .iter()
                .map(|r| {
                    let uname = r
                        .member
                        .as_ref()
                        .map(|m| m.uname.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("匿名用户");
                    let msg = unescape_html_entities(&r.content.message).replace('\n', " ");
                    vec![
                        Cell::cyan(r.rpid.to_string()),
                        Cell::plain(truncate_to_width(uname, 16)),
                        Cell::pink(r.like.to_string()),
                        Cell::plain(r.rcount.to_string()),
                        Cell::muted(format_timestamp(r.ctime)),
                        Cell::plain(truncate_to_width(msg.trim(), 36)),
                    ]
                })
                .collect();
            print_table(&cols, &rows);
            print_spacer();
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
                user_success!("{}成功: {}", if cancel { "取消点赞" } else { "点赞" }, video);
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
                    if also_like { "，并已同步点赞" } else { "" }
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

                let (dash_videos, dash_audios) =
                    crate::api::video::parse_dash_streams(&playurl);
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

                    let need_mux =
                        !audio_only && !video_only && !skip_mux && v_stream.is_some() && a_stream.is_some();

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
                            let _tmp_reg = TmpRegGuard(
                                v_tmp.path().to_path_buf(),
                                a_tmp.path().to_path_buf(),
                            );

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
                            let a_out = stem_sibling_path(&out_path, &format!(".audio.{audio_ext}"));
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
                            return Err(BiliLiveError::Api("未找到可下载的 DASH 媒体流".to_string()));
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
                            let seg_path = stem_sibling_path(&out_path, &format!("_part{}.mp4", idx + 1));
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
                                && let Some(track) = tracks
                                    .iter()
                                    .find(|t| !t["subtitle_url"].as_str().unwrap_or("").trim().is_empty())
                            {
                                let sub_url = track["subtitle_url"].as_str().unwrap_or("");
                                match crate::api::video::fetch_subtitle_json(sub_url) {
                                    Ok(sub_json) => {
                                        let body =
                                            sub_json["body"].as_array().cloned().unwrap_or_default();
                                        let srt = render_subtitle_srt(&body);
                                        let sub_path = stem_sibling_path(&primary_path, ".srt");
                                        if let Err(e) = ensure_parent_dir(&sub_path).and_then(|()| {
                                            std::fs::write(&sub_path, srt.as_bytes()).map_err(Into::into)
                                        }) {
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
                    let xml = crate::api::danmaku::fetch_danmaku_xml(resolved.cid, cookies.as_ref())?;
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

        VideoCommands::Upload {
            files,
            title,
            tid,
            tag,
            desc,
            copyright,
            source,
            cover,
            dynamic,
            dtime,
            no_reprint,
            line,
            limit,
        } => {
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

            let effective_tag = match tag {
                Some(ref raw) => crate::api::upload::normalize_tags(raw)?,
                None => "视频,日常".to_string(),
            };

            let parsed_dtime = dtime.as_deref().map(parse_schedule_dtime).transpose()?;

            let cookies = crate::auth::ensure_csrf()?;

            let cover_url = match cover {
                Some(ref c) if !c.trim().is_empty() => {
                    user_info!("正在处理稿件封面...");
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
                dynamic,
                dtime: parsed_dtime,
                no_reprint,
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
                    "cover": cover_url,
                    "parts": uploaded_parts,
                    "data": res,
                }))?;
            } else {
                let card = InfoCard::new("🎉 视频稿件投递成功")
                    .with_badge(
                        if bvid.is_empty() {
                            format!("av{aid}")
                        } else {
                            bvid.clone()
                        },
                        Color::Green,
                    )
                    .add_row("BV号", Cell::cyan(if bvid.is_empty() { "-" } else { &bvid }))
                    .add_plain_row("AV号", format!("av{aid}"))
                    .add_row("标题", Cell::bold_colored(&effective_title, Color::White))
                    .add_plain_row("分P数量", uploaded_parts.len().to_string())
                    .add_plain_row("分区 TID", tid.to_string())
                    .add_plain_row("标签", &effective_tag);
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
                                    .unwrap_or(if state >= 0 { "已通过" } else { "审核/退回" });
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
                    .unwrap_or(if state >= 0 { "正常开放" } else { "处理中/受限" });
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
                    print_section_header("分P转码与审核明细", Some(&format!("共 {} P", videos.len())));
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
                _ => return Err(BiliLiveError::Input(format!("不支持的时长单位 '{ch}' (支持 d/h/m/s)"))),
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
        assert_eq!(
            out,
            PathBuf::from("/tmp/dl/A_B_C______test_p2.mp4")
        );

        // Multi-page fixed template auto-appends _p{page}
        let fixed_multi = expand_output_template(
            Some("/tmp/dl/out.mp4"),
            &resolved,
            80,
            "avc",
            "mp4",
            true,
        );
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
