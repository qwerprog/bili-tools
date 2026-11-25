use crate::api::client::DEFAULT_USER_AGENT;
use crate::auth::cookies::Cookies;
use crate::error::{BiliLiveError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn deserialize_null_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn option_reply_control_is_empty(v: &Option<ReplyControl>) -> bool {
    v.as_ref().is_none_or(ReplyControl::is_empty)
}

fn option_up_action_is_empty(v: &Option<ReplyUpAction>) -> bool {
    v.as_ref().is_none_or(ReplyUpAction::is_empty)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommentPageInfo {
    #[serde(default)]
    pub num: u64,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub acount: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplyLevelInfo {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub current_level: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplyMember {
    #[serde(default)]
    pub uname: String,
    #[serde(default)]
    pub mid: String,
    #[serde(default)]
    pub level_info: ReplyLevelInfo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplyPicture {
    #[serde(default)]
    pub img_src: String,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub img_width: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub img_height: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplyContent {
    #[serde(default)]
    pub message: String,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub pictures: Vec<ReplyPicture>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplyControl {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_desc: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "is_false"
    )]
    pub up_reply: bool,
}

impl ReplyControl {
    pub fn is_empty(&self) -> bool {
        self.location.as_deref().is_none_or(|s| s.trim().is_empty())
            && self
                .time_desc
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
            && !self.up_reply
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplyUpAction {
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "is_false"
    )]
    pub like: bool,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "is_false"
    )]
    pub reply: bool,
}

impl ReplyUpAction {
    pub fn is_empty(&self) -> bool {
        !self.like && !self.reply
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplyItem {
    #[serde(default)]
    pub rpid: i64,
    #[serde(default)]
    pub oid: i64,
    #[serde(default)]
    pub mid: i64,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "is_zero_i64"
    )]
    pub root: i64,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "is_zero_i64"
    )]
    pub parent: i64,
    #[serde(default)]
    pub member: Option<ReplyMember>,
    #[serde(default)]
    pub content: ReplyContent,
    #[serde(default)]
    pub like: u64,
    #[serde(default)]
    pub rcount: u64,
    #[serde(default)]
    pub action: u32,
    #[serde(default)]
    pub ctime: i64,
    #[serde(default, skip_serializing_if = "option_reply_control_is_empty")]
    pub reply_control: Option<ReplyControl>,
    #[serde(default, skip_serializing_if = "option_up_action_is_empty")]
    pub up_action: Option<ReplyUpAction>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub replies: Vec<ReplyItem>,
}

impl ReplyItem {
    pub fn author_name(&self) -> &str {
        self.member
            .as_ref()
            .map(|m| m.uname.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("匿名用户")
    }

    pub fn author_level(&self) -> Option<u32> {
        self.member
            .as_ref()
            .map(|m| m.level_info.current_level)
            .filter(|&lv| lv > 0)
    }

    pub fn location_short(&self) -> Option<&str> {
        let loc = self.reply_control.as_ref()?.location.as_deref()?;
        let trimmed = loc
            .trim()
            .strip_prefix("IP属地：")
            .or_else(|| loc.trim().strip_prefix("IP属地:"))
            .unwrap_or(loc.trim())
            .trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    pub fn is_up(&self, upper_mid: Option<i64>) -> bool {
        upper_mid.is_some_and(|u| u > 0 && self.mid == u)
    }

    pub fn is_up_liked(&self) -> bool {
        self.up_action.as_ref().is_some_and(|ua| ua.like)
    }

    pub fn is_up_replied(&self) -> bool {
        self.up_action.as_ref().is_some_and(|ua| ua.reply)
            || self.reply_control.as_ref().is_some_and(|rc| rc.up_reply)
    }

    pub fn is_liked(&self) -> bool {
        self.action == 1
    }

    pub fn clean_message(&self) -> String {
        crate::utils::string::unescape_html_entities(self.content.message.trim())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommentUpper {
    #[serde(default)]
    pub mid: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Box<ReplyItem>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommentTop {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper: Option<Box<ReplyItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin: Option<Box<ReplyItem>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommentPageData {
    #[serde(default)]
    pub page: CommentPageInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper: Option<CommentUpper>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<CommentTop>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub replies: Vec<ReplyItem>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub hots: Vec<ReplyItem>,
    #[serde(skip)]
    pub raw: Value,
}

impl CommentPageData {
    pub fn upper_mid(&self) -> Option<i64> {
        self.upper.as_ref().map(|u| u.mid).filter(|&m| m > 0)
    }

    pub fn pinned_reply(&self) -> Option<&ReplyItem> {
        self.upper
            .as_ref()
            .and_then(|u| u.top.as_deref())
            .or_else(|| self.top.as_ref().and_then(|t| t.upper.as_deref()))
            .or_else(|| self.top.as_ref().and_then(|t| t.admin.as_deref()))
            .filter(|r| r.rpid > 0)
    }
}

fn map_comment_api_error(code: i64, msg: &str, action: &str) -> BiliLiveError {
    match code {
        -101 | -111 | -403 => BiliLiveError::Auth(format!("{action}失败 ({code}): {msg}")),
        12002 | 12052 => BiliLiveError::Api(format!(
            "{action}失败 ({code}): 评论区已关闭或禁止操作 ({msg})"
        )),
        12006 => BiliLiveError::Api(format!(
            "{action}失败 (12006): 当前视频下不存在该评论 RPID（请先运行 `bit video comments <VIDEO>` 查看真实的评论 RPID）"
        )),
        _ => BiliLiveError::Api(format!("{action}失败 ({code}): {msg}")),
    }
}

/// 获取视频评论列表 (`GET /x/v2/reply`)
pub fn get_comments(
    aid: i64,
    page: u32,
    sort: &str,
    cookies: Option<&Cookies>,
) -> Result<CommentPageData> {
    let url = format!(
        "{}/x/v2/reply?type=1&oid={}&pn={}&ps=20&sort={}",
        crate::api::client::api_base_url(),
        aid,
        page,
        sort
    );
    let referer = format!("https://www.bilibili.com/video/av{aid}");
    let mut req = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", &referer);

    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            req = req.with_header("Cookie", header);
        }
    }

    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "获取评论失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: Value = serde_json::from_str(resp.as_str()?)?;
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["message"].as_str().unwrap_or("未知错误");
        return Err(map_comment_api_error(code, msg, "获取评论"));
    }

    if json["data"].is_null() {
        return Ok(CommentPageData::default());
    }
    let raw_data = json["data"].clone();
    let mut data: CommentPageData = serde_json::from_value(raw_data.clone())?;
    data.raw = raw_data;
    Ok(data)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubReplyPageData {
    #[serde(default)]
    pub page: CommentPageInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper: Option<CommentUpper>,
    #[serde(default)]
    pub root: Option<ReplyItem>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub replies: Vec<ReplyItem>,
    #[serde(skip)]
    pub raw: Value,
}

impl SubReplyPageData {
    pub fn upper_mid(&self) -> Option<i64> {
        self.upper.as_ref().map(|u| u.mid).filter(|&m| m > 0)
    }
}

/// 获取指定根评论下的楼中楼二级回复列表 (`GET /x/v2/reply/reply`)
pub fn get_sub_replies(
    aid: i64,
    root_rpid: i64,
    page: u32,
    cookies: Option<&Cookies>,
) -> Result<SubReplyPageData> {
    let url = format!(
        "{}/x/v2/reply/reply?type=1&oid={}&root={}&pn={}&ps=20",
        crate::api::client::api_base_url(),
        aid,
        root_rpid,
        page
    );
    let referer = format!("https://www.bilibili.com/video/av{aid}");
    let mut req = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", &referer);

    if let Some(c) = cookies {
        let header = c.cookie_header();
        if !header.is_empty() {
            req = req.with_header("Cookie", header);
        }
    }

    let resp = req.send()?;
    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "获取楼中楼回复失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: Value = serde_json::from_str(resp.as_str()?)?;
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["message"].as_str().unwrap_or("未知错误");
        return Err(map_comment_api_error(code, msg, "获取楼中楼回复"));
    }

    if json["data"].is_null() {
        return Ok(SubReplyPageData::default());
    }
    let raw_data = json["data"].clone();
    let mut data: SubReplyPageData = serde_json::from_value(raw_data.clone())?;
    data.raw = raw_data;
    Ok(data)
}

/// 发布评论或回复 (`POST /x/v2/reply/add`)
pub fn post_comment(
    cookies: &Cookies,
    aid: i64,
    message: &str,
    root: Option<i64>,
    parent: Option<i64>,
) -> Result<Value> {
    if message.trim().is_empty() {
        return Err(BiliLiveError::Input("评论内容不能为空".to_string()));
    }

    let aid_str = aid.to_string();
    let mut params: Vec<(&str, String)> = vec![
        ("type", "1".to_string()),
        ("oid", aid_str),
        ("message", message.to_string()),
        ("plat", "1".to_string()),
        ("csrf", cookies.bili_jct.clone()),
    ];
    if let Some(r) = root {
        params.push(("root", r.to_string()));
    }
    if let Some(p) = parent {
        params.push(("parent", p.to_string()));
    }

    let body =
        serde_urlencoded::to_string(&params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/av{aid}");
    let url = format!("{}/x/v2/reply/add", crate::api::client::api_base_url());
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "发布评论失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: Value = serde_json::from_str(resp.as_str()?)?;
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["message"].as_str().unwrap_or("未知错误");
        return Err(map_comment_api_error(code, msg, "发布评论"));
    }

    Ok(json["data"].clone())
}

/// 点赞或取消点赞评论 (`POST /x/v2/reply/action`)
pub fn like_comment(cookies: &Cookies, aid: i64, rpid: i64, cancel: bool) -> Result<()> {
    if rpid <= 0 {
        return Err(BiliLiveError::Input("评论 rpid 必须大于 0".to_string()));
    }

    let aid_str = aid.to_string();
    let rpid_str = rpid.to_string();
    let action_str = if cancel { "0" } else { "1" };
    let params = [
        ("type", "1"),
        ("oid", aid_str.as_str()),
        ("rpid", rpid_str.as_str()),
        ("action", action_str),
        ("csrf", cookies.bili_jct.as_str()),
    ];

    let body =
        serde_urlencoded::to_string(params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/av{aid}");
    let url = format!("{}/x/v2/reply/action", crate::api::client::api_base_url());
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "评论点赞操作失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: Value = serde_json::from_str(resp.as_str()?)?;
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["message"].as_str().unwrap_or("未知错误");
        return Err(map_comment_api_error(code, msg, "评论点赞操作"));
    }

    Ok(())
}

/// 删除评论 (`POST /x/v2/reply/del`)
pub fn delete_comment(cookies: &Cookies, aid: i64, rpid: i64) -> Result<()> {
    if rpid <= 0 {
        return Err(BiliLiveError::Input("评论 rpid 必须大于 0".to_string()));
    }

    let aid_str = aid.to_string();
    let rpid_str = rpid.to_string();
    let params = [
        ("type", "1"),
        ("oid", aid_str.as_str()),
        ("rpid", rpid_str.as_str()),
        ("csrf", cookies.bili_jct.as_str()),
    ];

    let body =
        serde_urlencoded::to_string(params).map_err(|e| BiliLiveError::Parse(e.to_string()))?;

    let referer = format!("https://www.bilibili.com/video/av{aid}");
    let url = format!("{}/x/v2/reply/del", crate::api::client::api_base_url());
    let resp = crate::api::client::post(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Referer", &referer)
        .with_header("Cookie", cookies.cookie_header())
        .with_body(body)
        .send()?;

    if resp.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "删除评论失败: HTTP {} {}",
            resp.status_code, resp.reason_phrase
        )));
    }
    let json: Value = serde_json::from_str(resp.as_str()?)?;
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["message"].as_str().unwrap_or("未知错误");
        return Err(map_comment_api_error(code, msg, "删除评论"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cookies() -> Cookies {
        Cookies {
            sessdata: "sess".to_string(),
            bili_jct: "csrf_tok".to_string(),
            dede_user_id: "123".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_comment_null_arrays_and_null_member_deserialize() {
        let raw = serde_json::json!({
            "page": { "num": 1, "size": 20, "count": 1, "acount": 1 },
            "replies": [
                {
                    "rpid": 1001,
                    "oid": 79677524,
                    "mid": 42,
                    "rcount": 0,
                    "like": 5,
                    "action": 0,
                    "ctime": 1700000000,
                    "member": null,
                    "content": { "message": "hello" },
                    "replies": null
                }
            ],
            "hots": null
        });
        let page: CommentPageData = serde_json::from_value(raw).unwrap();
        assert_eq!(page.replies.len(), 1);
        assert!(page.hots.is_empty());
        assert!(page.replies[0].member.is_none());
        assert!(page.replies[0].replies.is_empty());

        let raw_null_replies = serde_json::json!({
            "page": { "num": 1, "size": 20, "count": 0, "acount": 0 },
            "replies": null,
            "hots": null
        });
        let empty_page: CommentPageData = serde_json::from_value(raw_null_replies).unwrap();
        assert!(empty_page.replies.is_empty());
        let reserialized = serde_json::to_value(&empty_page).unwrap();
        assert_eq!(reserialized["replies"], serde_json::json!([]));
    }

    #[test]
    fn test_comment_error_mapping() {
        assert!(matches!(
            map_comment_api_error(-101, "未登录", "发布评论"),
            BiliLiveError::Auth(_)
        ));
        assert!(matches!(
            map_comment_api_error(-111, "csrf", "删除评论"),
            BiliLiveError::Auth(_)
        ));
        assert!(matches!(
            map_comment_api_error(-403, "denied", "删除评论"),
            BiliLiveError::Auth(_)
        ));
        let err = map_comment_api_error(12002, "closed", "发布评论");
        assert!(matches!(err, BiliLiveError::Api(_)));
        assert!(err.to_string().contains("12002"));
    }

    #[test]
    fn test_comment_http_roundtrip_mock() {
        use std::io::{BufRead, BufReader, Read as _, Write};
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
            for idx in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':')
                        && k.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body_buf = vec![0u8; content_len];
                if content_len > 0 {
                    reader.read_exact(&mut body_buf).unwrap();
                }
                let body_str = String::from_utf8_lossy(&body_buf);

                let resp = match idx {
                    0 => {
                        assert!(
                            first.starts_with(
                                "GET /x/v2/reply?type=1&oid=79677524&pn=2&ps=20&sort=2"
                            )
                        );
                        r#"{"code":0,"data":{"page":{"num":2,"size":20,"count":0,"acount":0},"replies":null,"hots":null}}"#
                    }
                    1 => {
                        assert!(first.starts_with("POST /x/v2/reply/add"));
                        assert!(body_str.contains("oid=79677524"));
                        assert!(body_str.contains("csrf=csrf_tok"));
                        assert!(body_str.contains("plat=1"));
                        r#"{"code":0,"data":{"rpid":999,"rpid_str":"999"}}"#
                    }
                    2 => {
                        assert!(first.starts_with("POST /x/v2/reply/action"));
                        assert!(body_str.contains("rpid=999"));
                        assert!(body_str.contains("action=1"));
                        r#"{"code":0,"message":"0"}"#
                    }
                    _ => {
                        assert!(first.starts_with("POST /x/v2/reply/del"));
                        assert!(body_str.contains("rpid=999"));
                        r#"{"code":-111,"message":"csrf 校验失败"}"#
                    }
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp.len(),
                    resp
                )
                .unwrap();
            }
        });

        unsafe { std::env::set_var("BT_API_BASE_URL", &base) };
        let cookies = test_cookies();

        let list = get_comments(79677524, 2, "2", Some(&cookies)).unwrap();
        assert!(list.replies.is_empty());

        let posted = post_comment(&cookies, 79677524, "测试评论", None, None).unwrap();
        assert_eq!(posted["rpid"], 999);

        like_comment(&cookies, 79677524, 999, false).unwrap();

        let err = delete_comment(&cookies, 79677524, 999).unwrap_err();
        assert!(matches!(err, BiliLiveError::Auth(_)));

        // Validation guards before HTTP
        assert!(matches!(
            post_comment(&cookies, 79677524, "   ", None, None).unwrap_err(),
            BiliLiveError::Input(_)
        ));
        assert!(matches!(
            like_comment(&cookies, 79677524, 0, false).unwrap_err(),
            BiliLiveError::Input(_)
        ));
        assert!(matches!(
            delete_comment(&cookies, 79677524, -5).unwrap_err(),
            BiliLiveError::Input(_)
        ));

        handle.join().unwrap();
    }

    #[test]
    fn test_clean_json_strips_widget_bloat_and_preserves_raw() {
        let raw_payload = serde_json::json!({
            "page": { "num": 1, "size": 20, "count": 1, "acount": 2 },
            "upper": {
                "mid": 400880294,
                "top": {
                    "rpid": 9001,
                    "oid": 79677524,
                    "mid": 400880294,
                    "rcount": 0,
                    "like": 10,
                    "action": 0,
                    "ctime": 1700000000,
                    "member": {
                        "mid": "400880294",
                        "uname": "QwerProg",
                        "level_info": { "current_level": 6 },
                        "avatar_item": { "fallback_layers": { "layers": [] } },
                        "user_sailing": { "cardbg": {} },
                        "vip": { "label": {} }
                    },
                    "content": {
                        "message": "置顶公告 &amp; 说明",
                        "emote": { "[doge]": {} },
                        "pictures": [{ "img_src": "https://example.com/a.jpg", "img_width": 800, "img_height": 600 }]
                    },
                    "reply_control": { "location": "IP属地：江苏", "time_desc": "1小时前发布", "up_reply": false },
                    "up_action": { "like": false, "reply": false },
                    "track_info": "raw_track"
                }
            },
            "replies": [
                {
                    "rpid": 9002,
                    "oid": 79677524,
                    "mid": 123,
                    "rcount": 1,
                    "like": 3,
                    "action": 1,
                    "ctime": 1700000100,
                    "member": {
                        "mid": "123",
                        "uname": "路人甲",
                        "level_info": { "current_level": 5 },
                        "avatar_item": { "layers": [1, 2, 3] },
                        "user_sailing": { "pendant": {} },
                        "vip": { "type": 2 }
                    },
                    "content": {
                        "message": "支持",
                        "emote": { "[笑哭]": {} }
                    },
                    "reply_control": {},
                    "up_action": { "like": true, "reply": true }
                }
            ],
            "hots": null,
            "control": { "answer_guide_text": "noise" },
            "config": { "showadmin": 1 }
        });

        let mut page: CommentPageData = serde_json::from_value(raw_payload.clone()).unwrap();
        page.raw = raw_payload;

        let serialized = serde_json::to_value(&page).unwrap();
        let s_str = serde_json::to_string(&serialized).unwrap();
        for bloat in [
            "avatar_item",
            "fallback_layers",
            "user_sailing",
            "vip",
            "emote",
            "track_info",
            "answer_guide_text",
            "showadmin",
        ] {
            assert!(
                !s_str.contains(bloat),
                "unexpected bloat key {bloat}: {s_str}"
            );
        }
        assert!(serialized.get("control").is_none());
        assert!(serialized.get("config").is_none());

        // Empty up_action & empty reply_control omitted cleanly
        let top_obj = &serialized["upper"]["top"];
        assert!(top_obj.get("up_action").is_none());
        assert_eq!(
            top_obj["content"]["pictures"][0]["img_src"],
            "https://example.com/a.jpg"
        );

        let reply_obj = &serialized["replies"][0];
        assert!(reply_obj.get("reply_control").is_none());
        assert_eq!(reply_obj["up_action"]["like"], true);
        assert_eq!(reply_obj["up_action"]["reply"], true);

        // Raw preserves untouched payload
        assert!(page.raw["upper"]["top"]["member"]["avatar_item"].is_object());
        assert!(page.raw["replies"][0]["content"]["emote"].is_object());

        // Helper methods
        assert_eq!(page.upper_mid(), Some(400880294));
        let pinned = page.pinned_reply().unwrap();
        assert_eq!(pinned.rpid, 9001);
        assert_eq!(pinned.author_name(), "QwerProg");
        assert_eq!(pinned.author_level(), Some(6));
        assert_eq!(pinned.location_short(), Some("江苏"));
        assert!(pinned.is_up(page.upper_mid()));
        assert!(!pinned.is_up_liked());
        assert_eq!(pinned.clean_message(), "置顶公告 & 说明");

        let r2 = &page.replies[0];
        assert!(!r2.is_up(page.upper_mid()));
        assert!(r2.is_liked());
        assert!(r2.is_up_liked());
        assert!(r2.is_up_replied());
        assert_eq!(r2.location_short(), None);
    }
}
