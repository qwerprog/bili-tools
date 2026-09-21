use crate::cli::args::AuthCommands;
use crate::cli::output::{is_json, print_json};
use crate::error::Result;
use crate::{user_info, user_success, user_warning};

pub fn handle_auth(cmd: AuthCommands) -> Result<()> {
    match cmd {
        AuthCommands::Login => {
            if !is_json() {
                user_info!("开始登录流程...");
            }
            crate::auth::start_login()?;
            let cookies = crate::auth::cookies::read_cookies()?;
            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "uid": cookies.dede_user_id,
                    "room_id": cookies.room_id
                }))?;
            } else {
                user_success!("登录成功！UID: {}", cookies.dede_user_id);
                if cookies.room_id > 0 {
                    user_info!("检测到绑定的直播间号: {}", cookies.room_id);
                }
            }
        }
        AuthCommands::Status => {
            let cookies = match crate::auth::cookies::read_cookies() {
                Ok(c) => c,
                Err(_) => {
                    if is_json() {
                        print_json(&serde_json::json!({
                            "is_login": false,
                            "message": "未找到本地 Cookie"
                        }))?;
                    } else {
                        user_warning!("未检测到登录信息，请执行 `bt auth login` 登录");
                    }
                    return Ok(());
                }
            };

            match crate::api::passport::get_user_info(&cookies) {
                Ok(info) => {
                    let data = &info["data"];
                    let is_login = data["isLogin"].as_bool().unwrap_or(false);

                    if !is_login {
                        if is_json() {
                            print_json(&serde_json::json!({
                                "is_login": false,
                                "message": "账号未登录或登录凭证已过期"
                            }))?;
                        } else {
                            user_warning!("登录凭证已失效，请执行 `bt auth login` 重新登录");
                        }
                        return Ok(());
                    }

                    let uname = data["uname"].as_str().unwrap_or("未知");
                    let mid = data["mid"].as_i64().unwrap_or(0);
                    let coins = data["money"].as_f64().unwrap_or(0.0);
                    let level = data["level_info"]["current_level"].as_i64().unwrap_or(0);
                    let vip_status = if data["vipStatus"].as_i64().unwrap_or(0) == 1 {
                        "大会员"
                    } else {
                        "普通用户"
                    };

                    // 若历史 cookie 缺失 dede_user_id，自动补全保存
                    let mut mut_cookies = cookies;
                    if mut_cookies.dede_user_id.is_empty() && mid > 0 {
                        mut_cookies.dede_user_id = mid.to_string();
                        let _ = mut_cookies.save();
                    }

                    if is_json() {
                        print_json(&serde_json::json!({
                            "is_login": true,
                            "uid": mid.to_string(),
                            "mid": mid,
                            "uname": uname,
                            "level": level,
                            "money": coins,
                            "vip_status": vip_status,
                            "room_id": mut_cookies.room_id,
                            "raw": info
                        }))?;
                    } else {
                        println!("  {:>6}  {}", "用户", uname);
                        println!("  {:>6}  {}", "UID", mid);
                        println!("  {:>6}  Lv.{}", "等级", level);
                        println!("  {:>6}  {}", "硬币", coins);
                        println!("  {:>6}  {}", "身份", vip_status);
                        if mut_cookies.room_id > 0 {
                            println!("  {:>6}  {}", "直播间", mut_cookies.room_id);
                        }
                        user_success!("登录状态有效");
                    }
                }
                Err(e) => {
                    if is_json() {
                        print_json(&serde_json::json!({
                            "is_login": false,
                            "error": e.to_string()
                        }))?;
                    } else {
                        user_warning!("登录凭证请求失败: {}", e);
                        user_info!("请执行 `bt auth login` 重新登录");
                    }
                }
            }
        }
        AuthCommands::Logout => {
            if let Ok(cookies) = crate::auth::cookies::read_cookies() {
                let _ = crate::api::passport::logout(&cookies);
            }
            crate::auth::cookies::delete_cookies()?;
            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "message": "已成功退出登录并清除本地凭证"
                }))?;
            } else {
                user_success!("已成功退出登录并清除本地凭证");
            }
        }
        AuthCommands::Refresh => {
            let mut cookies = crate::auth::cookies::read_cookies()?;
            let (need_refresh, _) = crate::api::passport::check_cookie_need_refresh(&cookies)?;
            if !need_refresh {
                if is_json() {
                    print_json(&serde_json::json!({
                        "need_refresh": false,
                        "message": "Cookie 无需刷新"
                    }))?;
                } else {
                    user_info!("Cookie 当前状态良好，无需刷新");
                }
                return Ok(());
            }

            user_info!("检测到 Cookie 需要刷新，正在刷新...");
            crate::api::passport::refresh_cookie(&mut cookies)?;
            if is_json() {
                print_json(&serde_json::json!({
                    "need_refresh": true,
                    "success": true,
                    "message": "Cookie 刷新成功"
                }))?;
            } else {
                user_success!("Cookie 刷新成功并已保存！");
            }
        }
    }
    Ok(())
}
