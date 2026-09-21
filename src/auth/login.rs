use crate::api::passport::{
    PollStatus, SmsSendStatus, generate_qr_code, generate_web_qr_code, login_by_password,
    login_by_sms, poll_qr_status, poll_web_qr_status, send_sms_with_recaptcha,
};
use crate::auth::cookies::save_cookies_from_credentials;
use crate::error::{BiliLiveError, Result};
use crate::utils::paths::data_file;
use crate::utils::qrcode::{generate_and_save_qrcode, print_qrcode_in_terminal};
use crate::{user_info, user_success, user_warning};
use dialoguer::{Input, Password, Select, theme::ColorfulTheme};

// 登录入口：显示 dialoguer 菜单，根据选择分发到不同登录方式
pub fn start_login() -> Result<()> {
    let selection = Select::with_theme(&ColorfulTheme::default())
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

// 账号密码登录流程
fn login_by_password_flow() -> Result<()> {
    let username: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("请输入账号")
        .interact()
        .map_err(|e| BiliLiveError::Input(format!("读取账号失败: {e}")))?;
    let password: String = Password::with_theme(&ColorfulTheme::default())
        .with_prompt("请输入密码")
        .interact()
        .map_err(|e| BiliLiveError::Input(format!("读取密码失败: {e}")))?;

    let (sessdata, csrf_token) = login_by_password(&username, &password)?;
    save_cookies_from_credentials(&sessdata, &csrf_token)?;
    user_success!("登录成功！");
    Ok(())
}

// 短信登录流程（含滑块验证码处理）
fn login_by_sms_flow() -> Result<()> {
    let country_code: u32 = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("请输入手机国家代码")
        .default(86)
        .interact_text()
        .map_err(|e| BiliLiveError::Input(format!("读取国家代码失败: {e}")))?;
    let phone: u64 = Input::with_theme(&ColorfulTheme::default())
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
            let challenge: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("请输入 get.php 响应中的 challenge")
                .interact_text()
                .map_err(|e| BiliLiveError::Input(format!("读取 challenge 失败: {e}")))?;
            let validate: String = Input::with_theme(&ColorfulTheme::default())
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

    let code: u32 = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("请输入短信验证码")
        .interact_text()
        .map_err(|e| BiliLiveError::Input(format!("读取验证码失败: {e}")))?;

    let (sessdata, csrf_token) = login_by_sms(code, payload)?;
    save_cookies_from_credentials(&sessdata, &csrf_token)?;
    user_success!("登录成功！");
    Ok(())
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
    let qr_path = data_file("qrcode.png")?;
    generate_and_save_qrcode(&url, &qr_path)?;
    user_success!("二维码已保存到 {}", qr_path.display());
    user_info!("等待用户处理...");
    let result = poll_until_success(&key, tv);
    let _ = std::fs::remove_file(&qr_path);
    result
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
            } => {
                save_cookies_from_credentials(&sessdata, &csrf_token)?;
                user_success!("登录成功！");
                return Ok(());
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    Err(BiliLiveError::Api("扫码等待超时，请重新登录".to_string()))
}
