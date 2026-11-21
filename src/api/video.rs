use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};
use crate::utils::id::{IdKind, ResolvedInput};
use serde::{Deserialize, Serialize};
use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

/// 解析视频标识，返回 (Option<bvid>, Option<aid>)
#[allow(dead_code)]
pub fn parse_video_param(id: &str) -> (Option<String>, Option<i64>) {
    crate::utils::id::parse_video_param_offline(id)
}

fn get_video_info_from_target(
    target: &crate::utils::id::VideoTarget,
    raw_id: &str,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let bvid = target.bvid.as_deref();
    let aid_str = target.aid.map(|a| a.to_string());
    let mut params = Vec::new();
    if let Some(b) = bvid {
        params.push(("bvid", b));
    } else if let Some(ref a) = aid_str {
        params.push(("aid", a.as_str()));
    } else {
        return Err(BiliLiveError::Input(format!("无效的视频ID: {raw_id}")));
    }

    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let query = match crate::api::wbi::sign_params(&params, sessdata) {
        Ok(q) => q,
        Err(_) => {
            if let Some(b) = bvid {
                format!("bvid={b}")
            } else {
                format!("aid={}", aid_str.as_deref().unwrap_or(""))
            }
        }
    };

    let url = format!(
        "{}/x/web-interface/wbi/view?{query}",
        crate::api::client::api_base_url()
    );
    let mut req = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/");

    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            req = req.with_header("Cookie", header);
        }
    }

    let resp = req.send()?;
    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "获取视频信息失败")?;

    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取视频信息失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

fn fetch_pgc_season_data(query: &str, cookies: Option<&Cookies>) -> Result<serde_json::Value> {
    let url = format!(
        "{}/pgc/view/web/season?{query}",
        crate::api::client::api_base_url()
    );
    let req = attach_optional_cookies(
        crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Referer", "https://www.bilibili.com/"),
        cookies,
    );
    let resp = req.send()?;
    let json: serde_json::Value =
        crate::api::client::parse_json_response(&resp, "获取番剧/影视详情失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取番剧/影视详情失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    let result = json
        .get("result")
        .or_else(|| json.get("data"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if result.is_null() {
        return Err(BiliLiveError::Parse("番剧详情返回为空".to_string()));
    }
    Ok(result)
}

fn resolve_pgc_context(
    parsed: &ResolvedInput,
    cookies: Option<&Cookies>,
) -> Result<(Option<u32>, serde_json::Value)> {
    let season = match parsed.kind {
        IdKind::BangumiEpisode => {
            let ep_id = parsed
                .episode_id
                .ok_or_else(|| BiliLiveError::Input("缺少番剧 ep_id".to_string()))?;
            fetch_pgc_season_data(&format!("ep_id={ep_id}"), cookies)?
        }
        IdKind::BangumiSeason => {
            let ss_id = parsed
                .season_id
                .ok_or_else(|| BiliLiveError::Input("缺少番剧 season_id".to_string()))?;
            fetch_pgc_season_data(&format!("season_id={ss_id}"), cookies)?
        }
        IdKind::BangumiMedia => {
            let md_id = parsed
                .media_id
                .ok_or_else(|| BiliLiveError::Input("缺少番剧 media_id".to_string()))?;
            let review_url = format!(
                "{}/pgc/review/user?media_id={md_id}",
                crate::api::client::api_base_url()
            );
            let req = attach_optional_cookies(
                crate::api::client::get(&review_url)
                    .with_header("User-Agent", DEFAULT_USER_AGENT)
                    .with_header("Referer", "https://www.bilibili.com/"),
                cookies,
            );
            let resp = req.send()?;
            let rev_json: serde_json::Value =
                crate::api::client::parse_json_response(&resp, "获取番剧条目信息失败")?;
            if rev_json["code"].as_i64() != Some(0) {
                return Err(BiliLiveError::Api(format!(
                    "获取番剧条目信息失败: {}",
                    rev_json["message"].as_str().unwrap_or("未知错误")
                )));
            }
            let ss_id = rev_json["result"]["media"]["season_id"]
                .as_i64()
                .ok_or_else(|| BiliLiveError::Parse("番剧条目缺少关联 season_id".to_string()))?;
            fetch_pgc_season_data(&format!("season_id={ss_id}"), cookies)?
        }
        _ => unreachable!(),
    };

    let episodes = season["episodes"]
        .as_array()
        .filter(|arr| !arr.is_empty())
        .ok_or_else(|| BiliLiveError::Api("该番剧/影视暂无正片分集".to_string()))?;

    let mut matched_idx = 0usize;
    if let Some(target_ep) = parsed.episode_id
        && let Some(pos) = episodes.iter().position(|ep| {
            ep["id"].as_i64() == Some(target_ep) || ep["ep_id"].as_i64() == Some(target_ep)
        })
    {
        matched_idx = pos;
    }

    let primary_ep = &episodes[matched_idx];
    let season_title = season["season_title"]
        .as_str()
        .or_else(|| season["title"].as_str())
        .unwrap_or("未知番剧");
    let cover = primary_ep["cover"]
        .as_str()
        .or_else(|| season["cover"].as_str())
        .unwrap_or("");
    let bvid = primary_ep["bvid"].as_str().unwrap_or("");
    let aid = primary_ep["aid"].as_i64().unwrap_or(0);
    let cid = primary_ep["cid"].as_i64().unwrap_or(0);
    let pubdate = primary_ep["pub_time"].as_i64().unwrap_or(0);
    let up_mid = season["up_info"]["mid"].as_i64().unwrap_or(0);
    let up_name = season["up_info"]["uname"]
        .as_str()
        .unwrap_or("哔哩哔哩番剧");

    let pages: Vec<serde_json::Value> = episodes
        .iter()
        .enumerate()
        .map(|(idx, ep)| {
            let short_t = ep["title"].as_str().unwrap_or("");
            let long_t = ep["long_title"].as_str().unwrap_or("");
            let part = match (short_t.is_empty(), long_t.is_empty()) {
                (false, false) => format!("第{short_t}话 {long_t}"),
                (false, true) => format!("第{short_t}话"),
                (true, false) => long_t.to_string(),
                (true, true) => format!("EP{}", idx + 1),
            };
            let raw_dur = ep["duration"].as_i64().unwrap_or(0);
            // PGC episodes duration 单位通常为毫秒，大于 100_000 时换算为秒
            let dur_sec = if raw_dur > 100_000 {
                raw_dur / 1000
            } else {
                raw_dur
            };
            serde_json::json!({
                "page": idx + 1,
                "cid": ep["cid"].as_i64().unwrap_or(0),
                "bvid": ep["bvid"].as_str().unwrap_or(""),
                "aid": ep["aid"].as_i64().unwrap_or(0),
                "ep_id": ep["id"].as_i64().or_else(|| ep["ep_id"].as_i64()).unwrap_or(0),
                "part": part,
                "duration": dur_sec,
            })
        })
        .collect();

    let total_dur: i64 = pages
        .iter()
        .map(|p| p["duration"].as_i64().unwrap_or(0))
        .sum();

    let normalized = serde_json::json!({
        "is_pgc": true,
        "season_id": season["season_id"],
        "bvid": bvid,
        "aid": aid,
        "cid": cid,
        "videos": pages.len(),
        "title": season_title,
        "pic": cover,
        "desc": season["evaluate"].as_str().unwrap_or(""),
        "pubdate": pubdate,
        "duration": total_dur,
        "owner": {
            "mid": up_mid,
            "name": up_name,
        },
        "stat": {
            "view": season["stat"]["views"].as_i64().unwrap_or(0),
            "danmaku": season["stat"]["danmakus"].as_i64().unwrap_or(0),
            "reply": season["stat"]["reply"].as_i64().unwrap_or(0),
            "favorite": season["stat"]["favorites"].as_i64().unwrap_or(0),
            "coin": season["stat"]["coins"].as_i64().unwrap_or(0),
            "share": season["stat"]["share"].as_i64().unwrap_or(0),
            "like": season["stat"]["likes"].as_i64().unwrap_or(0),
        },
        "pages": pages,
    });

    let target_page = if parsed.kind == IdKind::BangumiEpisode {
        Some((matched_idx + 1) as u32)
    } else {
        parsed.page
    };
    Ok((target_page, normalized))
}

/// 获取视频或番剧详情（通吃 BV/av/ep/ss/md、完整 URL 与 b23.tv 短链接）
pub fn get_video_info(id: &str, cookies: Option<&Cookies>) -> Result<serde_json::Value> {
    let (_, info) = resolve_video_context(id, cookies)?;
    Ok(info)
}

/// 解析视频/番剧标识（含 URL `?p=N` 或 `ep` 集数定位）并获取详情，避免对短链接重复请求
pub fn resolve_video_context(
    id: &str,
    cookies: Option<&Cookies>,
) -> Result<(Option<u32>, serde_json::Value)> {
    let parsed = crate::utils::id::parse_input(id, false)?;
    match parsed.kind {
        IdKind::Video => {
            let target = crate::utils::id::VideoTarget {
                bvid: parsed.bvid,
                aid: parsed.aid,
                page: parsed.page,
            };
            let info = get_video_info_from_target(&target, id, cookies)?;
            Ok((target.page, info))
        }
        IdKind::Numeric => {
            let aid = parsed.id.parse::<i64>()?;
            let target = crate::utils::id::VideoTarget {
                bvid: None,
                aid: Some(aid),
                page: None,
            };
            let info = get_video_info_from_target(&target, id, cookies)?;
            Ok((None, info))
        }
        IdKind::BangumiEpisode | IdKind::BangumiSeason | IdKind::BangumiMedia => {
            resolve_pgc_context(&parsed, cookies)
        }
        _ => Err(BiliLiveError::Input(format!(
            "输入不是有效的视频或番剧标识: {id}"
        ))),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedVideoPage {
    pub bvid: String,
    pub aid: i64,
    pub up_mid: i64,
    pub title: String,
    pub pic: String,
    pub page: u32,
    pub cid: i64,
    pub part: String,
    pub duration: i64,
    pub total_pages: usize,
}

/// 从视频详情 JSON 中提取并校验指定分P（1-based），越界在请求流/字幕之前直接失败
pub fn extract_page_info(data: &serde_json::Value, page_num: u32) -> Result<ResolvedVideoPage> {
    if page_num == 0 {
        return Err(BiliLiveError::Input("分P页码必须大于等于 1".to_string()));
    }

    let bvid = data["bvid"].as_str().unwrap_or("").to_string();
    let aid = data["aid"].as_i64().unwrap_or(0);
    let up_mid = data["owner"]["mid"].as_i64().unwrap_or(0);
    let title = data["title"].as_str().unwrap_or("未知标题").to_string();
    let pic = data["pic"].as_str().unwrap_or("").to_string();

    if let Some(pages) = data["pages"].as_array()
        && !pages.is_empty()
    {
        let total_pages = pages.len();
        if page_num as usize > total_pages {
            return Err(BiliLiveError::Input(format!(
                "分P页码超出范围: 请求第 {page_num} P，该视频共 {total_pages} P"
            )));
        }

        let page_obj = pages
            .iter()
            .find(|p| p["page"].as_u64() == Some(page_num as u64))
            .or_else(|| pages.get((page_num - 1) as usize))
            .ok_or_else(|| BiliLiveError::Input(format!("未找到第 {page_num} P 信息")))?;

        let cid = page_obj["cid"]
            .as_i64()
            .or_else(|| {
                if page_num == 1 {
                    data["cid"].as_i64()
                } else {
                    None
                }
            })
            .ok_or_else(|| BiliLiveError::Parse(format!("第 {page_num} P 缺少 cid")))?;
        let part = page_obj["part"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&title)
            .to_string();
        let duration = page_obj["duration"]
            .as_i64()
            .unwrap_or_else(|| data["duration"].as_i64().unwrap_or(0));

        let page_bvid = page_obj["bvid"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| bvid.clone());
        let page_aid = page_obj["aid"].as_i64().filter(|&a| a > 0).unwrap_or(aid);

        return Ok(ResolvedVideoPage {
            bvid: page_bvid,
            aid: page_aid,
            up_mid,
            title,
            pic,
            page: page_num,
            cid,
            part,
            duration,
            total_pages,
        });
    }

    if page_num == 1
        && let Some(cid) = data["cid"].as_i64()
    {
        return Ok(ResolvedVideoPage {
            bvid,
            aid,
            up_mid,
            title: title.clone(),
            pic,
            page: 1,
            cid,
            part: title,
            duration: data["duration"].as_i64().unwrap_or(0),
            total_pages: 1,
        });
    }

    Err(BiliLiveError::Input(format!(
        "分P页码超出范围或视频缺少分P信息: 第 {page_num} P"
    )))
}

/// 解析并补全 (bvid, aid)，若缺少一方或输入为番剧 ep/ss/md 则自动通过接口查询补齐
pub fn resolve_aid_and_bvid(id: &str, cookies: Option<&Cookies>) -> Result<(String, i64)> {
    let parsed = crate::utils::id::parse_input(id, false)?;
    match parsed.kind {
        IdKind::Video => match (parsed.bvid, parsed.aid) {
            (Some(b), Some(a)) => Ok((b, a)),
            (Some(b), None) => {
                let target = crate::utils::id::VideoTarget {
                    bvid: Some(b.clone()),
                    aid: None,
                    page: None,
                };
                let info = get_video_info_from_target(&target, &b, cookies)?;
                let aid = info["aid"]
                    .as_i64()
                    .ok_or_else(|| BiliLiveError::Parse("未能获取到视频aid".to_string()))?;
                Ok((b, aid))
            }
            (None, Some(a)) => {
                let target = crate::utils::id::VideoTarget {
                    bvid: None,
                    aid: Some(a),
                    page: None,
                };
                let info = get_video_info_from_target(&target, &format!("av{a}"), cookies)?;
                let bvid = info["bvid"].as_str().unwrap_or("").to_string();
                Ok((bvid, a))
            }
            (None, None) => Err(BiliLiveError::Input(format!("无效的视频标识: {id}"))),
        },
        IdKind::Numeric => {
            let a = parsed.id.parse::<i64>()?;
            let target = crate::utils::id::VideoTarget {
                bvid: None,
                aid: Some(a),
                page: None,
            };
            let info = get_video_info_from_target(&target, &format!("av{a}"), cookies)?;
            let bvid = info["bvid"].as_str().unwrap_or("").to_string();
            Ok((bvid, a))
        }
        IdKind::BangumiEpisode | IdKind::BangumiSeason | IdKind::BangumiMedia => {
            let (target_page, info) = resolve_pgc_context(&parsed, cookies)?;
            let page = extract_page_info(&info, target_page.unwrap_or(1))?;
            if page.aid <= 0 && page.bvid.is_empty() {
                return Err(BiliLiveError::Parse(
                    "未能从番剧详情解析出关联视频 ID".to_string(),
                ));
            }
            Ok((page.bvid, page.aid))
        }
        _ => Err(BiliLiveError::Input(format!(
            "输入不是有效的视频标识: {id}"
        ))),
    }
}

/// 解析视频/番剧标识并返回纯数字 `aid`；若输入已包含 `aid` 则不额外请求视频详情接口
pub fn resolve_video_aid(id: &str, cookies: Option<&Cookies>) -> Result<i64> {
    let parsed = crate::utils::id::parse_input(id, false)?;
    match parsed.kind {
        IdKind::Video => {
            if let Some(aid) = parsed.aid {
                return Ok(aid);
            }
            if let Some(bvid) = parsed.bvid {
                let target = crate::utils::id::VideoTarget {
                    bvid: Some(bvid.clone()),
                    aid: None,
                    page: None,
                };
                let info = get_video_info_from_target(&target, &bvid, cookies)?;
                return info["aid"]
                    .as_i64()
                    .ok_or_else(|| BiliLiveError::Parse("未能获取到视频aid".to_string()));
            }
            Err(BiliLiveError::Input(format!("无效的视频标识: {id}")))
        }
        IdKind::Numeric => Ok(parsed.id.parse::<i64>()?),
        IdKind::BangumiEpisode | IdKind::BangumiSeason | IdKind::BangumiMedia => {
            let (_, aid) = resolve_aid_and_bvid(id, cookies)?;
            if aid > 0 {
                Ok(aid)
            } else {
                Err(BiliLiveError::Parse("未能从番剧解析出视频 aid".to_string()))
            }
        }
        _ => Err(BiliLiveError::Input(format!(
            "输入不是有效的视频标识: {id}"
        ))),
    }
}

/// 点赞或取消点赞
pub fn like_video(cookies: &Cookies, id: &str, cancel: bool) -> Result<()> {
    let (bvid, aid) = resolve_aid_and_bvid(id, Some(cookies))?;
    let like_val = if cancel { "2" } else { "1" };

    let params = [
        ("bvid", bvid.as_str()),
        ("aid", &aid.to_string()),
        ("like", like_val),
        ("csrf", cookies.bili_jct.as_str()),
    ];

    let body =
        serde_urlencoded::to_string(params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/{bvid}");
    let url = format!(
        "{}/x/web-interface/archive/like",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = crate::api::client::parse_json_response(&resp, "点赞操作失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "点赞操作失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(())
}

/// 投币视频
pub fn coin_video(cookies: &Cookies, id: &str, multiply: u32, also_like: bool) -> Result<bool> {
    if !(1..=2).contains(&multiply) {
        return Err(BiliLiveError::Input("投币数量必须为 1 或 2".to_string()));
    }
    let (bvid, aid) = resolve_aid_and_bvid(id, Some(cookies))?;
    let mult = multiply.to_string();
    let sel_like = if also_like { "1" } else { "0" };

    let params = [
        ("bvid", bvid.as_str()),
        ("aid", &aid.to_string()),
        ("multiply", &mult),
        ("select_like", sel_like),
        ("csrf", cookies.bili_jct.as_str()),
    ];

    let body =
        serde_urlencoded::to_string(params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/{bvid}");
    let url = format!(
        "{}/x/web-interface/coin/add",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = crate::api::client::parse_json_response(&resp, "投币操作失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "投币操作失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let liked = json["data"]["like"].as_bool().unwrap_or(false);
    Ok(liked)
}

/// 一键三连
pub fn triple_video(cookies: &Cookies, id: &str) -> Result<serde_json::Value> {
    let (bvid, aid) = resolve_aid_and_bvid(id, Some(cookies))?;

    let params = [
        ("bvid", bvid.as_str()),
        ("aid", &aid.to_string()),
        ("csrf", cookies.bili_jct.as_str()),
    ];

    let body =
        serde_urlencoded::to_string(params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/{bvid}");
    let url = format!(
        "{}/x/web-interface/archive/like/triple",
        crate::api::client::api_base_url()
    );
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    let json: serde_json::Value = crate::api::client::parse_json_response(&resp, "一键三连失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "一键三连失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    Ok(json["data"].clone())
}

fn attach_optional_cookies(mut req: minreq::Request, cookies: Option<&Cookies>) -> minreq::Request {
    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            req = req.with_header("Cookie", header);
        }
    }
    req
}

fn build_video_id_params<'a>(
    bvid: &'a str,
    aid_str: &'a str,
    use_avid: bool,
) -> Vec<(&'a str, &'a str)> {
    let mut params = Vec::new();
    if !bvid.is_empty() {
        params.push(("bvid", bvid));
    } else if !aid_str.is_empty() {
        params.push((if use_avid { "avid" } else { "aid" }, aid_str));
    }
    params
}

/// 获取番剧/影视播放流信息 (`GET /pgc/player/web/playurl`，失败回退 `get_video_playurl`)
pub fn get_pgc_playurl(
    bvid: &str,
    aid: i64,
    cid: i64,
    qn: u32,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let cid_str = cid.to_string();
    let qn_str = qn.to_string();
    let aid_str = aid.to_string();
    let mut params = vec![
        ("cid", cid_str.as_str()),
        ("qn", qn_str.as_str()),
        ("fnval", "4048"),
        ("fnver", "0"),
        ("fourk", "1"),
        ("otype", "json"),
    ];
    if !bvid.is_empty() {
        params.push(("bvid", bvid));
    }
    if aid > 0 {
        params.push(("avid", aid_str.as_str()));
    }
    let query =
        serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let url = format!(
        "{}/pgc/player/web/playurl?{query}",
        crate::api::client::api_base_url()
    );
    let req = attach_optional_cookies(
        crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Referer", "https://www.bilibili.com/"),
        cookies,
    );
    if let Ok(resp) = req.send()
        && resp.status_code < 400
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(resp.as_str()?)
        && json["code"].as_i64() == Some(0)
    {
        let res = json
            .get("result")
            .or_else(|| json.get("data"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if !res.is_null() {
            return Ok(res);
        }
    }
    get_video_playurl(bvid, aid, cid, qn, cookies)
}

/// 获取视频播放流信息 (`GET /x/player/wbi/playurl`)
pub fn get_video_playurl(
    bvid: &str,
    aid: i64,
    cid: i64,
    qn: u32,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let aid_str = aid.to_string();
    let cid_str = cid.to_string();
    let qn_str = qn.to_string();

    let mut params = build_video_id_params(bvid, &aid_str, true);
    params.push(("cid", &cid_str));
    params.push(("qn", &qn_str));
    params.push(("fnval", "4048"));
    params.push(("fnver", "0"));
    params.push(("fourk", "1"));
    params.push(("otype", "json"));

    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let query = match crate::api::wbi::sign_params(&params, sessdata) {
        Ok(q) => q,
        Err(_) => {
            serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?
        }
    };

    let url = format!(
        "{}/x/player/wbi/playurl?{query}",
        crate::api::client::api_base_url()
    );
    let referer = if !bvid.is_empty() {
        format!("https://www.bilibili.com/video/{bvid}")
    } else {
        "https://www.bilibili.com/".to_string()
    };

    let req = attach_optional_cookies(
        crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Referer", &referer),
        cookies,
    );

    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "获取视频流失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取视频流失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(json["data"].clone())
}

/// 获取播放器 v2 元数据（含字幕列表 `subtitle.subtitles`）
pub fn get_player_v2(
    bvid: &str,
    aid: i64,
    cid: i64,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let aid_str = aid.to_string();
    let cid_str = cid.to_string();

    let mut params = build_video_id_params(bvid, &aid_str, false);
    params.push(("cid", &cid_str));

    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let query = match crate::api::wbi::sign_params(&params, sessdata) {
        Ok(q) => q,
        Err(_) => {
            serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?
        }
    };

    let url = format!(
        "{}/x/player/wbi/v2?{query}",
        crate::api::client::api_base_url()
    );
    let req = attach_optional_cookies(
        crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Referer", "https://www.bilibili.com/"),
        cookies,
    );

    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "获取播放器信息失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取播放器信息失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(json["data"].clone())
}

/// 标准化 B站资源 URL（处理 `//...` 协议相对路径及 `/...` 相对路径）
pub fn normalize_remote_url(raw_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else if trimmed.starts_with('/') {
        format!("{}{}", crate::api::client::api_base_url(), trimmed)
    } else {
        trimmed.to_string()
    }
}

/// 拉取字幕 JSON 文件 (`{ body: [{ from, to, content }, ...] }`)
pub fn fetch_subtitle_json(raw_url: &str) -> Result<serde_json::Value> {
    let url = normalize_remote_url(raw_url);
    if url.is_empty() {
        return Err(BiliLiveError::Parse("字幕地址为空".to_string()));
    }
    let resp = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/")
        .send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "下载字幕失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    Ok(json)
}

/// 获取视频 AI 总结 (`GET /x/web-interface/view/conclusion/get`，严格使用 WBI 签名)
pub fn get_video_summary(
    bvid: &str,
    aid: i64,
    cid: i64,
    up_mid: i64,
    cookies: Option<&Cookies>,
) -> Result<serde_json::Value> {
    let aid_str = aid.to_string();
    let cid_str = cid.to_string();
    let up_mid_str = up_mid.to_string();

    let mut params = build_video_id_params(bvid, &aid_str, false);
    params.push(("cid", &cid_str));
    if up_mid > 0 {
        params.push(("up_mid", &up_mid_str));
    }

    let sessdata = cookies.map(|c| c.sessdata.as_str());
    let query = crate::api::wbi::sign_params(&params, sessdata)?;

    let url = format!(
        "{}/x/web-interface/view/conclusion/get?{query}",
        crate::api::client::api_base_url()
    );
    let req = attach_optional_cookies(
        crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Referer", "https://www.bilibili.com/"),
        cookies,
    );

    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "获取 AI 总结失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["message"].as_str().unwrap_or("未知错误");
        if matches!(code, -101 | -111 | -403) {
            return Err(BiliLiveError::Auth(format!(
                "获取 AI 总结失败 ({code}): {msg}"
            )));
        }
        return Err(BiliLiveError::Api(format!(
            "获取 AI 总结失败 ({code}): {msg}"
        )));
    }
    Ok(json["data"].clone())
}

fn resolve_bvid_or_aid_pair(
    id: &str,
    cookies: Option<&Cookies>,
) -> Result<(Option<String>, Option<i64>)> {
    let parsed = crate::utils::id::parse_input(id, false)?;
    match parsed.kind {
        IdKind::Video => Ok((parsed.bvid, parsed.aid)),
        IdKind::Numeric => Ok((None, Some(parsed.id.parse::<i64>()?))),
        IdKind::BangumiEpisode | IdKind::BangumiSeason | IdKind::BangumiMedia => {
            let (bvid, aid) = resolve_aid_and_bvid(id, cookies)?;
            Ok(((!bvid.is_empty()).then_some(bvid), (aid > 0).then_some(aid)))
        }
        _ => Err(BiliLiveError::Input(format!(
            "输入不是有效的视频标识: {id}"
        ))),
    }
}

/// 获取视频标签 (`GET /x/web-interface/view/detail/tag`)
pub fn get_video_tags(id: &str, cookies: Option<&Cookies>) -> Result<Vec<serde_json::Value>> {
    let (bvid, aid) = resolve_bvid_or_aid_pair(id, cookies)?;
    let aid_str = aid.map(|a| a.to_string());
    let mut params = Vec::new();
    if let Some(ref b) = bvid {
        params.push(("bvid", b.as_str()));
    } else if let Some(ref a) = aid_str {
        params.push(("aid", a.as_str()));
    } else {
        return Err(BiliLiveError::Input(format!("无效的视频标识: {id}")));
    }

    let query =
        serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let url = format!(
        "{}/x/web-interface/view/detail/tag?{query}",
        crate::api::client::api_base_url()
    );
    let req = attach_optional_cookies(
        crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Referer", "https://www.bilibili.com/"),
        cookies,
    );

    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "获取视频标签失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取视频标签失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    if json["data"].is_null() {
        return Ok(Vec::new());
    }
    Ok(json["data"].as_array().cloned().unwrap_or_default())
}

/// 获取相关推荐视频 (`GET /x/web-interface/archive/related`)
pub fn get_video_related(id: &str, cookies: Option<&Cookies>) -> Result<Vec<serde_json::Value>> {
    let (bvid, aid) = resolve_bvid_or_aid_pair(id, cookies)?;
    let aid_str = aid.map(|a| a.to_string());
    let mut params = Vec::new();
    if let Some(ref b) = bvid {
        params.push(("bvid", b.as_str()));
    } else if let Some(ref a) = aid_str {
        params.push(("aid", a.as_str()));
    } else {
        return Err(BiliLiveError::Input(format!("无效的视频标识: {id}")));
    }

    let query =
        serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    let url = format!(
        "{}/x/web-interface/archive/related?{query}",
        crate::api::client::api_base_url()
    );
    let req = attach_optional_cookies(
        crate::api::client::get(&url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Referer", "https://www.bilibili.com/"),
        cookies,
    );

    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "获取相关推荐失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: serde_json::Value = serde_json::from_str(resp.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取相关推荐失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    if json["data"].is_null() {
        return Ok(Vec::new());
    }
    Ok(json["data"].as_array().cloned().unwrap_or_default())
}

fn value_str_dual(v: &serde_json::Value, k1: &str, k2: &str) -> String {
    v.get(k1)
        .and_then(|x| x.as_str())
        .or_else(|| v.get(k2).and_then(|x| x.as_str()))
        .unwrap_or("")
        .to_string()
}

fn value_str_list_dual(v: &serde_json::Value, k1: &str, k2: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in [k1, k2] {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str()
                    && !s.trim().is_empty()
                    && !out.iter().any(|existing| existing == s)
                {
                    out.push(s.to_string());
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DashStream {
    pub id: u32,
    pub base_url: String,
    pub backup_url: Vec<String>,
    pub bandwidth: u64,
    pub mime_type: String,
    pub codecs: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: String,
    pub codecid: u32,
}

impl DashStream {
    /// 从 JSON 值解析 DASH 流对象（同时兼容 `camelCase`、`snake_case` 以及两者同时存在的情况）
    pub fn from_json_value(v: &serde_json::Value) -> Option<Self> {
        if !v.is_object() {
            return None;
        }
        Some(Self {
            id: v.get("id").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            base_url: value_str_dual(v, "base_url", "baseUrl"),
            backup_url: value_str_list_dual(v, "backup_url", "backupUrl"),
            bandwidth: v.get("bandwidth").and_then(|x| x.as_u64()).unwrap_or(0),
            mime_type: value_str_dual(v, "mime_type", "mimeType"),
            codecs: v
                .get("codecs")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            width: v.get("width").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            height: v.get("height").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            frame_rate: value_str_dual(v, "frame_rate", "frameRate"),
            codecid: v.get("codecid").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        })
    }

    pub fn all_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        if !self.base_url.trim().is_empty() {
            urls.push(self.base_url.clone());
        }
        for u in &self.backup_url {
            if !u.trim().is_empty() && !urls.contains(u) {
                urls.push(u.clone());
            }
        }
        urls
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DurlSegment {
    pub order: u32,
    pub length: u64,
    pub size: u64,
    pub url: String,
    pub backup_url: Vec<String>,
}

impl DurlSegment {
    pub fn from_json_value(v: &serde_json::Value) -> Option<Self> {
        if !v.is_object() {
            return None;
        }
        Some(Self {
            order: v.get("order").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            length: v.get("length").and_then(|x| x.as_u64()).unwrap_or(0),
            size: v.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
            url: v
                .get("url")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            backup_url: value_str_list_dual(v, "backup_url", "backupUrl"),
        })
    }

    pub fn all_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        if !self.url.trim().is_empty() {
            urls.push(self.url.clone());
        }
        for u in &self.backup_url {
            if !u.trim().is_empty() && !urls.contains(u) {
                urls.push(u.clone());
            }
        }
        urls
    }
}

/// 从 `playurl` 响应 `data` 解析 DASH 视频流与音频流（兼容 null `audio`/`dolby`/`flac` 及单对象 `flac.audio`）
pub fn parse_dash_streams(playurl_data: &serde_json::Value) -> (Vec<DashStream>, Vec<DashStream>) {
    let Some(dash) = playurl_data.get("dash") else {
        return (Vec::new(), Vec::new());
    };
    if dash.is_null() {
        return (Vec::new(), Vec::new());
    }

    let videos = dash
        .get("video")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(DashStream::from_json_value).collect())
        .unwrap_or_default();

    let mut audios: Vec<DashStream> = dash
        .get("audio")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(DashStream::from_json_value).collect())
        .unwrap_or_default();

    if let Some(dolby_arr) = dash
        .get("dolby")
        .and_then(|d| d.get("audio"))
        .and_then(|a| a.as_array())
    {
        for item in dolby_arr {
            if let Some(s) = DashStream::from_json_value(item) {
                audios.push(s);
            }
        }
    }

    if let Some(flac_audio) = dash.get("flac").and_then(|f| f.get("audio"))
        && flac_audio.is_object()
        && let Some(s) = DashStream::from_json_value(flac_audio)
    {
        audios.push(s);
    }

    (videos, audios)
}

/// 解析 `durl` 传统分段流列表
pub fn parse_durl_segments(playurl_data: &serde_json::Value) -> Vec<DurlSegment> {
    playurl_data
        .get("durl")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(DurlSegment::from_json_value)
                .collect()
        })
        .unwrap_or_default()
}

pub fn stream_matches_codec(stream: &DashStream, codec: &str) -> bool {
    let c = stream.codecs.to_ascii_lowercase();
    match codec.to_ascii_lowercase().as_str() {
        "avc" | "h264" => stream.codecid == 7 || c.starts_with("avc"),
        "hevc" | "h265" => stream.codecid == 12 || c.starts_with("hev") || c.starts_with("hvc"),
        "av1" => stream.codecid == 13 || c.starts_with("av01"),
        _ => false,
    }
}

pub fn codec_label(stream: &DashStream) -> &'static str {
    let c = stream.codecs.to_ascii_lowercase();
    if stream.codecid == 13 || c.starts_with("av01") {
        "av1"
    } else if stream.codecid == 12 || c.starts_with("hev") || c.starts_with("hvc") {
        "hevc"
    } else if stream.codecid == 7 || c.starts_with("avc") {
        "avc"
    } else {
        "unknown"
    }
}

/// 根据目标清晰度 `qn`（精确匹配，否则 <= qn 最高档，否则最低可用档）及偏好编码选择 DASH 视频流
pub fn select_dash_video(
    streams: &[DashStream],
    requested_qn: u32,
    preferred_codec: &str,
) -> Option<DashStream> {
    if streams.is_empty() {
        return None;
    }

    let mut qns: Vec<u32> = streams.iter().map(|s| s.id).collect();
    qns.sort_unstable();
    qns.dedup();

    let target_qn = if qns.contains(&requested_qn) {
        requested_qn
    } else {
        qns.iter()
            .copied()
            .filter(|&q| q <= requested_qn)
            .max()
            .unwrap_or(qns[0])
    };

    let candidates: Vec<&DashStream> = streams.iter().filter(|s| s.id == target_qn).collect();

    if let Some(best) = candidates
        .iter()
        .copied()
        .filter(|s| stream_matches_codec(s, preferred_codec))
        .max_by_key(|s| s.bandwidth)
    {
        return Some(best.clone());
    }

    for fallback_codec in ["avc", "hevc", "av1"] {
        if let Some(best) = candidates
            .iter()
            .copied()
            .filter(|s| stream_matches_codec(s, fallback_codec))
            .max_by_key(|s| s.bandwidth)
        {
            return Some(best.clone());
        }
    }

    candidates.into_iter().max_by_key(|s| s.bandwidth).cloned()
}

/// 选择最高码率 DASH 音频流
#[allow(dead_code)]
pub fn select_dash_audio(streams: &[DashStream]) -> Option<DashStream> {
    select_dash_audio_with_quality(streams, None)
}

fn audio_quality_rank(id: u32) -> u32 {
    match id {
        30251 => 500, // Hi-Res FLAC
        30250 => 400, // Dolby Atmos
        30280 => 300, // 192K
        30232 => 200, // 132K
        30216 => 100, // 64K
        other => other,
    }
}

/// 按可选目标音质选择 DASH 音频流（精确匹配优先，否则向下取最接近档，未指定取最高音质）
pub fn select_dash_audio_with_quality(
    streams: &[DashStream],
    requested_aq: Option<u32>,
) -> Option<DashStream> {
    if streams.is_empty() {
        return None;
    }
    if let Some(aq) = requested_aq {
        if let Some(exact) = streams
            .iter()
            .filter(|s| s.id == aq)
            .max_by_key(|s| s.bandwidth)
        {
            return Some(exact.clone());
        }
        let target_rank = audio_quality_rank(aq);
        if let Some(lower) = streams
            .iter()
            .filter(|s| audio_quality_rank(s.id) <= target_rank)
            .max_by_key(|s| (audio_quality_rank(s.id), s.bandwidth))
        {
            return Some(lower.clone());
        }
    }
    streams
        .iter()
        .max_by_key(|s| (audio_quality_rank(s.id), s.bandwidth))
        .cloned()
}

/// 根据音频流 ID 与编码推断真实后缀（避免将无损 FLAC / 杜比全景声硬编码为 .m4a）
pub fn audio_stream_extension(stream: &DashStream) -> &'static str {
    let c = stream.codecs.to_ascii_lowercase();
    if stream.id == 30251 || c.contains("flac") {
        "flac"
    } else if stream.id == 30250 || c.contains("ec-3") || c.contains("eac3") {
        "eac3"
    } else {
        "m4a"
    }
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

fn render_download_progress(
    downloaded: u64,
    total: Option<u64>,
    bps: f64,
    enable_color: bool,
) -> String {
    let speed_str = format!("{}/s", format_bytes_human(bps.max(0.0) as u64));
    match total.filter(|&t| t > 0) {
        Some(tot) => {
            let ratio = (downloaded as f64 / tot as f64).clamp(0.0, 1.0);
            let bar_w = 18usize;
            let filled = (ratio * bar_w as f64).round() as usize;
            let empty = bar_w.saturating_sub(filled);
            let pct = (ratio * 100.0).round() as u32;
            let f_bar = "━".repeat(filled);
            let e_bar = "─".repeat(empty);
            let size_str = format!(
                "{} / {}",
                format_bytes_human(downloaded),
                format_bytes_human(tot)
            );
            if enable_color {
                use crossterm::style::Stylize;
                format!(
                    "  {} {}{} {:>3}%  {}  {}",
                    "⬇".with(crate::ui::theme::BILI_PINK),
                    f_bar.with(crate::ui::theme::BILI_PINK),
                    e_bar.dark_grey(),
                    pct,
                    size_str.with(crate::ui::theme::BILI_CYAN),
                    speed_str.dark_grey()
                )
            } else {
                format!("  ⬇ [{f_bar}{e_bar}] {pct:>3}%  {size_str}  {speed_str}")
            }
        }
        None => {
            let size_str = format_bytes_human(downloaded);
            if enable_color {
                use crossterm::style::Stylize;
                format!(
                    "  {} 已下载 {}  {}",
                    "⬇".with(crate::ui::theme::BILI_PINK),
                    size_str.with(crate::ui::theme::BILI_CYAN),
                    speed_str.dark_grey()
                )
            } else {
                format!("  ⬇ 已下载 {size_str}  {speed_str}")
            }
        }
    }
}

/// 流式下载二进制资源到目标路径（支持 64KiB 分块 Ctrl+C 即时中断清理、TTY 实时进度条，并依次尝试备用地址）
pub fn download_binary_to_path(
    urls: &[String],
    referer: &str,
    cookies: Option<&Cookies>,
    dest_path: &Path,
) -> Result<u64> {
    if urls.is_empty() {
        return Err(BiliLiveError::Input("下载地址列表为空".to_string()));
    }
    crate::utils::interrupt::check_interrupted()?;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(None)
        .build()
        .into();

    let cookie_header = cookies.map(|c| c.cookie_header()).filter(|h| !h.is_empty());

    let show_progress = !crate::cli::output::is_json()
        && !crate::cli::output::is_quiet()
        && std::io::stderr().is_terminal();
    let use_ansi = show_progress && crate::ui::theme::stderr_color_enabled();

    let mut last_err: Option<BiliLiveError> = None;
    for raw_url in urls {
        crate::utils::interrupt::check_interrupted()?;
        let url = normalize_remote_url(raw_url);
        if url.is_empty() {
            continue;
        }

        let mut req = agent
            .get(&url)
            .header("User-Agent", DEFAULT_USER_AGENT)
            .header("Referer", referer);
        if let Some(ref ch) = cookie_header {
            req = req.header("Cookie", ch);
        }

        match req.call() {
            Ok(resp) => {
                let total_len = resp
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse::<u64>().ok());

                crate::utils::interrupt::register_temp_path(dest_path.to_path_buf());
                let mut file = match std::fs::File::create(dest_path) {
                    Ok(f) => f,
                    Err(e) => {
                        crate::utils::interrupt::unregister_temp_path(dest_path);
                        return Err(BiliLiveError::Io(e));
                    }
                };

                let mut reader = resp.into_body().into_reader();
                let mut buf = vec![0u8; 64 * 1024];
                let mut downloaded: u64 = 0;
                let started = Instant::now();
                let mut last_draw = Instant::now()
                    .checked_sub(Duration::from_millis(200))
                    .unwrap_or_else(Instant::now);
                let mut drew_bar = false;

                if show_progress && use_ansi {
                    let mut stderr = std::io::stderr();
                    let _ = crossterm::execute!(stderr, crossterm::cursor::Hide);
                    crate::utils::interrupt::set_stderr_cursor_hidden(true);
                }

                let copy_res: Result<()> = (|| {
                    loop {
                        crate::utils::interrupt::check_interrupted()?;
                        let n = reader.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        file.write_all(&buf[..n])?;
                        downloaded += n as u64;

                        if show_progress && last_draw.elapsed() >= Duration::from_millis(80) {
                            let secs = started.elapsed().as_secs_f64().max(0.001);
                            let line = render_download_progress(
                                downloaded,
                                total_len,
                                downloaded as f64 / secs,
                                use_ansi,
                            );
                            if use_ansi {
                                eprint!("\r\x1b[K{line}");
                            } else {
                                eprint!("\r{line}");
                            }
                            let _ = std::io::stderr().flush();
                            last_draw = Instant::now();
                            drew_bar = true;
                        }
                    }
                    file.flush()?;
                    Ok(())
                })();

                if show_progress {
                    let mut stderr = std::io::stderr();
                    if use_ansi {
                        let _ = crossterm::execute!(stderr, crossterm::cursor::Show);
                        crate::utils::interrupt::set_stderr_cursor_hidden(false);
                        if drew_bar {
                            eprint!("\r\x1b[K");
                        }
                    } else if drew_bar {
                        eprintln!();
                    }
                    let _ = stderr.flush();
                }

                match copy_res {
                    Ok(()) => {
                        crate::utils::interrupt::unregister_temp_path(dest_path);
                        return Ok(downloaded);
                    }
                    Err(BiliLiveError::Interrupted) => {
                        crate::utils::interrupt::unregister_temp_path(dest_path);
                        let _ = std::fs::remove_file(dest_path);
                        return Err(BiliLiveError::Interrupted);
                    }
                    Err(e) => {
                        crate::utils::interrupt::unregister_temp_path(dest_path);
                        let _ = std::fs::remove_file(dest_path);
                        last_err = Some(e);
                    }
                }
            }
            Err(e) => {
                last_err = Some(BiliLiveError::WebNetwork(e));
            }
        }
    }

    crate::utils::interrupt::unregister_temp_path(dest_path);
    let _ = std::fs::remove_file(dest_path);
    Err(last_err.unwrap_or_else(|| BiliLiveError::Api("下载失败：无可用地址".to_string())))
}

/// 使用 ffmpeg 将分离的视频流与音频流无损混流为目标文件
pub fn mux_dash_streams(
    ffmpeg_bin: &Path,
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
) -> Result<()> {
    let out = std::process::Command::new(ffmpeg_bin)
        .args(["-y", "-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(video_path)
        .arg("-i")
        .arg(audio_path)
        .args(["-c", "copy"])
        .arg(output_path)
        .output()?;

    if !out.status.success() {
        let _ = std::fs::remove_file(output_path);
        let code_str = out
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        let stderr_tail = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(BiliLiveError::ExternalTool(format!(
            "ffmpeg 混流失败 (exit {code_str}): {stderr_tail}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_video_param() {
        assert_eq!(
            parse_video_param("BV1uJ411r7hL"),
            (Some("BV1uJ411r7hL".to_string()), None)
        );
        assert_eq!(
            parse_video_param("bv1uJ411r7hL"),
            (Some("BV1uJ411r7hL".to_string()), None)
        );
        assert_eq!(parse_video_param("av79677524"), (None, Some(79677524)));
        assert_eq!(parse_video_param("AV79677524"), (None, Some(79677524)));
        assert_eq!(parse_video_param("79677524"), (None, Some(79677524)));
        assert_eq!(
            parse_video_param("https://www.bilibili.com/video/bv1uJ411r7hL?p=2&spm_id_from=333"),
            (Some("BV1uJ411r7hL".to_string()), None)
        );
        assert_eq!(
            parse_video_param("https://www.bilibili.com/video/av79677524/"),
            (None, Some(79677524))
        );
        assert_eq!(parse_video_param("invalid_input"), (None, None));
        assert_eq!(parse_video_param("ep12345"), (None, None));
    }

    #[test]
    fn test_resolve_video_aid_for_av_and_bv() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct RestoreApi(Option<String>);
        impl Drop for RestoreApi {
            fn drop(&mut self) {
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                        None => std::env::remove_var("BT_API_BASE_URL"),
                    }
                }
            }
        }
        let _restore = RestoreApi(std::env::var("BT_API_BASE_URL").ok());

        // av ID resolves locally without HTTP
        assert_eq!(resolve_video_aid("av79677524", None).unwrap(), 79677524);
        assert_eq!(
            resolve_video_aid("https://www.bilibili.com/video/av12345/", None).unwrap(),
            12345
        );

        // BV ID resolves via /x/web-interface/wbi/view (WBI /nav may fail -> fallback unsigned view)
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first_line = String::new();
                reader.read_line(&mut first_line).unwrap();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                let body = if first_line.contains("/x/web-interface/nav") {
                    r#"{"code":-101,"message":"not logged in"}"#.to_string()
                } else {
                    assert!(first_line.contains("/x/web-interface/wbi/view"));
                    assert!(first_line.contains("bvid=BV1uJ411r7hL"));
                    r#"{"code":0,"data":{"aid":79677524,"bvid":"BV1uJ411r7hL","title":"Test"}}"#
                        .to_string()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
                if first_line.contains("/x/web-interface/wbi/view") {
                    break;
                }
            }
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &base);
        }
        let aid =
            resolve_video_aid("https://www.bilibili.com/video/bv1uJ411r7hL?p=2", None).unwrap();
        handle.join().unwrap();
        assert_eq!(aid, 79677524);
    }

    #[test]
    fn test_coin_video_rejects_out_of_range() {
        let c = Cookies {
            sessdata: "s".to_string(),
            bili_jct: "j".to_string(),
            dede_user_id: "1".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            coin_video(&c, "BV1uJ411r7hL", 0, false).unwrap_err(),
            BiliLiveError::Input(_)
        ));
        assert!(matches!(
            coin_video(&c, "BV1uJ411r7hL", 3, false).unwrap_err(),
            BiliLiveError::Input(_)
        ));
    }

    #[test]
    fn test_extract_page_info_bounds_and_fallback() {
        let data = serde_json::json!({
            "bvid": "BV1uJ411r7hL",
            "aid": 79677524,
            "title": "Main Title",
            "pic": "https://example.com/cover.jpg",
            "duration": 300,
            "owner": { "mid": 42 },
            "cid": 1001,
            "pages": [
                { "cid": 1001, "page": 1, "part": "P1", "duration": 100 },
                { "cid": 1002, "page": 2, "part": "P2", "duration": 200 }
            ]
        });

        let p2 = extract_page_info(&data, 2).unwrap();
        assert_eq!(p2.cid, 1002);
        assert_eq!(p2.part, "P2");
        assert_eq!(p2.total_pages, 2);
        assert_eq!(p2.up_mid, 42);

        assert!(matches!(
            extract_page_info(&data, 0).unwrap_err(),
            BiliLiveError::Input(_)
        ));
        assert!(matches!(
            extract_page_info(&data, 99).unwrap_err(),
            BiliLiveError::Input(_)
        ));

        let no_pages = serde_json::json!({
            "bvid": "BV1uJ411r7hL",
            "aid": 79677524,
            "title": "Single",
            "cid": 555,
            "duration": 60,
            "owner": { "mid": 7 },
            "pages": []
        });
        let p1 = extract_page_info(&no_pages, 1).unwrap();
        assert_eq!(p1.cid, 555);
        assert_eq!(p1.part, "Single");
    }

    #[test]
    fn test_normalize_remote_url() {
        assert_eq!(
            normalize_remote_url("//aisubtitle.hdslb.com/bfs/sub.json"),
            "https://aisubtitle.hdslb.com/bfs/sub.json"
        );
        assert_eq!(
            normalize_remote_url("http://127.0.0.1:8080/mock/sub.json"),
            "http://127.0.0.1:8080/mock/sub.json"
        );
        assert_eq!(
            normalize_remote_url("https://i0.hdslb.com/cover.jpg"),
            "https://i0.hdslb.com/cover.jpg"
        );
    }

    #[test]
    fn test_parse_dash_and_select_streams() {
        let playurl = serde_json::json!({
            "dash": {
                "video": [
                    {
                        "id": 80,
                        "baseUrl": "https://cdn/80_avc.m4s",
                        "base_url": "https://cdn/80_avc.m4s",
                        "backupUrl": ["https://bak/80_avc.m4s"],
                        "backup_url": ["https://bak/80_avc.m4s"],
                        "bandwidth": 2000000,
                        "mimeType": "video/mp4",
                        "mime_type": "video/mp4",
                        "codecs": "avc1.640028",
                        "width": 1920,
                        "height": 1080,
                        "frameRate": "30",
                        "frame_rate": "30",
                        "codecid": 7
                    },
                    {
                        "id": 80,
                        "base_url": "https://cdn/80_hevc.m4s",
                        "backup_url": null,
                        "bandwidth": 1600000,
                        "mime_type": "video/mp4",
                        "codecs": "hev1.1.6.L120.90",
                        "width": 1920,
                        "height": 1080,
                        "frame_rate": "30",
                        "codecid": 12
                    },
                    {
                        "id": 64,
                        "base_url": "https://cdn/64_avc.m4s",
                        "bandwidth": 1000000,
                        "codecs": "avc1.64001F",
                        "codecid": 7
                    }
                ],
                "audio": null,
                "dolby": { "type": 0, "audio": null },
                "flac": {
                    "display": true,
                    "audio": {
                        "id": 30251,
                        "baseUrl": "https://cdn/flac.m4s",
                        "bandwidth": 320000,
                        "codecs": "fLaC"
                    }
                }
            }
        });

        let (videos, audios) = parse_dash_streams(&playurl);
        assert_eq!(videos.len(), 3);
        assert_eq!(audios.len(), 1);
        assert_eq!(audios[0].id, 30251);

        // Request 120 -> falls back to 80, prefers hevc
        let picked = select_dash_video(&videos, 120, "hevc").unwrap();
        assert_eq!(picked.id, 80);
        assert_eq!(picked.codecid, 12);
        assert_eq!(picked.base_url, "https://cdn/80_hevc.m4s");

        // Request 64 with av1 -> falls back to avc at 64
        let picked64 = select_dash_video(&videos, 64, "av1").unwrap();
        assert_eq!(picked64.id, 64);

        let picked_audio = select_dash_audio(&audios).unwrap();
        assert_eq!(picked_audio.id, 30251);
    }

    #[test]
    fn test_download_binary_to_path_backup_url_fallback() {
        use std::io::{BufRead, BufReader, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for idx in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut saw_referer = false;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if line.to_ascii_lowercase().starts_with("referer:")
                        && line.contains("bilibili.com")
                    {
                        saw_referer = true;
                    }
                }
                assert!(saw_referer);
                if idx == 0 {
                    assert!(first.contains("/primary.m4s"));
                    write!(
                        stream,
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 3\r\nConnection: close\r\n\r\nerr"
                    )
                    .unwrap();
                } else {
                    assert!(first.contains("/backup.m4s"));
                    let payload = b"STREAM_OK_BYTES";
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    stream.write_all(hdr.as_bytes()).unwrap();
                    stream.write_all(payload).unwrap();
                }
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("media.m4s");
        let urls = vec![
            format!("http://{addr}/primary.m4s"),
            format!("http://{addr}/backup.m4s"),
        ];
        let written =
            download_binary_to_path(&urls, "https://www.bilibili.com/", None, &out).unwrap();
        handle.join().unwrap();
        assert_eq!(written, 15);
        assert_eq!(std::fs::read(&out).unwrap(), b"STREAM_OK_BYTES");
    }

    #[test]
    fn test_summary_strict_wbi_and_null_safe_tags_related() {
        use std::io::{BufRead, BufReader, Write as _};
        use std::net::TcpListener;

        let _lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct RestoreApi(Option<String>);
        impl Drop for RestoreApi {
            fn drop(&mut self) {
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                        None => std::env::remove_var("BT_API_BASE_URL"),
                    }
                }
            }
        }
        let _restore = RestoreApi(std::env::var("BT_API_BASE_URL").ok());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for _ in 0..4 {
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
                let body = if first.contains("/x/web-interface/nav") {
                    r#"{"code":0,"data":{"wbi_img":{"img_url":"https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png","sub_url":"https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"}}}"#
                } else if first.contains("/x/web-interface/view/conclusion/get") {
                    assert!(first.contains("wts="));
                    assert!(first.contains("w_rid="));
                    r#"{"code":0,"data":{"code":-1,"stid":"","status":0,"model_result":null}}"#
                } else if first.contains("/x/web-interface/view/detail/tag") {
                    r#"{"code":0,"data":null}"#
                } else {
                    assert!(first.contains("/x/web-interface/archive/related"));
                    r#"{"code":0,"data":null}"#
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        unsafe { std::env::set_var("BT_API_BASE_URL", &base) };
        let summary = get_video_summary("BV1uJ411r7hL", 79677524, 146044693, 42, None).unwrap();
        assert_eq!(summary["code"], -1);
        assert!(summary["model_result"].is_null());

        let tags = get_video_tags("BV1uJ411r7hL", None).unwrap();
        assert!(tags.is_empty());

        let related = get_video_related("BV1uJ411r7hL", None).unwrap();
        assert!(related.is_empty());

        handle.join().unwrap();
    }
}
