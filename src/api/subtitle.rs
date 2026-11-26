//! B 站字幕轨上传、查询与删除。
//!
//! 接口来源：`bilibili_api` 17.4.2 的 `data/api/video.json` 中
//! `operate.submit_subtitle` 的定义，以及创作中心前端懒加载分块
//! （`creativecenter-v3` 的 chunk 3274 / 1459）里的 `dm/subtitle` 调用。
//! 这一族接口不在任何公开文档里，路径统一挂在 `/x/v2/dm/subtitle/` 下。
//!
//! 字幕绑定在 **cid（分P）** 上，不绑 aid；同一个 `(cid, lan)` 可以存在多条记录，
//! 那是不同版本，上传的语义是「新增一个版本」，不是覆盖旧版本。

use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};
use serde::{Deserialize, Serialize};

/// 创作中心字幕管理页的 Referer/Origin，`dm/subtitle` 系列接口需要它们
pub const SUBTITLE_REFERER: &str =
    "https://member.bilibili.com/platform/upload-manager/audience-zimu";
pub const SUBTITLE_ORIGIN: &str = "https://member.bilibili.com";

/// 上传侧只接受基础 ISO 语言码（实测 `zh-Hans`、`zh-CN`、`en-US`、`jp`、`ai-zh`
/// 都会返回 `79011 不合法的语言`）；注意与下载侧 `--lang` 的匹配规则不同。
pub const SUPPORTED_LANS: [&str; 5] = ["en", "zh", "zh-Hant", "ja", "ko"];

/// BCC（Bilibili Closed Caption）单条字幕
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BccCue {
    pub from: f64,
    pub to: f64,
    pub location: u8,
    pub content: String,
}

/// BCC 字幕文件。`data` 参数要求传这段 JSON 的**字符串**，字段顺序与 `Stroke` 大小写都必须一致。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bcc {
    pub font_size: f64,
    pub font_color: String,
    pub background_alpha: f64,
    pub background_color: String,
    #[serde(rename = "Stroke")]
    pub stroke: String,
    pub body: Vec<BccCue>,
}

impl Bcc {
    /// 与真机参考脚本一致的默认样式
    pub fn new(body: Vec<BccCue>) -> Self {
        Self {
            font_size: 0.4,
            font_color: "#FFFFFF".to_string(),
            background_alpha: 0.5,
            background_color: "#9C27B0".to_string(),
            stroke: "none".to_string(),
            body,
        }
    }
}

/// SRT 解析统计
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SrtStats {
    /// 成功匹配到时间轴的块数
    pub parsed: usize,
    /// 最终写入 BCC 的条目数
    pub kept: usize,
    /// 时间轴合法但内容为空等被丢弃的条目数
    pub skipped: usize,
}

/// 解析 SRT 原文并转换为 BCC。
///
/// 规则与真机参考实现 `subtitle_upload.ps1` 的 `ConvertFrom-Srt` 逐条对齐：
/// 去 BOM、统一换行、按空行切块、识别序号行、解析时间轴、删除 `<...>` 样式标签、
/// 强制时间轴单调递增，最后要求至少保留一条。
pub fn srt_to_bcc(text: &str) -> Result<(Bcc, SrtStats)> {
    let normalized = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    let mut cues: Vec<BccCue> = Vec::new();
    let mut stats = SrtStats::default();

    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block
            .split('\n')
            .filter(|line| !line.trim().is_empty())
            .collect();
        if lines.len() < 2 {
            continue;
        }

        let time_idx = if is_pure_digits(lines[0]) { 1 } else { 0 };
        if time_idx >= lines.len() {
            continue;
        }

        let Some((mut from, mut to)) = parse_time_range(lines[time_idx]) else {
            continue;
        };
        stats.parsed += 1;

        // 时间轴行之后没有内容时直接丢弃。参考脚本在这种畸形块上会因为 PowerShell
        // 反向切片（`$lines[2..1]`）把时间轴行本身当成正文，这里不复刻这个副作用。
        let joined = lines[time_idx + 1..].join("\n");
        let trimmed = joined.trim();
        let content = strip_style_tags(trimmed);
        if content.is_empty() {
            stats.skipped += 1;
            continue;
        }

        // 累加式时间轴会让 B 站播放器错乱（叠字、跳条），强制单调递增
        if let Some(last) = cues.last()
            && from < last.to
        {
            from = last.to;
        }
        if to <= from {
            to = from + 0.1;
        }

        cues.push(BccCue {
            from: round2(from),
            to: round2(to),
            location: 2,
            content,
        });
    }

    if cues.is_empty() {
        return Err(BiliLiveError::Parse(format!(
            "SRT 解析后没有任何可上传条目（解析到 {} 条，全部被丢弃）",
            stats.parsed
        )));
    }
    stats.kept = cues.len();

    Ok((Bcc::new(cues), stats))
}

/// 归一化并校验上传用的语言码。
///
/// 白名单内的值原样保留（包括合法的复合码 `zh-Hant`）；其余输入先取 `-` 前的主码
/// （`en-US` → `en`、`zh-CN` → `zh`），再映射 `zh-Hans` → `zh`，最后按白名单校验。
/// 不在白名单内的值在发请求前直接报 `Input`。
pub fn normalize_lan(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(BiliLiveError::Input(
            "字幕语言代码不能为空；允许的取值：en, zh, zh-Hant, ja, ko".to_string(),
        ));
    }

    if let Some(hit) = SUPPORTED_LANS
        .iter()
        .find(|lan| lan.eq_ignore_ascii_case(trimmed))
    {
        return Ok((*hit).to_string());
    }

    let primary = trimmed
        .split('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let normalized = match primary.as_str() {
        "zh-hans" => "zh".to_string(),
        other => other.to_string(),
    };

    if SUPPORTED_LANS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(BiliLiveError::Input(format!(
            "不支持的字幕语言代码: {raw}（归一化后为 {normalized}）；允许的取值：{}",
            SUPPORTED_LANS.join(", ")
        )))
    }
}

/// `status` 字段的可读文本：1 草稿、4 已删除、5 已发布
pub fn status_text(status: i64) -> &'static str {
    match status {
        1 => "草稿",
        4 => "已删除",
        5 => "已发布",
        _ => "未知",
    }
}

/// POST /x/v2/dm/subtitle/draft/save
///
/// `submit=true` 表示提交审核（表单 `submit=1`），`false` 仅存草稿（`submit=0`）。
/// `sign=true` 对应 `sign=1`（自制字幕签名），默认 `sign=0`。
///
/// 注意：该接口的响应体结构没有被完整记录，判断是否真正落库必须回查 `author/list`。
pub fn submit_subtitle(
    cookies: &Cookies,
    cid: i64,
    bvid: &str,
    lan: &str,
    bcc: &Bcc,
    submit: bool,
    sign: bool,
) -> Result<serde_json::Value> {
    if cid <= 0 {
        return Err(BiliLiveError::Input("字幕目标 cid 必须大于 0".to_string()));
    }
    if bvid.trim().is_empty() {
        return Err(BiliLiveError::Input("字幕目标 bvid 不能为空".to_string()));
    }

    let data = serde_json::to_string(bcc)?;
    let params: Vec<(&str, String)> = vec![
        ("type", "1".to_string()),
        ("oid", cid.to_string()),
        ("bvid", bvid.to_string()),
        ("lan", lan.to_string()),
        ("data", data),
        ("submit", if submit { "1" } else { "0" }.to_string()),
        ("sign", if sign { "1" } else { "0" }.to_string()),
        ("csrf", cookies.bili_jct.clone()),
        ("csrf_token", cookies.bili_jct.clone()),
    ];

    let json = post_subtitle_form("/x/v2/dm/subtitle/draft/save", cookies, &params, "提交字幕")?;
    Ok(json["data"].clone())
}

/// POST /x/v2/dm/subtitle/del
///
/// 软删除：条目不会消失，`status` 变成 4。
pub fn delete_subtitle(cookies: &Cookies, cid: i64, subtitle_id: &str) -> Result<()> {
    if cid <= 0 {
        return Err(BiliLiveError::Input("字幕目标 cid 必须大于 0".to_string()));
    }
    let subtitle_id = subtitle_id.trim();
    if subtitle_id.is_empty() {
        return Err(BiliLiveError::Input("字幕 ID 不能为空".to_string()));
    }

    let params: Vec<(&str, String)> = vec![
        ("type", "1".to_string()),
        ("oid", cid.to_string()),
        ("subtitle_id", subtitle_id.to_string()),
        ("csrf", cookies.bili_jct.clone()),
        ("csrf_token", cookies.bili_jct.clone()),
    ];
    post_subtitle_form("/x/v2/dm/subtitle/del", cookies, &params, "删除字幕")?;
    Ok(())
}

/// GET /x/v2/dm/subtitle/search/author/list
///
/// 当前账号的全部字幕（含草稿和已删除）。`status` 为 `None` 时发送 `0`（全量）。
/// 这是唯一稳定的查询接口；`search/list`、`search/cid` 在真机上会间歇性返回 `-400`。
pub fn list_my_subtitles(
    cookies: &Cookies,
    page: u32,
    size: u32,
    status: Option<i64>,
) -> Result<serde_json::Value> {
    let query = serde_urlencoded::to_string([
        ("page", page.to_string()),
        ("size", size.to_string()),
        ("status", status.unwrap_or(0).to_string()),
    ])
    .map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let url = format!(
        "{}/x/v2/dm/subtitle/search/author/list?{query}",
        crate::api::client::api_base_url()
    );

    let resp = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", SUBTITLE_REFERER)
        .with_header("Origin", SUBTITLE_ORIGIN)
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let json = parse_subtitle_response(&resp, "查询字幕列表")?;
    Ok(json["data"].clone())
}

/// 翻页拉取当前账号的全部字幕，返回（全部条目, 账号字幕总数）。
///
/// `data.subtitles` 是记录数组，按 `mtime` 倒序；`data.total` 与 `data.page.total`
/// 都是账号字幕总数（实测两者一致）。
pub fn list_all_subtitles(cookies: &Cookies, size: u32) -> Result<(Vec<serde_json::Value>, i64)> {
    let size = size.clamp(1, 50);
    let mut all = Vec::new();
    let mut account_total = 0i64;
    let mut page = 1u32;

    loop {
        let data = list_my_subtitles(cookies, page, size, None)?;
        let items = data["subtitles"]
            .as_array()
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        account_total = data["total"]
            .as_i64()
            .or_else(|| data["page"]["total"].as_i64())
            .unwrap_or(account_total);

        let effective_size = data["page"]["size"]
            .as_i64()
            .filter(|&s| s > 0)
            .unwrap_or(size as i64);
        let total = data["page"]["total"]
            .as_i64()
            .or_else(|| data["total"].as_i64())
            .unwrap_or(account_total);
        let last_page = if total <= 0 {
            1
        } else {
            ((total + effective_size - 1) / effective_size).max(1) as u32
        };

        all.extend(items.iter().cloned());
        if items.is_empty() || page >= last_page {
            break;
        }
        page += 1;
    }

    Ok((all, account_total))
}

fn post_subtitle_form(
    path: &str,
    cookies: &Cookies,
    params: &[(&str, String)],
    action: &str,
) -> Result<serde_json::Value> {
    let body =
        serde_urlencoded::to_string(params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let url = format!("{}{path}", crate::api::client::api_base_url());
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", SUBTITLE_REFERER)
        .with_header("Origin", SUBTITLE_ORIGIN)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    parse_subtitle_response(&resp, action)
}

fn parse_subtitle_response(
    resp: &minreq::Response,
    action: &str,
) -> Result<serde_json::Value> {
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "{action}失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["message"].as_str().unwrap_or("未知错误");
        return Err(map_subtitle_api_error(code, msg, action));
    }
    Ok(json)
}

fn map_subtitle_api_error(code: i64, msg: &str, action: &str) -> BiliLiveError {
    match code {
        -101 => BiliLiveError::Auth("账号未登录，请先执行 `bit auth login` 进行登录".to_string()),
        -111 => BiliLiveError::Auth(
            "CSRF 校验失败（-111），请重新登录以刷新 bili_jct 后再试".to_string(),
        ),
        79011 => BiliLiveError::Input(format!(
            "不合法的字幕语言代码（79011：{msg}）；允许的取值：{}",
            SUPPORTED_LANS.join(", ")
        )),
        -400 => BiliLiveError::Api(format!(
            "{action}失败: -400 请求错误，通常是参数或表单编码问题（{msg}）"
        )),
        _ => BiliLiveError::Api(format!("{action}失败: {msg} (code={code})")),
    }
}

/// SRT 时间戳正则的等价实现：
/// `^\s*(\d+):(\d{1,2}):(\d{1,2})[,.](\d{1,3})\s*-->\s*(\d+):(\d{1,2}):(\d{1,2})[,.](\d{1,3})`
fn parse_time_range(line: &str) -> Option<(f64, f64)> {
    let rest = line.trim_start();
    let (from, rest) = parse_timestamp(rest)?;
    let rest = rest.trim_start().strip_prefix("-->")?;
    let (to, _) = parse_timestamp(rest.trim_start())?;
    Some((from, to))
}

fn parse_timestamp(input: &str) -> Option<(f64, &str)> {
    let (hours, rest) = take_digits(input, 1, usize::MAX)?;
    let rest = rest.strip_prefix(':')?;
    let (minutes, rest) = take_digits(rest, 1, 2)?;
    let rest = rest.strip_prefix(':')?;
    let (seconds, rest) = take_digits(rest, 1, 2)?;
    let rest = rest
        .strip_prefix(',')
        .or_else(|| rest.strip_prefix('.'))?;
    let (millis, rest) = take_digits(rest, 1, 3)?;

    let total = hours as f64 * 3600.0
        + minutes as f64 * 60.0
        + seconds as f64
        + millis as f64 / 1000.0;
    Some((total, rest))
}

fn take_digits(input: &str, min: usize, max: usize) -> Option<(u64, &str)> {
    let bytes = input.as_bytes();
    let mut count = 0;
    while count < bytes.len() && count < max && bytes[count].is_ascii_digit() {
        count += 1;
    }
    if count < min {
        return None;
    }
    let (digits, rest) = input.split_at(count);
    Some((digits.parse().ok()?, rest))
}

fn is_pure_digits(line: &str) -> bool {
    !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit())
}

/// 删除 `<...>` 形式的样式标签；未闭合的 `<` 或空标签 `<>` 按字面保留。
fn strip_style_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let after_open = &rest[open + 1..];
        match after_open.find('>') {
            Some(close) if close > 0 => {
                out.push_str(&rest[..open]);
                rest = &after_open[close + 1..];
            }
            _ => {
                out.push_str(&rest[..=open]);
                rest = after_open;
            }
        }
    }
    out.push_str(rest);
    out
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(from: f64, to: f64, content: &str) -> BccCue {
        BccCue {
            from,
            to,
            location: 2,
            content: content.to_string(),
        }
    }

    #[test]
    fn parses_basic_srt_with_index_and_style_tags() {
        let srt = "\u{feff}1\r\n00:00:01,500 --> 00:00:03,000\r\n<i>Hello</i>\r\n\r\n2\r\n00:00:04.000 --> 00:00:05.250\r\n第二行\r\n继续\r\n";
        let (bcc, stats) = srt_to_bcc(srt).unwrap();
        assert_eq!(stats.parsed, 2);
        assert_eq!(stats.kept, 2);
        assert_eq!(stats.skipped, 0);
        assert_eq!(bcc.body[0], cue(1.5, 3.0, "Hello"));
        assert_eq!(bcc.body[1], cue(4.0, 5.25, "第二行\n继续"));
        assert_eq!(bcc.font_size, 0.4);
        assert_eq!(bcc.background_color, "#9C27B0");
        assert_eq!(bcc.stroke, "none");
        assert_eq!(bcc.body[0].location, 2);
    }

    #[test]
    fn enforces_monotonic_timeline_and_min_duration() {
        let srt = "1\n00:00:01,000 --> 00:00:05,000\nA\n\n2\n00:00:03,000 --> 00:00:04,000\nB\n\n3\n00:00:06,000 --> 00:00:06,000\nC\n";
        let (bcc, stats) = srt_to_bcc(srt).unwrap();
        assert_eq!(stats.kept, 3);
        assert_eq!(bcc.body[1].from, 5.0);
        assert_eq!(bcc.body[1].to, 5.0 + 0.1);
        assert_eq!(bcc.body[2].from, 6.0);
        assert_eq!(bcc.body[2].to, 6.1);
    }

    #[test]
    fn drops_empty_content_blocks_and_unparsable_blocks() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\n<i></i>\n\nnot a subtitle\n\n2\n00:00:03,000 --> 00:00:04,000\n";
        let err = srt_to_bcc(srt).unwrap_err();
        assert!(matches!(err, BiliLiveError::Parse(_)));

        let srt2 = "1\n00:00:01,000 --> 00:00:02,000\n<i></i>\n\n2\n00:00:03,000 --> 00:00:04,000\nOK\n";
        let (bcc, stats) = srt_to_bcc(srt2).unwrap();
        assert_eq!(stats.parsed, 2);
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.kept, 1);
        assert_eq!(bcc.body[0].content, "OK");
    }

    #[test]
    fn serializes_bcc_with_expected_field_order_and_stroke_casing() {
        let (bcc, _) = srt_to_bcc("1\n00:00:00,050 --> 00:00:01,050\n第一行\n").unwrap();
        let json = serde_json::to_string(&bcc).unwrap();
        assert!(json.starts_with(r##"{"font_size":0.4,"font_color":"#FFFFFF","background_alpha":0.5,"background_color":"#9C27B0","Stroke":"none","body":[{"from":0.05,"to":1.05,"location":2,"content":"第一行"}]}"##));
    }

    #[test]
    fn normalizes_language_codes() {
        assert_eq!(normalize_lan("zh").unwrap(), "zh");
        assert_eq!(normalize_lan("zh-Hans").unwrap(), "zh");
        assert_eq!(normalize_lan("zh-CN").unwrap(), "zh");
        assert_eq!(normalize_lan("en-US").unwrap(), "en");
        assert_eq!(normalize_lan("zh-Hant").unwrap(), "zh-Hant");
        assert_eq!(normalize_lan("EN").unwrap(), "en");
        assert!(matches!(normalize_lan("jp"), Err(BiliLiveError::Input(_))));
        assert!(matches!(
            normalize_lan("ai-zh"),
            Err(BiliLiveError::Input(_))
        ));
        assert!(matches!(normalize_lan("  "), Err(BiliLiveError::Input(_))));
    }

    #[test]
    fn strip_style_tags_keeps_unclosed_and_empty_tags() {
        assert_eq!(strip_style_tags("a<b>c</b>d"), "acd");
        assert_eq!(strip_style_tags("a<>b"), "a<>b");
        assert_eq!(strip_style_tags("a<b"), "a<b");
        assert_eq!(strip_style_tags("{\\an8}<font color=\"#fff\">x</font>"), "{\\an8}x");
    }

    #[test]
    fn status_text_covers_known_statuses() {
        assert_eq!(status_text(1), "草稿");
        assert_eq!(status_text(4), "已删除");
        assert_eq!(status_text(5), "已发布");
        assert_eq!(status_text(9), "未知");
    }
}
