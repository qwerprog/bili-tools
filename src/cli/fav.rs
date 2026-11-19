use crate::cli::args::{FavCommands, FavExportFormat};
use crate::cli::output::{confirm_action, is_json, print_json, print_spacer};
use crate::error::{BiliLiveError, Result};
use crate::ui::components::{Cell, Column, print_section_header, print_table};
use crate::utils::string::format_num;
use crate::{user_info, user_success, user_warning};
use crossterm::style::Color;
use std::io::Write;

pub(crate) fn resolve_own_fav_mid() -> Result<(i64, crate::auth::cookies::Cookies)> {
    let mut cookies = crate::auth::ensure_login()?;
    if cookies.dede_user_id.trim().is_empty() {
        let info = crate::api::passport::get_user_info(&cookies)?;
        if info["code"].as_i64() != Some(0) {
            return Err(BiliLiveError::Auth(format!(
                "获取当前用户信息失败: {}",
                info["message"].as_str().unwrap_or("未知错误")
            )));
        }
        let is_login = info["data"]["isLogin"].as_bool().unwrap_or(false);
        let mid = info["data"]["mid"].as_i64().unwrap_or(0);
        if !is_login || mid <= 0 {
            return Err(BiliLiveError::Auth(
                "当前登录状态无效，请重新执行 `bt auth login`".to_string(),
            ));
        }
        cookies.dede_user_id = mid.to_string();
        let _ = cookies.save();
        return Ok((mid, cookies));
    }

    let mid = cookies
        .dede_user_id
        .trim()
        .parse::<i64>()
        .map_err(|e| BiliLiveError::Parse(format!("UID解析失败: {e}")))?;
    Ok((mid, cookies))
}

pub fn handle_fav(cmd: FavCommands) -> Result<()> {
    match cmd {
        FavCommands::List { mid } => {
            let (target_mid, cookies) = match mid {
                Some(ref input) => {
                    let resolved = crate::utils::id::resolve_user(input, false)?;
                    let c = crate::auth::cookies::read_cookies().ok();
                    (resolved, c)
                }
                None => {
                    let (m, c) = resolve_own_fav_mid()?;
                    (m, Some(c))
                }
            };

            let data = crate::api::fav::get_user_fav_folders(target_mid, cookies.as_ref())?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            print_spacer();
            print_section_header("收藏夹列表", Some(&format!("UID: {target_mid}")));
            print_spacer();

            match data["list"].as_array() {
                Some(list) if !list.is_empty() => {
                    let cols = [
                        Column::left("收藏夹 ID", 14),
                        Column::left("收藏夹标题", 28),
                        Column::right("视频数", 8),
                        Column::left("可见性", 8),
                    ];
                    let rows: Vec<Vec<Cell>> = list
                        .iter()
                        .map(|folder| {
                            let id = folder["id"].as_i64().unwrap_or(0);
                            let title = folder["title"].as_str().unwrap_or("-");
                            let media_count = folder["media_count"].as_i64().unwrap_or(0);
                            let attr = folder["attr"].as_i64().unwrap_or(0);
                            let is_public = (attr & 1) == 0;
                            let attr_cell = if is_public {
                                Cell::colored("公开", Color::Green)
                            } else {
                                Cell::muted("私密")
                            };
                            vec![
                                Cell::cyan(id.to_string()),
                                Cell::plain(title),
                                Cell::pink(media_count.to_string()),
                                attr_cell,
                            ]
                        })
                        .collect();
                    print_table(&cols, &rows);
                    print_spacer();
                }
                _ => {
                    user_info!("暂无收藏夹");
                }
            }
        }

        FavCommands::Show {
            media_id,
            page,
            page_size,
        } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let resolved_id = crate::utils::id::resolve_fav(&media_id, false)?;
            let data = crate::api::fav::get_fav_resources(
                resolved_id,
                page,
                page_size,
                cookies.as_ref(),
            )?;

            if is_json() {
                print_json(&data)?;
                return Ok(());
            }

            let folder_title = data["info"]["title"].as_str().unwrap_or("收藏夹");
            let total_count = data["info"]["media_count"].as_i64().unwrap_or(0);

            print_spacer();
            print_section_header(
                &format!("收藏夹 · \"{folder_title}\""),
                Some(&format!("共 {total_count} 条 · 第 {page} 页")),
            );
            print_spacer();

            match data["medias"].as_array() {
                Some(medias) if !medias.is_empty() => {
                    let cols = [
                        Column::left("BV号", 14),
                        Column::left("标题", 36),
                        Column::left("UP主", 16),
                        Column::right("播放", 10),
                    ];
                    let rows: Vec<Vec<Cell>> = medias
                        .iter()
                        .map(|item| {
                            let bvid = item["bvid"].as_str().unwrap_or("-");
                            let title = item["title"].as_str().unwrap_or("-");
                            let upper = item["upper"]["name"].as_str().unwrap_or("-");
                            let play = format_num(&item["cnt_info"]["play"]);

                            vec![
                                Cell::cyan(bvid),
                                Cell::plain(title),
                                Cell::plain(upper),
                                Cell::pink(play),
                            ]
                        })
                        .collect();
                    print_table(&cols, &rows);
                    print_spacer();
                }
                _ => {
                    user_warning!("此收藏夹暂无内容");
                }
            }
        }

        FavCommands::Export {
            media_id,
            output,
            format,
        } => {
            let cookies = crate::auth::cookies::read_cookies().ok();
            let resolved_id = crate::utils::id::resolve_fav(&media_id, false)?;
            if !is_json() && output.is_some() {
                user_info!("正在拉取收藏夹 (ID: {}) 全部数据...", resolved_id);
            }

            let (folder_info, medias) =
                crate::api::fav::get_all_fav_resources(resolved_id, cookies.as_ref())?;

            let content = match format {
                FavExportFormat::Csv => {
                    let mut csv = String::from("bvid,aid,title,upper_name,play,duration,link\n");
                    for item in &medias {
                        let bvid = item["bvid"].as_str().unwrap_or("");
                        let aid = item["id"].as_i64().unwrap_or(0);
                        let title = item["title"].as_str().unwrap_or("");
                        let upper = item["upper"]["name"].as_str().unwrap_or("");
                        let play = item["cnt_info"]["play"].as_i64().unwrap_or(0);
                        let duration = item["duration"].as_i64().unwrap_or(0);
                        let link = format!("https://www.bilibili.com/video/{}", bvid);

                        csv.push_str(&format!(
                            "{},{},{},{},{},{},{}\n",
                            escape_csv(bvid),
                            aid,
                            escape_csv(title),
                            escape_csv(upper),
                            play,
                            duration,
                            escape_csv(&link)
                        ));
                    }
                    csv
                }
                FavExportFormat::Text => {
                    let mut txt = String::new();
                    let folder_title = folder_info["title"].as_str().unwrap_or("收藏夹");
                    txt.push_str(&format!("# 收藏夹: {}\n\n", folder_title));
                    for item in &medias {
                        let bvid = item["bvid"].as_str().unwrap_or("");
                        let title = item["title"].as_str().unwrap_or("");
                        let upper = item["upper"]["name"].as_str().unwrap_or("");
                        let link = format!("https://www.bilibili.com/video/{}", bvid);
                        txt.push_str(&format!("- {} | {} ({}) : {}\n", title, upper, bvid, link));
                    }
                    txt
                }
                FavExportFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
                    "info": folder_info,
                    "count": medias.len(),
                    "medias": medias
                }))?,
            };

            if let Some(path) = output {
                if let Some(parent) = std::path::Path::new(&path).parent()
                    && !parent.as_os_str().is_empty()
                {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, content.as_bytes())?;
                if is_json() {
                    print_json(&serde_json::json!({
                        "media_id": resolved_id,
                        "count": medias.len(),
                        "file": path,
                        "format": format.as_str()
                    }))?;
                } else {
                    user_success!("成功导出 {} 条收藏视频到: {}", medias.len(), path);
                }
            } else if is_json() {
                match format {
                    FavExportFormat::Json => {
                        print_json(&serde_json::json!({
                            "info": folder_info,
                            "count": medias.len(),
                            "medias": medias
                        }))?;
                    }
                    FavExportFormat::Csv | FavExportFormat::Text => {
                        print_json(&serde_json::json!({
                            "media_id": resolved_id,
                            "format": format.as_str(),
                            "info": folder_info,
                            "count": medias.len(),
                            "medias": medias,
                            "content": content
                        }))?;
                    }
                }
            } else {
                let mut stdout = std::io::stdout().lock();
                if let Err(e) = stdout.write_all(content.as_bytes())
                    && e.kind() != std::io::ErrorKind::BrokenPipe
                {
                    return Err(e.into());
                }
                if !content.ends_with('\n')
                    && let Err(e) = writeln!(stdout)
                    && e.kind() != std::io::ErrorKind::BrokenPipe
                {
                    return Err(e.into());
                }
            }
        }

        FavCommands::Add { media_id, video } => {
            let resolved_id = crate::utils::id::resolve_fav(&media_id, false)?;
            let cookies = crate::auth::ensure_csrf()?;
            let aid = crate::api::video::resolve_video_aid(&video, Some(&cookies))?;
            crate::api::fav::deal_fav_resource(&cookies, aid, &[resolved_id], &[])?;

            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "media_id": resolved_id,
                    "aid": aid,
                    "video": video,
                    "message": "已添加到收藏夹"
                }))?;
            } else {
                user_success!("已将 {} (av{}) 添加到收藏夹 {}", video, aid, resolved_id);
            }
        }

        FavCommands::Remove { media_id, video } => {
            let resolved_id = crate::utils::id::resolve_fav(&media_id, false)?;
            let cookies = crate::auth::ensure_csrf()?;
            let aid = crate::api::video::resolve_video_aid(&video, Some(&cookies))?;
            crate::api::fav::deal_fav_resource(&cookies, aid, &[], &[resolved_id])?;

            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "media_id": resolved_id,
                    "aid": aid,
                    "video": video,
                    "message": "已从收藏夹移除"
                }))?;
            } else {
                user_success!("已将 {} (av{}) 从收藏夹 {} 移除", video, aid, resolved_id);
            }
        }

        FavCommands::Clean { media_id } => {
            let resolved_id = crate::utils::id::resolve_fav(&media_id, false)?;
            if !confirm_action(&format!(
                "确定要清理收藏夹 {resolved_id} 中的所有失效视频吗？"
            ))? {
                user_info!("已取消操作");
                return Ok(());
            }
            let cookies = crate::auth::ensure_csrf()?;
            crate::api::fav::clean_fav_folder(&cookies, resolved_id)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "media_id": resolved_id,
                    "message": "已清理失效收藏内容"
                }))?;
            } else {
                user_success!("已清理收藏夹 {} 中的失效视频", resolved_id);
            }
        }

        FavCommands::Create { title, private } => {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(BiliLiveError::Input("收藏夹名称不能为空".to_string()));
            }
            let cookies = crate::auth::ensure_csrf()?;
            let data = crate::api::fav::create_fav_folder(&cookies, trimmed, private)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "id": data["id"],
                    "fid": data["fid"],
                    "title": data["title"],
                    "private": private,
                    "data": data
                }))?;
            } else {
                let new_id = data["id"].as_i64().unwrap_or(0);
                user_success!("已创建收藏夹 \"{}\" (ID: {})", trimmed, new_id);
            }
        }

        FavCommands::Delete { media_id } => {
            let resolved_id = crate::utils::id::resolve_fav(&media_id, false)?;
            if !confirm_action(&format!(
                "确定要删除收藏夹 {resolved_id} 吗？此操作不可撤销"
            ))? {
                user_info!("已取消操作");
                return Ok(());
            }
            let cookies = crate::auth::ensure_csrf()?;
            crate::api::fav::delete_fav_folder(&cookies, resolved_id)?;

            if is_json() {
                print_json(&serde_json::json!({
                    "success": true,
                    "media_id": resolved_id,
                    "message": "已删除收藏夹"
                }))?;
            } else {
                user_success!("已删除收藏夹 {}", resolved_id);
            }
        }
    }

    Ok(())
}

fn escape_csv(val: &str) -> String {
    format!("\"{}\"", val.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    #[test]
    fn resolve_own_fav_mid_backfills_empty_dede_user_id() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct Guard(Option<String>);
        impl Drop for Guard {
            fn drop(&mut self) {
                crate::utils::paths::set_cookie_file_override(None);
                crate::cli::output::init_output(false, false, false, false, None);
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                        None => std::env::remove_var("BT_API_BASE_URL"),
                    }
                }
            }
        }
        let _guard = Guard(std::env::var("BT_API_BASE_URL").ok());

        let tmp = tempfile::TempDir::new().unwrap();
        let cookie_path = tmp.path().join("cookies.json");
        let cookies = crate::auth::cookies::Cookies {
            sessdata: "sess".into(),
            bili_jct: "jct".into(),
            dede_user_id: String::new(),
            buvid3: "b3".into(),
            buvid4: "b4".into(),
            ..Default::default()
        };
        cookies.save_to(&cookie_path).unwrap();
        crate::utils::paths::set_cookie_file_override(Some(cookie_path.clone()));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            assert!(first.starts_with("GET /x/web-interface/nav "));
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let body = r#"{"code":0,"data":{"isLogin":true,"mid":987654}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        crate::cli::output::init_output(true, true, false, true, Some(cookie_path.clone()));
        unsafe {
            std::env::set_var("BT_API_BASE_URL", &base);
        }

        let (mid, updated) = resolve_own_fav_mid().unwrap();
        handle.join().unwrap();
        assert_eq!(mid, 987654);
        assert_eq!(updated.dede_user_id, "987654");
    }

    #[test]
    fn fav_clean_and_delete_require_confirmation_in_json_mode_without_yes() {
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::cli::output::init_output(true, true, false, true, None);
        let err_clean = handle_fav(FavCommands::Clean {
            media_id: "100".into(),
        })
        .unwrap_err();
        let err_del = handle_fav(FavCommands::Delete {
            media_id: "100".into(),
        })
        .unwrap_err();
        crate::cli::output::init_output(false, false, false, false, None);
        assert!(matches!(err_clean, BiliLiveError::Input(_)));
        assert!(matches!(err_del, BiliLiveError::Input(_)));
        assert!(
            err_clean
                .to_string()
                .contains("Confirmation required in non-interactive mode; pass --yes")
        );
    }

    #[test]
    fn fav_export_writes_csv_file_and_json_stdout_mode_succeeds() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        struct Guard(Option<String>);
        impl Drop for Guard {
            fn drop(&mut self) {
                crate::cli::output::init_output(false, false, false, false, None);
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("BT_API_BASE_URL", v),
                        None => std::env::remove_var("BT_API_BASE_URL"),
                    }
                }
            }
        }
        let _guard = Guard(std::env::var("BT_API_BASE_URL").ok());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 0 && line != "\r\n" {
                    line.clear();
                }
                let body = r#"{"code":0,"data":{"info":{"id":100,"title":"TestFav"},"medias":[{"id":1,"bvid":"BV1test","title":"V1","upper":{"name":"UP1"},"cnt_info":{"play":100},"duration":60}],"has_more":false}}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        let tmp = tempfile::TempDir::new().unwrap();
        let csv_path = tmp.path().join("fav.csv");
        crate::cli::output::init_output(true, true, false, true, None);
        unsafe {
            std::env::set_var("BT_API_BASE_URL", &base);
        }

        handle_fav(FavCommands::Export {
            media_id: "ml100".into(),
            output: Some(csv_path.display().to_string()),
            format: FavExportFormat::Csv,
        })
        .unwrap();
        handle_fav(FavCommands::Export {
            media_id: "100".into(),
            output: None,
            format: FavExportFormat::Csv,
        })
        .unwrap();
        handle.join().unwrap();

        let written = std::fs::read_to_string(&csv_path).unwrap();
        assert!(written.starts_with("bvid,aid,title,upper_name,play,duration,link\n"));
        assert!(written.contains("BV1test"));
    }
}
