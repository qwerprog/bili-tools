use crate::api::comment::{CommentPageData, ReplyItem, SubReplyPageData};
use crate::ui::components::{Cell, Column, InfoCard, print_table, wrap_to_width};
use crate::ui::theme::{BILI_CYAN, BILI_PINK};
use crate::utils::string::{display_width, format_timestamp, truncate_to_width};
use crossterm::style::{Color, Stylize};
use std::fmt::Write as _;

/// 构建当前页需要展示的根评论序列；第 1 页优先展示置顶评论并按 `rpid` 去重
pub fn build_root_entries(data: &CommentPageData, page: u32) -> Vec<(&ReplyItem, bool)> {
    let mut entries = Vec::with_capacity(data.replies.len() + 1);
    let mut pinned_rpid = None;

    if page == 1
        && let Some(pinned) = data.pinned_reply()
    {
        pinned_rpid = Some(pinned.rpid);
        entries.push((pinned, true));
    }

    for item in &data.replies {
        if pinned_rpid.is_some_and(|p| p == item.rpid) {
            continue;
        }
        entries.push((item, false));
    }

    entries
}

pub fn format_author_badges(
    item: &ReplyItem,
    upper_mid: Option<i64>,
    is_pinned: bool,
    use_color: bool,
) -> String {
    let uname = item.author_name();
    let is_up = item.is_up(upper_mid);
    let level = item.author_level();

    let mut parts = Vec::new();
    if use_color {
        if is_up {
            parts.push(uname.with(BILI_PINK).bold().to_string());
        } else {
            parts.push(uname.bold().to_string());
        }
        if is_pinned {
            parts.push("[置顶]".with(BILI_PINK).bold().to_string());
        }
        if is_up {
            parts.push("[UP]".with(BILI_CYAN).bold().to_string());
        }
        if let Some(lv) = level {
            parts.push(format!("Lv.{lv}").dark_grey().to_string());
        }
    } else {
        parts.push(uname.to_string());
        if is_pinned {
            parts.push("[置顶]".to_string());
        }
        if is_up {
            parts.push("[UP]".to_string());
        }
        if let Some(lv) = level {
            parts.push(format!("Lv.{lv}"));
        }
    }
    parts.join(" ")
}

fn format_meta_tail(item: &ReplyItem) -> String {
    let ts = format_timestamp(item.ctime);
    match item.location_short() {
        Some(loc) => format!("{ts}  ·  {loc}"),
        None => ts,
    }
}

fn format_stats_line(item: &ReplyItem, use_color: bool) -> String {
    let like_label = if item.is_liked() {
        format!("♥ {} (已赞)", item.like)
    } else {
        format!("♥ {}", item.like)
    };

    let mut segs = Vec::new();
    if use_color {
        segs.push(like_label.with(BILI_PINK).to_string());
        if item.rcount > 0 {
            segs.push(format!("💬 {} 条回复", item.rcount).dark_grey().to_string());
        }
        if item.is_up_liked() {
            segs.push("[UP主觉得很赞]".with(BILI_PINK).to_string());
        }
        if item.is_up_replied() {
            segs.push("[UP回复]".with(BILI_CYAN).to_string());
        }
    } else {
        segs.push(like_label);
        if item.rcount > 0 {
            segs.push(format!("💬 {} 条回复", item.rcount));
        }
        if item.is_up_liked() {
            segs.push("[UP主觉得很赞]".to_string());
        }
        if item.is_up_replied() {
            segs.push("[UP回复]".to_string());
        }
    }

    segs.join("  ")
}

fn format_picture_tag(count: usize, use_color: bool) -> String {
    let tag = format!("[🖼 图片 ×{count}]");
    if use_color {
        tag.with(BILI_CYAN).to_string()
    } else {
        tag
    }
}

fn render_inline_sub_replies(
    out: &mut String,
    parent: &ReplyItem,
    upper_mid: Option<i64>,
    video_label: &str,
    width: usize,
    use_color: bool,
) {
    if parent.replies.is_empty() && parent.rcount == 0 {
        return;
    }

    let remaining = parent.rcount.saturating_sub(parent.replies.len() as u64);
    let sub_body_w = width.saturating_sub(11).max(28);

    if parent.replies.is_empty() {
        if use_color {
            let _ = writeln!(out, "      {}", "│".dark_grey());
            let _ = writeln!(
                out,
                "      {} {} {}",
                "╰─".dark_grey(),
                format!("↳ 共 {remaining} 条回复，查看全部:").dark_grey(),
                format!("bit video comments {video_label} --root {}", parent.rpid).with(BILI_CYAN)
            );
        } else {
            let _ = writeln!(out, "      │");
            let _ = writeln!(
                out,
                "      ╰─ ↳ 共 {remaining} 条回复，查看全部: bit video comments {video_label} --root {}",
                parent.rpid
            );
        }
        return;
    }

    if use_color {
        let _ = writeln!(out, "      {}", "│".dark_grey());
    } else {
        let _ = writeln!(out, "      │");
    }

    let child_count = parent.replies.len();
    for (si, child) in parent.replies.iter().enumerate() {
        let is_last_branch = si + 1 == child_count && remaining == 0;
        let branch_sym = if is_last_branch { "╰─" } else { "├─" };
        let cont_sym = if is_last_branch { "  " } else { "│ " };

        let author = format_author_badges(child, upper_mid, false, use_color);
        let tail = format_meta_tail(child);
        let like_str = if child.like > 0 || child.is_liked() {
            if child.is_liked() {
                format!("  ♥ {}(已赞)", child.like)
            } else {
                format!("  ♥ {}", child.like)
            }
        } else {
            String::new()
        };
        let rpid_str = format!("#{}", child.rpid);

        if use_color {
            let _ = writeln!(
                out,
                "      {} {}  {}  {}{}",
                branch_sym.dark_grey(),
                author,
                tail.as_str().dark_grey(),
                if like_str.is_empty() {
                    String::new()
                } else {
                    format!("{}  ", like_str.trim_start().with(BILI_PINK))
                },
                rpid_str.as_str().dark_grey()
            );
        } else {
            let _ = writeln!(
                out,
                "      {branch_sym} {author}  {tail}{like_str}  {rpid_str}"
            );
        }

        let clean = child.clean_message();
        let body_lines = if clean.is_empty() && !child.content.pictures.is_empty() {
            Vec::new()
        } else {
            wrap_to_width(&clean, sub_body_w)
        };
        for line in body_lines {
            if use_color {
                let _ = writeln!(out, "      {} {}", cont_sym.dark_grey(), line);
            } else {
                let _ = writeln!(out, "      {cont_sym} {line}");
            }
        }
        if !child.content.pictures.is_empty() {
            let pic_tag = format_picture_tag(child.content.pictures.len(), use_color);
            if use_color {
                let _ = writeln!(out, "      {} {}", cont_sym.dark_grey(), pic_tag);
            } else {
                let _ = writeln!(out, "      {cont_sym} {pic_tag}");
            }
        }
    }

    if remaining > 0 {
        if use_color {
            let _ = writeln!(
                out,
                "      {} {} {}",
                "╰─".dark_grey(),
                format!("↳ 还有 {remaining} 条回复，查看全部:").dark_grey(),
                format!("bit video comments {video_label} --root {}", parent.rpid).with(BILI_CYAN)
            );
        } else {
            let _ = writeln!(
                out,
                "      ╰─ ↳ 还有 {remaining} 条回复，查看全部: bit video comments {video_label} --root {}",
                parent.rpid
            );
        }
    }
}

/// 渲染默认可读的多行树状评论视图（保留完整换行与内嵌楼中楼）
pub fn render_comment_list(
    data: &CommentPageData,
    video_label: &str,
    page: u32,
    sort_label: &str,
    width: usize,
    use_color: bool,
) -> String {
    let entries = build_root_entries(data, page);
    if entries.is_empty() {
        return String::new();
    }

    let upper_mid = data.upper_mid();
    let total_pages = data.page.count.div_ceil(data.page.size.max(1)).max(1);
    let body_w = width.saturating_sub(8).max(32);
    let sep_w = width.saturating_sub(4).clamp(24, 96);
    let mut out = String::new();
    let _ = writeln!(out);

    let mut normal_idx = 0usize;
    for (idx, (reply, is_pinned)) in entries.iter().enumerate() {
        let index_badge = if *is_pinned {
            "📌".to_string()
        } else {
            normal_idx += 1;
            format!("#{normal_idx}")
        };
        let index_col = if use_color {
            if *is_pinned {
                index_badge.as_str().with(BILI_PINK).bold().to_string()
            } else {
                index_badge.as_str().with(BILI_CYAN).bold().to_string()
            }
        } else {
            index_badge.clone()
        };

        let author = format_author_badges(reply, upper_mid, *is_pinned, use_color);
        let tail = format_meta_tail(reply);
        let rpid_tag = format!("#{}", reply.rpid);

        if use_color {
            let _ = writeln!(
                out,
                "  {index_col}  {author}  {}  {}",
                tail.as_str().dark_grey(),
                rpid_tag.as_str().dark_grey()
            );
        } else {
            let _ = writeln!(out, "  {index_col}  {author}  {tail}  {rpid_tag}");
        }

        let clean = reply.clean_message();
        let body_lines = if clean.is_empty() && !reply.content.pictures.is_empty() {
            Vec::new()
        } else {
            wrap_to_width(&clean, body_w)
        };
        for seg in body_lines {
            let _ = writeln!(out, "      {seg}");
        }

        if !reply.content.pictures.is_empty() {
            let _ = writeln!(
                out,
                "      {}",
                format_picture_tag(reply.content.pictures.len(), use_color)
            );
        }

        let _ = writeln!(out, "      {}", format_stats_line(reply, use_color));

        render_inline_sub_replies(&mut out, reply, upper_mid, video_label, width, use_color);

        if idx + 1 < entries.len() {
            let rule = "─".repeat(sep_w);
            if use_color {
                let _ = writeln!(out, "  {}", rule.as_str().dark_grey());
            } else {
                let _ = writeln!(out, "  {rule}");
            }
        }
    }

    let _ = writeln!(out);
    let mut tips = Vec::new();
    if (page as u64) < total_pages {
        tips.push(format!("下一页: --page {}", page + 1));
    }
    tips.push("楼中楼: --root <RPID>".to_string());
    tips.push("交互浏览: -i".to_string());
    tips.push("紧凑表格: --compact".to_string());
    let tip_line = format!("💡 {sort_label} · {}", tips.join("  ·  "));
    if use_color {
        let _ = writeln!(out, "  {}", tip_line.as_str().dark_grey());
    } else {
        let _ = writeln!(out, "  {tip_line}");
    }

    out
}

/// 构建楼中楼详情页的根评论信息卡片
pub fn build_root_info_card(
    root_item: &ReplyItem,
    upper_mid: Option<i64>,
    reply_count: u64,
) -> InfoCard {
    let mut author_label = root_item.author_name().to_string();
    if root_item.is_up(upper_mid) {
        author_label.push_str(" [UP]");
    }
    if let Some(lv) = root_item.author_level() {
        author_label.push_str(&format!(" Lv.{lv}"));
    }

    let mut time_line = format_timestamp(root_item.ctime);
    if let Some(loc) = root_item.location_short() {
        time_line.push_str(&format!("  ·  {loc}"));
    }

    let mut stats = format!("{} 赞  ·  {} 条回复", root_item.like, reply_count);
    if root_item.is_up_liked() {
        stats.push_str("  ·  [UP主觉得很赞]");
    }

    let mut body = root_item.clean_message();
    if !root_item.content.pictures.is_empty() {
        let pic = format!("[🖼 图片 ×{}]", root_item.content.pictures.len());
        if body.is_empty() {
            body = pic;
        } else {
            body.push('\n');
            body.push_str(&pic);
        }
    }

    InfoCard::new("💬 根评论详情")
        .with_badge(format!("RPID {}", root_item.rpid), BILI_CYAN)
        .add_row("作者", Cell::bold_colored(author_label, Color::White))
        .add_row("互动", Cell::pink(stats))
        .add_plain_row("时间", time_line)
        .add_plain_row("内容", body)
}

/// 渲染指定根评论下的完整多行楼中楼列表
pub fn render_sub_reply_list(
    sub: &SubReplyPageData,
    root_rpid: i64,
    video_label: &str,
    page: u32,
    width: usize,
    use_color: bool,
) -> String {
    if sub.replies.is_empty() {
        return String::new();
    }

    let upper_mid = sub.upper_mid();
    let wrap_w = width.saturating_sub(6).max(32);
    let total_pages = sub.page.count.div_ceil(sub.page.size.max(20)).max(1);
    let mut out = String::new();
    let _ = writeln!(out);

    for (idx, r) in sub.replies.iter().enumerate() {
        let author = format_author_badges(r, upper_mid, false, use_color);
        let tail = format_meta_tail(r);
        let like_label = if r.is_liked() {
            format!("♥ {} (已赞)", r.like)
        } else {
            format!("♥ {}", r.like)
        };
        let rpid_tag = format!("#{}", r.rpid);

        if use_color {
            let _ = writeln!(
                out,
                "  {}  {}  {}  {}  {}",
                format!("#{}", idx + 1).with(BILI_CYAN).bold(),
                author,
                like_label.as_str().with(BILI_PINK),
                tail.as_str().dark_grey(),
                rpid_tag.as_str().dark_grey()
            );
        } else {
            let _ = writeln!(
                out,
                "  #{}  {author}  {like_label}  {tail}  {rpid_tag}",
                idx + 1
            );
        }

        let clean = r.clean_message();
        let body_lines = if clean.is_empty() && !r.content.pictures.is_empty() {
            Vec::new()
        } else {
            wrap_to_width(&clean, wrap_w)
        };
        for seg in body_lines {
            let _ = writeln!(out, "      {seg}");
        }
        if !r.content.pictures.is_empty() {
            let _ = writeln!(
                out,
                "      {}",
                format_picture_tag(r.content.pictures.len(), use_color)
            );
        }
        if r.is_up_liked() {
            let badge = if use_color {
                "[UP主觉得很赞]".with(BILI_PINK).to_string()
            } else {
                "[UP主觉得很赞]".to_string()
            };
            let _ = writeln!(out, "      {badge}");
        }
        let _ = writeln!(out);
    }

    if (page as u64) < total_pages {
        let tip = format!(
            "💡 下一页楼中楼: bit video comments {video_label} --root {root_rpid} --page {}  ·  交互浏览: -i",
            page + 1
        );
        if use_color {
            let _ = writeln!(out, "  {}", tip.as_str().dark_grey());
        } else {
            let _ = writeln!(out, "  {tip}");
        }
    }

    out
}

/// 紧凑单行表格模式 (`--compact`)
pub fn print_comments_compact_table(data: &CommentPageData) {
    let cols = [
        Column::right("RPID", 14),
        Column::left("用户", 16),
        Column::right("赞", 6),
        Column::right("回复", 6),
        Column::left("时间", 16),
        Column::left("评论内容", 36),
    ];
    let rows: Vec<Vec<Cell>> = data
        .replies
        .iter()
        .map(|r| {
            let msg = r.clean_message().replace('\n', " ");
            vec![
                Cell::cyan(r.rpid.to_string()),
                Cell::plain(truncate_to_width(r.author_name(), 16)),
                Cell::pink(r.like.to_string()),
                Cell::plain(r.rcount.to_string()),
                Cell::muted(format_timestamp(r.ctime)),
                Cell::plain(truncate_to_width(msg.trim(), 36)),
            ]
        })
        .collect();
    print_table(&cols, &rows);
    let _ = display_width("");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::comment::{
        CommentPageInfo, CommentUpper, ReplyContent, ReplyControl, ReplyLevelInfo, ReplyMember,
        ReplyPicture, ReplyUpAction,
    };

    fn sample_comment_page() -> CommentPageData {
        let pinned = ReplyItem {
            rpid: 1001,
            oid: 79677524,
            mid: 400880294,
            rcount: 0,
            like: 99,
            action: 0,
            ctime: 1700000000,
            member: Some(ReplyMember {
                uname: "QwerProg".to_string(),
                mid: "400880294".to_string(),
                level_info: ReplyLevelInfo { current_level: 6 },
            }),
            content: ReplyContent {
                message: "置顶说明第一行\n置顶说明第二行".to_string(),
                pictures: vec![ReplyPicture {
                    img_src: "https://example.com/1.png".to_string(),
                    img_width: 100,
                    img_height: 100,
                }],
            },
            reply_control: Some(ReplyControl {
                location: Some("IP属地：江苏".to_string()),
                time_desc: None,
                up_reply: false,
            }),
            up_action: None,
            replies: vec![],
            ..Default::default()
        };

        let normal = ReplyItem {
            rpid: 2002,
            oid: 79677524,
            mid: 555,
            rcount: 5,
            like: 12,
            action: 1,
            ctime: 1700000100,
            member: Some(ReplyMember {
                uname: "测试读者".to_string(),
                mid: "555".to_string(),
                level_info: ReplyLevelInfo { current_level: 5 },
            }),
            content: ReplyContent {
                message: "这是一条非常长的一级评论，用于确认默认终端展示模式不会在 36 列就提前截断导致内容缺失".to_string(),
                pictures: vec![],
            },
            reply_control: Some(ReplyControl {
                location: Some("IP属地：广东".to_string()),
                time_desc: None,
                up_reply: true,
            }),
            up_action: Some(ReplyUpAction {
                like: true,
                reply: true,
            }),
            replies: vec![ReplyItem {
                rpid: 2003,
                oid: 79677524,
                mid: 400880294,
                root: 2002,
                parent: 2002,
                like: 3,
                ctime: 1700000200,
                member: Some(ReplyMember {
                    uname: "QwerProg".to_string(),
                    mid: "400880294".to_string(),
                    level_info: ReplyLevelInfo { current_level: 6 },
                }),
                content: ReplyContent {
                    message: "这是一条同样不会被截断的楼中楼完整长回复内容，包含足够多的字符以验证完整多行换行渲染".to_string(),
                    pictures: vec![],
                },
                reply_control: Some(ReplyControl {
                    location: Some("IP属地：新加坡".to_string()),
                    time_desc: None,
                    up_reply: false,
                }),
                ..Default::default()
            }],
            ..Default::default()
        };

        CommentPageData {
            page: CommentPageInfo {
                num: 1,
                size: 20,
                count: 25,
                acount: 30,
            },
            upper: Some(CommentUpper {
                mid: 400880294,
                top: Some(Box::new(pinned.clone())),
            }),
            top: None,
            // Intentionally duplicate pinned rpid in replies to verify deduplication
            replies: vec![pinned, normal],
            hots: vec![],
            raw: serde_json::Value::Null,
        }
    }

    #[test]
    fn build_root_entries_dedups_pinned_only_on_first_page() {
        let data = sample_comment_page();
        let p1 = build_root_entries(&data, 1);
        assert_eq!(p1.len(), 2);
        assert_eq!(p1[0].0.rpid, 1001);
        assert!(p1[0].1);
        assert_eq!(p1[1].0.rpid, 2002);
        assert!(!p1[1].1);

        let p2 = build_root_entries(&data, 2);
        assert_eq!(p2.len(), 2);
        assert!(!p2[0].1);
    }

    #[test]
    fn render_comment_list_preserves_multiline_badges_and_full_subreplies() {
        let data = sample_comment_page();
        let rendered = render_comment_list(&data, "BV1xx", 1, "按热度", 100, false);
        let stripped = rendered.replace(['\n', ' '], "");

        assert!(rendered.contains("[置顶]"));
        assert!(rendered.contains("[UP]"));
        assert!(rendered.contains("Lv.6"));
        assert!(rendered.contains("江苏"));
        assert!(rendered.contains("[🖼 图片 ×1]"));
        assert!(rendered.contains("置顶说明第一行\n      置顶说明第二行"));
        assert!(stripped.contains("不会在36列就提前截断导致内容缺失"));
        assert!(rendered.contains("♥ 12 (已赞)"));
        assert!(rendered.contains("[UP主觉得很赞]"));
        assert!(rendered.contains("[UP回复]"));
        assert!(stripped.contains("以验证完整多行换行渲染"));
        assert!(rendered.contains("还有 4 条回复，查看全部: bit video comments BV1xx --root 2002"));
        assert!(rendered.contains("下一页: --page 2"));
    }
}
