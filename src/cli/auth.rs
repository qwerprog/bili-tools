use crate::auth::LoginOptions;
use crate::auth::session::{SessionState, inspect_session};
use crate::cli::args::AuthCommands;
use crate::cli::output::{is_json, is_quiet, print_json, print_spacer};
use crate::error::{BiliLiveError, Result};
use crate::{user_info, user_success, user_warning};

pub fn handle_auth(cmd: AuthCommands) -> Result<()> {
    match cmd {
        AuthCommands::Login {
            method,
            sessdata,
            bili_jct,
            dede_user_id,
        } => {
            if !is_json() {
                user_info!("开始登录流程...");
            }
            crate::auth::start_login_with_options(LoginOptions {
                method,
                sessdata,
                bili_jct,
                dede_user_id,
            })?;
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
            match inspect_session()? {
                SessionState::NoCookies => {
                    if is_json() {
                        print_json(&serde_json::json!({
                            "is_login": false,
                            "message": "未找到本地 Cookie"
                        }))?;
                    } else {
                        user_warning!("未检测到登录信息，请执行 `bt auth login` 登录");
                    }
                }
                SessionState::Unauthenticated { .. } => {
                    if is_json() {
                        print_json(&serde_json::json!({
                            "is_login": false,
                            "message": "账号未登录或登录凭证已过期"
                        }))?;
                    } else {
                        user_warning!("登录凭证已失效，请执行 `bt auth login` 重新登录");
                    }
                }
                SessionState::Authenticated { mut cookies, info } => {
                    let data = &info["data"];
                    let uname = data["uname"].as_str().unwrap_or("未知");
                    let mid = data["mid"].as_i64().unwrap_or(0);
                    let coins = data["money"].as_f64().unwrap_or(0.0);
                    let level = data["level_info"]["current_level"].as_i64().unwrap_or(0);
                    let vip_status = if data["vipStatus"].as_i64().unwrap_or(0) == 1 {
                        "大会员"
                    } else {
                        "普通用户"
                    };

                    // 若历史 cookie 缺失 dede_user_id 或 room_id，自动补全保存
                    let mut dirty = false;
                    if cookies.dede_user_id.is_empty() && mid > 0 {
                        cookies.dede_user_id = mid.to_string();
                        dirty = true;
                    }
                    if cookies.room_id <= 0
                        && mid > 0
                        && let Ok(rid) = crate::api::passport::get_roomid_by_mid(mid)
                        && rid > 0
                    {
                        cookies.room_id = rid;
                        dirty = true;
                    }
                    if dirty {
                        let _ = cookies.save();
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
                            "room_id": cookies.room_id,
                            "raw": info
                        }))?;
                    } else {
                        print_spacer();
                        let vip_cell = if data["vipStatus"].as_i64().unwrap_or(0) == 1 {
                            crate::ui::components::Cell::bold_colored(
                                vip_status,
                                crate::ui::theme::BILI_PINK,
                            )
                        } else {
                            crate::ui::components::Cell::plain(vip_status)
                        };

                        let mut card = crate::ui::components::InfoCard::new("账号信息")
                            .with_badge("● 已登录", crossterm::style::Color::Green)
                            .add_row(
                                "昵称",
                                crate::ui::components::Cell::bold_colored(
                                    uname,
                                    crossterm::style::Color::White,
                                ),
                            )
                            .add_row("UID", crate::ui::components::Cell::cyan(mid.to_string()))
                            .add_row(
                                "等级",
                                crate::ui::components::Cell::pink(format!("Lv.{level}")),
                            )
                            .add_plain_row("硬币", format!("{coins}"))
                            .add_row("身份", vip_cell);

                        if cookies.room_id > 0 {
                            card = card.add_row(
                                "直播间",
                                crate::ui::components::Cell::cyan(cookies.room_id.to_string()),
                            );
                        }
                        card.print();
                        print_spacer();
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
            let mut cookies = match crate::auth::cookies::read_cookies() {
                Ok(c) => c,
                Err(BiliLiveError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(BiliLiveError::Auth(
                        "未检测到登录信息，请执行 `bt auth login` 进行登录".to_string(),
                    ));
                }
                Err(e) => return Err(e),
            };
            if cookies.sessdata.trim().is_empty() {
                return Err(BiliLiveError::Auth(
                    "未检测到登录信息，请执行 `bt auth login` 进行登录".to_string(),
                ));
            }
            if cookies.bili_jct.trim().is_empty() {
                return Err(BiliLiveError::Auth(
                    "当前登录凭据缺少 bili_jct (CSRF Token)，无法刷新 Cookie".to_string(),
                ));
            }
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
        AuthCommands::Daily => {
            let cookies = crate::auth::ensure_login()?;
            let exp = crate::api::passport::get_exp_reward(&cookies)?;
            let sign = crate::api::passport::do_live_sign(&cookies)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "exp_reward": exp,
                    "live_sign": sign
                }))?;
                return Ok(());
            }

            if is_quiet() {
                return Ok(());
            }

            let mut tasks = Vec::new();
            if exp.email {
                tasks.push("邮箱");
            }
            if exp.tel {
                tasks.push("手机");
            }
            if exp.safe_question {
                tasks.push("密保");
            }
            if exp.identify_card {
                tasks.push("实名");
            }
            let account_tasks = if tasks.is_empty() {
                "无".to_string()
            } else {
                tasks.join(" · ")
            };

            let live_sign_cell = if sign.signed {
                let reward = if sign.text.is_empty() {
                    sign.message.clone()
                } else {
                    sign.text.clone()
                };
                crate::ui::components::Cell::colored(
                    format!("✔ 签到成功 ({reward})"),
                    crossterm::style::Color::Green,
                )
            } else if sign.already_signed {
                crate::ui::components::Cell::cyan("✔ 今日已签到")
            } else if sign.offline {
                crate::ui::components::Cell::muted(format!("○ {}", sign.message))
            } else {
                crate::ui::components::Cell::plain(&sign.message)
            };

            let mut card = crate::ui::components::InfoCard::new("📅 每日经验与直播签到")
                .with_badge(
                    format!("今日已获 {}/65 经验", exp.today_exp),
                    crate::ui::theme::BILI_PINK,
                )
                .add_plain_row(
                    "每日登录",
                    if exp.login { "✔ 已完成 (+5)" } else { "○ 未完成" },
                )
                .add_plain_row(
                    "每日观看",
                    if exp.watch { "✔ 已完成 (+5)" } else { "○ 未完成" },
                )
                .add_plain_row("每日投币", format!("{}/50 经验", exp.coins))
                .add_plain_row(
                    "每日分享",
                    if exp.share { "✔ 已完成 (+5)" } else { "○ 未完成" },
                )
                .add_row("账号任务", crate::ui::components::Cell::muted(account_tasks))
                .add_row("直播签到", live_sign_cell);

            if let (Some(had), Some(all)) = (sign.had_sign_days, sign.all_days) {
                card = card.add_plain_row("连续签到", format!("{had}/{all} 天"));
            }

            print_spacer();
            card.print();
            print_spacer();
        }
    }
    Ok(())
}
