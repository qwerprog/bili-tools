use crate::api::client::DEFAULT_USER_AGENT;
use crate::api::passport::get_roomid;
use crate::error::{BiliLiveError, Result};
use crate::user_success;
use crate::utils::paths::{data_file, write_private};
use serde::{Deserialize, Serialize};

/// 完整的 Bilibili 登录凭据与设备指纹结构
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Cookies {
    #[serde(default, alias = "DedeUserID")]
    pub dede_user_id: String,

    #[serde(default, alias = "DedeUserID__ckMd5")]
    pub dede_user_id_ck_md5: String,

    #[serde(default, alias = "SESSDATA")]
    pub sessdata: String,

    #[serde(default, alias = "csrf_token", alias = "bili_jct")]
    pub bili_jct: String,

    #[serde(default, alias = "buvid3")]
    pub buvid3: String,

    #[serde(default, alias = "buvid4")]
    pub buvid4: String,

    #[serde(default)]
    pub refresh_token: String,

    #[serde(default)]
    pub room_id: i32,

    #[serde(default)]
    pub live_key: Option<u64>,
}

impl Cookies {
    /// 获取 CSRF token（即 bili_jct）
    #[allow(dead_code)]
    pub fn csrf(&self) -> &str {
        &self.bili_jct
    }

    /// 兼容旧方法名
    #[allow(dead_code)]
    pub fn csrf_token(&self) -> &str {
        &self.bili_jct
    }

    /// 是否已包含有效的 SESSDATA
    #[allow(dead_code)]
    pub fn is_logged_in(&self) -> bool {
        !self.sessdata.is_empty()
    }

    /// 构建用于 HTTP Request 的完整 Cookie 请求头字符串
    pub fn cookie_header(&self) -> String {
        let mut parts = Vec::new();
        if !self.sessdata.is_empty() {
            parts.push(format!("SESSDATA={}", self.sessdata));
        }
        if !self.bili_jct.is_empty() {
            parts.push(format!("bili_jct={}", self.bili_jct));
        }
        if !self.dede_user_id.is_empty() {
            parts.push(format!("DedeUserID={}", self.dede_user_id));
        }
        if !self.dede_user_id_ck_md5.is_empty() {
            parts.push(format!("DedeUserID__ckMd5={}", self.dede_user_id_ck_md5));
        }
        if !self.buvid3.is_empty() {
            parts.push(format!("buvid3={}", self.buvid3));
        }
        if !self.buvid4.is_empty() {
            parts.push(format!("buvid4={}", self.buvid4));
        }
        parts.join("; ")
    }

    /// 保存到配置文件，以 0600 权限写入
    pub fn save(&self) -> Result<()> {
        let path = data_file("cookies.json")?;
        let cookies_json = serde_json::to_string_pretty(self)?;
        write_private(&path, cookies_json.as_bytes())?;
        Ok(())
    }

    /// 确保拥有 buvid3 和 buvid4；若缺失则自动从接口拉取并更新保存
    pub fn ensure_buvid(&mut self) -> Result<()> {
        if (self.buvid3.is_empty() || self.buvid4.is_empty())
            && let Ok((b3, b4)) = fetch_buvid()
        {
            if self.buvid3.is_empty() {
                self.buvid3 = b3;
            }
            if self.buvid4.is_empty() {
                self.buvid4 = b4;
            }
            let _ = self.save();
        }
        Ok(())
    }
}

/// 从 B站官方接口获取设备指纹 buvid3 和 buvid4
pub fn fetch_buvid() -> Result<(String, String)> {
    let response = crate::api::client::get("https://api.bilibili.com/x/frontend/finger/spi")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let json: serde_json::Value = serde_json::from_str(response.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取 buvid 失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let b3 = json["data"]["b_3"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let b4 = json["data"]["b_4"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    Ok((b3, b4))
}

/// 兼容旧版快速凭证保存函数
#[allow(dead_code)]
pub fn save_cookies_from_credentials(sessdata: &str, csrf_token: &str) -> Result<()> {
    let mut cookies = read_cookies().unwrap_or_default();
    cookies.sessdata = sessdata.to_string();
    cookies.bili_jct = csrf_token.to_string();

    // 尝试拉取直播间 ID
    if let Ok(rid) = get_roomid(sessdata) {
        cookies.room_id = rid;
    }

    // 尝试拉取设备指纹
    let _ = cookies.ensure_buvid();
    cookies.save()?;
    user_success!("Cookies 保存成功");
    Ok(())
}

/// 保存完整的 Cookies 对象
#[allow(dead_code)]
pub fn save_full_cookies(cookies: &Cookies) -> Result<()> {
    cookies.save()?;
    user_success!("Cookies 保存成功");
    Ok(())
}

pub fn update_live_key(live_key: Option<u64>) -> Result<()> {
    let mut cookies = read_cookies()?;
    cookies.live_key = live_key;
    cookies.save()?;
    Ok(())
}

pub fn delete_cookies() -> Result<()> {
    let path = data_file("cookies.json")?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn read_cookies() -> Result<Cookies> {
    let path = data_file("cookies.json")?;
    let cookies_str = std::fs::read_to_string(&path).map_err(BiliLiveError::Io)?;
    let cookies: Cookies = serde_json::from_str(&cookies_str).map_err(BiliLiveError::Json)?;
    Ok(cookies)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backward_compatibility_deserialization() {
        let old_json = r#"{
            "room_id": 31686846,
            "sessdata": "sess_12345",
            "csrf_token": "csrf_67890",
            "live_key": 708884257747206334
        }"#;

        let cookies: Cookies = serde_json::from_str(old_json).unwrap();
        assert_eq!(cookies.room_id, 31686846);
        assert_eq!(cookies.sessdata, "sess_12345");
        assert_eq!(cookies.bili_jct, "csrf_67890");
        assert_eq!(cookies.csrf(), "csrf_67890");
        assert_eq!(cookies.csrf_token(), "csrf_67890");
        assert_eq!(cookies.live_key, Some(708884257747206334));
        assert!(cookies.dede_user_id.is_empty());
        assert!(cookies.buvid3.is_empty());
    }

    #[test]
    fn test_full_cookie_header_generation() {
        let cookies = Cookies {
            dede_user_id: "123456".to_string(),
            dede_user_id_ck_md5: "md5hash".to_string(),
            sessdata: "mysession".to_string(),
            bili_jct: "mycsrf".to_string(),
            buvid3: "b3_sample".to_string(),
            buvid4: "b4_sample".to_string(),
            refresh_token: "ref_tok".to_string(),
            room_id: 999,
            live_key: None,
        };

        let header = cookies.cookie_header();
        assert!(header.contains("SESSDATA=mysession"));
        assert!(header.contains("bili_jct=mycsrf"));
        assert!(header.contains("DedeUserID=123456"));
        assert!(header.contains("DedeUserID__ckMd5=md5hash"));
        assert!(header.contains("buvid3=b3_sample"));
        assert!(header.contains("buvid4=b4_sample"));
    }

    #[test]
    fn test_cookie_file_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cookies.json");

        let cookies = Cookies {
            sessdata: "test_session".to_string(),
            bili_jct: "test_csrf".to_string(),
            ..Default::default()
        };

        let content = serde_json::to_string_pretty(&cookies).unwrap();
        write_private(&path, content.as_bytes()).unwrap();

        let loaded: Cookies = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.sessdata, "test_session");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
    }
}
