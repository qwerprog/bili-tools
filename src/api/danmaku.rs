use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};
use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use serde::Serialize;
use std::io::Read;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DanmakuItem {
    pub time: f64,
    pub mode: u8,
    pub font_size: u32,
    pub color: u32,
    pub color_hex: String,
    pub send_time: i64,
    pub pool: u8,
    pub sender_hash: String,
    pub dmid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<i32>,
    pub text: String,
}

/// 获取视频弹幕原始 XML：优先 `{comment_base_url}/{cid}.xml`，失败则回退 `{api_base_url}/x/v1/dm/list.so?oid={cid}`
pub fn fetch_danmaku_xml(cid: i64, cookies: Option<&Cookies>) -> Result<String> {
    let primary_url = format!("{}/{}.xml", crate::api::client::comment_base_url(), cid);
    let fallback_url = format!(
        "{}/x/v1/dm/list.so?oid={}",
        crate::api::client::api_base_url(),
        cid
    );

    let mut last_err: Option<BiliLiveError> = None;
    for url in [&primary_url, &fallback_url] {
        match crate::api::client::get_bytes(url, Some("https://www.bilibili.com/"), cookies) {
            Ok(raw) => {
                let encoding = raw.headers.get("content-encoding").map(|s| s.as_str());
                match decode_danmaku_bytes(&raw.body, encoding) {
                    Ok(xml) => {
                        let trimmed = xml.trim_start_matches('\u{feff}').trim_start();
                        if trimmed.starts_with('<') {
                            return Ok(xml);
                        }
                        last_err = Some(BiliLiveError::Parse(format!(
                            "弹幕响应不是有效的 XML ({url})"
                        )));
                    }
                    Err(e) => {
                        last_err = Some(e);
                    }
                }
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| BiliLiveError::Api("获取弹幕 XML 失败".to_string())))
}

/// 解码弹幕响应字节：兼容 gzip、原始 RFC 1951 deflate、zlib 及纯 UTF-8 XML
pub fn decode_danmaku_bytes(bytes: &[u8], content_encoding: Option<&str>) -> Result<String> {
    if bytes.is_empty() {
        return Err(BiliLiveError::Parse("弹幕响应为空".to_string()));
    }

    let enc = content_encoding.unwrap_or("").trim().to_ascii_lowercase();
    let stripped_bom = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);

    let try_gzip = |data: &[u8]| -> Option<String> {
        let mut dec = GzDecoder::new(data);
        let mut s = String::new();
        dec.read_to_string(&mut s).ok().map(|_| s)
    };
    let try_raw_deflate = |data: &[u8]| -> Option<String> {
        let mut dec = DeflateDecoder::new(data);
        let mut s = String::new();
        dec.read_to_string(&mut s).ok().map(|_| s)
    };
    let try_zlib = |data: &[u8]| -> Option<String> {
        let mut dec = ZlibDecoder::new(data);
        let mut s = String::new();
        dec.read_to_string(&mut s).ok().map(|_| s)
    };
    let try_plain_utf8 =
        |data: &[u8]| -> Option<String> { std::str::from_utf8(data).ok().map(|s| s.to_string()) };

    if (enc.contains("gzip") || bytes.starts_with(&[0x1F, 0x8B]))
        && let Some(s) = try_gzip(bytes)
    {
        return Ok(s);
    }

    if enc.contains("deflate")
        && let Some(s) = try_raw_deflate(bytes).or_else(|| try_zlib(bytes))
    {
        return Ok(s);
    }

    // 检查未压缩纯 XML（跳过 BOM 与前导空白）
    let first_non_ws = stripped_bom
        .iter()
        .copied()
        .find(|b| !b.is_ascii_whitespace());
    if first_non_ws == Some(b'<')
        && let Some(s) = try_plain_utf8(stripped_bom)
    {
        return Ok(s);
    }

    // 通用探测顺序：raw RFC 1951 deflate -> zlib -> gzip -> plain UTF-8
    if let Some(s) = try_raw_deflate(bytes)
        .or_else(|| try_zlib(bytes))
        .or_else(|| try_gzip(bytes))
        .or_else(|| try_plain_utf8(stripped_bom))
    {
        return Ok(s);
    }

    Err(BiliLiveError::Parse(
        "无法解压或解码弹幕 XML 数据".to_string(),
    ))
}

/// 单遍 XML 实体反转义（避免链式 replace 造成二次反转义）
pub fn unescape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let bytes = input.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'&'
            && let Some(rel_semi) = input[i..].find(';')
        {
            // 实体长度合理上限，避免把普通文本中的 `&...;` 跨行误判
            if rel_semi > 1 && rel_semi <= 16 {
                let entity = &input[i + 1..i + rel_semi];
                let decoded: Option<char> = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                        u32::from_str_radix(&entity[2..], 16)
                            .ok()
                            .and_then(char::from_u32)
                    }
                    _ if entity.starts_with('#') => {
                        entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                    }
                    _ => None,
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                    i += rel_semi + 1;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// 引号感知地定位 `<tag ...>` 的结束 `>` 位置（以及是否自闭合 `/>`）
fn find_tag_end(s: &str, start_after_open: usize) -> Option<(usize, bool)> {
    let mut quote: Option<u8> = None;
    let bytes = s.as_bytes();
    let mut i = start_after_open;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => {
                if b == b'"' || b == b'\'' {
                    quote = Some(b);
                } else if b == b'>' {
                    let self_closing = i > start_after_open && bytes[i - 1] == b'/';
                    return Some((i, self_closing));
                }
            }
        }
        i += 1;
    }
    None
}

/// 从标签属性串中提取指定属性的值（支持单引号和双引号）
fn extract_attr(attrs: &str, target: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let name = &attrs[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let q = bytes[i];
                i += 1;
                let val_start = i;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
                let val = &attrs[val_start..i];
                if i < bytes.len() {
                    i += 1;
                }
                if name == target {
                    return Some(unescape_xml(val));
                }
            } else {
                let val_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                let val = &attrs[val_start..i];
                if name == target {
                    return Some(unescape_xml(val));
                }
            }
        }
    }
    None
}

/// 在 `<d ...>` 节点正文内扫描 `</d>` 闭合标签，跳过 `<![CDATA[...]]>` 区间以避免内容中的 `</d>` 提前截断
fn find_d_close(xml: &str, mut pos: usize) -> Option<usize> {
    while pos < xml.len() {
        let rel = xml[pos..].find('<')?;
        pos += rel;
        if xml[pos..].starts_with("<![CDATA[") {
            let end = xml[pos + 9..].find("]]>")?;
            pos += 9 + end + 3;
        } else if xml[pos..].starts_with("</d>") {
            return Some(pos);
        } else {
            pos += 1;
        }
    }
    None
}

/// 解析 B站弹幕 XML 中所有 `<d p="...">text</d>` 节点并按出现时间升序排列
pub fn parse_danmaku_xml(xml: &str) -> Result<Vec<DanmakuItem>> {
    let mut items = Vec::new();
    let mut i = 0;
    let len = xml.len();

    while i < len {
        let Some(rel_lt) = xml[i..].find('<') else {
            break;
        };
        i += rel_lt;

        if xml[i..].starts_with("<!--") {
            if let Some(end) = xml[i + 4..].find("-->") {
                i += 4 + end + 3;
                continue;
            } else {
                break;
            }
        }
        if xml[i..].starts_with("<?") {
            if let Some(end) = xml[i + 2..].find("?>") {
                i += 2 + end + 2;
                continue;
            } else {
                break;
            }
        }
        if xml[i..].starts_with("<![CDATA[") {
            if let Some(end) = xml[i + 9..].find("]]>") {
                i += 9 + end + 3;
                continue;
            } else {
                break;
            }
        }

        if xml[i..].starts_with("<d") {
            let after_d = i + 2;
            let next_byte = xml.as_bytes().get(after_d).copied();
            if matches!(next_byte, Some(b) if b.is_ascii_whitespace() || b == b'>' || b == b'/') {
                let Some((tag_end, self_closing)) = find_tag_end(xml, after_d) else {
                    break;
                };
                if self_closing {
                    i = tag_end + 1;
                    continue;
                }
                let attr_end = tag_end;
                let attrs_slice = &xml[after_d..attr_end];
                let content_start = tag_end + 1;
                let Some(close_pos) = find_d_close(xml, content_start) else {
                    break;
                };
                let raw_inner = &xml[content_start..close_pos];
                i = close_pos + 4;

                let Some(p_attr) = extract_attr(attrs_slice, "p") else {
                    continue;
                };
                let parts: Vec<&str> = p_attr.split(',').collect();
                if parts.len() < 2 {
                    continue;
                }
                let Ok(time) = parts[0].trim().parse::<f64>() else {
                    continue;
                };
                if !time.is_finite() || time < 0.0 {
                    continue;
                }
                let Ok(mode) = parts[1].trim().parse::<u8>() else {
                    continue;
                };
                let font_size = parts
                    .get(2)
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(25);
                let color = parts
                    .get(3)
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(0xFFFFFF)
                    & 0x00FF_FFFF;
                let send_time = parts
                    .get(4)
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .unwrap_or(0);
                let pool = parts
                    .get(5)
                    .and_then(|s| s.trim().parse::<u8>().ok())
                    .unwrap_or(0);
                let sender_hash = parts
                    .get(6)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let dmid = parts
                    .get(7)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let weight = parts.get(8).and_then(|s| s.trim().parse::<i32>().ok());

                let trimmed_inner = raw_inner.trim();
                let text = if let Some(rest) = trimmed_inner.strip_prefix("<![CDATA[")
                    && let Some(inner_cdata) = rest.strip_suffix("]]>")
                {
                    inner_cdata.to_string()
                } else {
                    unescape_xml(raw_inner)
                };

                items.push(DanmakuItem {
                    time,
                    mode,
                    font_size,
                    color,
                    color_hex: format!("#{:06X}", color),
                    send_time,
                    pool,
                    sender_hash,
                    dmid,
                    weight,
                    text,
                });
                continue;
            }
        }

        i += 1;
    }

    items.sort_by(|a, b| a.time.total_cmp(&b.time));
    Ok(items)
}

/// 格式化 ASS 时间戳：`H:MM:SS.cc`（四舍五入到厘秒，避免 59.999 进位异常）
pub fn format_ass_time(secs: f64) -> String {
    let total_cs = (secs.max(0.0) * 100.0).round() as u64;
    let h = total_cs / 360_000;
    let m = (total_cs / 6_000) % 60;
    let s = (total_cs / 100) % 60;
    let cs = total_cs % 100;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

/// 将 RGB888 颜色转换为 ASS BGR 十六进制格式 `&HBBGGRR&`
pub fn format_ass_color(rgb: u32) -> String {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    format!("&H{b:02X}{g:02X}{r:02X}&")
}

/// 转义 ASS 对话文本中的反斜杠、花括号及换行符
pub fn escape_ass_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    for ch in normalized.chars() {
        match ch {
            '\\' => out.push_str("\\\u{200b}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\N"),
            _ => out.push(ch),
        }
    }
    out
}

fn estimate_pixel_width(text: &str, px_size: f64) -> f64 {
    let mut total = 0.0;
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        let w = UnicodeWidthChar::width(ch).unwrap_or(1);
        total += if w >= 2 {
            1.0 * px_size
        } else {
            0.58 * px_size
        };
    }
    total.max(px_size)
}

#[derive(Clone, Copy)]
struct ScrollTrack {
    start: f64,
    dur: f64,
    width: f64,
    speed: f64,
}

/// 将弹幕列表导出为 ASS 字幕格式（支持滚动/顶部/底部定位及轨道防重叠）
pub fn to_ass(items: &[DanmakuItem], title: &str) -> String {
    const RES_W: f64 = 1920.0;
    const RES_H: f64 = 1080.0;
    const SCROLL_DUR: f64 = 8.0;
    const STATIC_DUR: f64 = 4.0;
    const LANE_HEIGHT: f64 = 54.0;
    const MARGIN_Y: f64 = 20.0;
    const MAX_LANES: usize = 16;

    let clean_title = title.replace(['\r', '\n'], " ");
    let mut out = String::new();
    out.push_str("[Script Info]\n");
    out.push_str(&format!("Title: {clean_title}\n"));
    out.push_str("ScriptType: v4.00+\n");
    out.push_str("PlayResX: 1920\n");
    out.push_str("PlayResY: 1080\n");
    out.push_str("ScaledBorderAndShadow: yes\n");
    out.push_str("Collisions: Normal\n\n");

    out.push_str("[V4+ Styles]\n");
    out.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n");
    out.push_str("Style: Danmaku,Microsoft YaHei,50,&H00FFFFFF,&H00FFFFFF,&H00000000,&H66000000,1,0,0,0,100,100,0,0,1,2,0,2,20,20,20,1\n\n");

    out.push_str("[Events]\n");
    out.push_str(
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );

    let mut scroll_lanes: Vec<Option<ScrollTrack>> = vec![None; MAX_LANES];
    let mut top_lanes: Vec<f64> = vec![0.0; MAX_LANES];
    let mut bottom_lanes: Vec<f64> = vec![0.0; MAX_LANES];

    for item in items {
        // 跳过不可渲染的高级/代码/BAS 弹幕及纯空白文本
        if matches!(item.mode, 7..=9) || item.text.trim().is_empty() {
            continue;
        }

        let font_base = (item.font_size.clamp(12, 72)) as f64;
        let px_size = font_base * 2.0;
        let width = estimate_pixel_width(&item.text, px_size);
        let escaped_text = escape_ass_text(&item.text);

        let color_tag = if item.color != 0xFFFFFF {
            format!("\\c{}", format_ass_color(item.color))
        } else {
            String::new()
        };
        let size_tag = if item.font_size != 25 {
            format!("\\fs{}", px_size.round() as u32)
        } else {
            String::new()
        };

        let t = item.time.max(0.0);
        match item.mode {
            // 顶部弹幕
            5 => {
                let lane = top_lanes
                    .iter()
                    .position(|&end| t >= end)
                    .unwrap_or_else(|| {
                        top_lanes
                            .iter()
                            .enumerate()
                            .min_by(|a, b| a.1.total_cmp(b.1))
                            .map(|(idx, _)| idx)
                            .unwrap_or(0)
                    });
                let end_t = t + STATIC_DUR;
                top_lanes[lane] = end_t;
                let y = (MARGIN_Y + lane as f64 * LANE_HEIGHT).round() as i32;
                let start_s = format_ass_time(t);
                let end_s = format_ass_time(end_t);
                out.push_str(&format!(
                    "Dialogue: 1,{start_s},{end_s},Danmaku,,0,0,0,,{{\\an8\\pos(960,{y}){size_tag}{color_tag}}}{escaped_text}\n"
                ));
            }
            // 底部弹幕
            4 => {
                let lane = bottom_lanes
                    .iter()
                    .position(|&end| t >= end)
                    .unwrap_or_else(|| {
                        bottom_lanes
                            .iter()
                            .enumerate()
                            .min_by(|a, b| a.1.total_cmp(b.1))
                            .map(|(idx, _)| idx)
                            .unwrap_or(0)
                    });
                let end_t = t + STATIC_DUR;
                bottom_lanes[lane] = end_t;
                let y = (RES_H - MARGIN_Y - lane as f64 * LANE_HEIGHT).round() as i32;
                let start_s = format_ass_time(t);
                let end_s = format_ass_time(end_t);
                out.push_str(&format!(
                    "Dialogue: 1,{start_s},{end_s},Danmaku,,0,0,0,,{{\\an2\\pos(960,{y}){size_tag}{color_tag}}}{escaped_text}\n"
                ));
            }
            // 滚动弹幕（含逆向 mode=6）
            _ => {
                let v = (RES_W + width) / SCROLL_DUR;
                let mut chosen = None;
                for (idx, slot) in scroll_lanes.iter().enumerate() {
                    match slot {
                        None => {
                            chosen = Some(idx);
                            break;
                        }
                        Some(prev) => {
                            let tail_entered = t >= prev.start + prev.width / prev.speed;
                            let no_overtake = t + RES_W / v >= prev.start + prev.dur;
                            if tail_entered && no_overtake {
                                chosen = Some(idx);
                                break;
                            }
                        }
                    }
                }
                let lane = chosen.unwrap_or_else(|| {
                    scroll_lanes
                        .iter()
                        .enumerate()
                        .min_by(|a, b| {
                            let fa = a.1.map(|p| p.start + p.width / p.speed).unwrap_or(0.0);
                            let fb = b.1.map(|p| p.start + p.width / p.speed).unwrap_or(0.0);
                            fa.total_cmp(&fb)
                        })
                        .map(|(idx, _)| idx)
                        .unwrap_or(0)
                });

                scroll_lanes[lane] = Some(ScrollTrack {
                    start: t,
                    dur: SCROLL_DUR,
                    width,
                    speed: v,
                });

                let y = (MARGIN_Y + lane as f64 * LANE_HEIGHT).round() as i32;
                let end_t = t + SCROLL_DUR;
                let start_s = format_ass_time(t);
                let end_s = format_ass_time(end_t);
                let w_px = width.ceil() as i32;
                let (x1, x2) = if item.mode == 6 {
                    (-w_px, RES_W as i32)
                } else {
                    (RES_W as i32, -w_px)
                };
                out.push_str(&format!(
                    "Dialogue: 0,{start_s},{end_s},Danmaku,,0,0,0,,{{\\an7\\move({x1},{y},{x2},{y}){size_tag}{color_tag}}}{escaped_text}\n"
                ));
            }
        }
    }

    out
}

/// 将弹幕列表导出为纯文本时间轴格式
pub fn to_txt(items: &[DanmakuItem]) -> String {
    let mut out = String::new();
    for item in items {
        let total_cs = (item.time.max(0.0) * 100.0).round() as u64;
        let m = total_cs / 6_000;
        let s = (total_cs / 100) % 60;
        let cs = total_cs % 100;
        out.push_str(&format!("[{m:02}:{s:02}.{cs:02}] {}\n", item.text));
    }
    out
}

fn escape_xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// 将弹幕列表导出为 B 站标准 XML 格式（兼容 biliup / DanmakuFactory / 弹弹play）
pub fn to_xml(items: &[DanmakuItem], chat_id: i64) -> String {
    let mut out = String::with_capacity(256 + items.len() * 96);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<i>\n");
    out.push_str("  <chatserver>chat.bilibili.com</chatserver>\n");
    out.push_str(&format!("  <chatid>{chat_id}</chatid>\n"));
    out.push_str("  <mission>0</mission>\n");
    out.push_str(&format!(
        "  <maxlimit>{}</maxlimit>\n",
        items.len().max(1000)
    ));
    out.push_str("  <state>0</state>\n");
    out.push_str("  <real_name>0</real_name>\n");
    out.push_str("  <source>bili-tools</source>\n");

    for (idx, item) in items.iter().enumerate() {
        let sender = if item.sender_hash.is_empty() {
            "0"
        } else {
            item.sender_hash.as_str()
        };
        let dmid = if item.dmid.is_empty() {
            (idx + 1).to_string()
        } else {
            item.dmid.clone()
        };
        out.push_str(&format!(
            "  <d p=\"{:.2},{},{},{},{},{},{},{}\">{}</d>\n",
            item.time.max(0.0),
            item.mode,
            item.font_size,
            item.color & 0x00FF_FFFF,
            item.send_time,
            item.pool,
            sender,
            dmid,
            escape_xml_text(&item.text)
        ));
    }

    out.push_str("</i>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
    use std::io::Write;

    #[test]
    fn test_decode_danmaku_bytes_all_encodings() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><i><d p="1.5,1,25,16777215,1700000000,0,abc,1">你好弹幕</d></i>"#;

        // 1. Plain XML (with BOM & leading space)
        let mut bom_xml = vec![0xEF, 0xBB, 0xBF, b' '];
        bom_xml.extend_from_slice(xml.as_bytes());
        assert_eq!(
            decode_danmaku_bytes(&bom_xml, None).unwrap(),
            format!(" {xml}")
        );

        // 2. Raw RFC 1951 deflate (Bilibili's actual wire format)
        let mut def_enc = DeflateEncoder::new(Vec::new(), Compression::default());
        def_enc.write_all(xml.as_bytes()).unwrap();
        let def_bytes = def_enc.finish().unwrap();
        assert_eq!(
            decode_danmaku_bytes(&def_bytes, Some("deflate")).unwrap(),
            xml
        );
        assert_eq!(decode_danmaku_bytes(&def_bytes, None).unwrap(), xml);

        // 3. Zlib (RFC 1950)
        let mut zlib_enc = ZlibEncoder::new(Vec::new(), Compression::default());
        zlib_enc.write_all(xml.as_bytes()).unwrap();
        let zlib_bytes = zlib_enc.finish().unwrap();
        assert_eq!(
            decode_danmaku_bytes(&zlib_bytes, Some("deflate")).unwrap(),
            xml
        );

        // 4. Gzip (RFC 1952)
        let mut gz_enc = GzEncoder::new(Vec::new(), Compression::default());
        gz_enc.write_all(xml.as_bytes()).unwrap();
        let gz_bytes = gz_enc.finish().unwrap();
        assert_eq!(decode_danmaku_bytes(&gz_bytes, Some("gzip")).unwrap(), xml);
        assert_eq!(decode_danmaku_bytes(&gz_bytes, None).unwrap(), xml);
    }

    #[test]
    fn test_unescape_xml_single_pass_and_entities() {
        assert_eq!(unescape_xml("&amp;lt;"), "&lt;");
        assert_eq!(
            unescape_xml("A &lt; B &amp;&amp; C &gt; D &quot;hi&quot; &apos;x&apos;"),
            "A < B && C > D \"hi\" 'x'"
        );
        assert_eq!(unescape_xml("&#123;&#x1F600;"), "{😀");
        assert_eq!(unescape_xml("&unknown; &123"), "&unknown; &123");
    }

    #[test]
    fn test_parse_danmaku_xml_quotes_cdata_and_sorting() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<i>
  <!-- comment -->
  <d p='20.5,5,36,16711680,1700000002,0,u2,222,8'>顶部&amp;lt;红字</d>
  <d p="1.25,1,25,16777215,1700000001,0,u1,111" data-extra="a>b">第一条 &lt;tag&gt;</d>
  <d p="bad,1,25,0,0,0,u3,333">broken</d>
  <d p="5.0,4,25,65280,1700000003,0,u4,444"><![CDATA[原样<CDATA>&amp;]]></d>
  <d p="9.0,1,25,16777215,1700000004,0,u5,555"/>
</i>"#;
        let items = parse_danmaku_xml(xml).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].time, 1.25);
        assert_eq!(items[0].text, "第一条 <tag>");
        assert_eq!(items[0].color_hex, "#FFFFFF");
        assert_eq!(items[1].time, 5.0);
        assert_eq!(items[1].mode, 4);
        assert_eq!(items[1].text, "原样<CDATA>&amp;");
        assert_eq!(items[2].time, 20.5);
        assert_eq!(items[2].mode, 5);
        assert_eq!(items[2].weight, Some(8));
        assert_eq!(items[2].text, "顶部&lt;红字");

        let cdata_close_xml = r#"<?xml version="1.0" encoding="UTF-8"?><i><d p="1.0,1,25,16777215,1700000000,0,abc,1"><![CDATA[foo</d>bar]]></d></i>"#;
        let cdata_items = parse_danmaku_xml(cdata_close_xml).unwrap();
        assert_eq!(cdata_items.len(), 1);
        assert_eq!(cdata_items[0].text, "foo</d>bar");
    }

    #[test]
    fn test_to_ass_formatting_color_escape_and_collision() {
        assert_eq!(format_ass_time(59.999), "0:01:00.00");
        assert_eq!(format_ass_time(3661.05), "1:01:01.05");
        assert_eq!(format_ass_color(0xFE0302), "&H0203FE&");
        assert_eq!(escape_ass_text("a{b}\\N\nc"), "a\\{b\\}\\\u{200b}N\\Nc");

        let items = vec![
            DanmakuItem {
                time: 1.0,
                mode: 1,
                font_size: 25,
                color: 0xFFFFFF,
                color_hex: "#FFFFFF".to_string(),
                send_time: 0,
                pool: 0,
                sender_hash: "a".to_string(),
                dmid: "1".to_string(),
                weight: None,
                text: "同时弹幕1".to_string(),
            },
            DanmakuItem {
                time: 1.0,
                mode: 1,
                font_size: 25,
                color: 0xFF0000,
                color_hex: "#FF0000".to_string(),
                send_time: 0,
                pool: 0,
                sender_hash: "b".to_string(),
                dmid: "2".to_string(),
                weight: None,
                text: "同时弹幕2".to_string(),
            },
            DanmakuItem {
                time: 2.0,
                mode: 5,
                font_size: 25,
                color: 0x00FF00,
                color_hex: "#00FF00".to_string(),
                send_time: 0,
                pool: 0,
                sender_hash: "c".to_string(),
                dmid: "3".to_string(),
                weight: None,
                text: "顶部".to_string(),
            },
            DanmakuItem {
                time: 2.5,
                mode: 4,
                font_size: 25,
                color: 0x0000FF,
                color_hex: "#0000FF".to_string(),
                send_time: 0,
                pool: 0,
                sender_hash: "d".to_string(),
                dmid: "4".to_string(),
                weight: None,
                text: "底部".to_string(),
            },
            DanmakuItem {
                time: 3.0,
                mode: 7,
                font_size: 25,
                color: 0xFFFFFF,
                color_hex: "#FFFFFF".to_string(),
                send_time: 0,
                pool: 2,
                sender_hash: "e".to_string(),
                dmid: "5".to_string(),
                weight: None,
                text: "高级模式跳过".to_string(),
            },
        ];

        let ass = to_ass(&items, "测试标题");
        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("[V4+ Styles]"));
        assert!(ass.contains("[Events]"));
        assert!(
            ass.contains("Dialogue: 0,0:00:01.00,0:00:09.00,Danmaku,,0,0,0,,{\\an7\\move(1920,20,")
        );
        // 第二条同时弹幕被分配到下一轨道 y=74，避免重叠
        assert!(ass.contains("\\move(1920,74,") && ass.contains("\\c&H0000FF&"));
        assert!(ass.contains("\\an8\\pos(960,20)\\c&H00FF00&"));
        assert!(ass.contains("\\an2\\pos(960,1060)\\c&HFF0000&"));
        assert!(!ass.contains("高级模式跳过"));
    }

    #[test]
    fn test_fetch_danmaku_xml_primary_and_fallback() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let _lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct RestoreEnv(Option<String>, Option<String>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("BT_COMMENT_BASE_URL", v),
                        None => std::env::remove_var("BT_COMMENT_BASE_URL"),
                    }
                    match &self.1 {
                        Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                        None => std::env::remove_var("BT_API_BASE_URL"),
                    }
                }
            }
        }
        let _restore = RestoreEnv(
            std::env::var("BT_COMMENT_BASE_URL").ok(),
            std::env::var("BT_API_BASE_URL").ok(),
        );

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><i><d p="12.345,1,25,16777215,1700000000,0,abc,1">回退成功</d></i>"#;
        let mut def_enc = DeflateEncoder::new(Vec::new(), Compression::default());
        def_enc.write_all(xml.as_bytes()).unwrap();
        let compressed = def_enc.finish().unwrap();

        let primary_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let primary_base = format!("http://{}", primary_listener.local_addr().unwrap());
        let h1 = std::thread::spawn(move || {
            let (mut stream, _) = primary_listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found"
            )
            .unwrap();
        });

        let fallback_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fallback_base = format!("http://{}", fallback_listener.local_addr().unwrap());
        let h2 = std::thread::spawn(move || {
            let (mut stream, _) = fallback_listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            assert!(first.contains("/x/v1/dm/list.so?oid=146044693"));
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                compressed.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(&compressed).unwrap();
        });

        unsafe {
            std::env::set_var("BT_COMMENT_BASE_URL", &primary_base);
            std::env::set_var("BT_API_BASE_URL", &fallback_base);
        }

        let got = fetch_danmaku_xml(146044693, None).unwrap();
        h1.join().unwrap();
        h2.join().unwrap();
        let parsed = parse_danmaku_xml(&got).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "回退成功");
        assert_eq!(parsed[0].time, 12.345);
    }
}
