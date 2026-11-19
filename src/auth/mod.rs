pub mod cookies;
pub mod login;
pub mod session;

pub use login::{LoginOptions, start_login, start_login_with_options};

use crate::error::{BiliLiveError, Result};
use std::io::IsTerminal;

/// 确保具备登录凭据。若本地凭证存在且包含 SESSDATA，直接返回并补全指纹；
/// 若未登录，直接返回认证错误提示运行 `bt auth login`（不劫持只读命令进入扫码菜单）。
pub fn ensure_login() -> Result<cookies::Cookies> {
    match cookies::read_cookies() {
        Ok(mut c) if !c.sessdata.trim().is_empty() => {
            let _ = c.ensure_buvid();
            Ok(c)
        }
        _ => Err(BiliLiveError::Auth(
            "未检测到登录信息，请执行 `bt auth login` 进行登录".to_string(),
        )),
    }
}

/// 确保具备登录凭据且包含有效的 CSRF Token (`bili_jct`)。
pub fn ensure_csrf() -> Result<cookies::Cookies> {
    let c = ensure_login()?;
    if c.bili_jct.trim().is_empty() {
        return Err(BiliLiveError::Auth(
            "当前登录凭据缺少 bili_jct (CSRF Token)，请重新登录或通过 --bili-jct 补充".to_string(),
        ));
    }
    Ok(c)
}

/// 专用于 `bt live start` 的交互式登录引导：仅在交互式 TTY 且非 `--json` / 非 `-y` 时引导扫码登录。
pub fn ensure_csrf_interactive() -> Result<cookies::Cookies> {
    let cookies = match cookies::read_cookies() {
        Ok(mut c) if !c.sessdata.trim().is_empty() => {
            let _ = c.ensure_buvid();
            c
        }
        _ => {
            if crate::cli::output::is_json()
                || crate::cli::output::is_yes()
                || !std::io::stdin().is_terminal()
            {
                return Err(BiliLiveError::Auth(
                    "未检测到登录信息，请执行 `bt auth login` 进行登录".to_string(),
                ));
            }
            crate::user_info!("未检测到登录信息，开始登录流程...");
            start_login()?;
            let mut fresh = cookies::read_cookies()?;
            let _ = fresh.ensure_buvid();
            fresh
        }
    };

    if cookies.bili_jct.trim().is_empty() {
        return Err(BiliLiveError::Auth(
            "当前登录凭据缺少 bili_jct (CSRF Token)，请重新登录或通过 --bili-jct 补充".to_string(),
        ));
    }
    Ok(cookies)
}
