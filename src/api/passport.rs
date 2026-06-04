use crate::api::client::DEFAULT_USER_AGENT;
use crate::error::{BiliLiveError, Result};
use base64::{Engine as _, engine::general_purpose};
use rand::rngs::OsRng;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
use serde::Deserialize;
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

// 从登录响应 JSON 中提取 SESSDATA 和 bili_jct
fn extract_credentials(json: &Value) -> Result<(String, String)> {
    let cookies = json["data"]["cookie_info"]["cookies"]
        .as_array()
        .ok_or_else(|| BiliLiveError::Parse("缺少 cookie_info".to_string()))?;

    let mut sessdata = String::new();
    let mut csrf_token = String::new();
    for cookie in cookies {
        match cookie["name"].as_str() {
            Some("SESSDATA") => sessdata = cookie["value"].as_str().unwrap_or("").to_string(),
            Some("bili_jct") => csrf_token = cookie["value"].as_str().unwrap_or("").to_string(),
            _ => {}
        }
    }

    if sessdata.is_empty() || csrf_token.is_empty() {
        return Err(BiliLiveError::Parse("无法提取登录凭证".to_string()));
    }
    Ok((sessdata, csrf_token))
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

    let response =
        minreq::post("https://passport.bilibili.com/x/passport-tv-login/qrcode/auth_code")
            .with_header("User-Agent", DEFAULT_USER_AGENT)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_body(body)
            .send()?;

    let json: Value = serde_json::from_str(response.as_str()?)?;
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
// 86039 = 等待扫码、86038 = 已扫码待确认、code=0 = 登录成功
pub enum PollStatus {
    /// 等待扫码（86039）
    Waiting,
    /// 已扫码，等待确认（86038）
    Scanned,
    /// 登录成功，携带凭证
    Success {
        sessdata: String,
        csrf_token: String,
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

    let response = minreq::post("https://passport.bilibili.com/x/passport-tv-login/qrcode/poll")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = serde_json::from_str(response.as_str()?)?;

    match json["code"].as_i64() {
        Some(0) => {
            let (sessdata, csrf_token) = extract_credentials(&json)?;
            Ok(PollStatus::Success {
                sessdata,
                csrf_token,
            })
        }
        Some(86039) => Ok(PollStatus::Waiting),
        Some(86038) => Ok(PollStatus::Scanned),
        _ => Err(BiliLiveError::Api(format!(
            "二维码状态异常: {}",
            json["message"].as_str().unwrap_or("未知错误")
        ))),
    }
}

// ── 账号密码 / 短信登录（Android 端 API） ────────────────────

// 获取 RSA 公钥，用于加密密码
fn get_key() -> Result<(String, String)> {
    let query = format!("appkey={}", ANDROID_APPKEY);
    let sign = md5_sign(&query, ANDROID_APPSEC);
    let url = format!(
        "https://passport.bilibili.com/x/passport-login/web/key?{}&sign={}",
        query, sign
    );

    let response = minreq::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;
    let json: Value = serde_json::from_str(response.as_str()?)?;
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
pub fn login_by_password(username: &str, password: &str) -> Result<(String, String)> {
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

    let response = minreq::post("https://passport.bilibili.com/x/passport-login/oauth2/login")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = serde_json::from_str(response.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "账号密码登录失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    if let Some(status) = json["data"]["status"].as_i64() {
        if status != 0 {
            return Err(BiliLiveError::Api(format!(
                "账号密码登录失败: {}",
                json["data"]["message"].as_str().unwrap_or("未知错误")
            )));
        }
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

    let response = minreq::post("https://passport.bilibili.com/x/passport-login/sms/send")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = serde_json::from_str(response.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "发送短信失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }

    let data = &json["data"];
    if let Some(captcha_key) = data["captcha_key"].as_str() {
        if !captcha_key.is_empty() {
            payload["captcha_key"] = Value::from(captcha_key);
            return Ok(SmsSendStatus::Ready { payload });
        }
    }

    if let Some(url) = data["recaptcha_url"].as_str() {
        if !url.is_empty() {
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
    }

    Err(BiliLiveError::Api("短信发送返回未知结果".to_string()))
}

pub fn login_by_sms(code: u32, mut payload: Value) -> Result<(String, String)> {
    payload["code"] = Value::from(code);
    let urlencoded = serde_urlencoded::to_string(&payload)
        .map_err(|e| BiliLiveError::Parse(format!("URL编码失败: {}", e)))?;
    let sign = md5_sign(&urlencoded, ANDROID_APPSEC);
    let body = format!("{}&sign={}", urlencoded, sign);

    let response = minreq::post("https://passport.bilibili.com/x/passport-login/login/sms")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_body(body)
        .send()?;

    let json: Value = serde_json::from_str(response.as_str()?)?;
    if json["code"].as_i64() != Some(0) {
        return Err(BiliLiveError::Api(format!(
            "短信登录失败: {}",
            json["message"].as_str().unwrap_or("未知错误")
        )));
    }
    if let Some(status) = json["data"]["status"].as_i64() {
        if status != 0 {
            return Err(BiliLiveError::Api(format!(
                "短信登录失败: {}",
                json["data"]["message"].as_str().unwrap_or("未知错误")
            )));
        }
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

pub fn get_roomid(sessdata: &str) -> Result<i32> {
    let response = minreq::get("https://api.bilibili.com/x/web-interface/nav")
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .with_header("Cookie", format!("SESSDATA={}", sessdata))
        .send()?;

    let nav_response: NavResponse = serde_json::from_str(response.as_str()?)?;
    let user_code = nav_response.data.mid.to_string();

    let url = format!(
        "https://api.live.bilibili.com/room/v1/Room/getRoomInfoOld?mid={}",
        user_code
    );
    let response = minreq::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let room_info: RoomInfoResponse = serde_json::from_str(response.as_str()?)?;
    Ok(room_info.data.roomid as i32)
}
