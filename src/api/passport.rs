use crate::api::client::DEFAULT_USER_AGENT;
use crate::error::{BiliLiveError, Result};
use base64::{Engine as _, engine::general_purpose};
use rand::rngs::OsRng;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

// B站 TV 端 API 密钥，用于生成短链接二维码
const TV_APPKEY: &str = "4409e2ce8ffd12b8";
const TV_APPSEC: &str = "59b43e04ad6965f34319062b478f83dd";
// B站 Android 端 API 密钥，用于账号密码/短信登录
const ANDROID_APPKEY: &str = "783bbb7264451d82";
const ANDROID_APPSEC: &str = "2653583c8873dea268ab9386918b1d65";

// B站 API 通用签名方式：query_string + appsec 做 MD5
fn md5_sign(query: &str, appsec: &str) -> String {
    let digest = md5::compute(format!("{}{}", query, appsec));
    format!("{:x}", digest)
}

/// 对参数列表排序、拼接后做 MD5 签名
fn sign_params(params: &[(&str, &str)], appsec: &str) -> String {
    let mut sorted = params.to_vec();
    sorted.sort_by_key(|(k, _)| *k);
    let query = sorted
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    md5_sign(&query, appsec)
}

/// 构建带签名的 form body（TV 登录）
fn signed_form(params: Vec<(&str, String)>, appsec: &str) -> String {
    let mut params = params;
    params.sort_by_key(|(k, _)| *k);
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    let sig_params: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let sig = sign_params(&sig_params, appsec);
    format!("{}&sign={}", query, sig)
}

fn ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string()
}

// 从登录响应 JSON 中提取凭证
fn extract_credentials(json: &Value) -> Result<(String, String, String, String, String)> {
    let cookies = json["data"]["cookie_info"]["cookies"]
        .as_array()
        .ok_or_else(|| BiliLiveError::Parse("缺少 cookie_info".to_string()))?;

    let mut sessdata = String::new();
    let mut csrf_token = String::new();
    let mut dede_user_id = String::new();
    let mut dede_user_id_ck_md5 = String::new();
    for cookie in cookies {
        match cookie["name"].as_str() {
            Some("SESSDATA") => sessdata = cookie["value"].as_str().unwrap_or("").to_string(),
            Some("bili_jct") => csrf_token = cookie["value"].as_str().unwrap_or("").to_string(),
            Some("DedeUserID") => dede_user_id = cookie["value"].as_str().unwrap_or("").to_string(),
            Some("DedeUserID__ckMd5") => {
                dede_user_id_ck_md5 = cookie["value"].as_str().unwrap_or("").to_string()
            }
            _ => {}
        }
    }

    if sessdata.is_empty() || csrf_token.is_empty() {
        return Err(BiliLiveError::Parse("无法提取登录凭证".to_string()));
    }
    let refresh_token = json["data"]["refresh_token"]
        .as_str()
        .or_else(|| json["data"]["token_info"]["refresh_token"].as_str())
        .unwrap_or("")
        .to_string();

    Ok((
        sessdata,
        csrf_token,
        dede_user_id,
        dede_user_id_ck_md5,
        refresh_token,
    ))
}

// ── TV 扫码登录（二维码更短，相比 Web API 更易扫描）────────────────

// 生成 TV 端二维码，返回短链接和 auth_code

pub struct QRCodeData {
    /// 用于生成二维码的短链接
    pub url: String,
    /// 用于轮询状态的 auth_code
    pub auth_code: String,
}

pub fn generate_qr_code() -> Result<QRCodeData> {
    let body = signed_form(
        vec![
            ("appkey", TV_APPKEY.to_string()),
            ("local_id", "0".to_string()),
            ("ts", ts()),
        ],
        TV_APPSEC,
    );

    let auth_code_url = format!(
        "{}/x/passport-tv-login/qrcode/auth_code",
        crate::api::client::passport_base_url()
    );
    let response = crate::api::client::post(&auth_code_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = crate::api::client::parse_json_response(&response, "获取二维码失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "获取二维码失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let url = json["data"]["url"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("缺少 url".to_string()))?
        .to_string();
    let auth_code = json["data"]["auth_code"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("缺少 auth_code".to_string()))?
        .to_string();

    Ok(QRCodeData { url, auth_code })
}

// 轮询 TV 二维码状态
// TV: 86039 = 尚未确认，86038 = 过期；Web 的状态码位于 data.code。
pub enum PollStatus {
    /// 等待扫码或确认
    Waiting,
    /// 已扫码，等待确认
    Scanned,
    /// 登录成功，携带凭证
    Success {
        sessdata: String,
        csrf_token: String,
        dede_user_id: String,
        dede_user_id_ck_md5: String,
        refresh_token: String,
    },
}

pub fn poll_qr_status(auth_code: &str) -> Result<PollStatus> {
    let body = signed_form(
        vec![
            ("appkey", TV_APPKEY.to_string()),
            ("auth_code", auth_code.to_string()),
            ("local_id", "0".to_string()),
            ("ts", ts()),
        ],
        TV_APPSEC,
    );

    let poll_url = format!(
        "{}/x/passport-tv-login/qrcode/poll",
        crate::api::client::passport_base_url()
    );
    let response = crate::api::client::post(&poll_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = crate::api::client::parse_json_response(&response, "轮询二维码状态失败")?;

    parse_tv_poll(&json)
}

fn parse_tv_poll(json: &Value) -> Result<PollStatus> {
    match json["code"].as_i64() {
        Some(0) => {
            let (sessdata, csrf_token, dede_user_id, dede_user_id_ck_md5, refresh_token) =
                extract_credentials(json)?;
            Ok(PollStatus::Success {
                sessdata,
                csrf_token,
                dede_user_id,
                dede_user_id_ck_md5,
                refresh_token,
            })
        }
        Some(86039) => Ok(PollStatus::Waiting),
        Some(86090) => Ok(PollStatus::Scanned),
        Some(86038) => Err(BiliLiveError::Api("二维码已过期，请重新登录".to_string())),
        _ => Err(BiliLiveError::Api(format!(
            "二维码状态异常: {}",
            json["message"].as_str().unwrap_or("未知错误")
        ))),
    }
}

// Web 扫码优先从 Set-Cookie 提取凭据，兼容旧版 URL 返回格式。
pub struct WebQRCodeData {
    pub url: String,
    pub qrcode_key: String,
}

pub fn generate_web_qr_code() -> Result<WebQRCodeData> {
    let gen_url = format!(
        "{}/x/passport-login/web/qrcode/generate",
        crate::api::client::passport_base_url()
    );
    let response = crate::api::client::get(&gen_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&response, "Web 扫码请求失败")?;
    check_web_response(&json)?;
    let url = required_string(&json["data"], "url")?;
    let qrcode_key = required_string(&json["data"], "qrcode_key")?;
    Ok(WebQRCodeData { url, qrcode_key })
}

pub fn poll_web_qr_status(qrcode_key: &str) -> Result<PollStatus> {
    let poll_base = format!(
        "{}/x/passport-login/web/qrcode/poll",
        crate::api::client::passport_base_url()
    );
    let mut url = Url::parse(&poll_base).map_err(|e| BiliLiveError::Parse(e.to_string()))?;
    url.query_pairs_mut().append_pair("qrcode_key", qrcode_key);
    poll_web_qr_url(url.as_str())
}

fn poll_web_qr_url(url: &str) -> Result<PollStatus> {
    let mut response = ureq::get(url)
        .header("User-Agent", DEFAULT_USER_AGENT)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .max_redirects(0)
        .build()
        .call()?;
    let headers = response.headers().clone();
    let body = response.body_mut().read_to_string()?;
    let json: Value = serde_json::from_str(&body)?;
    parse_web_poll(&json, &headers)
}

fn check_web_response(json: &Value) -> Result<()> {
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "Web 扫码请求失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(())
}

fn required_string(data: &Value, field: &str) -> Result<String> {
    data[field]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BiliLiveError::Parse(format!("缺少 {field}")))
}

fn parse_web_poll(json: &Value, headers: &ureq::http::HeaderMap) -> Result<PollStatus> {
    check_web_response(json)?;
    let data = &json["data"];
    match data["code"].as_i64() {
        Some(86101) => Ok(PollStatus::Waiting),
        Some(86090) => Ok(PollStatus::Scanned),
        Some(86038) => Err(BiliLiveError::Api("二维码已过期，请重新登录".to_string())),
        Some(0) => {
            let url = data["url"].as_str().and_then(|url| Url::parse(url).ok());
            let credential = |name: &str| {
                // 每条 Set-Cookie 只读取第一个 name=value；Expires 中的逗号不是分隔符。
                // Cookie 值原样保留，不能按 URL 再解码。
                headers
                    .get_all(ureq::http::header::SET_COOKIE)
                    .iter()
                    .filter_map(|header| header.to_str().ok())
                    .filter_map(|header| header.split(';').next()?.split_once('='))
                    .filter(|(key, value)| key.trim() == name && !value.trim().is_empty())
                    .map(|(_, value)| value.trim().to_string())
                    .next_back()
                    .or_else(|| {
                        url.as_ref()?
                            .query_pairs()
                            .find(|(key, value)| key == name && !value.is_empty())
                            .map(|(_, value)| value.into_owned())
                    })
                    .ok_or_else(|| {
                        BiliLiveError::Parse(format!("登录响应的 Cookie 和 URL 均缺少 {name}"))
                    })
            };
            let opt_credential = |name: &str| -> Option<String> {
                headers
                    .get_all(ureq::http::header::SET_COOKIE)
                    .iter()
                    .filter_map(|header| header.to_str().ok())
                    .filter_map(|header| header.split(';').next()?.split_once('='))
                    .filter(|(key, value)| key.trim() == name && !value.trim().is_empty())
                    .map(|(_, value)| value.trim().to_string())
                    .next_back()
                    .or_else(|| {
                        url.as_ref()?
                            .query_pairs()
                            .find(|(key, value)| key == name && !value.is_empty())
                            .map(|(_, value)| value.into_owned())
                    })
            };
            let sessdata = credential("SESSDATA")?;
            let csrf_token = credential("bili_jct")?;
            let dede_user_id = opt_credential("DedeUserID").unwrap_or_default();
            let dede_user_id_ck_md5 = opt_credential("DedeUserID__ckMd5").unwrap_or_default();
            let refresh_token = data["refresh_token"]
                .as_str()
                .unwrap_or_default()
                .to_string();

            Ok(PollStatus::Success {
                sessdata,
                csrf_token,
                dede_user_id,
                dede_user_id_ck_md5,
                refresh_token,
            })
        }
        _ => Err(BiliLiveError::Api(format!(
            "二维码状态异常: {}",
            data["message"].as_str().unwrap_or("未知错误")
        ))),
    }
}

// ── 账号密码 / 短信登录（Android 端 API） ────────────────────

// 获取 RSA 公钥，用于加密密码
fn get_key() -> Result<(String, String)> {
    let query = format!("appkey={}", ANDROID_APPKEY);
    let sign = md5_sign(&query, ANDROID_APPSEC);
    let url = format!(
        "{}/x/passport-login/web/key?{}&sign={}",
        crate::api::client::passport_base_url(),
        query,
        sign
    );

    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&response, "获取登录公钥失败")?;
    let data = json
        .get("data")
        .ok_or_else(|| BiliLiveError::Parse("缺少 data".to_string()))?;
    let hash = data
        .get("hash")
        .and_then(Value::as_str)
        .ok_or_else(|| BiliLiveError::Parse("缺少 hash".to_string()))?;
    let key = data
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| BiliLiveError::Parse("缺少 key".to_string()))?;
    Ok((hash.to_string(), key.to_string()))
}

// 使用 B站 Android API 进行账号密码登录
pub fn login_by_password(
    username: &str,
    password: &str,
) -> Result<(String, String, String, String, String)> {
    let (hash, pub_key) = get_key()?;
    let pub_key = RsaPublicKey::from_public_key_pem(&pub_key)
        .map_err(|e| BiliLiveError::Parse(format!("解析公钥失败: {}", e)))?;

    let mut rng = OsRng;
    let enc_data = pub_key
        .encrypt(
            &mut rng,
            Pkcs1v15Encrypt,
            format!("{}{}", hash, password).as_bytes(),
        )
        .map_err(|e| BiliLiveError::Parse(format!("加密密码失败: {}", e)))?;
    let encrypt_password = general_purpose::STANDARD_NO_PAD.encode(enc_data);

    let params = vec![
        ("actionKey", "appkey".to_string()),
        ("appkey", ANDROID_APPKEY.to_string()),
        ("build", "6270200".to_string()),
        ("captcha", "".to_string()),
        ("challenge", "".to_string()),
        ("channel", "bili".to_string()),
        ("device", "phone".to_string()),
        ("mobi_app", "android".to_string()),
        ("password", encrypt_password),
        ("permission", "ALL".to_string()),
        ("platform", "android".to_string()),
        ("seccode", "".to_string()),
        ("subid", "1".to_string()),
        ("ts", ts()),
        ("username", username.to_string()),
        ("validate", "".to_string()),
    ];

    let urlencoded = serde_urlencoded::to_string(&params)
        .map_err(|e| BiliLiveError::Parse(format!("URL编码失败: {}", e)))?;
    let sign = md5_sign(&urlencoded, ANDROID_APPSEC);
    let body = format!("{}&sign={}", urlencoded, sign);

    let login_url = format!(
        "{}/x/passport-login/oauth2/login",
        crate::api::client::passport_base_url()
    );
    let response = crate::api::client::post(&login_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = crate::api::client::parse_json_response(&response, "账号密码登录失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "账号密码登录失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    if let Some(status) = json["data"]["status"].as_i64()
        && status != 0
    {
        return Err(BiliLiveError::Api(format!(
            "账号密码登录失败: {}",
            json["data"]["message"].as_str().unwrap_or("未知错误")
        )));
    }

    extract_credentials(&json).map_err(|_| {
        BiliLiveError::Api(
            "账号密码登录未返回 cookie_info，可能触发风控，请使用短信/扫码登录".to_string(),
        )
    })
}

pub enum SmsSendStatus {
    Ready {
        payload: Value,
    },
    NeedRecaptcha {
        url: String,
        recaptcha_token: String,
    },
}

fn generate_fake_buvid() -> String {
    let u = uuid::Uuid::new_v4()
        .to_string()
        .to_uppercase()
        .replace('-', "");
    format!(
        "{}-{}-{}-{}-{}infoc",
        &u[0..8],
        &u[8..12],
        &u[12..16],
        &u[16..20],
        &u[20..]
    )
}

// 发送短信验证码，可能触发滑块验证码（recaptcha）
pub fn send_sms_with_recaptcha(
    phone_number: u64,
    country_code: u32,
    challenge: Option<&str>,
    validate: Option<&str>,
    recaptcha: Option<&str>,
) -> Result<SmsSendStatus> {
    let mut payload = serde_json::json!({
        "actionKey": "appkey",
        "appkey": ANDROID_APPKEY,
        "build": 6510400,
        "buvid": generate_fake_buvid(),
        "channel": "bili",
        "cid": country_code,
        "device": "phone",
        "mobi_app": "android",
        "platform": "android",
        "tel": phone_number,
        "ts": ts(),
    });

    if let (Some(c), Some(v), Some(r)) = (challenge, validate, recaptcha) {
        payload["gee_challenge"] = Value::from(c);
        payload["gee_seccode"] = Value::from(format!("{v}|jordan"));
        payload["gee_validate"] = Value::from(v);
        payload["recaptcha_token"] = Value::from(r);
    }

    let urlencoded = serde_urlencoded::to_string(&payload)
        .map_err(|e| BiliLiveError::Parse(format!("URL编码失败: {}", e)))?;
    let sign = md5_sign(&urlencoded, ANDROID_APPSEC);
    let body = format!("{}&sign={}", urlencoded, sign);

    let sms_send_url = format!(
        "{}/x/passport-login/sms/send",
        crate::api::client::passport_base_url()
    );
    let response = crate::api::client::post(&sms_send_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = crate::api::client::parse_json_response(&response, "发送短信失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "发送短信失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let data = &json["data"];
    if let Some(captcha_key) = data["captcha_key"].as_str()
        && !captcha_key.is_empty()
    {
        payload["captcha_key"] = Value::from(captcha_key);
        return Ok(SmsSendStatus::Ready { payload });
    }

    if let Some(url) = data["recaptcha_url"].as_str()
        && !url.is_empty()
    {
        let recaptcha_token = Url::parse(url)
            .ok()
            .and_then(|u| {
                u.query_pairs()
                    .find(|(k, _)| k == "recaptcha_token")
                    .map(|(_, v)| v.to_string())
            })
            .ok_or_else(|| BiliLiveError::Parse("无法解析 recaptcha_token".to_string()))?;
        return Ok(SmsSendStatus::NeedRecaptcha {
            url: url.to_string(),
            recaptcha_token,
        });
    }

    Err(BiliLiveError::Api("短信发送返回未知结果".to_string()))
}

pub fn login_by_sms(
    code: u32,
    mut payload: Value,
) -> Result<(String, String, String, String, String)> {
    payload["code"] = Value::from(code);
    let urlencoded = serde_urlencoded::to_string(&payload)
        .map_err(|e| BiliLiveError::Parse(format!("URL编码失败: {}", e)))?;
    let sign = md5_sign(&urlencoded, ANDROID_APPSEC);
    let body = format!("{}&sign={}", urlencoded, sign);

    let sms_login_url = format!(
        "{}/x/passport-login/login/sms",
        crate::api::client::passport_base_url()
    );
    let response = crate::api::client::post(&sms_login_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = crate::api::client::parse_json_response(&response, "短信登录失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "短信登录失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    if let Some(status) = json["data"]["status"].as_i64()
        && status != 0
    {
        return Err(BiliLiveError::Api(format!(
            "短信登录失败: {}",
            json["data"]["message"].as_str().unwrap_or("未知错误")
        )));
    }

    extract_credentials(&json).map_err(|_| {
        BiliLiveError::Api("短信登录未返回 cookie_info，可能触发风控，请使用扫码登录".to_string())
    })
}

// ── 通用接口 ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct NavResponse {
    data: NavData,
}

#[derive(Debug, Deserialize)]
struct NavData {
    mid: i64,
}

#[derive(Debug, Deserialize)]
struct RoomInfoResponse {
    data: RoomInfoData,
}

#[derive(Debug, Deserialize)]
struct RoomInfoData {
    roomid: i64,
}

pub fn get_roomid_by_mid(mid: i64) -> Result<i32> {
    let url = format!(
        "{}/room/v1/Room/getRoomInfoOld?mid={}",
        crate::api::client::live_api_base_url(),
        mid
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let room_info: RoomInfoResponse =
        crate::api::client::parse_json_response(&response, "获取直播间号失败")?;
    Ok(room_info.data.roomid as i32)
}

pub fn get_roomid(sessdata: &str) -> Result<i32> {
    let nav_url = format!("{}/x/web-interface/nav", crate::api::client::api_base_url());
    let response = crate::api::client::get(&nav_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Cookie", format!("SESSDATA={}", sessdata))
        .send()?;

    let nav_response: NavResponse =
        crate::api::client::parse_json_response(&response, "获取用户信息失败")?;
    get_roomid_by_mid(nav_response.data.mid)
}

const REFRESH_PUBLIC_KEY_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDLgd2OAkcGVtoE3ThUREbio0Eg
Uc/prcajMKXvkCKFCWhJYJcLkcM2DKKcSeFpD/j6Boy538YXnR6VhcuUJOhH2x71
nzPjfdTcqMz7djHum0qSZA0AyCBDABUqCrfNgCiJ00Ra7GmRj+YCK1NJEuewlb40
JNrRuoEUXpabUzGB8QIDAQAB
-----END PUBLIC KEY-----"#;

pub fn get_user_info(cookies: &crate::auth::cookies::Cookies) -> Result<serde_json::Value> {
    let nav_url = format!("{}/x/web-interface/nav", crate::api::client::api_base_url());
    let response = crate::api::client::get(&nav_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    // Note: preserves direct JSON parsing to satisfy immutable tests/e2e_tier2.rs::t2_f04_03
    let json: serde_json::Value = serde_json::from_str(response.as_str()?)?;
    Ok(json)
}

pub fn logout(cookies: &crate::auth::cookies::Cookies) -> Result<()> {
    if !cookies.sessdata.is_empty() {
        let body = format!("biliCSRF={}", cookies.bili_jct);
        let exit_url = format!("{}/login/exit/v2", crate::api::client::passport_base_url());
        let _ = crate::api::client::post(&exit_url)
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_header("Cookie", cookies.cookie_header())
            .with_body(body)
            .send();
    }
    crate::auth::cookies::delete_cookies()?;
    Ok(())
}

pub fn check_cookie_need_refresh(cookies: &crate::auth::cookies::Cookies) -> Result<(bool, i64)> {
    let url = format!(
        "{}/x/passport-login/web/cookie/info?csrf={}",
        crate::api::client::passport_base_url(),
        cookies.bili_jct
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Cookie", cookies.cookie_header())
        .send()?;

    let json: Value =
        crate::api::client::parse_json_response(&response, "检查 Cookie 刷新状态失败")?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "检查 Cookie 刷新状态失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let refresh = json["data"]["refresh"].as_bool().unwrap_or(false);
    let ts = json["data"]["timestamp"].as_i64().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    });

    Ok((refresh, ts))
}

pub fn get_correspond_path(timestamp: i64) -> Result<String> {
    use rsa::Oaep;
    let pub_key = RsaPublicKey::from_public_key_pem(REFRESH_PUBLIC_KEY_PEM)
        .map_err(|e| BiliLiveError::Parse(format!("解析刷新公钥失败: {e}")))?;
    let mut rng = OsRng;
    let msg = format!("refresh_{timestamp}");
    let enc = pub_key
        .encrypt(&mut rng, Oaep::new::<sha2::Sha256>(), msg.as_bytes())
        .map_err(|e| BiliLiveError::Parse(format!("加密对应路径失败: {e}")))?;
    let hex = enc.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(hex)
}

pub fn get_refresh_csrf(correspond_path: &str, sessdata: &str) -> Result<String> {
    let url = format!(
        "{}/correspond/1/{correspond_path}",
        crate::api::client::web_base_url()
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Cookie", format!("SESSDATA={sessdata}"))
        .send()?;

    if response.status_code >= 400 {
        return Err(BiliLiveError::Api(format!(
            "获取 refresh_csrf 页面失败: HTTP {} {}",
            response.status_code, response.reason_phrase
        )));
    }
    let html = response.as_str()?;
    if let Some(start) = html.find("<div id=\"1-name\">") {
        let rest = &html[start + "<div id=\"1-name\">".len()..];
        if let Some(end) = rest.find("</div>") {
            let token = rest[..end].trim();
            if !token.is_empty() {
                return Ok(token.to_string());
            }
        }
    }
    Err(BiliLiveError::Api(
        "未能从对应页面提取 refresh_csrf".to_string(),
    ))
}

pub fn refresh_cookie(cookies: &mut crate::auth::cookies::Cookies) -> Result<()> {
    if cookies.refresh_token.is_empty() {
        return Err(BiliLiveError::Api(
            "缺少 refresh_token，无法自动刷新 Cookie，请使用扫码重新登录".to_string(),
        ));
    }

    let (_, ts) = check_cookie_need_refresh(cookies)?;
    let correspond_path = get_correspond_path(ts)?;
    let refresh_csrf = get_refresh_csrf(&correspond_path, &cookies.sessdata)?;
    let old_refresh_token = cookies.refresh_token.clone();

    let body = format!(
        "csrf={}&refresh_csrf={}&source=main_web&refresh_token={}",
        cookies.bili_jct, refresh_csrf, old_refresh_token
    );

    let refresh_url = format!(
        "{}/x/passport-login/web/cookie/refresh",
        crate::api::client::passport_base_url()
    );
    let mut response = ureq::post(&refresh_url)
        .header("User-Agent", DEFAULT_USER_AGENT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Cookie", &cookies.cookie_header())
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .max_redirects(0)
        .build()
        .send(&body)?;

    let headers = response.headers().clone();
    let resp_body = response.body_mut().read_to_string()?;
    let json: Value = serde_json::from_str(&resp_body)?;

    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "刷新 Cookie 失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let new_refresh_token = json["data"]["refresh_token"]
        .as_str()
        .ok_or_else(|| BiliLiveError::Parse("缺少新的 refresh_token".to_string()))?
        .to_string();

    for header in headers.get_all(ureq::http::header::SET_COOKIE) {
        if let Ok(val) = header.to_str()
            && let Some(first_part) = val.split(';').next()
            && let Some((k, v)) = first_part.split_once('=')
        {
            match k.trim() {
                "SESSDATA" => cookies.sessdata = v.trim().to_string(),
                "bili_jct" => cookies.bili_jct = v.trim().to_string(),
                "DedeUserID" => cookies.dede_user_id = v.trim().to_string(),
                "DedeUserID__ckMd5" => cookies.dede_user_id_ck_md5 = v.trim().to_string(),
                _ => {}
            }
        }
    }

    let confirm_body = format!(
        "csrf={}&refresh_token={}",
        cookies.bili_jct, old_refresh_token
    );
    let confirm_url = format!(
        "{}/x/passport-login/web/confirm/refresh",
        crate::api::client::passport_base_url()
    );
    let confirm_resp = crate::api::client::post(&confirm_url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Cookie", cookies.cookie_header())
        .with_body(confirm_body)
        .send()?;

    let confirm_json: Value =
        crate::api::client::parse_json_response(&confirm_resp, "确认刷新失败")?;
    if confirm_json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "确认刷新失败: {}",
            confirm_json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    cookies.refresh_token = new_refresh_token;
    cookies.save()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExpRewardStatus {
    pub login: bool,
    pub watch: bool,
    pub coins: i64,
    pub share: bool,
    pub email: bool,
    pub tel: bool,
    pub safe_question: bool,
    pub identify_card: bool,
    pub today_exp: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LiveSignResult {
    pub signed: bool,
    pub already_signed: bool,
    pub offline: bool,
    pub text: String,
    pub special_text: String,
    pub had_sign_days: Option<i64>,
    pub all_days: Option<i64>,
    pub message: String,
}

pub(crate) fn parse_exp_reward(json: &Value) -> Result<ExpRewardStatus> {
    let code = json["code"].as_i64().unwrap_or(-1);
    if code == -101 {
        return Err(BiliLiveError::Auth(
            "登录凭证已失效，请重新登录".to_string(),
        ));
    }
    if code != 0 {
        return Err(BiliLiveError::Api(format!(
            "获取每日经验状态失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    let data = &json["data"];
    let login = data["login"].as_bool().unwrap_or(false);
    let watch = data["watch"].as_bool().unwrap_or(false);
    let coins = data["coins"].as_i64().unwrap_or(0);
    let share = data["share"].as_bool().unwrap_or(false);
    let email = data["email"].as_bool().unwrap_or(false);
    let tel = data["tel"].as_bool().unwrap_or(false);
    let safe_question = data["safe_question"].as_bool().unwrap_or(false);
    let identify_card = data["identify_card"].as_bool().unwrap_or(false);
    let today_exp = (if login { 5 } else { 0 })
        + (if watch { 5 } else { 0 })
        + coins.clamp(0, 50)
        + (if share { 5 } else { 0 });

    Ok(ExpRewardStatus {
        login,
        watch,
        coins,
        share,
        email,
        tel,
        safe_question,
        identify_card,
        today_exp,
    })
}

pub fn get_exp_reward(cookies: &crate::auth::cookies::Cookies) -> Result<ExpRewardStatus> {
    let url = format!(
        "{}/x/member/web/exp/reward",
        crate::api::client::api_base_url()
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://www.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&response, "获取每日经验状态失败")?;
    parse_exp_reward(&json)
}

pub(crate) fn parse_live_sign(json: &Value) -> Result<LiveSignResult> {
    let code = json["code"].as_i64().unwrap_or(-1);
    let message = json["message"].as_str().unwrap_or("").to_string();
    match code {
        0 => {
            let data = &json["data"];
            Ok(LiveSignResult {
                signed: true,
                already_signed: false,
                offline: false,
                text: data["text"].as_str().unwrap_or("").to_string(),
                special_text: data["specialText"].as_str().unwrap_or("").to_string(),
                had_sign_days: data["hadSignDays"].as_i64(),
                all_days: data["allDays"].as_i64(),
                message: if message.is_empty() {
                    "签到成功".to_string()
                } else {
                    message
                },
            })
        }
        1011040 => Ok(LiveSignResult {
            signed: false,
            already_signed: true,
            offline: false,
            text: String::new(),
            special_text: String::new(),
            had_sign_days: None,
            all_days: None,
            message: if message.is_empty() {
                "今天已经签到过了".to_string()
            } else {
                message
            },
        }),
        1 => Ok(LiveSignResult {
            signed: false,
            already_signed: false,
            offline: true,
            text: String::new(),
            special_text: String::new(),
            had_sign_days: None,
            all_days: None,
            message: if message.is_empty() {
                "签到活动已下线，无法使用".to_string()
            } else {
                message
            },
        }),
        -101 => Err(BiliLiveError::Auth(
            "登录凭证已失效，请重新登录".to_string(),
        )),
        _ => Err(BiliLiveError::Api(format!(
            "直播签到失败: {}",
            if message.is_empty() {
                "未知错误"
            } else {
                &message
            }
        ))),
    }
}

pub fn do_live_sign(cookies: &crate::auth::cookies::Cookies) -> Result<LiveSignResult> {
    let url = format!(
        "{}/xlive/web-ucenter/v1/sign/DoSign",
        crate::api::client::live_api_base_url()
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Referer", "https://live.bilibili.com/")
        .with_header("Cookie", cookies.cookie_header())
        .send()?;
    let json: Value = crate::api::client::parse_json_response(&response, "直播签到失败")?;
    parse_live_sign(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn web_poll_distinguishes_request_and_scan_status() {
        assert!(matches!(
            parse_web_poll(
                &json!({"code":0,"data":{"code":86101}}),
                &ureq::http::HeaderMap::new()
            )
            .unwrap(),
            PollStatus::Waiting
        ));
        assert!(matches!(
            parse_web_poll(
                &json!({"code":0,"data":{"code":86090}}),
                &ureq::http::HeaderMap::new()
            )
            .unwrap(),
            PollStatus::Scanned
        ));
        assert!(
            parse_web_poll(
                &json!({"code":0,"data":{"code":86038}}),
                &ureq::http::HeaderMap::new()
            )
            .is_err()
        );
        assert!(
            parse_web_poll(
                &json!({"code":-400,"data":{"code":86101}}),
                &ureq::http::HeaderMap::new()
            )
            .is_err()
        );
        assert!(
            parse_web_poll(&json!({"code":0,"data":{}}), &ureq::http::HeaderMap::new()).is_err()
        );
    }

    #[test]
    fn web_credentials_are_decoded_once() {
        let response = json!({"code":0,"data":{"code":0,"url":"https://example.test/callback?SESSDATA=a%252Cb%2Bc&bili_jct=csrf"}});
        let PollStatus::Success {
            sessdata,
            csrf_token,
            ..
        } = parse_web_poll(&response, &ureq::http::HeaderMap::new()).unwrap()
        else {
            panic!("expected success")
        };
        assert_eq!(sessdata, "a%2Cb+c");
        assert_eq!(csrf_token, "csrf");
        assert!(
            parse_web_poll(
                &json!({"code":0,"data":{"code":0,"url":"https://example.test/?SESSDATA=test"}}),
                &ureq::http::HeaderMap::new()
            )
            .is_err()
        );
    }

    #[test]
    fn web_cookies_work_without_url_and_keep_encoded_values() {
        let mut headers = ureq::http::HeaderMap::new();
        headers.append("set-cookie", "SESSDATA=session%2Ctoken+part==; Path=/; Expires=Wed, 21 Oct 2030 07:28:00 GMT; HttpOnly".parse().unwrap());
        headers.append(
            "set-cookie",
            "bili_jct=csrf; Path=/; Secure".parse().unwrap(),
        );
        headers.append("set-cookie", "DedeUserID=123; Path=/".parse().unwrap());
        for url in [
            json!(null),
            json!(""),
            json!("https://example.test/?SESSDATA=old&bili_jct=old"),
        ] {
            let response = json!({"code":0,"data":{"code":0,"url":url}});
            let PollStatus::Success {
                sessdata,
                csrf_token,
                ..
            } = parse_web_poll(&response, &headers).unwrap()
            else {
                panic!("expected success")
            };
            assert_eq!(sessdata, "session%2Ctoken+part==");
            assert_eq!(csrf_token, "csrf");
        }
        headers.remove("set-cookie");
        headers.append(
            "set-cookie",
            "DedeUserID=123; Path=/; SESSDATA=fake".parse().unwrap(),
        );
        assert!(parse_web_poll(&json!({"code":0,"data":{"code":0}}), &headers).is_err());
    }

    #[test]
    fn web_poll_preserves_multiple_cookie_headers_over_http() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::time::Duration;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/poll", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let body = r#"{"code":0,"data":{"code":0,"url":"https://example.test/callback"}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nSet-Cookie: SESSDATA=test%2Csession; Path=/; Expires=Wed, 21 Oct 2030 07:28:00 GMT\r\nSet-Cookie: bili_jct=test-csrf; Path=/\r\nSet-Cookie: DedeUserID=123; Path=/\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let result = poll_web_qr_url(&url).unwrap();
        server.join().unwrap();
        let PollStatus::Success {
            sessdata,
            csrf_token,
            ..
        } = result
        else {
            panic!("expected success")
        };
        assert_eq!(sessdata, "test%2Csession");
        assert_eq!(csrf_token, "test-csrf");
    }

    #[test]
    fn tv_expiration_is_not_scanned() {
        assert!(matches!(
            parse_tv_poll(&json!({"code":86039})).unwrap(),
            PollStatus::Waiting
        ));
        assert!(parse_tv_poll(&json!({"code":86038})).is_err());
        let response = json!({"code":0,"data":{"cookie_info":{"cookies":[{"name":"SESSDATA","value":"session"},{"name":"bili_jct","value":"csrf"}]}}});
        assert!(matches!(
            parse_tv_poll(&response).unwrap(),
            PollStatus::Success { .. }
        ));
        assert!(parse_tv_poll(&json!({"code":0,"data":{}})).is_err());
    }

    #[test]
    fn test_correspond_path_generation() {
        let ts = 1684466082562;
        let path = get_correspond_path(ts).unwrap();
        assert_eq!(path.len(), 256);
    }

    #[test]
    fn test_parse_exp_reward_and_auth_errors() {
        let ok = parse_exp_reward(&json!({
            "code": 0,
            "data": {
                "login": true,
                "watch": true,
                "coins": 30,
                "share": false,
                "email": true,
                "tel": true,
                "safe_question": false,
                "identify_card": true
            }
        }))
        .unwrap();
        assert_eq!(ok.today_exp, 40);
        assert!(ok.login);
        assert!(ok.watch);
        assert!(!ok.share);

        let err = parse_exp_reward(&json!({"code": -101, "message": "账号未登录"})).unwrap_err();
        assert!(matches!(err, BiliLiveError::Auth(_)));
    }

    #[test]
    fn test_parse_live_sign_handles_success_already_signed_offline_and_auth() {
        let signed = parse_live_sign(&json!({
            "code": 0,
            "message": "0",
            "data": {
                "text": "3000用户经验, 1个辣条",
                "specialText": "再签到2天可获...",
                "hadSignDays": 5,
                "allDays": 30
            }
        }))
        .unwrap();
        assert!(signed.signed);
        assert!(!signed.already_signed);
        assert!(!signed.offline);
        assert_eq!(signed.had_sign_days, Some(5));
        assert_eq!(signed.all_days, Some(30));

        let already = parse_live_sign(&json!({
            "code": 1011040,
            "message": "今天已经签到过了",
            "data": null
        }))
        .unwrap();
        assert!(!already.signed);
        assert!(already.already_signed);
        assert!(!already.offline);

        let offline = parse_live_sign(&json!({
            "code": 1,
            "message": "签到活动已下线，无法使用。",
            "data": null
        }))
        .unwrap();
        assert!(!offline.signed);
        assert!(!offline.already_signed);
        assert!(offline.offline);

        let auth_err = parse_live_sign(&json!({"code": -101, "message": "未登录"})).unwrap_err();
        assert!(matches!(auth_err, BiliLiveError::Auth(_)));
    }

    #[test]
    fn test_daily_endpoints_respect_base_url_overrides() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let _lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct RestoreEnvs(Option<String>, Option<String>);
        impl Drop for RestoreEnvs {
            fn drop(&mut self) {
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                        None => std::env::remove_var("BT_API_BASE_URL"),
                    }
                    match &self.1 {
                        Some(v) => std::env::set_var("BT_LIVE_API_BASE_URL", v),
                        None => std::env::remove_var("BT_LIVE_API_BASE_URL"),
                    }
                }
            }
        }
        let _restore = RestoreEnvs(
            std::env::var("BT_API_BASE_URL").ok(),
            std::env::var("BT_LIVE_API_BASE_URL").ok(),
        );

        let api_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api_base = format!("http://{}", api_listener.local_addr().unwrap());
        let api_handle = std::thread::spawn(move || {
            let (mut stream, _) = api_listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first_line = String::new();
            reader.read_line(&mut first_line).unwrap();
            assert!(first_line.starts_with("GET /x/member/web/exp/reward "));
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let body = r#"{"code":0,"data":{"login":true,"watch":false,"coins":10,"share":true,"email":true,"tel":true,"safe_question":true,"identify_card":true}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let live_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let live_base = format!("http://{}", live_listener.local_addr().unwrap());
        let live_handle = std::thread::spawn(move || {
            let (mut stream, _) = live_listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first_line = String::new();
            reader.read_line(&mut first_line).unwrap();
            assert!(first_line.starts_with("GET /xlive/web-ucenter/v1/sign/DoSign "));
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let body = r#"{"code":0,"message":"0","data":{"text":"3000用户经验","specialText":"","hadSignDays":2,"allDays":31}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        unsafe {
            std::env::set_var("BT_API_BASE_URL", &api_base);
            std::env::set_var("BT_LIVE_API_BASE_URL", &live_base);
        }
        let cookies = crate::auth::cookies::Cookies {
            sessdata: "test_sess".to_string(),
            bili_jct: "test_jct".to_string(),
            ..Default::default()
        };
        let exp = get_exp_reward(&cookies).unwrap();
        let sign = do_live_sign(&cookies).unwrap();
        api_handle.join().unwrap();
        live_handle.join().unwrap();

        assert_eq!(exp.today_exp, 20);
        assert!(sign.signed);
        assert_eq!(sign.had_sign_days, Some(2));
    }
}
