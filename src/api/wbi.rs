use crate::api::client::DEFAULT_USER_AGENT;
use crate::error::{BiliLiveError, Result};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49,
    33, 9, 42, 19, 29, 28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40,
    61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11,
    36, 20, 34, 44, 52,
];

struct CachedKeys {
    img_key: String,
    sub_key: String,
    fetched_at: Instant,
}

static WBI_CACHE: LazyLock<Mutex<Option<CachedKeys>>> = LazyLock::new(|| Mutex::new(None));

/// 对 imgKey 和 subKey 进行字符顺序打乱重排，截取前 32 位生成 mixin_key
pub fn get_mixin_key(orig: &[u8]) -> String {
    MIXIN_KEY_ENC_TAB
        .iter()
        .take(32)
        .filter_map(|&i| orig.get(i).copied().map(|b| b as char))
        .collect()
}

/// 按照 WBI 规范进行 URL 编码：
/// 1. 保留字母数字及 - _ . ~
/// 2. 过滤掉 "!'()*" 字符
/// 3. 其他字符进行大写字母的百分号编码，空格为 %20
pub fn encode_component(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            result.push(c);
        } else if "!'()*".contains(c) {
            // 过滤特殊字符
            continue;
        } else {
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            for b in bytes {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

/// 从 URL 中提取纯文件名（不含扩展名），用于从 BFS 图片 URL 获取 Token key
pub fn take_filename(url: &str) -> Option<String> {
    let clean_url = url.split('?').next()?.split('#').next()?;
    let filename = clean_url.rsplit_once('/')?.1;
    let name = filename.split_once('.').map(|(n, _)| n).unwrap_or(filename);
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// 使用指定时间戳对参数进行 WBI 签名
pub fn encode_wbi_with_timestamp(
    params: &[(&str, &str)],
    img_key: &str,
    sub_key: &str,
    timestamp: u64,
) -> String {
    let raw = format!("{img_key}{sub_key}");
    let mixin_key = get_mixin_key(raw.as_bytes());

    let ts_str = timestamp.to_string();
    let mut sorted: Vec<(&str, &str)> = params
        .iter()
        .copied()
        .filter(|(k, _)| *k != "wts" && *k != "w_rid")
        .collect();
    sorted.push(("wts", &ts_str));
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    let query = sorted
        .iter()
        .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v)))
        .collect::<Vec<_>>()
        .join("&");

    let sign_payload = format!("{query}{mixin_key}");
    let digest = md5::compute(sign_payload);
    let w_rid = format!("{:x}", digest);

    format!("{query}&w_rid={w_rid}")
}

/// 为请求参数进行 WBI 签名（使用当前时间戳）
pub fn encode_wbi(params: &[(&str, &str)], img_key: &str, sub_key: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    encode_wbi_with_timestamp(params, img_key, sub_key, now)
}

/// 从 Bilibili 导航接口获取最新的 img_key 和 sub_key（带内存缓存 1 小时）
pub fn get_wbi_keys(sessdata: Option<&str>) -> Result<(String, String)> {
    if let Ok(guard) = WBI_CACHE.lock()
        && let Some(ref cached) = *guard
        && cached.fetched_at.elapsed() < Duration::from_secs(3600)
    {
        return Ok((cached.img_key.clone(), cached.sub_key.clone()));
    }

    let mut req = crate::api::client::get("https://api.bilibili.com/x/web-interface/nav")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/");

    if let Some(sess) = sessdata
        && !sess.is_empty()
    {
        req = req.with_header("Cookie", format!("SESSDATA={sess}"));
    }

    let resp = req.send()?;
    let text = resp.as_str()?;
    let json: serde_json::Value = serde_json::from_str(text)?;

    let img_url = json["data"]["wbi_img"]["img_url"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("缺少 wbi_img.img_url".to_string()))?;
    let sub_url = json["data"]["wbi_img"]["sub_url"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("缺少 wbi_img.sub_url".to_string()))?;

    let img_key = take_filename(img_url)
        .ok_or_else(|| BiliLiveError::Parse(format!("无法解析 img_key: {img_url}")))?;
    let sub_key = take_filename(sub_url)
        .ok_or_else(|| BiliLiveError::Parse(format!("无法解析 sub_key: {sub_url}")))?;

    if let Ok(mut guard) = WBI_CACHE.lock() {
        *guard = Some(CachedKeys {
            img_key: img_key.clone(),
            sub_key: sub_key.clone(),
            fetched_at: Instant::now(),
        });
    }

    Ok((img_key, sub_key))
}

/// 便捷方法：获取 keys 并完成参数签名
pub fn sign_params(params: &[(&str, &str)], sessdata: Option<&str>) -> Result<String> {
    let (img_key, sub_key) = get_wbi_keys(sessdata)?;
    Ok(encode_wbi(params, &img_key, &sub_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_take_filename() {
        assert_eq!(
            take_filename("https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png"),
            Some("7cd084941338484aae1ad9425b84077c".to_string())
        );
        assert_eq!(
            take_filename("https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"),
            Some("4932caff0ff746eab6f01bf08b70ac45".to_string())
        );
        assert_eq!(
            take_filename("https://i0.hdslb.com/bfs/wbi/mykey.png?foo=bar.baz#fragment"),
            Some("mykey".to_string())
        );
        assert_eq!(
            take_filename("https://i0.hdslb.com/bfs/wbi/rawkey"),
            Some("rawkey".to_string())
        );
    }

    #[test]
    fn test_get_mixin_key() {
        let concat_key = "7cd084941338484aae1ad9425b84077c4932caff0ff746eab6f01bf08b70ac45";
        assert_eq!(
            get_mixin_key(concat_key.as_bytes()),
            "ea1db124af3c7062474693fa704f4ff8"
        );
    }

    #[test]
    fn test_encode_wbi_official_vector() {
        let params = [
            ("foo", "114"),
            ("bar", "514"),
            ("zab", "1919810"),
        ];
        let img_key = "7cd084941338484aae1ad9425b84077c";
        let sub_key = "4932caff0ff746eab6f01bf08b70ac45";
        let timestamp = 1702204169;

        let query = encode_wbi_with_timestamp(&params, img_key, sub_key, timestamp);
        assert_eq!(
            query,
            "bar=514&foo=114&wts=1702204169&zab=1919810&w_rid=8f6f2b5b3d485fe1886cec6a0be8c5d4"
        );
    }

    #[test]
    fn test_encode_wbi_special_chars_and_chinese() {
        let params = [
            ("foo", "114"),
            ("bar", "514"),
            ("hello", "世 界!*()"),
        ];
        let img_key = "7cd084941338484aae1ad9425b84077c";
        let sub_key = "4932caff0ff746eab6f01bf08b70ac45";
        let timestamp = 1744823207;

        let query = encode_wbi_with_timestamp(&params, img_key, sub_key, timestamp);
        // Note: "!*()" are stripped, leaving "世 界" which encodes to "%E4%B8%96%20%E7%95%8C"
        assert_eq!(
            query,
            "bar=514&foo=114&hello=%E4%B8%96%20%E7%95%8C&wts=1744823207&w_rid=93acf59d85f74453e40cea00056c3daf"
        );
    }

    #[test]
    fn test_encode_wbi_dedup_wts() {
        let params = [
            ("foo", "114"),
            ("wts", "old_ts"),
            ("w_rid", "old_rid"),
            ("bar", "514"),
            ("zab", "1919810"),
        ];
        let img_key = "7cd084941338484aae1ad9425b84077c";
        let sub_key = "4932caff0ff746eab6f01bf08b70ac45";
        let timestamp = 1702204169;

        let query = encode_wbi_with_timestamp(&params, img_key, sub_key, timestamp);
        assert_eq!(
            query,
            "bar=514&foo=114&wts=1702204169&zab=1919810&w_rid=8f6f2b5b3d485fe1886cec6a0be8c5d4"
        );
    }
}
