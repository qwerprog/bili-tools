use crate::api::passport::{
    PollStatus, SmsSendStatus, generate_qr_code, generate_web_qr_code, login_by_password,
    login_by_sms, poll_qr_status, poll_web_qr_status, send_sms_with_recaptcha,
};
use crate::auth::cookies::Cookies;
use crate::cli::args::LoginMethod;
use crate::error::{BiliLiveError, Result};
use crate::ui::theme::bili_dialoguer_theme;
use crate::utils::paths::cookies_path;
use crate::utils::qrcode::{generate_and_save_qrcode, print_qrcode_in_terminal};
use crate::{user_info, user_success, user_warning};
use dialoguer::{Input, Password, Select};
use std::io::IsTerminal;

#[derive(Default, Debug, Clone)]
pub struct LoginOptions {
    pub method: Option<LoginMethod>,
    pub sessdata: Option<String>,
    pub bili_jct: Option<String>,
    pub dede_user_id: Option<String>,
}

/// 保存完整的登录凭据并持久化
fn save_login_credentials(
    sessdata: String,
    csrf_token: String,
    dede_user_id: String,
    dede_user_id_ck_md5: String,
    refresh_token: String,
) -> Result<()> {
    let mut cookies = Cookies {
        sessdata: sessdata.clone(),
        bili_jct: csrf_token,
        dede_user_id,
        dede_user_id_ck_md5,
        refresh_token,
        ..Default::default()
    };
    if let Ok(rid) = crate::api::passport::get_roomid(&sessdata) {
        cookies.room_id = rid;
    }
    let _ = cookies.ensure_buvid();
    cookies.save()?;
    Ok(())
}

// 登录入口：显示 dialoguer 菜单，根据选择分发到不同登录方式
pub fn start_login() -> Result<()> {
    start_login_with_options(LoginOptions::default())
}

pub fn start_login_with_options(opts: LoginOptions) -> Result<()> {
    let effective_method = opts
        .method
        .or_else(|| opts.sessdata.as_ref().map(|_| LoginMethod::Cookie));

    match effective_method {
        Some(LoginMethod::Qr) => {
            if crate::cli::output::is_json() || crate::cli::output::is_yes() {
                return Err(BiliLiveError::Input(
                    "JSON/自动确认模式下不支持二维码扫码登录，请使用 --method cookie --sessdata <S> --bili-jct <J>"
                        .to_string(),
                ));
            }
            login_by_qrcode(false)
        }
        Some(LoginMethod::Tv) => {
            if crate::cli::output::is_json() || crate::cli::output::is_yes() {
                return Err(BiliLiveError::Input(
                    "JSON/自动确认模式下不支持二维码扫码登录，请使用 --method cookie --sessdata <S> --bili-jct <J>"
                        .to_string(),
                ));
            }
            login_by_qrcode(true)
        }
        Some(LoginMethod::Sms) => {
            if crate::cli::output::is_json() || !std::io::stdin().is_terminal() {
                return Err(BiliLiveError::Input(
                    "短信登录需要交互式终端；非交互模式下请使用 --method cookie --sessdata <S>"
                        .to_string(),
                ));
            }
            login_by_sms_flow()
        }
        Some(LoginMethod::Password) => {
            if crate::cli::output::is_json() || !std::io::stdin().is_terminal() {
                return Err(BiliLiveError::Input(
                    "账号密码登录需要交互式终端；非交互模式下请使用 --method cookie --sessdata <S>"
                        .to_string(),
                ));
            }
            login_by_password_flow()
        }
        Some(LoginMethod::Cookie) => login_by_cookie(opts.sessdata, opts.bili_jct, opts.dede_user_id),
        None => {
            if crate::cli::output::is_json() || !std::io::stdin().is_terminal() {
                return Err(BiliLiveError::Input(
                    "非交互模式下请指定 --method（例如 --method cookie --sessdata <S> --bili-jct <J>）"
                        .to_string(),
                ));
            }

            let selection = Select::with_theme(&bili_dialoguer_theme())
                .with_prompt("选择一种登录方式")
                .default(0)
                .item("扫码登录（Web，推荐）")
                .item("短信登录")
                .item("账号密码")
                .item("浏览器登录")
                .item("扫码登录（TV，备用）")
                .interact()
                .map_err(|e| crate::error::BiliLiveError::Input(format!("选择登录方式失败: {e}")))?;

            match selection {
                0 => login_by_qrcode(false),
                1 => login_by_sms_flow(),
                2 => login_by_password_flow(),
                3 => login_by_browser(),
                4 => login_by_qrcode(true),
                _ => Ok(()),
            }
        }
    }
}

fn login_by_cookie(
    sessdata: Option<String>,
    bili_jct: Option<String>,
    dede_user_id: Option<String>,
) -> Result<()> {
    let sess = sessdata
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            BiliLiveError::Input("使用 --method cookie 时必须提供 --sessdata".to_string())
        })?;

    let jct = bili_jct.map(|s| s.trim().to_string()).unwrap_or_default();
    if jct.is_empty() {
        user_warning!("未提供 --bili-jct，涉及写入或开播的 CSRF 操作将受限");
    }

    let uid = dede_user_id
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let mut cookies = Cookies {
        sessdata: sess.clone(),
        bili_jct: jct,
        dede_user_id: uid,
        ..Default::default()
    };

    if cookies.dede_user_id.is_empty()
        && let Ok(info) = crate::api::passport::get_user_info(&cookies)
        && info["code"].as_i64() == Some(0)
        && info["data"]["isLogin"].as_bool() == Some(true)
        && let Some(mid) = info["data"]["mid"].as_i64()
        && mid > 0
    {
        cookies.dede_user_id = mid.to_string();
    }

    if cookies.room_id <= 0 {
        if let Ok(mid) = cookies.dede_user_id.parse::<i64>()
            && mid > 0
        {
            if let Ok(rid) = crate::api::passport::get_roomid_by_mid(mid) {
                cookies.room_id = rid;
            }
        } else if let Ok(rid) = crate::api::passport::get_roomid(&sess) {
            cookies.room_id = rid;
        }
    }

    let _ = cookies.ensure_buvid();
    cookies.save()?;
    Ok(())
}

// 账号密码登录流程
fn login_by_password_flow() -> Result<()> {
    let username: String = Input::with_theme(&bili_dialoguer_theme())
        .with_prompt("请输入账号")
        .interact()
        .map_err(|e| BiliLiveError::Input(format!("读取账号失败: {e}")))?;
    let password: String = Password::with_theme(&bili_dialoguer_theme())
        .with_prompt("请输入密码")
        .interact()
        .map_err(|e| BiliLiveError::Input(format!("读取密码失败: {e}")))?;

    let (sessdata, csrf_token, dede_user_id, dede_user_id_ck_md5, refresh_token) =
        login_by_password(&username, &password)?;
    save_login_credentials(
        sessdata,
        csrf_token,
        dede_user_id,
        dede_user_id_ck_md5,
        refresh_token,
    )?;
    user_success!("登录成功！");
    Ok(())
}

// 短信登录流程（含滑块验证码处理）
fn login_by_sms_flow() -> Result<()> {
    let country_code: u32 = Input::with_theme(&bili_dialoguer_theme())
        .with_prompt("请输入手机国家代码")
        .default(86)
        .interact_text()
        .map_err(|e| BiliLiveError::Input(format!("读取国家代码失败: {e}")))?;
    let phone: u64 = Input::with_theme(&bili_dialoguer_theme())
        .with_prompt("请输入手机号")
        .interact_text()
        .map_err(|e| BiliLiveError::Input(format!("读取手机号失败: {e}")))?;

    let payload = match send_sms_with_recaptcha(phone, country_code, None, None, None)? {
        SmsSendStatus::Ready { payload } => payload,
        SmsSendStatus::NeedRecaptcha {
            url,
            recaptcha_token,
        } => {
            user_warning!("需要滑动验证码");
            user_info!("请复制此链接至浏览器打开并完成滑动验证：{}", url);
            user_info!("完成后在开发者工具 Network 中查看 get.php / ajax.php 响应");
            let challenge: String = Input::with_theme(&bili_dialoguer_theme())
                .with_prompt("请输入 get.php 响应中的 challenge")
                .interact_text()
                .map_err(|e| BiliLiveError::Input(format!("读取 challenge 失败: {e}")))?;
            let validate: String = Input::with_theme(&bili_dialoguer_theme())
                .with_prompt("请输入 ajax.php 响应中的 validate")
                .interact_text()
                .map_err(|e| BiliLiveError::Input(format!("读取 validate 失败: {e}")))?;

            match send_sms_with_recaptcha(
                phone,
                country_code,
                Some(&challenge),
                Some(&validate),
                Some(&recaptcha_token),
            )? {
                SmsSendStatus::Ready { payload } => payload,
                SmsSendStatus::NeedRecaptcha { .. } => {
                    return Err(BiliLiveError::Api("滑动验证失败，请重试".to_string()));
                }
            }
        }
    };

    let code: u32 = Input::with_theme(&bili_dialoguer_theme())
        .with_prompt("请输入短信验证码")
        .interact_text()
        .map_err(|e| BiliLiveError::Input(format!("读取验证码失败: {e}")))?;

    let (sessdata, csrf_token, dede_user_id, dede_user_id_ck_md5, refresh_token) =
        login_by_sms(code, payload)?;
    save_login_credentials(
        sessdata,
        csrf_token,
        dede_user_id,
        dede_user_id_ck_md5,
        refresh_token,
    )?;
    user_success!("登录成功！");
    Ok(())
}

fn resolve_qrcode_path() -> Result<std::path::PathBuf> {
    let cookie_path = cookies_path()?;
    let parent = match cookie_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => std::path::Path::new("."),
    };
    Ok(parent.join("qrcode.png"))
}

// Web 为默认登录方式，TV 可由用户手动选择。
fn login_by_qrcode(tv: bool) -> Result<()> {
    user_info!("开始B站{}二维码登录流程...", if tv { "TV" } else { "Web" });
    let (url, key) = if tv {
        let qr = generate_qr_code()?;
        (qr.url, qr.auth_code)
    } else {
        let qr = generate_web_qr_code()?;
        (qr.url, qr.qrcode_key)
    };
    user_info!("请使用B站手机客户端扫描如下二维码");
    print_qrcode_in_terminal(&url)?;
    let qr_path = resolve_qrcode_path()?;
    generate_and_save_qrcode(&url, &qr_path)?;
    let _cleanup = crate::utils::interrupt::TempFileCleanupGuard::new(qr_path.clone());
    user_success!("二维码已保存到 {}", qr_path.display());
    user_info!("等待用户处理...");
    poll_until_success(&key, tv)
}

fn login_by_browser() -> Result<()> {
    let qr = generate_qr_code()?;
    if !qr.url.starts_with("http") {
        user_warning!("该链接为 App 协议，浏览器可能无法打开，建议使用扫码登录");
    }
    user_info!("请复制此链接至浏览器中完成登录：{}", qr.url);
    poll_until_success(&qr.auth_code, true)
}

fn poll_until_success(key: &str, tv: bool) -> Result<()> {
    let started = std::time::Instant::now();
    let mut scanned = false;
    while started.elapsed() < std::time::Duration::from_secs(180) {
        crate::utils::interrupt::check_interrupted()?;
        let status = if tv {
            poll_qr_status(key)?
        } else {
            poll_web_qr_status(key)?
        };
        match status {
            PollStatus::Waiting => {}
            PollStatus::Scanned => {
                if !scanned {
                    user_info!("已扫码，请在手机上确认登录");
                    scanned = true;
                }
            }
            PollStatus::Success {
                sessdata,
                csrf_token,
                dede_user_id,
                dede_user_id_ck_md5,
                refresh_token,
            } => {
                save_login_credentials(
                    sessdata,
                    csrf_token,
                    dede_user_id,
                    dede_user_id_ck_md5,
                    refresh_token,
                )?;
                user_success!("登录成功！");
                return Ok(());
            }
        }
        crate::utils::interrupt::sleep_interruptible(std::time::Duration::from_secs(2))?;
    }
    Err(BiliLiveError::Api("扫码等待超时，请重新登录".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct ResetLoginTestState;
    impl Drop for ResetLoginTestState {
        fn drop(&mut self) {
            crate::utils::paths::set_cookie_file_override(None);
            crate::cli::output::init_output(false, false, false, false, None);
        }
    }

    #[test]
    fn resolve_qrcode_path_respects_cookie_file_override_and_empty_parent() {
        let _lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _reset = ResetLoginTestState;

        let dir = tempfile::tempdir().unwrap();
        let nested_cookie = dir.path().join("custom").join("c.json");
        crate::utils::paths::set_cookie_file_override(Some(nested_cookie));
        assert_eq!(
            resolve_qrcode_path().unwrap(),
            dir.path().join("custom").join("qrcode.png")
        );

        crate::utils::paths::set_cookie_file_override(Some(PathBuf::from("relative_c.json")));
        assert_eq!(
            resolve_qrcode_path().unwrap(),
            PathBuf::from(".").join("qrcode.png")
        );
    }

    #[test]
    fn qr_and_tv_login_reject_json_mode_immediately() {
        let _lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _reset = ResetLoginTestState;

        let dir = tempfile::tempdir().unwrap();
        let custom_cookie = dir.path().join("qr_test.json");
        crate::utils::paths::set_cookie_file_override(Some(custom_cookie));
        crate::cli::output::init_output(true, false, false, false, None);

        for method in [LoginMethod::Qr, LoginMethod::Tv] {
            let err = start_login_with_options(LoginOptions {
                method: Some(method),
                ..Default::default()
            })
            .unwrap_err();
            assert!(
                matches!(err, BiliLiveError::Input(_)),
                "expected Input error for {method:?} in json mode, got {err:?}"
            );
            assert!(!dir.path().join("qrcode.png").exists());
        }
    }
}
