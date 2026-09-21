pub mod cookies;
pub mod login;
pub mod session;

pub use login::start_login;
#[allow(unused_imports)]
pub use session::check_status;

use crate::error::{BiliLiveError, Result};

/// 确保具备登录凭据。若本地凭证存在且包含 SESSDATA，直接返回并补全指纹；
/// 若未登录，在非 JSON 模式下引导登录流程，在 JSON 模式下返回认证错误。
pub fn ensure_login() -> Result<cookies::Cookies> {
    match cookies::read_cookies() {
        Ok(mut c) if !c.sessdata.is_empty() => {
            let _ = c.ensure_buvid();
            Ok(c)
        }
        _ => {
            if crate::cli::output::is_json() {
                return Err(BiliLiveError::Auth(
                    "未检测到登录信息，请执行 `bt auth login` 进行登录".to_string(),
                ));
            }
            crate::user_info!("未检测到登录信息，开始登录流程...");
            start_login()?;
            let mut fresh = cookies::read_cookies()?;
            let _ = fresh.ensure_buvid();
            Ok(fresh)
        }
    }
}
