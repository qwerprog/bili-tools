use crate::error::{BiliLiveError, Result};
use serde::Serialize;
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdKind {
    Video,
    BangumiEpisode,
    BangumiSeason,
    BangumiMedia,
    LiveRoom,
    User,
    Fav,
    Numeric,
    ShortLink,
}

impl IdKind {
    pub fn display_label(self) -> &'static str {
        match self {
            Self::Video => "视频 (Video)",
            Self::BangumiEpisode => "番剧单集 (EP)",
            Self::BangumiSeason => "番剧季度 (SS)",
            Self::BangumiMedia => "番剧条目 (MD)",
            Self::LiveRoom => "直播间 (Live)",
            Self::User => "用户空间 (User)",
            Self::Fav => "收藏夹 (Fav)",
            Self::Numeric => "纯数字 ID (Numeric)",
            Self::ShortLink => "短链接 (b23.tv)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedInput {
    pub input: String,
    pub kind: IdKind,
    /// Canonical identifier: "BV...", "av123", "ep123", "ss123", "md123", "12345", or short code
    pub id: String,
    pub bvid: Option<String>,
    pub aid: Option<i64>,
    pub page: Option<u32>,
    pub resolved_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episode_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub season_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fav_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_url: Option<String>,
    pub network_used: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoTarget {
    pub bvid: Option<String>,
    pub aid: Option<i64>,
    pub page: Option<u32>,
}

impl ResolvedInput {
    fn base(input: &str, kind: IdKind, id: String) -> Self {
        Self {
            input: input.to_string(),
            kind,
            id,
            bvid: None,
            aid: None,
            page: None,
            resolved_url: None,
            episode_id: None,
            season_id: None,
            media_id: None,
            room_id: None,
            mid: None,
            fav_id: None,
            short_url: None,
            network_used: false,
        }
    }
}

fn normalize_bvid(raw: &str) -> Option<String> {
    if raw.len() < 4 {
        return None;
    }
    let prefix = raw.get(..2)?;
    if !prefix.eq_ignore_ascii_case("bv") {
        return None;
    }
    let rest = &raw[2..];
    if rest.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(format!("BV{rest}"))
    } else {
        None
    }
}

fn parse_positive_i64(raw: &str) -> Option<i64> {
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    raw.parse::<i64>().ok().filter(|&n| n > 0)
}

fn parse_prefixed_i64(raw: &str, prefix: &str) -> Option<i64> {
    let plen = prefix.len();
    if raw.len() <= plen {
        return None;
    }
    let head = raw.get(..plen)?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    parse_positive_i64(&raw[plen..])
}

fn has_known_host_prefix(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    [
        "b23.tv/",
        "www.b23.tv/",
        "bili2233.cn/",
        "www.bilibili.com/",
        "bilibili.com/",
        "m.bilibili.com/",
        "live.bilibili.com/",
        "space.bilibili.com/",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

fn is_standard_short_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "b23.tv" | "www.b23.tv" | "bili2233.cn"
    )
}

fn is_short_link(url: &Url, short_base: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if is_standard_short_host(host) {
        return true;
    }
    if let Some(base_host) = short_base.host_str()
        && host.eq_ignore_ascii_case(base_host)
        && url.port_or_known_default() == short_base.port_or_known_default()
    {
        // Avoid treating mock redirects directly to /video/..., /bangumi/..., etc. as short links
        let path = url.path();
        if !path.starts_with("/video/")
            && !path.starts_with("/s/video/")
            && !path.starts_with("/bangumi/")
            && !path.starts_with("/medialist/")
            && !path.starts_with("/list/")
        {
            return true;
        }
    }
    false
}

fn canonical_short_url(url: &Url) -> (String, String) {
    let code = url.path().trim_matches('/').to_string();
    let short_url = match url.host_str() {
        Some(h) if is_standard_short_host(h) => {
            if code.is_empty() {
                "https://b23.tv".to_string()
            } else {
                format!("https://b23.tv/{code}")
            }
        }
        _ => url.as_str().to_string(),
    };
    (code, short_url)
}

fn parse_bare_id(input: &str) -> Option<ResolvedInput> {
    if input.contains('/') || input.contains("://") {
        return None;
    }

    if let Some(bvid) = normalize_bvid(input) {
        let mut r = ResolvedInput::base(input, IdKind::Video, bvid.clone());
        r.bvid = Some(bvid.clone());
        r.resolved_url = Some(format!("https://www.bilibili.com/video/{bvid}"));
        return Some(r);
    }

    if let Some(aid) = parse_prefixed_i64(input, "av") {
        let id = format!("av{aid}");
        let mut r = ResolvedInput::base(input, IdKind::Video, id.clone());
        r.aid = Some(aid);
        r.resolved_url = Some(format!("https://www.bilibili.com/video/{id}"));
        return Some(r);
    }

    if let Some(ep) = parse_prefixed_i64(input, "ep") {
        let id = format!("ep{ep}");
        let mut r = ResolvedInput::base(input, IdKind::BangumiEpisode, id.clone());
        r.episode_id = Some(ep);
        r.resolved_url = Some(format!("https://www.bilibili.com/bangumi/play/{id}"));
        return Some(r);
    }

    if let Some(ss) = parse_prefixed_i64(input, "ss") {
        let id = format!("ss{ss}");
        let mut r = ResolvedInput::base(input, IdKind::BangumiSeason, id.clone());
        r.season_id = Some(ss);
        r.resolved_url = Some(format!("https://www.bilibili.com/bangumi/play/{id}"));
        return Some(r);
    }

    if let Some(md) = parse_prefixed_i64(input, "md") {
        let id = format!("md{md}");
        let mut r = ResolvedInput::base(input, IdKind::BangumiMedia, id.clone());
        r.media_id = Some(md);
        r.resolved_url = Some(format!("https://www.bilibili.com/bangumi/media/{id}"));
        return Some(r);
    }

    if let Some(ml) = parse_prefixed_i64(input, "ml") {
        let mut r = ResolvedInput::base(input, IdKind::Fav, ml.to_string());
        r.fav_id = Some(ml);
        r.resolved_url = Some(format!("https://www.bilibili.com/medialist/detail/ml{ml}"));
        return Some(r);
    }

    if let Some(num) = parse_positive_i64(input) {
        return Some(ResolvedInput::base(input, IdKind::Numeric, num.to_string()));
    }

    None
}

fn extract_page_param(url: &Url) -> Option<u32> {
    url.query_pairs()
        .find(|(k, _)| k == "p")
        .and_then(|(_, v)| v.parse::<u32>().ok())
        .filter(|&p| p >= 1)
}

fn extract_fid_param(url: &Url) -> Option<i64> {
    url.query_pairs()
        .find(|(k, _)| k == "fid")
        .and_then(|(_, v)| parse_positive_i64(&v))
}

fn parse_bilibili_url(orig_input: &str, url: &Url) -> Result<ResolvedInput> {
    let host = url
        .host_str()
        .ok_or_else(|| BiliLiveError::Input(format!("URL 缺少主机名: {orig_input}")))?
        .to_ascii_lowercase();

    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();

    // 1. Favlist URLs (MUST precede space.bilibili.com/<mid>)
    if matches!(
        host.as_str(),
        "space.bilibili.com" | "www.bilibili.com" | "bilibili.com" | "m.bilibili.com"
    ) && segments.contains(&"favlist")
        && let Some(fid) = extract_fid_param(url)
    {
        let mid = if host == "space.bilibili.com" {
            segments.first().and_then(|s| parse_positive_i64(s))
        } else {
            None
        };
        let mut r = ResolvedInput::base(orig_input, IdKind::Fav, fid.to_string());
        r.fav_id = Some(fid);
        r.mid = mid;
        r.resolved_url = Some(if let Some(m) = mid {
            format!("https://space.bilibili.com/{m}/favlist?fid={fid}")
        } else {
            format!("https://www.bilibili.com/medialist/detail/ml{fid}")
        });
        return Ok(r);
    }

    if matches!(
        host.as_str(),
        "www.bilibili.com" | "bilibili.com" | "m.bilibili.com"
    ) {
        let ml_seg = match segments.as_slice() {
            ["medialist", "detail", ml, ..] | ["list", ml, ..] => Some(*ml),
            _ => None,
        };
        if let Some(seg) = ml_seg
            && let Some(ml) = parse_prefixed_i64(seg, "ml")
        {
            let mut r = ResolvedInput::base(orig_input, IdKind::Fav, ml.to_string());
            r.fav_id = Some(ml);
            r.resolved_url = Some(format!("https://www.bilibili.com/medialist/detail/ml{ml}"));
            return Ok(r);
        }
    }

    // 2. Video URLs
    if matches!(
        host.as_str(),
        "www.bilibili.com" | "bilibili.com" | "m.bilibili.com"
    ) {
        let vid_seg = match segments.as_slice() {
            ["video", vid, ..] | ["s", "video", vid, ..] => Some(*vid),
            _ => None,
        };
        if let Some(vid) = vid_seg {
            let page = extract_page_param(url);
            let page_suffix = match page {
                Some(p) => format!("?p={p}"),
                None => String::new(),
            };
            if let Some(bvid) = normalize_bvid(vid) {
                let mut r = ResolvedInput::base(orig_input, IdKind::Video, bvid.clone());
                r.bvid = Some(bvid.clone());
                r.page = page;
                r.resolved_url = Some(format!(
                    "https://www.bilibili.com/video/{bvid}{page_suffix}"
                ));
                return Ok(r);
            }
            if let Some(aid) = parse_prefixed_i64(vid, "av") {
                let id = format!("av{aid}");
                let mut r = ResolvedInput::base(orig_input, IdKind::Video, id.clone());
                r.aid = Some(aid);
                r.page = page;
                r.resolved_url = Some(format!("https://www.bilibili.com/video/{id}{page_suffix}"));
                return Ok(r);
            }
        }
    }

    // 3. Bangumi URLs
    if matches!(
        host.as_str(),
        "www.bilibili.com" | "bilibili.com" | "m.bilibili.com"
    ) {
        match segments.as_slice() {
            ["bangumi", "play", seg, ..] => {
                if let Some(ep) = parse_prefixed_i64(seg, "ep") {
                    let id = format!("ep{ep}");
                    let mut r = ResolvedInput::base(orig_input, IdKind::BangumiEpisode, id.clone());
                    r.episode_id = Some(ep);
                    r.resolved_url = Some(format!("https://www.bilibili.com/bangumi/play/{id}"));
                    return Ok(r);
                }
                if let Some(ss) = parse_prefixed_i64(seg, "ss") {
                    let id = format!("ss{ss}");
                    let mut r = ResolvedInput::base(orig_input, IdKind::BangumiSeason, id.clone());
                    r.season_id = Some(ss);
                    r.resolved_url = Some(format!("https://www.bilibili.com/bangumi/play/{id}"));
                    return Ok(r);
                }
            }
            ["bangumi", "media", seg, ..] => {
                if let Some(md) = parse_prefixed_i64(seg, "md") {
                    let id = format!("md{md}");
                    let mut r = ResolvedInput::base(orig_input, IdKind::BangumiMedia, id.clone());
                    r.media_id = Some(md);
                    r.resolved_url = Some(format!("https://www.bilibili.com/bangumi/media/{id}"));
                    return Ok(r);
                }
            }
            _ => {}
        }
    }

    // 4. Live URLs
    if host == "live.bilibili.com" {
        let room_seg = match segments.as_slice() {
            [id_seg] | ["h5", id_seg, ..] | ["blanc", id_seg, ..] => Some(*id_seg),
            _ => None,
        };
        if let Some(seg) = room_seg
            && let Some(room_id) = parse_positive_i64(seg)
        {
            let mut r = ResolvedInput::base(orig_input, IdKind::LiveRoom, room_id.to_string());
            r.room_id = Some(room_id);
            r.resolved_url = Some(format!("https://live.bilibili.com/{room_id}"));
            return Ok(r);
        }
    }

    // 5. User Space URLs
    if host == "space.bilibili.com"
        && let Some(mid_seg) = segments.first()
        && let Some(mid) = parse_positive_i64(mid_seg)
    {
        let mut r = ResolvedInput::base(orig_input, IdKind::User, mid.to_string());
        r.mid = Some(mid);
        r.resolved_url = Some(format!("https://space.bilibili.com/{mid}"));
        return Ok(r);
    }

    if host == "m.bilibili.com"
        && let ["space", mid_seg, ..] = segments.as_slice()
        && let Some(mid) = parse_positive_i64(mid_seg)
    {
        let mut r = ResolvedInput::base(orig_input, IdKind::User, mid.to_string());
        r.mid = Some(mid);
        r.resolved_url = Some(format!("https://space.bilibili.com/{mid}"));
        return Ok(r);
    }

    Err(BiliLiveError::Input(format!(
        "不支持的 B站 URL 路径: {orig_input}"
    )))
}

fn rewrite_onto_base(orig: &Url, base: &Url) -> Result<Url> {
    let mut target = base.clone();
    target.set_path(orig.path());
    target.set_query(orig.query());
    Ok(target)
}

fn expand_short_link(orig_input: &str, url: &Url, short_base: &Url) -> Result<ResolvedInput> {
    let (_, orig_short_url) = canonical_short_url(url);
    let mut current = rewrite_onto_base(url, short_base)?;

    for _hop in 0..3 {
        let resp = ureq::get(current.as_str())
            .header("User-Agent", crate::api::client::DEFAULT_USER_AGENT)
            .config()
            .timeout_global(Some(Duration::from_secs(15)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .call()?;

        if resp.status().is_redirection() {
            let loc = resp
                .headers()
                .get(ureq::http::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    BiliLiveError::Parse(format!("短链接重定向缺少 Location: {current}"))
                })?;

            let next_url = current
                .join(loc)
                .map_err(|e| BiliLiveError::Parse(format!("短链接目标 URL 解析失败: {e}")))?;

            if is_short_link(&next_url, short_base) {
                current = if next_url.host_str().is_some_and(is_standard_short_host) {
                    rewrite_onto_base(&next_url, short_base)?
                } else {
                    next_url
                };
                continue;
            }

            let mut resolved = parse_bilibili_url(orig_input, &next_url)?;
            resolved.short_url = Some(orig_short_url);
            resolved.network_used = true;
            return Ok(resolved);
        }

        if resp.status().is_client_error() || resp.status().is_server_error() {
            return Err(BiliLiveError::Api(format!(
                "短链接解析失败 (HTTP {}): {}",
                resp.status().as_u16(),
                current
            )));
        }

        return Err(BiliLiveError::Parse(format!(
            "短链接未返回重定向 Location: {current}"
        )));
    }

    Err(BiliLiveError::Api(format!(
        "短链接重定向次数过多: {orig_short_url}"
    )))
}

pub(crate) fn parse_input_with_short_base(
    input: &str,
    no_network: bool,
    short_base_str: &str,
) -> Result<ResolvedInput> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(BiliLiveError::Input("输入不能为空".to_string()));
    }

    if let Some(bare) = parse_bare_id(trimmed) {
        return Ok(bare);
    }

    let url = if trimmed.contains("://") {
        Url::parse(trimmed)
            .map_err(|e| BiliLiveError::Input(format!("无法解析 URL '{trimmed}': {e}")))?
    } else if has_known_host_prefix(trimmed) {
        Url::parse(&format!("https://{trimmed}"))
            .map_err(|e| BiliLiveError::Input(format!("无法解析 URL '{trimmed}': {e}")))?
    } else {
        return Err(BiliLiveError::Input(format!(
            "无法识别的 B站 ID 或 URL: {trimmed}"
        )));
    };

    let short_base = Url::parse(short_base_str)
        .map_err(|e| BiliLiveError::Parse(format!("短链基础地址配置无效: {e}")))?;

    if is_short_link(&url, &short_base) {
        let (code, short_url) = canonical_short_url(&url);
        if code.is_empty() {
            return Err(BiliLiveError::Input(format!(
                "短链接缺少路径代码: {trimmed}"
            )));
        }
        if no_network {
            let mut r = ResolvedInput::base(trimmed, IdKind::ShortLink, code);
            r.resolved_url = Some(short_url.clone());
            r.short_url = Some(short_url);
            r.network_used = false;
            return Ok(r);
        }
        return expand_short_link(trimmed, &url, &short_base);
    }

    parse_bilibili_url(trimmed, &url)
}

pub fn parse_input(input: &str, no_network: bool) -> Result<ResolvedInput> {
    let short_base = crate::api::client::short_link_base_url();
    parse_input_with_short_base(input, no_network, &short_base)
}

pub fn resolve_video(input: &str, no_network: bool) -> Result<VideoTarget> {
    let r = parse_input(input, no_network)?;
    match r.kind {
        IdKind::Video => Ok(VideoTarget {
            bvid: r.bvid,
            aid: r.aid,
            page: r.page,
        }),
        IdKind::Numeric => {
            let aid = r.id.parse::<i64>()?;
            Ok(VideoTarget {
                bvid: None,
                aid: Some(aid),
                page: None,
            })
        }
        _ => Err(BiliLiveError::Input(format!(
            "输入不是有效的视频标识: {input}"
        ))),
    }
}

#[allow(dead_code)]
pub fn resolve_room(input: &str, no_network: bool) -> Result<i64> {
    let r = parse_input(input, no_network)?;
    match r.kind {
        IdKind::LiveRoom => r
            .room_id
            .ok_or_else(|| BiliLiveError::Parse("缺少直播间 room_id".to_string())),
        IdKind::Numeric => Ok(r.id.parse::<i64>()?),
        _ => Err(BiliLiveError::Input(format!(
            "输入不是有效的直播间标识: {input}"
        ))),
    }
}

pub fn resolve_user(input: &str, no_network: bool) -> Result<i64> {
    let r = parse_input(input, no_network)?;
    match r.kind {
        IdKind::User => r
            .mid
            .ok_or_else(|| BiliLiveError::Parse("缺少用户 mid".to_string())),
        IdKind::Numeric => Ok(r.id.parse::<i64>()?),
        _ => Err(BiliLiveError::Input(format!(
            "输入不是有效的用户标识: {input}"
        ))),
    }
}

#[allow(dead_code)]
pub fn resolve_fav(input: &str, no_network: bool) -> Result<i64> {
    let r = parse_input(input, no_network)?;
    match r.kind {
        IdKind::Fav => r
            .fav_id
            .ok_or_else(|| BiliLiveError::Parse("缺少收藏夹 fav_id".to_string())),
        IdKind::Numeric => Ok(r.id.parse::<i64>()?),
        _ => Err(BiliLiveError::Input(format!(
            "输入不是有效的收藏夹标识: {input}"
        ))),
    }
}

#[allow(dead_code)]
pub fn parse_video_param_offline(input: &str) -> (Option<String>, Option<i64>) {
    match resolve_video(input, true) {
        Ok(target) => (target.bvid, target.aid),
        Err(_) => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_ids_and_normalizes_bv_prefix_preserving_suffix_case() {
        let bv = parse_input("bv1uJ411r7hL", true).unwrap();
        assert_eq!(bv.kind, IdKind::Video);
        assert_eq!(bv.id, "BV1uJ411r7hL");
        assert_eq!(bv.bvid.as_deref(), Some("BV1uJ411r7hL"));
        assert_eq!(
            bv.resolved_url.as_deref(),
            Some("https://www.bilibili.com/video/BV1uJ411r7hL")
        );

        let av = parse_input("AV79677524", true).unwrap();
        assert_eq!(av.kind, IdKind::Video);
        assert_eq!(av.id, "av79677524");
        assert_eq!(av.aid, Some(79677524));

        let ep = parse_input("EP123456", true).unwrap();
        assert_eq!(ep.kind, IdKind::BangumiEpisode);
        assert_eq!(ep.episode_id, Some(123456));

        let ss = parse_input("ss98765", true).unwrap();
        assert_eq!(ss.kind, IdKind::BangumiSeason);
        assert_eq!(ss.season_id, Some(98765));

        let md = parse_input("md2822", true).unwrap();
        assert_eq!(md.kind, IdKind::BangumiMedia);
        assert_eq!(md.media_id, Some(2822));

        let num = parse_input("79677524", true).unwrap();
        assert_eq!(num.kind, IdKind::Numeric);
        assert_eq!(num.id, "79677524");
    }

    #[test]
    fn parses_bilibili_urls_and_strips_tracking_params() {
        let video = parse_input(
            "https://www.bilibili.com/video/bv1uJ411r7hL/?p=2&spm_id_from=333.1007&vd_source=abc",
            true,
        )
        .unwrap();
        assert_eq!(video.kind, IdKind::Video);
        assert_eq!(video.bvid.as_deref(), Some("BV1uJ411r7hL"));
        assert_eq!(video.page, Some(2));
        assert_eq!(
            video.resolved_url.as_deref(),
            Some("https://www.bilibili.com/video/BV1uJ411r7hL?p=2")
        );

        let fav = parse_input(
            "https://space.bilibili.com/12345/favlist?fid=67890&ftype=create",
            true,
        )
        .unwrap();
        assert_eq!(fav.kind, IdKind::Fav);
        assert_eq!(fav.fav_id, Some(67890));
        assert_eq!(fav.mid, Some(12345));

        let user = parse_input("https://space.bilibili.com/12345/video", true).unwrap();
        assert_eq!(user.kind, IdKind::User);
        assert_eq!(user.mid, Some(12345));

        let live = parse_input("https://live.bilibili.com/h5/21452505?from=1", true).unwrap();
        assert_eq!(live.kind, IdKind::LiveRoom);
        assert_eq!(live.room_id, Some(21452505));
    }

    #[test]
    fn short_link_offline_and_mock_redirect_expansion() {
        let offline = parse_input("https://b23.tv/pigt3PQ", true).unwrap();
        assert_eq!(offline.kind, IdKind::ShortLink);
        assert_eq!(offline.id, "pigt3PQ");
        assert!(!offline.network_used);

        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: https://www.bilibili.com/video/BV1GJ411x7h7?p=3&vd_source=xyz\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });

        let expanded = parse_input_with_short_base("https://b23.tv/pigt3PQ", false, &base).unwrap();
        handle.join().unwrap();
        assert_eq!(expanded.kind, IdKind::Video);
        assert_eq!(expanded.bvid.as_deref(), Some("BV1GJ411x7h7"));
        assert_eq!(expanded.page, Some(3));
        assert_eq!(
            expanded.short_url.as_deref(),
            Some("https://b23.tv/pigt3PQ")
        );
        assert!(expanded.network_used);
    }

    #[test]
    fn rejects_invalid_inputs_and_wrong_context_types() {
        assert!(parse_input("", true).is_err());
        assert!(parse_input("   ", true).is_err());
        assert!(parse_input("not-a-bili-id", true).is_err());
        assert!(parse_input("https://example.com/video/BV1uJ411r7hL", true).is_err());
        assert!(resolve_video("ep12345", true).is_err());
        assert!(resolve_room("BV1uJ411r7hL", true).is_err());
        assert!(resolve_user("https://live.bilibili.com/100", true).is_err());
        assert!(resolve_fav("https://space.bilibili.com/12345", true).is_err());
    }
}
