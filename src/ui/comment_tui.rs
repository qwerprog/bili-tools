use crate::api::comment::{
    CommentPageData, CommentPageInfo, ReplyContent, ReplyItem, ReplyMember, SubReplyPageData,
};
use crate::auth::cookies::Cookies;
use crate::cli::args::CommentSort;
use crate::cli::output::is_json;
use crate::error::{BiliLiveError, Result};
use crate::ui::comment_fmt::{build_root_entries, format_author_badges};
use crate::ui::components::{
    apply_selection_bg, slice_input_window, truncate_ansi_to_width, wrap_to_width,
};
use crate::ui::theme::{BILI_CYAN, BILI_PINK, color_enabled};
use crate::utils::string::{display_width, format_timestamp, pad_to_width, truncate_to_width};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::Stylize;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{execute, queue};
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TOAST_TTL: Duration = Duration::from_secs(4);
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SELECT_BG_SGR: &str = "\x1b[48;2;46;52;68m";

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct CommentTuiContext {
    pub aid: i64,
    pub video_label: String,
    pub video_title: String,
    pub initial_sort: CommentSort,
    pub initial_page: u32,
    pub initial_root: Option<i64>,
    pub cookies: Option<Cookies>,
}

/// RAII 终端守卫：离开作用域自动退出备用屏幕并恢复标准终端状态
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode().map_err(BiliLiveError::Io)?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, Clear(ClearType::All), Hide)
            .map_err(BiliLiveError::Io)?;
        crate::utils::interrupt::set_alt_screen_active(true);
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        crate::utils::interrupt::set_alt_screen_active(false);
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CommentThread {
    pub root: ReplyItem,
    pub is_pinned: bool,
    pub expanded: bool,
    pub loading_sub: bool,
    pub sub_replies: Vec<ReplyItem>,
    pub sub_page: u32,
    pub sub_total: u64,
    pub sub_fetched: bool,
    pub sub_exhausted: bool,
}

impl CommentThread {
    pub fn from_root(root: ReplyItem, is_pinned: bool) -> Self {
        let sub_replies = root.replies.clone();
        let sub_total = root.rcount;
        let expanded = !sub_replies.is_empty();
        Self {
            root,
            is_pinned,
            expanded,
            loading_sub: false,
            sub_replies,
            sub_page: 0,
            sub_total,
            sub_fetched: false,
            sub_exhausted: false,
        }
    }

    pub fn has_more_subs(&self) -> bool {
        if self.sub_exhausted {
            return false;
        }
        self.sub_total > self.sub_replies.len() as u64
            || (!self.sub_fetched && self.root.rcount > self.sub_replies.len() as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeRef {
    Root { thread_idx: usize },
    SubReply { thread_idx: usize, sub_idx: usize },
    LoadMoreSubs { thread_idx: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposeTarget {
    NewComment,
    Reply {
        root_rpid: i64,
        parent_rpid: i64,
        reply_to_uname: String,
        is_sub_reply: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InputMode {
    Browse,
    Search {
        chars: Vec<char>,
        cursor: usize,
        origin_node: usize,
        origin_scroll: usize,
    },
    SearchNav {
        origin_node: usize,
        origin_scroll: usize,
    },
    Compose {
        target: ComposeTarget,
        chars: Vec<char>,
        cursor: usize,
    },
    ConfirmDelete {
        rpid: i64,
        preview: String,
    },
    Help,
}

#[derive(Debug, Clone)]
pub(crate) struct ToastMessage {
    pub text: String,
    pub is_error: bool,
    pub created_at: Instant,
}

#[derive(Debug)]
enum TuiEvent {
    PageLoaded {
        req_id: u64,
        page: u32,
        sort: CommentSort,
        result: std::result::Result<CommentPageData, String>,
    },
    RootLoaded {
        req_id: u64,
        root_rpid: i64,
        page: u32,
        result: std::result::Result<SubReplyPageData, String>,
    },
    SubRepliesLoaded {
        req_id: u64,
        thread_rpid: i64,
        page: u32,
        append: bool,
        result: std::result::Result<SubReplyPageData, String>,
    },
    LikeDone {
        rpid: i64,
        cancel: bool,
        result: std::result::Result<(), String>,
    },
    PostDone {
        target: ComposeTarget,
        draft: String,
        sent_message: String,
        result: std::result::Result<(i64, Option<ReplyItem>), String>,
    },
    DeleteDone {
        rpid: i64,
        result: std::result::Result<(), String>,
    },
}

pub(crate) fn format_reply_message(target: &ComposeTarget, raw_input: &str) -> String {
    let trimmed = raw_input.trim();
    match target {
        ComposeTarget::Reply {
            is_sub_reply: true,
            reply_to_uname,
            ..
        } if !trimmed.starts_with("回复 @") => {
            format!("回复 @{reply_to_uname} :{trimmed}")
        }
        _ => trimmed.to_string(),
    }
}

pub(crate) fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    if needle.is_empty() {
        return true;
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

pub(crate) fn reply_matches_query(item: &ReplyItem, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    contains_case_insensitive(item.author_name(), q)
        || contains_case_insensitive(&item.clean_message(), q)
        || item
            .location_short()
            .is_some_and(|loc| contains_case_insensitive(loc, q))
        || item.rpid.to_string().contains(q)
}

pub(crate) fn highlight_query_spans(
    text: &str,
    query: &str,
    is_sel: bool,
    use_color: bool,
) -> String {
    let q = query.trim();
    if !use_color || q.is_empty() {
        return if use_color && is_sel {
            text.white().bold().to_string()
        } else {
            text.to_string()
        };
    }

    let q_lower: Vec<char> = q.chars().flat_map(|c| c.to_lowercase()).collect();
    if q_lower.is_empty() {
        return if is_sel {
            text.white().bold().to_string()
        } else {
            text.to_string()
        };
    }

    let chars: Vec<char> = text.chars().collect();
    // 展开每个原始字符对应的 to_lowercase() 字符序列起始位置，确保大小写展开长度不等时也能安全对齐原字符切片
    let mut lower_chars: Vec<char> = Vec::with_capacity(chars.len());
    let mut lower_to_orig: Vec<usize> = Vec::with_capacity(chars.len());
    for (orig_idx, &ch) in chars.iter().enumerate() {
        for lc in ch.to_lowercase() {
            lower_chars.push(lc);
            lower_to_orig.push(orig_idx);
        }
    }

    let mut matched = vec![false; chars.len()];
    if lower_chars.len() >= q_lower.len() {
        let mut i = 0usize;
        while i + q_lower.len() <= lower_chars.len() {
            if lower_chars[i..i + q_lower.len()] == q_lower[..] {
                let start_orig = lower_to_orig[i];
                let end_orig = lower_to_orig[i + q_lower.len() - 1];
                for flag in &mut matched[start_orig..=end_orig] {
                    *flag = true;
                }
                i += q_lower.len();
            } else {
                i += 1;
            }
        }
    }

    let mut out = String::new();
    let mut idx = 0usize;
    while idx < chars.len() {
        let is_hit = matched[idx];
        let start = idx;
        while idx < chars.len() && matched[idx] == is_hit {
            idx += 1;
        }
        let seg: String = chars[start..idx].iter().collect();
        if is_hit {
            out.push_str(&seg.yellow().bold().underlined().to_string());
        } else if is_sel {
            out.push_str(&seg.white().bold().to_string());
        } else {
            out.push_str(&seg);
        }
    }
    out
}

pub(crate) struct CommentTuiState {
    pub video_label: String,
    pub video_title: String,
    pub root_mode_rpid: Option<i64>,
    pub sort: CommentSort,
    pub page: u32,
    pub page_info: CommentPageInfo,
    pub upper_mid: Option<i64>,
    pub threads: Vec<CommentThread>,
    pub selected_node: usize,
    pub scroll_top_line: usize,
    pub loading_page: bool,
    pub pending_action: bool,
    pub page_req_id: u64,
    pub sub_req_id: u64,
    pub mode: InputMode,
    pub search_query: String,
    pub toast: Option<ToastMessage>,
    pub self_uid: Option<i64>,
    pub can_write: bool,
    pub spinner_tick: usize,
}

impl CommentTuiState {
    pub fn new(ctx: &CommentTuiContext) -> Self {
        let self_uid = ctx
            .cookies
            .as_ref()
            .and_then(|c| c.dede_user_id.trim().parse::<i64>().ok())
            .filter(|&uid| uid > 0);
        let can_write = ctx
            .cookies
            .as_ref()
            .is_some_and(|c| !c.sessdata.is_empty() && !c.bili_jct.is_empty());

        Self {
            video_label: ctx.video_label.clone(),
            video_title: ctx.video_title.clone(),
            root_mode_rpid: ctx.initial_root,
            sort: ctx.initial_sort,
            page: ctx.initial_page,
            page_info: CommentPageInfo::default(),
            upper_mid: None,
            threads: Vec::new(),
            selected_node: 0,
            scroll_top_line: 0,
            loading_page: true,
            pending_action: false,
            page_req_id: 1,
            sub_req_id: 0,
            mode: InputMode::Browse,
            search_query: String::new(),
            toast: None,
            self_uid,
            can_write,
            spinner_tick: 0,
        }
    }

    pub fn set_toast(&mut self, text: impl Into<String>, is_error: bool) {
        self.toast = Some(ToastMessage {
            text: text.into(),
            is_error,
            created_at: Instant::now(),
        });
    }

    pub fn active_toast(&self) -> Option<&ToastMessage> {
        self.toast
            .as_ref()
            .filter(|t| t.created_at.elapsed() < TOAST_TTL)
    }

    pub fn flatten_nodes(&self) -> Vec<NodeRef> {
        let mut nodes = Vec::new();
        for (ti, th) in self.threads.iter().enumerate() {
            nodes.push(NodeRef::Root { thread_idx: ti });
            if th.expanded {
                for si in 0..th.sub_replies.len() {
                    nodes.push(NodeRef::SubReply {
                        thread_idx: ti,
                        sub_idx: si,
                    });
                }
                if self.root_mode_rpid.is_none() && (th.has_more_subs() || th.loading_sub) {
                    nodes.push(NodeRef::LoadMoreSubs { thread_idx: ti });
                }
            }
        }
        nodes
    }

    pub fn expand_threads_matching_query(&mut self) {
        let q = self.search_query.trim();
        if q.is_empty() {
            return;
        }
        for th in &mut self.threads {
            if !th.expanded && th.sub_replies.iter().any(|sr| reply_matches_query(sr, q)) {
                th.expanded = true;
            }
        }
    }

    pub fn matching_node_indices(&self) -> Vec<usize> {
        let q = self.search_query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        self.flatten_nodes()
            .into_iter()
            .enumerate()
            .filter_map(|(idx, node)| {
                let hit = match node {
                    NodeRef::Root { thread_idx } => self
                        .threads
                        .get(thread_idx)
                        .is_some_and(|th| reply_matches_query(&th.root, q)),
                    NodeRef::SubReply {
                        thread_idx,
                        sub_idx,
                    } => self
                        .threads
                        .get(thread_idx)
                        .and_then(|th| th.sub_replies.get(sub_idx))
                        .is_some_and(|sr| reply_matches_query(sr, q)),
                    NodeRef::LoadMoreSubs { .. } => false,
                };
                hit.then_some(idx)
            })
            .collect()
    }

    pub fn search_match_count(&self) -> usize {
        self.matching_node_indices().len()
    }

    pub fn jump_to_first_match_from(&mut self, start_node: usize) -> bool {
        self.expand_threads_matching_query();
        let matches = self.matching_node_indices();
        if matches.is_empty() {
            return false;
        }
        let target = matches
            .iter()
            .copied()
            .find(|&idx| idx >= start_node)
            .unwrap_or(matches[0]);
        self.selected_node = target;
        true
    }

    pub fn jump_next_match(&mut self) -> bool {
        let matches = self.matching_node_indices();
        if matches.is_empty() {
            return false;
        }
        let target = matches
            .iter()
            .copied()
            .find(|&idx| idx > self.selected_node)
            .unwrap_or(matches[0]);
        self.selected_node = target;
        true
    }

    pub fn jump_prev_match(&mut self) -> bool {
        let matches = self.matching_node_indices();
        if matches.is_empty() {
            return false;
        }
        let target = matches
            .iter()
            .rev()
            .copied()
            .find(|&idx| idx < self.selected_node)
            .unwrap_or(*matches.last().unwrap());
        self.selected_node = target;
        true
    }

    pub fn current_match_ordinal(&self) -> Option<(usize, usize)> {
        let matches = self.matching_node_indices();
        if matches.is_empty() {
            return None;
        }
        matches
            .iter()
            .position(|&idx| idx == self.selected_node)
            .map(|pos| (pos + 1, matches.len()))
    }

    pub fn clamp_selection(&mut self) {
        let len = self.flatten_nodes().len();
        if len == 0 {
            self.selected_node = 0;
        } else if self.selected_node >= len {
            self.selected_node = len - 1;
        }
    }

    pub fn selected_node_ref(&self) -> Option<NodeRef> {
        self.flatten_nodes().get(self.selected_node).copied()
    }

    pub fn selected_item(&self) -> Option<(&ReplyItem, usize, bool)> {
        match self.selected_node_ref()? {
            NodeRef::Root { thread_idx } => self
                .threads
                .get(thread_idx)
                .map(|th| (&th.root, thread_idx, false)),
            NodeRef::SubReply {
                thread_idx,
                sub_idx,
            } => self
                .threads
                .get(thread_idx)
                .and_then(|th| th.sub_replies.get(sub_idx))
                .map(|sr| (sr, thread_idx, true)),
            NodeRef::LoadMoreSubs { .. } => None,
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.flatten_nodes().len();
        if len == 0 {
            self.selected_node = 0;
            return;
        }
        let cur = self.selected_node as isize;
        let next = (cur + delta).clamp(0, (len - 1) as isize) as usize;
        self.selected_node = next;
    }

    pub fn select_root_of_current(&mut self) {
        let Some(node) = self.selected_node_ref() else {
            return;
        };
        let thread_idx = match node {
            NodeRef::Root { thread_idx }
            | NodeRef::SubReply { thread_idx, .. }
            | NodeRef::LoadMoreSubs { thread_idx } => thread_idx,
        };
        if let Some(th) = self.threads.get_mut(thread_idx) {
            th.expanded = false;
        }
        let nodes = self.flatten_nodes();
        if let Some(pos) = nodes
            .iter()
            .position(|n| matches!(n, NodeRef::Root { thread_idx: ti } if *ti == thread_idx))
        {
            self.selected_node = pos;
        }
    }

    pub fn apply_page_loaded(
        &mut self,
        req_id: u64,
        page: u32,
        sort: CommentSort,
        result: std::result::Result<CommentPageData, String>,
    ) {
        if req_id != self.page_req_id {
            return;
        }
        self.loading_page = false;
        match result {
            Ok(data) => {
                let entries = build_root_entries(&data, page);
                if entries.is_empty() && page > 1 && !self.threads.is_empty() {
                    self.set_toast("已经是最后一页", false);
                    return;
                }
                self.page = page;
                self.sort = sort;
                self.page_info = data.page.clone();
                self.upper_mid = data.upper_mid();
                self.threads = entries
                    .into_iter()
                    .map(|(item, pinned)| CommentThread::from_root(item.clone(), pinned))
                    .collect();
                self.selected_node = 0;
                self.scroll_top_line = 0;
            }
            Err(err) => {
                self.set_toast(format!("加载评论失败: {err}"), true);
            }
        }
    }

    pub fn apply_root_loaded(
        &mut self,
        req_id: u64,
        root_rpid: i64,
        page: u32,
        result: std::result::Result<SubReplyPageData, String>,
    ) {
        if req_id != self.page_req_id {
            return;
        }
        self.loading_page = false;
        match result {
            Ok(sub) => {
                if sub.replies.is_empty() && page > 1 && !self.threads.is_empty() {
                    self.set_toast("已经是最后一页楼中楼", false);
                    return;
                }
                self.page = page;
                self.page_info = sub.page.clone();
                self.upper_mid = sub.upper_mid();
                let root_item = sub.root.unwrap_or_else(|| ReplyItem {
                    rpid: root_rpid,
                    rcount: sub.page.count,
                    ..Default::default()
                });
                let total = sub.page.count.max(root_item.rcount);
                let exhausted = sub.replies.is_empty()
                    || (page as u64 * sub.page.size.max(20) >= total && total > 0);
                let thread = CommentThread {
                    root: root_item,
                    is_pinned: false,
                    expanded: true,
                    loading_sub: false,
                    sub_replies: sub.replies,
                    sub_page: page,
                    sub_total: total,
                    sub_fetched: true,
                    sub_exhausted: exhausted,
                };
                self.threads = vec![thread];
                self.selected_node = 0;
                self.scroll_top_line = 0;
            }
            Err(err) => {
                self.set_toast(format!("加载楼中楼失败: {err}"), true);
            }
        }
    }

    pub fn apply_sub_replies_loaded(
        &mut self,
        req_id: u64,
        thread_rpid: i64,
        page: u32,
        append: bool,
        result: std::result::Result<SubReplyPageData, String>,
    ) {
        if req_id != self.sub_req_id {
            return;
        }
        let Some(th) = self.threads.iter_mut().find(|t| t.root.rpid == thread_rpid) else {
            return;
        };
        th.loading_sub = false;
        match result {
            Ok(sub) => {
                if let Some(upper) = sub.upper_mid() {
                    self.upper_mid = Some(upper);
                }
                if !append {
                    th.sub_replies.clear();
                }
                let mut existing: HashSet<i64> = th.sub_replies.iter().map(|r| r.rpid).collect();
                let mut newly_added = 0usize;
                for item in sub.replies {
                    if existing.insert(item.rpid) {
                        th.sub_replies.push(item);
                        newly_added += 1;
                    }
                }
                th.expanded = true;
                th.sub_fetched = true;
                th.sub_page = page;
                let total = sub.page.count.max(th.root.rcount);
                th.sub_total = total;
                let page_sz = sub.page.size.max(20);
                if newly_added == 0
                    || (total > 0 && th.sub_replies.len() as u64 >= total)
                    || (page as u64 * page_sz >= total && total > 0)
                {
                    th.sub_exhausted = true;
                }
                self.clamp_selection();
            }
            Err(err) => {
                self.set_toast(format!("展开楼中楼失败: {err}"), true);
            }
        }
    }

    pub fn apply_like_done(
        &mut self,
        rpid: i64,
        cancel: bool,
        result: std::result::Result<(), String>,
    ) {
        self.pending_action = false;
        match result {
            Ok(()) => {
                self.update_reply_by_rpid(rpid, |item| {
                    if cancel {
                        if item.action == 1 {
                            item.like = item.like.saturating_sub(1);
                        }
                        item.action = 0;
                    } else {
                        if item.action != 1 {
                            item.like = item.like.saturating_add(1);
                        }
                        item.action = 1;
                    }
                });
                self.set_toast(
                    if cancel {
                        "已取消点赞"
                    } else {
                        "✔ 已点赞评论"
                    },
                    false,
                );
            }
            Err(err) => {
                self.set_toast(format!("点赞失败: {err}"), true);
            }
        }
    }

    pub fn apply_post_done(
        &mut self,
        aid: i64,
        target: ComposeTarget,
        draft: String,
        sent_message: String,
        result: std::result::Result<(i64, Option<ReplyItem>), String>,
    ) {
        self.pending_action = false;
        match result {
            Ok((rpid, created_opt)) => {
                let mut item = created_opt.unwrap_or_default();
                if item.rpid <= 0 {
                    item.rpid = rpid;
                }
                if item.oid <= 0 {
                    item.oid = aid;
                }
                if item.mid <= 0 {
                    item.mid = self.self_uid.unwrap_or(0);
                }
                if item.ctime <= 0 {
                    item.ctime = now_unix_secs();
                }
                if item.content.message.trim().is_empty() {
                    item.content = ReplyContent {
                        message: sent_message,
                        ..item.content
                    };
                }
                if item.member.is_none() {
                    item.member = Some(ReplyMember {
                        uname: "我".to_string(),
                        mid: self.self_uid.map(|u| u.to_string()).unwrap_or_default(),
                        ..Default::default()
                    });
                }

                match target {
                    ComposeTarget::NewComment => {
                        let existing_idx =
                            self.threads.iter().position(|t| t.root.rpid == item.rpid);
                        let target_idx = if let Some(idx) = existing_idx {
                            idx
                        } else {
                            let insert_idx =
                                usize::from(self.threads.first().is_some_and(|t| t.is_pinned));
                            self.threads
                                .insert(insert_idx, CommentThread::from_root(item, false));
                            self.page_info.count = self.page_info.count.saturating_add(1);
                            self.page_info.acount = self.page_info.acount.saturating_add(1);
                            insert_idx
                        };
                        let inserted_rpid = self.threads[target_idx].root.rpid;
                        if !self.search_query.trim().is_empty()
                            && !self.flatten_nodes().iter().any(|n| {
                                matches!(n, NodeRef::Root { thread_idx } if self.threads[*thread_idx].root.rpid == inserted_rpid)
                            })
                        {
                            self.search_query.clear();
                        }
                        let nodes = self.flatten_nodes();
                        if let Some(pos) = nodes
                            .iter()
                            .position(|n| matches!(n, NodeRef::Root { thread_idx } if *thread_idx == target_idx))
                        {
                            self.selected_node = pos;
                        }
                        self.set_toast(format!("✔ 评论发表成功 (#{rpid})"), false);
                    }
                    ComposeTarget::Reply {
                        root_rpid,
                        parent_rpid,
                        ..
                    } => {
                        if item.root <= 0 {
                            item.root = root_rpid;
                        }
                        if item.parent <= 0 {
                            item.parent = parent_rpid;
                        }
                        let new_rpid = item.rpid;
                        if let Some(thread_idx) =
                            self.threads.iter().position(|t| t.root.rpid == root_rpid)
                        {
                            let th = &mut self.threads[thread_idx];
                            if !th.sub_replies.iter().any(|sr| sr.rpid == new_rpid) {
                                th.sub_replies.push(item);
                                th.root.rcount = th.root.rcount.saturating_add(1);
                                th.sub_total = th.sub_total.saturating_add(1);
                                self.page_info.acount = self.page_info.acount.saturating_add(1);
                            }
                            th.expanded = true;
                            let sub_idx = th
                                .sub_replies
                                .iter()
                                .position(|sr| sr.rpid == new_rpid)
                                .unwrap_or_else(|| th.sub_replies.len().saturating_sub(1));
                            if !self.search_query.trim().is_empty()
                                && !self.flatten_nodes().iter().any(|n| {
                                    matches!(
                                        n,
                                        NodeRef::SubReply {
                                            thread_idx: ti,
                                            sub_idx: si
                                        } if *ti == thread_idx && *si == sub_idx
                                    )
                                })
                            {
                                self.search_query.clear();
                            }
                            let nodes = self.flatten_nodes();
                            if let Some(pos) = nodes.iter().position(|n| {
                                matches!(
                                    n,
                                    NodeRef::SubReply {
                                        thread_idx: ti,
                                        sub_idx: si
                                    } if *ti == thread_idx && *si == sub_idx
                                )
                            }) {
                                self.selected_node = pos;
                            }
                        }
                        self.set_toast(format!("✔ 回复成功 (#{rpid})"), false);
                    }
                }
            }
            Err(err) => {
                self.set_toast(format!("发表失败: {err}"), true);
                self.mode = InputMode::Compose {
                    target,
                    cursor: draft.chars().count(),
                    chars: draft.chars().collect(),
                };
            }
        }
    }

    pub fn apply_delete_done(&mut self, rpid: i64, result: std::result::Result<(), String>) {
        self.pending_action = false;
        match result {
            Ok(()) => {
                self.remove_reply_by_rpid(rpid);
                self.clamp_selection();
                self.set_toast(format!("✔ 评论已删除 (#{rpid})"), false);
            }
            Err(err) => {
                self.set_toast(format!("删除失败: {err}"), true);
            }
        }
    }

    fn update_reply_by_rpid<F: FnMut(&mut ReplyItem)>(&mut self, rpid: i64, mut f: F) {
        for th in &mut self.threads {
            if th.root.rpid == rpid {
                f(&mut th.root);
            }
            for sr in &mut th.sub_replies {
                if sr.rpid == rpid {
                    f(sr);
                }
            }
        }
    }

    fn remove_reply_by_rpid(&mut self, rpid: i64) {
        let before = self.threads.len();
        self.threads.retain(|th| th.root.rpid != rpid);
        if self.threads.len() < before {
            self.page_info.acount = self.page_info.acount.saturating_sub(1);
            self.page_info.count = self.page_info.count.saturating_sub(1);
            return;
        }
        for th in &mut self.threads {
            let sub_before = th.sub_replies.len();
            th.sub_replies.retain(|sr| sr.rpid != rpid);
            if th.sub_replies.len() < sub_before {
                th.root.rcount = th.root.rcount.saturating_sub(1);
                th.sub_total = th.sub_total.saturating_sub(1);
                self.page_info.acount = self.page_info.acount.saturating_sub(1);
            }
        }
    }
}

/// 交互式终端评论浏览器 (`bit video comments <VIDEO> -i`)
pub fn run_comment_tui(ctx: CommentTuiContext) -> Result<()> {
    if is_json() {
        return Err(BiliLiveError::Input(
            "交互式评论浏览器不支持 --json 模式；请移除 --json 或改用默认输出".to_string(),
        ));
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(BiliLiveError::Input(
            "交互式评论浏览器需要交互式终端 (TTY)；管道环境请移除 -i".to_string(),
        ));
    }

    let _guard = TerminalGuard::enter()?;
    let (tx, rx) = mpsc::channel::<TuiEvent>();
    let mut state = CommentTuiState::new(&ctx);

    if let Some(root_rpid) = ctx.initial_root {
        spawn_fetch_root(
            &tx,
            ctx.aid,
            root_rpid,
            ctx.initial_page,
            ctx.cookies.clone(),
            state.page_req_id,
        );
    } else {
        spawn_fetch_page(
            &tx,
            ctx.aid,
            ctx.initial_page,
            ctx.initial_sort,
            ctx.cookies.clone(),
            state.page_req_id,
        );
    }

    let mut dirty = true;
    let mut last_spinner = Instant::now();

    loop {
        if crate::utils::interrupt::is_interrupted() {
            return Err(BiliLiveError::Interrupted);
        }

        while let Ok(ev) = rx.try_recv() {
            dirty = true;
            match ev {
                TuiEvent::PageLoaded {
                    req_id,
                    page,
                    sort,
                    result,
                } => state.apply_page_loaded(req_id, page, sort, result),
                TuiEvent::RootLoaded {
                    req_id,
                    root_rpid,
                    page,
                    result,
                } => state.apply_root_loaded(req_id, root_rpid, page, result),
                TuiEvent::SubRepliesLoaded {
                    req_id,
                    thread_rpid,
                    page,
                    append,
                    result,
                } => state.apply_sub_replies_loaded(req_id, thread_rpid, page, append, result),
                TuiEvent::LikeDone {
                    rpid,
                    cancel,
                    result,
                } => state.apply_like_done(rpid, cancel, result),
                TuiEvent::PostDone {
                    target,
                    draft,
                    sent_message,
                    result,
                } => state.apply_post_done(ctx.aid, target, draft, sent_message, result),
                TuiEvent::DeleteDone { rpid, result } => state.apply_delete_done(rpid, result),
            }
        }

        let any_loading = state.loading_page
            || state.pending_action
            || state.threads.iter().any(|t| t.loading_sub);
        if any_loading && last_spinner.elapsed() >= Duration::from_millis(100) {
            state.spinner_tick = (state.spinner_tick + 1) % SPINNER_FRAMES.len();
            last_spinner = Instant::now();
            dirty = true;
        }
        if state
            .toast
            .as_ref()
            .is_some_and(|t| t.created_at.elapsed() >= TOAST_TTL)
        {
            state.toast = None;
            dirty = true;
        }

        if dirty {
            render_tui_frame(&mut state)?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(40)).map_err(BiliLiveError::Io)? {
            match event::read().map_err(BiliLiveError::Io)? {
                Event::Resize(_, _) => {
                    dirty = true;
                }
                Event::Key(KeyEvent {
                    code,
                    modifiers,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    dirty = true;
                    if handle_key_event(&mut state, &ctx, &tx, code, modifiers) {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn handle_key_event(
    state: &mut CommentTuiState,
    ctx: &CommentTuiContext,
    tx: &mpsc::Sender<TuiEvent>,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    if matches!((code, modifiers), (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL)) {
        return true;
    }

    let mode = std::mem::replace(&mut state.mode, InputMode::Browse);
    match mode {
        InputMode::Help => {
            // 任意键返回浏览模式
            state.mode = InputMode::Browse;
            false
        }
        InputMode::ConfirmDelete { rpid, preview } => {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    if let Some(ref ck) = ctx.cookies {
                        state.pending_action = true;
                        let tx_c = tx.clone();
                        let ck_c = ck.clone();
                        let aid = ctx.aid;
                        std::thread::spawn(move || {
                            let res = crate::api::comment::delete_comment(&ck_c, aid, rpid)
                                .map_err(|e| e.to_string());
                            let _ = tx_c.send(TuiEvent::DeleteDone { rpid, result: res });
                        });
                    }
                    state.mode = InputMode::Browse;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    state.mode = InputMode::Browse;
                }
                _ => {
                    state.mode = InputMode::ConfirmDelete { rpid, preview };
                }
            }
            false
        }
        InputMode::Compose {
            target,
            mut chars,
            mut cursor,
        } => {
            match (code, modifiers) {
                (KeyCode::Esc, _) => {
                    state.mode = InputMode::Browse;
                    return false;
                }
                (KeyCode::Left, _) => {
                    cursor = cursor.saturating_sub(1);
                }
                (KeyCode::Right, _) => {
                    if cursor < chars.len() {
                        cursor += 1;
                    }
                }
                (KeyCode::Home, _) => cursor = 0,
                (KeyCode::End, _) => cursor = chars.len(),
                (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => cursor = 0,
                (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                    cursor = chars.len();
                }
                (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                    chars.drain(..cursor);
                    cursor = 0;
                }
                (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) => {
                    chars.truncate(cursor);
                }
                (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
                    while cursor > 0 && chars[cursor - 1].is_whitespace() {
                        cursor -= 1;
                        chars.remove(cursor);
                    }
                    while cursor > 0 && !chars[cursor - 1].is_whitespace() {
                        cursor -= 1;
                        chars.remove(cursor);
                    }
                }
                (KeyCode::Backspace, _) => {
                    if cursor > 0 {
                        cursor -= 1;
                        chars.remove(cursor);
                    }
                }
                (KeyCode::Delete, _) => {
                    if cursor < chars.len() {
                        chars.remove(cursor);
                    }
                }
                (KeyCode::Enter, _) => {
                    let raw: String = chars.iter().collect();
                    let trimmed = raw.trim().to_string();
                    if trimmed.is_empty() {
                        state.set_toast("评论内容不能为空", true);
                        state.mode = InputMode::Compose {
                            target,
                            chars,
                            cursor,
                        };
                        return false;
                    }
                    let Some(ref ck) = ctx.cookies else {
                        state.set_toast("请先执行 `bit auth login` 登录后再发言", true);
                        state.mode = InputMode::Browse;
                        return false;
                    };
                    let msg = format_reply_message(&target, &trimmed);
                    let (root_arg, parent_arg) = match &target {
                        ComposeTarget::NewComment => (None, None),
                        ComposeTarget::Reply {
                            root_rpid,
                            parent_rpid,
                            ..
                        } => (Some(*root_rpid), Some(*parent_rpid)),
                    };
                    state.pending_action = true;
                    let tx_c = tx.clone();
                    let ck_c = ck.clone();
                    let aid = ctx.aid;
                    std::thread::spawn(move || {
                        let sent_message = msg.clone();
                        let res = crate::api::comment::post_comment(
                            &ck_c, aid, &msg, root_arg, parent_arg,
                        )
                        .map(|v| {
                            let rpid = v["rpid"].as_i64().unwrap_or(0);
                            let created = v
                                .get("reply")
                                .and_then(|rv| serde_json::from_value::<ReplyItem>(rv.clone()).ok())
                                .filter(|r| r.rpid > 0 || rpid > 0);
                            (rpid, created)
                        })
                        .map_err(|e| e.to_string());
                        let _ = tx_c.send(TuiEvent::PostDone {
                            target,
                            draft: trimmed,
                            sent_message,
                            result: res,
                        });
                    });
                    state.mode = InputMode::Browse;
                    return false;
                }
                (KeyCode::Char(c), m)
                    if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                {
                    chars.insert(cursor, c);
                    cursor += 1;
                }
                _ => {}
            }
            state.mode = InputMode::Compose {
                target,
                chars,
                cursor,
            };
            false
        }
        InputMode::Search {
            mut chars,
            mut cursor,
            origin_node,
            origin_scroll,
        } => {
            let mut update_preview = false;
            match (code, modifiers) {
                (KeyCode::Esc, _) => {
                    state.search_query.clear();
                    state.selected_node = origin_node;
                    state.scroll_top_line = origin_scroll;
                    state.clamp_selection();
                    state.mode = InputMode::Browse;
                    return false;
                }
                (KeyCode::Enter, _) => {
                    let q: String = chars.iter().collect();
                    let trimmed = q.trim().to_string();
                    if trimmed.is_empty() {
                        state.search_query.clear();
                        state.selected_node = origin_node;
                        state.scroll_top_line = origin_scroll;
                        state.clamp_selection();
                        state.mode = InputMode::Browse;
                        return false;
                    }
                    state.search_query = trimmed;
                    state.expand_threads_matching_query();
                    if state.matching_node_indices().is_empty() {
                        state.set_toast(
                            format!("当前页未找到包含 \"{}\" 的评论", state.search_query),
                            true,
                        );
                        state.mode = InputMode::Search {
                            chars,
                            cursor,
                            origin_node,
                            origin_scroll,
                        };
                    } else {
                        if state.current_match_ordinal().is_none() {
                            state.jump_to_first_match_from(origin_node);
                        }
                        state.mode = InputMode::SearchNav {
                            origin_node,
                            origin_scroll,
                        };
                    }
                    return false;
                }
                (KeyCode::Left, _) => {
                    cursor = cursor.saturating_sub(1);
                }
                (KeyCode::Right, _) => {
                    if cursor < chars.len() {
                        cursor += 1;
                    }
                }
                (KeyCode::Home, _) => cursor = 0,
                (KeyCode::End, _) => cursor = chars.len(),
                (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => cursor = 0,
                (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                    cursor = chars.len();
                }
                (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                    chars.drain(..cursor);
                    cursor = 0;
                    update_preview = true;
                }
                (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) => {
                    chars.truncate(cursor);
                    update_preview = true;
                }
                (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
                    while cursor > 0 && chars[cursor - 1].is_whitespace() {
                        cursor -= 1;
                        chars.remove(cursor);
                    }
                    while cursor > 0 && !chars[cursor - 1].is_whitespace() {
                        cursor -= 1;
                        chars.remove(cursor);
                    }
                    update_preview = true;
                }
                (KeyCode::Backspace, _) => {
                    if cursor > 0 {
                        cursor -= 1;
                        chars.remove(cursor);
                        update_preview = true;
                    }
                }
                (KeyCode::Delete, _) => {
                    if cursor < chars.len() {
                        chars.remove(cursor);
                        update_preview = true;
                    }
                }
                (KeyCode::Char(c), m)
                    if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                {
                    chars.insert(cursor, c);
                    cursor += 1;
                    update_preview = true;
                }
                _ => {}
            }
            if update_preview {
                let q: String = chars.iter().collect();
                state.search_query = q.trim().to_string();
                if state.search_query.is_empty() {
                    state.selected_node = origin_node;
                    state.scroll_top_line = origin_scroll;
                    state.clamp_selection();
                } else {
                    state.jump_to_first_match_from(origin_node);
                }
            }
            state.mode = InputMode::Search {
                chars,
                cursor,
                origin_node,
                origin_scroll,
            };
            false
        }
        InputMode::SearchNav {
            origin_node,
            origin_scroll,
        } => {
            match (code, modifiers) {
                (KeyCode::Char('q'), _) => return true,
                (KeyCode::Esc, _) => {
                    state.search_query.clear();
                    state.selected_node = origin_node;
                    state.scroll_top_line = origin_scroll;
                    state.clamp_selection();
                    state.mode = InputMode::Browse;
                    return false;
                }
                (KeyCode::Enter, _) => {
                    state.search_query.clear();
                    state.clamp_selection();
                    state.mode = InputMode::Browse;
                    return false;
                }
                (KeyCode::Char('n'), _) | (KeyCode::Tab, _) => {
                    state.jump_next_match();
                }
                (KeyCode::Char('p'), _) | (KeyCode::Char('N'), _) | (KeyCode::BackTab, _) => {
                    state.jump_prev_match();
                }
                (KeyCode::Char('j'), _) | (KeyCode::Down, _) => state.move_selection(1),
                (KeyCode::Char('k'), _) | (KeyCode::Up, _) => state.move_selection(-1),
                (KeyCode::Char('/'), _) => {
                    let chars: Vec<char> = state.search_query.chars().collect();
                    let cursor = chars.len();
                    state.mode = InputMode::Search {
                        chars,
                        cursor,
                        origin_node,
                        origin_scroll,
                    };
                    return false;
                }
                _ => {}
            }
            state.mode = InputMode::SearchNav {
                origin_node,
                origin_scroll,
            };
            false
        }
        InputMode::Browse => handle_browse_key(state, ctx, tx, code, modifiers),
    }
}

fn handle_browse_key(
    state: &mut CommentTuiState,
    ctx: &CommentTuiContext,
    tx: &mpsc::Sender<TuiEvent>,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    match (code, modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => return true,
        (KeyCode::Char('/'), _) => {
            state.search_query.clear();
            state.mode = InputMode::Search {
                chars: Vec::new(),
                cursor: 0,
                origin_node: state.selected_node,
                origin_scroll: state.scroll_top_line,
            };
        }
        (KeyCode::Char('?'), _) => {
            state.mode = InputMode::Help;
        }
        (KeyCode::Char('j'), _) | (KeyCode::Down, _) => state.move_selection(1),
        (KeyCode::Char('k'), _) | (KeyCode::Up, _) => state.move_selection(-1),
        (KeyCode::PageDown, _) => state.move_selection(5),
        (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => state.move_selection(5),
        (KeyCode::PageUp, _) => state.move_selection(-5),
        (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => state.move_selection(-5),
        (KeyCode::Char('g'), _) | (KeyCode::Home, _) => state.selected_node = 0,
        (KeyCode::Char('G'), _) | (KeyCode::End, _) => {
            let len = state.flatten_nodes().len();
            state.selected_node = len.saturating_sub(1);
        }
        (KeyCode::Enter, _) | (KeyCode::Char(' '), _) => {
            trigger_expand_or_load_subs(state, ctx, tx);
        }
        (KeyCode::Char('l'), _) | (KeyCode::Right, _) => {
            trigger_expand_right(state, ctx, tx);
        }
        (KeyCode::Char('h'), _) | (KeyCode::Left, _) => {
            state.select_root_of_current();
        }
        (KeyCode::Char('n'), _) | (KeyCode::Char(']'), _) => {
            if !state.loading_page {
                let next_page = state.page.saturating_add(1);
                let total_pages = state
                    .page_info
                    .count
                    .div_ceil(state.page_info.size.max(1))
                    .max(1);
                if (next_page as u64) > total_pages && !state.threads.is_empty() {
                    state.set_toast("已经是最后一页", false);
                } else {
                    state.page_req_id += 1;
                    state.loading_page = true;
                    if let Some(root_rpid) = state.root_mode_rpid {
                        spawn_fetch_root(
                            tx,
                            ctx.aid,
                            root_rpid,
                            next_page,
                            ctx.cookies.clone(),
                            state.page_req_id,
                        );
                    } else {
                        spawn_fetch_page(
                            tx,
                            ctx.aid,
                            next_page,
                            state.sort,
                            ctx.cookies.clone(),
                            state.page_req_id,
                        );
                    }
                }
            }
        }
        (KeyCode::Char('p'), _) | (KeyCode::Char('['), _) => {
            if !state.loading_page {
                if state.page <= 1 {
                    state.set_toast("已经是第一页", false);
                } else {
                    let prev_page = state.page - 1;
                    state.page_req_id += 1;
                    state.loading_page = true;
                    if let Some(root_rpid) = state.root_mode_rpid {
                        spawn_fetch_root(
                            tx,
                            ctx.aid,
                            root_rpid,
                            prev_page,
                            ctx.cookies.clone(),
                            state.page_req_id,
                        );
                    } else {
                        spawn_fetch_page(
                            tx,
                            ctx.aid,
                            prev_page,
                            state.sort,
                            ctx.cookies.clone(),
                            state.page_req_id,
                        );
                    }
                }
            }
        }
        (KeyCode::Char('s'), _) => {
            if state.root_mode_rpid.is_some() {
                state.set_toast("楼中楼视图不支持切换排序", false);
            } else if !state.loading_page {
                let next_sort = match state.sort {
                    CommentSort::Hot => CommentSort::Time,
                    CommentSort::Time => CommentSort::Hot,
                };
                state.page_req_id += 1;
                state.loading_page = true;
                spawn_fetch_page(
                    tx,
                    ctx.aid,
                    1,
                    next_sort,
                    ctx.cookies.clone(),
                    state.page_req_id,
                );
            }
        }
        (KeyCode::Char('R'), _) | (KeyCode::F(5), _) => {
            if !state.loading_page {
                state.page_req_id += 1;
                state.loading_page = true;
                if let Some(root_rpid) = state.root_mode_rpid {
                    spawn_fetch_root(
                        tx,
                        ctx.aid,
                        root_rpid,
                        state.page,
                        ctx.cookies.clone(),
                        state.page_req_id,
                    );
                } else {
                    spawn_fetch_page(
                        tx,
                        ctx.aid,
                        state.page,
                        state.sort,
                        ctx.cookies.clone(),
                        state.page_req_id,
                    );
                }
            }
        }
        (KeyCode::Char('L'), _) => {
            if !state.can_write {
                state.set_toast("请先执行 `bit auth login` 登录后再点赞", true);
            } else if let Some((item, _, _)) = state.selected_item() {
                let rpid = item.rpid;
                let cancel = item.is_liked();
                if let Some(ref ck) = ctx.cookies {
                    state.pending_action = true;
                    let tx_c = tx.clone();
                    let ck_c = ck.clone();
                    let aid = ctx.aid;
                    std::thread::spawn(move || {
                        let res = crate::api::comment::like_comment(&ck_c, aid, rpid, cancel)
                            .map_err(|e| e.to_string());
                        let _ = tx_c.send(TuiEvent::LikeDone {
                            rpid,
                            cancel,
                            result: res,
                        });
                    });
                }
            }
        }
        (KeyCode::Char('r'), _) => {
            if !state.can_write {
                state.set_toast("请先执行 `bit auth login` 登录后再回复", true);
            } else if let Some((item, thread_idx, is_sub)) = state.selected_item() {
                let root_rpid = state.threads[thread_idx].root.rpid;
                state.mode = InputMode::Compose {
                    target: ComposeTarget::Reply {
                        root_rpid,
                        parent_rpid: item.rpid,
                        reply_to_uname: item.author_name().to_string(),
                        is_sub_reply: is_sub,
                    },
                    chars: Vec::new(),
                    cursor: 0,
                };
            }
        }
        (KeyCode::Char('c'), _) => {
            if !state.can_write {
                state.set_toast("请先执行 `bit auth login` 登录后再发表评论", true);
            } else if let Some(root_rpid) = state.root_mode_rpid {
                let root_uname = state
                    .threads
                    .first()
                    .map(|t| t.root.author_name().to_string())
                    .unwrap_or_else(|| "楼主".to_string());
                state.mode = InputMode::Compose {
                    target: ComposeTarget::Reply {
                        root_rpid,
                        parent_rpid: root_rpid,
                        reply_to_uname: root_uname,
                        is_sub_reply: false,
                    },
                    chars: Vec::new(),
                    cursor: 0,
                };
            } else {
                state.mode = InputMode::Compose {
                    target: ComposeTarget::NewComment,
                    chars: Vec::new(),
                    cursor: 0,
                };
            }
        }
        (KeyCode::Char('d'), _) => {
            if !state.can_write {
                state.set_toast("请先执行 `bit auth login` 登录后再操作", true);
            } else if let Some((item, _, _)) = state.selected_item() {
                if state.self_uid.is_some_and(|uid| uid == item.mid) {
                    let preview = truncate_to_width(&item.clean_message().replace('\n', " "), 24);
                    state.mode = InputMode::ConfirmDelete {
                        rpid: item.rpid,
                        preview,
                    };
                } else {
                    state.set_toast("仅可删除自己发表的评论", true);
                }
            }
        }
        _ => {}
    }
    false
}

fn trigger_expand_right(
    state: &mut CommentTuiState,
    ctx: &CommentTuiContext,
    tx: &mpsc::Sender<TuiEvent>,
) {
    let Some(node) = state.selected_node_ref() else {
        return;
    };
    match node {
        NodeRef::Root { thread_idx } => {
            let Some(th) = state.threads.get_mut(thread_idx) else {
                return;
            };
            if !th.sub_fetched && th.has_more_subs() {
                if th.loading_sub {
                    return;
                }
                let root_rpid = th.root.rpid;
                th.loading_sub = true;
                th.expanded = true;
                state.sub_req_id += 1;
                spawn_fetch_subs(
                    tx,
                    ctx.aid,
                    root_rpid,
                    1,
                    false,
                    ctx.cookies.clone(),
                    state.sub_req_id,
                );
            } else if !th.expanded && !th.sub_replies.is_empty() {
                th.expanded = true;
            } else if th.expanded && !th.sub_replies.is_empty() {
                state.move_selection(1);
            }
        }
        NodeRef::LoadMoreSubs { .. } | NodeRef::SubReply { .. } => {
            trigger_expand_or_load_subs(state, ctx, tx);
        }
    }
}

fn trigger_expand_or_load_subs(
    state: &mut CommentTuiState,
    ctx: &CommentTuiContext,
    tx: &mpsc::Sender<TuiEvent>,
) {
    let Some(node) = state.selected_node_ref() else {
        return;
    };
    match node {
        NodeRef::LoadMoreSubs { thread_idx } => {
            let Some(th) = state.threads.get_mut(thread_idx) else {
                return;
            };
            if th.loading_sub || !th.has_more_subs() {
                return;
            }
            let next_sub_page = if th.sub_fetched { th.sub_page + 1 } else { 1 };
            let append = th.sub_fetched;
            let root_rpid = th.root.rpid;
            th.loading_sub = true;
            state.sub_req_id += 1;
            spawn_fetch_subs(
                tx,
                ctx.aid,
                root_rpid,
                next_sub_page,
                append,
                ctx.cookies.clone(),
                state.sub_req_id,
            );
        }
        NodeRef::Root { thread_idx } => {
            let Some(th) = state.threads.get_mut(thread_idx) else {
                return;
            };
            if !th.sub_fetched && th.has_more_subs() {
                if th.loading_sub {
                    return;
                }
                let root_rpid = th.root.rpid;
                th.loading_sub = true;
                th.expanded = true;
                state.sub_req_id += 1;
                spawn_fetch_subs(
                    tx,
                    ctx.aid,
                    root_rpid,
                    1,
                    false,
                    ctx.cookies.clone(),
                    state.sub_req_id,
                );
            } else if !th.sub_replies.is_empty() {
                th.expanded = !th.expanded;
                state.clamp_selection();
            }
        }
        NodeRef::SubReply { thread_idx, .. } => {
            let Some(th) = state.threads.get_mut(thread_idx) else {
                return;
            };
            if !th.loading_sub && th.has_more_subs() {
                let next_sub_page = if th.sub_fetched { th.sub_page + 1 } else { 1 };
                let append = th.sub_fetched;
                let root_rpid = th.root.rpid;
                th.loading_sub = true;
                state.sub_req_id += 1;
                spawn_fetch_subs(
                    tx,
                    ctx.aid,
                    root_rpid,
                    next_sub_page,
                    append,
                    ctx.cookies.clone(),
                    state.sub_req_id,
                );
            }
        }
    }
}

fn spawn_fetch_page(
    tx: &mpsc::Sender<TuiEvent>,
    aid: i64,
    page: u32,
    sort: CommentSort,
    cookies: Option<Cookies>,
    req_id: u64,
) {
    let tx_c = tx.clone();
    std::thread::spawn(move || {
        let res = crate::api::comment::get_comments(aid, page, sort.as_api_str(), cookies.as_ref())
            .map_err(|e| e.to_string());
        let _ = tx_c.send(TuiEvent::PageLoaded {
            req_id,
            page,
            sort,
            result: res,
        });
    });
}

fn spawn_fetch_root(
    tx: &mpsc::Sender<TuiEvent>,
    aid: i64,
    root_rpid: i64,
    page: u32,
    cookies: Option<Cookies>,
    req_id: u64,
) {
    let tx_c = tx.clone();
    std::thread::spawn(move || {
        let res = crate::api::comment::get_sub_replies(aid, root_rpid, page, cookies.as_ref())
            .map_err(|e| e.to_string());
        let _ = tx_c.send(TuiEvent::RootLoaded {
            req_id,
            root_rpid,
            page,
            result: res,
        });
    });
}

fn spawn_fetch_subs(
    tx: &mpsc::Sender<TuiEvent>,
    aid: i64,
    thread_rpid: i64,
    page: u32,
    append: bool,
    cookies: Option<Cookies>,
    req_id: u64,
) {
    let tx_c = tx.clone();
    std::thread::spawn(move || {
        let res = crate::api::comment::get_sub_replies(aid, thread_rpid, page, cookies.as_ref())
            .map_err(|e| e.to_string());
        let _ = tx_c.send(TuiEvent::SubRepliesLoaded {
            req_id,
            thread_rpid,
            page,
            append,
            result: res,
        });
    });
}

fn render_tui_frame(state: &mut CommentTuiState) -> Result<()> {
    let (cols_u16, rows_u16) = crossterm::terminal::size().unwrap_or((88, 26));
    let cols = (cols_u16 as usize).max(40);
    let rows = (rows_u16 as usize).max(12);
    let use_color = color_enabled();

    let mut stdout = std::io::stdout();
    queue!(stdout, Hide, MoveTo(0, 0)).map_err(BiliLiveError::Io)?;

    // ── 顶部第 0 行：视频与分页信息 ──
    let sort_label = if state.root_mode_rpid.is_some() {
        "楼中楼"
    } else {
        match state.sort {
            CommentSort::Hot => "按热度",
            CommentSort::Time => "按时间",
        }
    };
    let total_pages = state
        .page_info
        .count
        .div_ceil(state.page_info.size.max(1))
        .max(1);
    let total_count = if state.root_mode_rpid.is_some() {
        state.page_info.count
    } else {
        state.page_info.acount.max(state.page_info.count)
    };
    let right_meta = format!(
        "{sort_label} · 第 {}/{} 页 · 共 {} 条",
        state.page, total_pages, total_count
    );
    let title_part = if state.video_title.is_empty() {
        state.video_label.clone()
    } else {
        format!("{} · {}", state.video_label, state.video_title)
    };
    let right_w = display_width(&right_meta);
    let left_budget = cols.saturating_sub(right_w + 17).max(12);
    let left_trunc = truncate_to_width(&title_part, left_budget);
    let used_left = display_width(" ✦ 评论浏览 · ") + display_width(&left_trunc);
    let gap = cols.saturating_sub(used_left + right_w + 2).max(1);

    if use_color {
        write!(
            stdout,
            "\x1b[K {} {} {} {}{}{}\r\n",
            "✦".with(BILI_PINK).bold(),
            "评论浏览".bold(),
            "·".dark_grey(),
            left_trunc.with(BILI_CYAN),
            " ".repeat(gap),
            right_meta.as_str().dark_grey()
        )
        .ok();
    } else {
        write!(
            stdout,
            "\x1b[K ✦ 评论浏览 · {left_trunc}{}{right_meta}\r\n",
            " ".repeat(gap)
        )
        .ok();
    }

    // ── 顶部第 1 行：分隔线 ──
    let sep = "─".repeat(cols);
    if use_color {
        write!(stdout, "{}\r\n", sep.as_str().dark_grey()).ok();
    } else {
        write!(stdout, "{sep}\r\n").ok();
    }

    // 底部占行数：Browse/ConfirmDelete 占 3 行（分隔线 + 状态/提示 + 快捷键），Compose/Search 占 4 行（输入框 3 行 + 快捷键）
    let footer_rows = match state.mode {
        InputMode::Compose { .. } | InputMode::Search { .. } => 4usize,
        _ => 3usize,
    };
    let header_rows = 2usize;
    let body_height = rows.saturating_sub(header_rows + footer_rows).max(3);

    // ── 中部视口 ──
    if matches!(state.mode, InputMode::Help) {
        render_help_lines(&mut stdout, body_height, cols, use_color);
    } else if state.loading_page && state.threads.is_empty() {
        let spin = SPINNER_FRAMES[state.spinner_tick % SPINNER_FRAMES.len()];
        let msg = format!("  {spin} 正在加载评论列表...");
        render_centered_placeholder(&mut stdout, body_height, &msg, use_color);
    } else if state.threads.is_empty() {
        render_centered_placeholder(&mut stdout, body_height, "  暂无评论", use_color);
    } else {
        let nodes = state.flatten_nodes();
        let mut visual_lines: Vec<String> = Vec::with_capacity(body_height * 2);
        let mut node_ranges: Vec<(usize, usize)> = Vec::new();
        let sel_idx = state.selected_node.min(nodes.len().saturating_sub(1));

        for (ni, node) in nodes.iter().enumerate() {
            let is_sel = ni == sel_idx;
            let start_line = visual_lines.len();
            render_node_lines(state, *node, is_sel, cols, use_color, &mut visual_lines);
            let end_line = visual_lines.len();
            node_ranges.push((start_line, end_line));
        }

        if let Some(&(sel_start, sel_end)) = node_ranges.get(sel_idx) {
            if sel_start < state.scroll_top_line {
                state.scroll_top_line = sel_start;
            } else if sel_end > state.scroll_top_line + body_height {
                state.scroll_top_line = sel_end.saturating_sub(body_height);
            }
        }
        let max_scroll = visual_lines.len().saturating_sub(body_height);
        if state.scroll_top_line > max_scroll {
            state.scroll_top_line = max_scroll;
        }

        let end_vis = (state.scroll_top_line + body_height).min(visual_lines.len());
        let slice = &visual_lines[state.scroll_top_line..end_vis];
        for line in slice {
            write!(stdout, "\x1b[K{line}\r\n").ok();
        }
        for _ in slice.len()..body_height {
            write!(stdout, "\x1b[K\r\n").ok();
        }
    }

    // ── 底部区域 ──
    let mut cursor_pos: Option<(u16, u16)> = None;
    match &state.mode {
        InputMode::Search { chars, cursor, .. } => {
            let box_inner = cols.saturating_sub(4).max(20);
            let match_cnt = state.search_match_count();
            let box_label = if chars.is_empty() {
                "🔍 搜索当前页评论 (昵称/正文/IP/RPID) ".to_string()
            } else if match_cnt == 0 {
                "🔍 搜索评论 (无匹配 · Esc 取消) ".to_string()
            } else if let Some((cur, total)) = state.current_match_ordinal() {
                format!("🔍 搜索评论 (命中 {cur}/{total} · Enter 开始 n/p 跳转 · Esc 取消) ")
            } else {
                format!("🔍 搜索评论 (匹配 {match_cnt} 条 · Enter 开始 n/p 跳转 · Esc 取消) ")
            };
            let label_trunc = truncate_to_width(&box_label, box_inner.saturating_sub(4));
            let top_dashes = box_inner.saturating_sub(display_width(&label_trunc) + 2);
            if use_color {
                write!(
                    stdout,
                    "\x1b[K {}{}{}\r\n",
                    "╭─ ".dark_grey(),
                    label_trunc.as_str().with(BILI_CYAN).bold(),
                    format!("{}╮", "─".repeat(top_dashes)).dark_grey()
                )
                .ok();
            } else {
                write!(
                    stdout,
                    "\x1b[K ╭─ {}{}╮\r\n",
                    label_trunc,
                    "─".repeat(top_dashes)
                )
                .ok();
            }

            let prompt_str = "/ ";
            let text_slots = box_inner
                .saturating_sub(display_width(prompt_str) + 2)
                .max(8);
            let (visible_input, cursor_col_offset) = slice_input_window(chars, *cursor, text_slots);
            let input_row_y = (rows.saturating_sub(3)) as u16;
            cursor_pos = Some((
                (3 + display_width(prompt_str) + cursor_col_offset) as u16,
                input_row_y,
            ));

            if chars.is_empty() {
                let ph = pad_to_width(
                    &truncate_to_width(
                        "输入关键词实时定位，Enter 后按 n/p 逐条跳转 · Esc 取消...",
                        text_slots,
                    ),
                    text_slots,
                );
                if use_color {
                    write!(
                        stdout,
                        "\x1b[K {} {}{} {}\r\n",
                        "│".dark_grey(),
                        prompt_str.with(BILI_CYAN).bold(),
                        ph.dark_grey(),
                        "│".dark_grey()
                    )
                    .ok();
                } else {
                    write!(stdout, "\x1b[K │ {prompt_str}{ph} │\r\n").ok();
                }
            } else {
                let val_padded = pad_to_width(&visible_input, text_slots);
                if use_color {
                    write!(
                        stdout,
                        "\x1b[K {} {}{} {}\r\n",
                        "│".dark_grey(),
                        prompt_str.with(BILI_CYAN).bold(),
                        val_padded,
                        "│".dark_grey()
                    )
                    .ok();
                } else {
                    write!(stdout, "\x1b[K │ {prompt_str}{val_padded} │\r\n").ok();
                }
            }

            if use_color {
                write!(
                    stdout,
                    "\x1b[K {}\r\n",
                    format!("╰{}╯", "─".repeat(box_inner)).dark_grey()
                )
                .ok();
            } else {
                write!(stdout, "\x1b[K ╰{}╯\r\n", "─".repeat(box_inner)).ok();
            }

            let legend = truncate_to_width(
                " Enter 进入 n/p 逐条跳转 · Ctrl+U 清空 · Esc 取消搜索",
                cols.saturating_sub(1),
            );
            if use_color {
                write!(stdout, "\x1b[K{}", legend.as_str().dark_grey()).ok();
            } else {
                write!(stdout, "\x1b[K{legend}").ok();
            }
        }
        InputMode::SearchNav { .. } => {
            if use_color {
                write!(stdout, "{}\r\n", sep.as_str().dark_grey()).ok();
            } else {
                write!(stdout, "{sep}\r\n").ok();
            }
            let total = state.search_match_count();
            let status_line = if let Some(toast) = state.active_toast() {
                format!(" {}", toast.text)
            } else if let Some((cur, total)) = state.current_match_ordinal() {
                format!(
                    " 🔍 搜索 \"{}\" [{cur}/{total}] · 按 Enter 定位到此评论",
                    state.search_query
                )
            } else {
                format!(
                    " 🔍 搜索 \"{}\" (共 {total} 条匹配) · 按 Enter 定位到光标评论",
                    state.search_query
                )
            };
            let status_trunc = truncate_to_width(&status_line, cols.saturating_sub(1));
            if use_color {
                let styled = if state.active_toast().is_some_and(|t| t.is_error) {
                    status_trunc.as_str().yellow().to_string()
                } else {
                    status_trunc.as_str().with(BILI_CYAN).bold().to_string()
                };
                write!(stdout, "\x1b[K{styled}\r\n").ok();
            } else {
                write!(stdout, "\x1b[K{status_trunc}\r\n").ok();
            }

            let legend = truncate_to_width(
                " n 下一个 · p 上一个 · j/k 微调 · Enter 跳到光标评论 · / 重搜 · Esc 退出搜索",
                cols.saturating_sub(1),
            );
            if use_color {
                write!(stdout, "\x1b[K{}", legend.as_str().dark_grey()).ok();
            } else {
                write!(stdout, "\x1b[K{legend}").ok();
            }
        }
        InputMode::Compose {
            target,
            chars,
            cursor,
        } => {
            let box_inner = cols.saturating_sub(4).max(20);
            let box_label = match target {
                ComposeTarget::NewComment => {
                    format!("💬 评论视频 · 新建一级评论 ({} 字) ", chars.len())
                }
                ComposeTarget::Reply { reply_to_uname, .. } => {
                    format!("💬 回复 @{reply_to_uname} (楼中楼 · {} 字) ", chars.len())
                }
            };
            let label_trunc = truncate_to_width(&box_label, box_inner.saturating_sub(4));
            let top_dashes = box_inner.saturating_sub(display_width(&label_trunc) + 2);
            if use_color {
                write!(
                    stdout,
                    "\x1b[K {}{}{}\r\n",
                    "╭─ ".dark_grey(),
                    label_trunc.as_str().with(BILI_PINK).bold(),
                    format!("{}╮", "─".repeat(top_dashes)).dark_grey()
                )
                .ok();
            } else {
                write!(
                    stdout,
                    "\x1b[K ╭─ {}{}╮\r\n",
                    label_trunc,
                    "─".repeat(top_dashes)
                )
                .ok();
            }

            let prompt_str = "❯ ";
            let text_slots = box_inner
                .saturating_sub(display_width(prompt_str) + 2)
                .max(8);
            let (visible_input, cursor_col_offset) = slice_input_window(chars, *cursor, text_slots);
            let input_row_y = (rows.saturating_sub(3)) as u16;
            cursor_pos = Some((
                (3 + display_width(prompt_str) + cursor_col_offset) as u16,
                input_row_y,
            ));

            if chars.is_empty() {
                let placeholder_raw = match target {
                    ComposeTarget::NewComment => {
                        "对当前视频发表新的一级评论，Enter 提交 · Esc 取消...".to_string()
                    }
                    ComposeTarget::Reply { reply_to_uname, .. } => {
                        format!("输入回复 @{reply_to_uname} 的内容，Enter 提交 · Esc 取消...")
                    }
                };
                let ph = pad_to_width(&truncate_to_width(&placeholder_raw, text_slots), text_slots);
                if use_color {
                    write!(
                        stdout,
                        "\x1b[K {} {}{} {}\r\n",
                        "│".dark_grey(),
                        prompt_str.with(BILI_CYAN).bold(),
                        ph.dark_grey(),
                        "│".dark_grey()
                    )
                    .ok();
                } else {
                    write!(stdout, "\x1b[K │ {prompt_str}{ph} │\r\n").ok();
                }
            } else {
                let val_padded = pad_to_width(&visible_input, text_slots);
                if use_color {
                    write!(
                        stdout,
                        "\x1b[K {} {}{} {}\r\n",
                        "│".dark_grey(),
                        prompt_str.with(BILI_CYAN).bold(),
                        val_padded,
                        "│".dark_grey()
                    )
                    .ok();
                } else {
                    write!(stdout, "\x1b[K │ {prompt_str}{val_padded} │\r\n").ok();
                }
            }

            if use_color {
                write!(
                    stdout,
                    "\x1b[K {}\r\n",
                    format!("╰{}╯", "─".repeat(box_inner)).dark_grey()
                )
                .ok();
            } else {
                write!(stdout, "\x1b[K ╰{}╯\r\n", "─".repeat(box_inner)).ok();
            }

            let legend = truncate_to_width(
                " Enter 提交  ·  Ctrl+A/E 首尾  ·  Ctrl+U/K/W 删词  ·  Esc 返回",
                cols.saturating_sub(1),
            );
            if use_color {
                write!(stdout, "\x1b[K{}", legend.as_str().dark_grey()).ok();
            } else {
                write!(stdout, "\x1b[K{legend}").ok();
            }
        }
        InputMode::ConfirmDelete { rpid, preview } => {
            if use_color {
                write!(stdout, "{}\r\n", sep.as_str().dark_grey()).ok();
            } else {
                write!(stdout, "{sep}\r\n").ok();
            }
            let prompt = truncate_to_width(
                &format!(" ⚠ 确认删除评论 #{rpid}「{preview}」？[y] 确认删除  [n/Esc] 取消"),
                cols.saturating_sub(1),
            );
            if use_color {
                write!(stdout, "\x1b[K{}\r\n", prompt.as_str().yellow().bold()).ok();
            } else {
                write!(stdout, "\x1b[K{prompt}\r\n").ok();
            }
            let legend =
                truncate_to_width(" y / Enter 确认  ·  n / Esc 取消", cols.saturating_sub(1));
            if use_color {
                write!(stdout, "\x1b[K{}", legend.as_str().dark_grey()).ok();
            } else {
                write!(stdout, "\x1b[K{legend}").ok();
            }
        }
        InputMode::Browse | InputMode::Help => {
            if use_color {
                write!(stdout, "{}\r\n", sep.as_str().dark_grey()).ok();
            } else {
                write!(stdout, "{sep}\r\n").ok();
            }
            let status_rendered = if state.loading_page || state.pending_action {
                let spin = SPINNER_FRAMES[state.spinner_tick % SPINNER_FRAMES.len()];
                let raw =
                    truncate_to_width(&format!(" {spin} 请求处理中..."), cols.saturating_sub(1));
                if use_color {
                    raw.as_str().dark_grey().to_string()
                } else {
                    raw
                }
            } else if let Some(toast) = state.active_toast() {
                let raw = truncate_to_width(&format!(" {}", toast.text), cols.saturating_sub(1));
                if use_color {
                    if toast.is_error {
                        raw.as_str().yellow().to_string()
                    } else {
                        raw.as_str().with(BILI_CYAN).to_string()
                    }
                } else {
                    raw
                }
            } else if state.can_write {
                let msg = "已登录 · 可点赞/回复/发表评论";
                if use_color {
                    let full = format!(" {} {}", "●".green().bold(), msg.dark_grey());
                    truncate_ansi_to_width(&full, cols.saturating_sub(1))
                } else {
                    truncate_to_width(&format!(" ● {msg}"), cols.saturating_sub(1))
                }
            } else if use_color {
                let full = format!(
                    " {} {}",
                    "●".red().bold(),
                    "未登录 (只读模式) · 执行 `bit auth login` 后可点赞/回复/发表评论".dark_grey()
                );
                truncate_ansi_to_width(&full, cols.saturating_sub(1))
            } else {
                truncate_to_width(
                    " ● 未登录 (只读模式) · 执行 `bit auth login` 后可点赞/回复/发表评论",
                    cols.saturating_sub(1),
                )
            };
            write!(stdout, "\x1b[K{status_rendered}\r\n").ok();

            let raw_legend = if state.root_mode_rpid.is_some() {
                " hjkl/←↓↑→ 移动·折展 · / 搜索 · n/p 翻页 · L 点赞 · r 回复此条 · d 删除 · ? 帮助 · q 退出"
            } else {
                " hjkl/←↓↑→ 移动·折展 · / 搜索 · n/p 翻页 · s 排序 · L 点赞 · r 回复此条 · c 评论视频 · d 删除 · ? 帮助 · q 退出"
            };
            let legend = truncate_to_width(raw_legend, cols.saturating_sub(1));
            if use_color {
                write!(stdout, "\x1b[K{}", legend.as_str().dark_grey()).ok();
            } else {
                write!(stdout, "\x1b[K{legend}").ok();
            }
        }
    }

    if let Some((cx, cy)) = cursor_pos {
        queue!(stdout, MoveTo(cx, cy), Show).map_err(BiliLiveError::Io)?;
    }
    stdout.flush().map_err(BiliLiveError::Io)?;
    Ok(())
}

fn render_centered_placeholder(
    stdout: &mut std::io::Stdout,
    body_height: usize,
    msg: &str,
    use_color: bool,
) {
    let top_pad = body_height / 3;
    for _ in 0..top_pad {
        let _ = write!(stdout, "\x1b[K\r\n");
    }
    if use_color {
        let _ = write!(stdout, "\x1b[K{}\r\n", msg.dark_grey());
    } else {
        let _ = write!(stdout, "\x1b[K{msg}\r\n");
    }
    for _ in (top_pad + 1)..body_height {
        let _ = write!(stdout, "\x1b[K\r\n");
    }
}

fn render_help_lines(
    stdout: &mut std::io::Stdout,
    body_height: usize,
    cols: usize,
    use_color: bool,
) {
    let help_rows = [
        "  ✦ 快捷键说明 (按任意键返回)",
        "",
        "    j / ↓ , k / ↑      上下移动选中评论或楼中楼",
        "    l / →              展开楼中楼 / 进入子回复",
        "    h / ←              收起当前楼中楼并回到根评论",
        "    /                  搜索当前页评论（回车后 n/p 跳转，Enter 定位，Esc 退出）",
        "    PgUp / PgDn        快速上下翻动 5 条",
        "    g / Home , G / End 跳转到顶部 / 底部",
        "    Enter / Space      展开/收起楼中楼或加载更多回复",
        "    n / ] , p / [      下一页 / 上一页",
        "    s                  切换排序（按热度 ↔ 按时间）",
        "    R / F5             刷新当前页",
        "    L (Shift+L)        点赞 / 取消点赞当前选中评论",
        "    r                  回复当前选中的评论或楼中楼",
        "    c                  对当前视频发表新的一级评论",
        "    d                  删除自己发表的评论",
        "    q / Esc / Ctrl+C   退出浏览",
    ];
    for i in 0..body_height {
        let text = help_rows.get(i).copied().unwrap_or("");
        let trunc = truncate_to_width(text, cols.saturating_sub(1));
        if use_color && i == 0 {
            let _ = write!(
                stdout,
                "\x1b[K{}\r\n",
                trunc.as_str().with(BILI_PINK).bold()
            );
        } else {
            let _ = write!(stdout, "\x1b[K{trunc}\r\n");
        }
    }
}

fn push_node_line(line: String, is_sel: bool, cols: usize, use_color: bool, out: &mut Vec<String>) {
    if is_sel && use_color {
        out.push(apply_selection_bg(&line, cols, SELECT_BG_SGR));
    } else {
        out.push(truncate_ansi_to_width(&line, cols));
    }
}

fn render_node_lines(
    state: &CommentTuiState,
    node: NodeRef,
    is_sel: bool,
    cols: usize,
    use_color: bool,
    out: &mut Vec<String>,
) {
    let head_gutter = if is_sel {
        if use_color {
            "▶".with(BILI_PINK).bold().to_string()
        } else {
            ">".to_string()
        }
    } else {
        " ".to_string()
    };
    let cont_gutter = if is_sel {
        if use_color {
            "▌".with(BILI_PINK).bold().to_string()
        } else {
            "|".to_string()
        }
    } else {
        " ".to_string()
    };

    match node {
        NodeRef::Root { thread_idx } => {
            let th = &state.threads[thread_idx];
            let r = &th.root;
            let author = format_author_badges(r, state.upper_mid, th.is_pinned, use_color);
            let ts = format_timestamp(r.ctime);
            let loc = r
                .location_short()
                .map(|l| format!(" · {l}"))
                .unwrap_or_default();
            let like_txt = if r.is_liked() {
                format!("♥ {}(已赞)", r.like)
            } else {
                format!("♥ {}", r.like)
            };
            let reply_txt = if r.rcount > 0 {
                format!("💬 {}", r.rcount)
            } else {
                String::new()
            };

            let search_q = state.search_query.trim();
            let is_match = !search_q.is_empty() && reply_matches_query(r, search_q);
            let header = if use_color {
                let mut h = format!(
                    " {head_gutter} {author}  {}  {}",
                    format!("{ts}{loc}").as_str().dark_grey(),
                    like_txt.as_str().with(BILI_PINK)
                );
                if !reply_txt.is_empty() {
                    h.push_str(&format!("  {}", reply_txt.as_str().dark_grey()));
                }
                if r.is_up_liked() {
                    h.push_str(&format!("  {}", "[UP主觉得很赞]".with(BILI_PINK)));
                }
                if r.is_up_replied() {
                    h.push_str(&format!("  {}", "[UP回复]".with(BILI_CYAN)));
                }
                if is_match {
                    h.push_str(&format!("  {}", "[匹配]".yellow().bold()));
                }
                h
            } else {
                let mut h = format!(" {head_gutter} {author}  {ts}{loc}  {like_txt}");
                if !reply_txt.is_empty() {
                    h.push_str(&format!("  {reply_txt}"));
                }
                if r.is_up_liked() {
                    h.push_str("  [UP主觉得很赞]");
                }
                if r.is_up_replied() {
                    h.push_str("  [UP回复]");
                }
                if is_match {
                    h.push_str("  [匹配]");
                }
                h
            };
            push_node_line(header, is_sel, cols, use_color, out);

            let body_w = cols.saturating_sub(8).max(20);
            let clean = r.clean_message();
            let body_lines = if clean.is_empty() && !r.content.pictures.is_empty() {
                Vec::new()
            } else {
                wrap_to_width(&clean, body_w)
            };
            for seg in body_lines {
                let highlighted = highlight_query_spans(&seg, search_q, is_sel, use_color);
                let line = format!(" {cont_gutter}   {highlighted}");
                push_node_line(line, is_sel, cols, use_color, out);
            }
            if !r.content.pictures.is_empty() {
                let pic = format!("[🖼 图片 ×{}]", r.content.pictures.len());
                let line = if use_color {
                    format!(" {cont_gutter}   {}", pic.with(BILI_CYAN))
                } else {
                    format!(" {cont_gutter}   {pic}")
                };
                push_node_line(line, is_sel, cols, use_color, out);
            }

            if !th.expanded && r.rcount > 0 {
                let hint = format!("↳ 共 {} 条回复 (按 Enter/l 展开)", r.rcount);
                let line = if use_color {
                    format!(" {cont_gutter}   {}", hint.dark_grey())
                } else {
                    format!(" {cont_gutter}   {hint}")
                };
                push_node_line(line, is_sel, cols, use_color, out);
            }
            if !th.expanded || (th.sub_replies.is_empty() && !th.has_more_subs() && !th.loading_sub)
            {
                out.push(String::new());
            }
        }
        NodeRef::SubReply {
            thread_idx,
            sub_idx,
        } => {
            let th = &state.threads[thread_idx];
            let sr = &th.sub_replies[sub_idx];
            let is_last_visual = sub_idx + 1 == th.sub_replies.len()
                && !(state.root_mode_rpid.is_none() && (th.has_more_subs() || th.loading_sub));
            let branch = if is_last_visual { "╰─" } else { "├─" };
            let cont = if is_last_visual { "  " } else { "│ " };

            let author = format_author_badges(sr, state.upper_mid, false, use_color);
            let ts = format_timestamp(sr.ctime);
            let loc = sr
                .location_short()
                .map(|l| format!(" · {l}"))
                .unwrap_or_default();
            let like_txt = if sr.like > 0 || sr.is_liked() {
                if sr.is_liked() {
                    format!("  ♥ {}(已赞)", sr.like)
                } else {
                    format!("  ♥ {}", sr.like)
                }
            } else {
                String::new()
            };

            let search_q = state.search_query.trim();
            let is_match = !search_q.is_empty() && reply_matches_query(sr, search_q);
            let header = if use_color {
                let mut h = format!(
                    " {head_gutter}   {} {author}  {}{}",
                    branch.dark_grey(),
                    format!("{ts}{loc}").as_str().dark_grey(),
                    if like_txt.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", like_txt.trim_start().with(BILI_PINK))
                    }
                );
                if is_match {
                    h.push_str(&format!("  {}", "[匹配]".yellow().bold()));
                }
                h
            } else {
                let mut h = format!(" {head_gutter}   {branch} {author}  {ts}{loc}{like_txt}");
                if is_match {
                    h.push_str("  [匹配]");
                }
                h
            };
            push_node_line(header, is_sel, cols, use_color, out);

            let sub_w = cols.saturating_sub(11).max(20);
            let clean = sr.clean_message();
            let body_lines = if clean.is_empty() && !sr.content.pictures.is_empty() {
                Vec::new()
            } else {
                wrap_to_width(&clean, sub_w)
            };
            for seg in body_lines {
                let highlighted = highlight_query_spans(&seg, search_q, is_sel, use_color);
                let line = if use_color {
                    format!(" {cont_gutter}   {} {highlighted}", cont.dark_grey())
                } else {
                    format!(" {cont_gutter}   {cont} {highlighted}")
                };
                push_node_line(line, is_sel, cols, use_color, out);
            }
            if !sr.content.pictures.is_empty() {
                let pic = format!("[🖼 图片 ×{}]", sr.content.pictures.len());
                let line = if use_color {
                    format!(
                        " {cont_gutter}   {} {}",
                        cont.dark_grey(),
                        pic.with(BILI_CYAN)
                    )
                } else {
                    format!(" {cont_gutter}   {cont} {pic}")
                };
                push_node_line(line, is_sel, cols, use_color, out);
            }
            if is_last_visual {
                out.push(String::new());
            }
        }
        NodeRef::LoadMoreSubs { thread_idx } => {
            let th = &state.threads[thread_idx];
            let text = if th.loading_sub {
                let spin = SPINNER_FRAMES[state.spinner_tick % SPINNER_FRAMES.len()];
                format!("{spin} 正在加载更多楼中楼回复...")
            } else {
                let rem = th.sub_total.saturating_sub(th.sub_replies.len() as u64);
                if rem > 0 {
                    format!("↳ 展开更多楼中楼 (还有 {rem} 条，按 Enter 加载)")
                } else {
                    "↳ 展开更多楼中楼 (按 Enter 加载)".to_string()
                }
            };
            let line = if use_color {
                let styled = if is_sel {
                    text.as_str().with(BILI_PINK).bold().to_string()
                } else {
                    text.as_str().with(BILI_CYAN).to_string()
                };
                format!(" {head_gutter}   {} {styled}", "╰─".dark_grey())
            } else {
                format!(" {head_gutter}   ╰─ {text}")
            };
            push_node_line(line, is_sel, cols, use_color, out);
            out.push(String::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::comment::{CommentUpper, ReplyContent, ReplyMember};

    fn make_ctx(cookies: Option<Cookies>) -> CommentTuiContext {
        CommentTuiContext {
            aid: 79677524,
            video_label: "BV1xx".to_string(),
            video_title: "测试视频".to_string(),
            initial_sort: CommentSort::Hot,
            initial_page: 1,
            initial_root: None,
            cookies,
        }
    }

    fn make_reply(rpid: i64, mid: i64, uname: &str, msg: &str, rcount: u64) -> ReplyItem {
        ReplyItem {
            rpid,
            oid: 79677524,
            mid,
            rcount,
            like: 5,
            ctime: 1700000000,
            member: Some(ReplyMember {
                uname: uname.to_string(),
                mid: mid.to_string(),
                ..Default::default()
            }),
            content: ReplyContent {
                message: msg.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn flatten_nodes_and_pinned_dedup_work_cleanly() {
        let mut state = CommentTuiState::new(&make_ctx(None));
        let mut pinned = make_reply(100, 10, "UP", "置顶", 3);
        pinned.replies = vec![make_reply(101, 20, "子评", "回复1", 0)];

        let normal = make_reply(200, 30, "普通", "第二条", 0);

        let page_data = CommentPageData {
            page: CommentPageInfo {
                num: 1,
                size: 20,
                count: 2,
                acount: 3,
            },
            upper: Some(CommentUpper {
                mid: 10,
                top: Some(Box::new(pinned.clone())),
            }),
            replies: vec![pinned, normal],
            ..Default::default()
        };

        // Stale req_id ignored
        state.apply_page_loaded(999, 1, CommentSort::Hot, Ok(page_data.clone()));
        assert!(state.threads.is_empty());

        // Matching req_id populates deduped threads
        state.apply_page_loaded(1, 1, CommentSort::Hot, Ok(page_data));
        assert_eq!(state.threads.len(), 2);
        assert!(state.threads[0].is_pinned);
        assert_eq!(state.threads[0].root.rpid, 100);

        // Node list: Root(0), SubReply(0,0), LoadMoreSubs(0), Root(1)
        let nodes = state.flatten_nodes();
        assert_eq!(
            nodes,
            vec![
                NodeRef::Root { thread_idx: 0 },
                NodeRef::SubReply {
                    thread_idx: 0,
                    sub_idx: 0
                },
                NodeRef::LoadMoreSubs { thread_idx: 0 },
                NodeRef::Root { thread_idx: 1 },
            ]
        );

        // Selecting a sub-reply and pressing collapse selects its root
        state.selected_node = 1;
        state.select_root_of_current();
        assert_eq!(state.selected_node, 0);
        assert!(!state.threads[0].expanded);
        assert_eq!(
            state.flatten_nodes(),
            vec![
                NodeRef::Root { thread_idx: 0 },
                NodeRef::Root { thread_idx: 1 }
            ]
        );
    }

    #[test]
    fn sub_reply_merge_dedup_like_and_delete_update_state() {
        let cookies = Cookies {
            sessdata: "s".to_string(),
            bili_jct: "csrf".to_string(),
            dede_user_id: "30".to_string(),
            ..Default::default()
        };
        let mut state = CommentTuiState::new(&make_ctx(Some(cookies)));
        assert!(state.can_write);
        assert_eq!(state.self_uid, Some(30));

        let mut root = make_reply(200, 30, "自己", "主评", 2);
        root.replies = vec![make_reply(201, 40, "甲", "预置子评", 0)];

        let page_data = CommentPageData {
            page: CommentPageInfo {
                num: 1,
                size: 20,
                count: 1,
                acount: 3,
            },
            replies: vec![root],
            ..Default::default()
        };
        state.apply_page_loaded(1, 1, CommentSort::Hot, Ok(page_data));

        state.sub_req_id = 1;
        let sub_data = SubReplyPageData {
            page: CommentPageInfo {
                num: 1,
                size: 20,
                count: 2,
                acount: 0,
            },
            replies: vec![
                make_reply(201, 40, "甲", "预置子评", 0),
                make_reply(202, 30, "自己", "新子评", 0),
            ],
            ..Default::default()
        };
        state.apply_sub_replies_loaded(1, 200, 1, true, Ok(sub_data));
        assert_eq!(state.threads[0].sub_replies.len(), 2);
        assert!(state.threads[0].sub_exhausted);
        assert!(!state.threads[0].has_more_subs());

        // Like & unlike update action + count
        state.apply_like_done(202, false, Ok(()));
        assert_eq!(state.threads[0].sub_replies[1].like, 6);
        assert_eq!(state.threads[0].sub_replies[1].action, 1);

        state.apply_like_done(202, true, Ok(()));
        assert_eq!(state.threads[0].sub_replies[1].like, 5);
        assert_eq!(state.threads[0].sub_replies[1].action, 0);

        // Delete removes sub-reply cleanly
        state.selected_node = 2;
        state.apply_delete_done(202, Ok(()));
        assert_eq!(state.threads[0].sub_replies.len(), 1);
        assert_eq!(state.threads[0].root.rcount, 1);
    }

    #[test]
    fn format_reply_message_avoids_double_prefix() {
        let sub = ComposeTarget::Reply {
            root_rpid: 1,
            parent_rpid: 2,
            reply_to_uname: "Alice".to_string(),
            is_sub_reply: true,
        };
        assert_eq!(format_reply_message(&sub, "收到"), "回复 @Alice :收到");
        assert_eq!(
            format_reply_message(&sub, "回复 @Alice :已收到"),
            "回复 @Alice :已收到"
        );

        let top = ComposeTarget::Reply {
            root_rpid: 1,
            parent_rpid: 1,
            reply_to_uname: "Alice".to_string(),
            is_sub_reply: false,
        };
        assert_eq!(format_reply_message(&top, "顶一下"), "顶一下");
    }

    #[test]
    fn post_done_inserts_comment_and_reply_immediately_without_waiting_for_refresh() {
        let cookies = Cookies {
            sessdata: "s".to_string(),
            bili_jct: "csrf".to_string(),
            dede_user_id: "30".to_string(),
            ..Default::default()
        };
        let mut state = CommentTuiState::new(&make_ctx(Some(cookies)));
        let pinned = make_reply(100, 10, "UP", "置顶", 0);
        let normal = make_reply(200, 40, "路人", "旧评论", 0);
        state.apply_page_loaded(
            1,
            1,
            CommentSort::Hot,
            Ok(CommentPageData {
                page: CommentPageInfo {
                    num: 1,
                    size: 20,
                    count: 2,
                    acount: 2,
                },
                upper: Some(CommentUpper {
                    mid: 10,
                    top: Some(Box::new(pinned.clone())),
                }),
                replies: vec![pinned, normal],
                ..Default::default()
            }),
        );

        // 1. New top-level comment inserts right after pinned and becomes selected
        let created_root = make_reply(300, 30, "自己", "刚发的一级评论", 0);
        state.apply_post_done(
            79677524,
            ComposeTarget::NewComment,
            "刚发的一级评论".to_string(),
            "刚发的一级评论".to_string(),
            Ok((300, Some(created_root))),
        );
        assert_eq!(state.threads.len(), 3);
        assert_eq!(state.threads[0].root.rpid, 100);
        assert_eq!(state.threads[1].root.rpid, 300);
        assert_eq!(state.page_info.acount, 3);
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::Root { thread_idx: 1 })
        );

        // 2. Reply when server omits `reply` object synthesizes fallback reply and selects it
        state.apply_post_done(
            79677524,
            ComposeTarget::Reply {
                root_rpid: 300,
                parent_rpid: 300,
                reply_to_uname: "自己".to_string(),
                is_sub_reply: false,
            },
            "楼中楼测试".to_string(),
            "楼中楼测试".to_string(),
            Ok((301, None)),
        );
        assert!(state.threads[1].expanded);
        assert_eq!(state.threads[1].root.rcount, 1);
        assert_eq!(state.threads[1].sub_replies.len(), 1);
        assert_eq!(state.threads[1].sub_replies[0].rpid, 301);
        assert_eq!(
            state.threads[1].sub_replies[0].content.message,
            "楼中楼测试"
        );
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::SubReply {
                thread_idx: 1,
                sub_idx: 0
            })
        );
    }

    #[test]
    fn vim_hjkl_navigation_and_shift_l_like_are_disjoint_and_selection_highlight_is_ansi_safe() {
        let cookies = Cookies {
            sessdata: "s".to_string(),
            bili_jct: "csrf".to_string(),
            dede_user_id: "30".to_string(),
            ..Default::default()
        };
        let ctx = make_ctx(Some(cookies));
        let mut state = CommentTuiState::new(&ctx);
        let mut root = make_reply(200, 10, "QwerProg", "主评内容", 1);
        root.action = 1;
        root.replies = vec![make_reply(201, 40, "Alice", "子评内容", 0)];
        state.apply_page_loaded(
            1,
            1,
            CommentSort::Hot,
            Ok(CommentPageData {
                page: CommentPageInfo {
                    num: 1,
                    size: 20,
                    count: 1,
                    acount: 2,
                },
                upper: Some(CommentUpper {
                    mid: 10,
                    top: Some(Box::new(root.clone())),
                }),
                replies: vec![root],
                ..Default::default()
            }),
        );

        let (tx, _rx) = mpsc::channel();
        // `h` collapses, `l` expands, second `l` steps into sub-reply (never triggers like)
        handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('h'),
            KeyModifiers::NONE,
        );
        assert!(!state.threads[0].expanded);
        assert!(!state.pending_action);

        handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('l'),
            KeyModifiers::NONE,
        );
        assert!(state.threads[0].expanded);
        assert!(!state.pending_action);
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::Root { thread_idx: 0 })
        );

        handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('l'),
            KeyModifiers::NONE,
        );
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::SubReply {
                thread_idx: 0,
                sub_idx: 0
            })
        );
        assert!(!state.pending_action);

        // Render colored root node under narrow width: no broken replacement char, has selection bg & ▶/▌
        let mut out = Vec::new();
        render_node_lines(
            &state,
            NodeRef::Root { thread_idx: 0 },
            true,
            48,
            true,
            &mut out,
        );
        assert!(!out.is_empty());
        assert!(out[0].contains(SELECT_BG_SGR));
        assert!(out[0].contains('▶'));
        assert!(out[1].contains(SELECT_BG_SGR));
        assert!(out[1].contains('▌'));
        for line in &out {
            assert!(
                !line.contains('\u{FFFD}'),
                "broken unicode in line: {line:?}"
            );
            assert!(
                crate::ui::components::display_width_ansi(line) <= 48,
                "line exceeded width 48: {line:?}"
            );
        }
    }

    #[test]
    fn slash_search_navigates_np_commits_on_enter_and_cancels_on_esc() {
        use crate::api::comment::ReplyControl;

        let ctx = make_ctx(None);
        let mut state = CommentTuiState::new(&ctx);

        let mut th1 = make_reply(101, 10, "Alice", "这个 CLI 很好用", 1);
        th1.reply_control = Some(ReplyControl {
            location: Some("IP属地：江苏".to_string()),
            ..Default::default()
        });
        th1.replies = vec![make_reply(102, 20, "Bob", "Rust 实现很丝滑", 0)];

        let mut th2 = make_reply(201, 30, "Charlie", "Rust 生态不错", 0);
        th2.reply_control = Some(ReplyControl {
            location: Some("IP属地：广东".to_string()),
            ..Default::default()
        });

        let th3 = make_reply(301, 40, "RustFan", "支持一下", 0);

        state.apply_page_loaded(
            1,
            1,
            CommentSort::Hot,
            Ok(CommentPageData {
                page: CommentPageInfo {
                    num: 1,
                    size: 20,
                    count: 3,
                    acount: 4,
                },
                replies: vec![th1, th2, th3],
                ..Default::default()
            }),
        );
        // Start on thread 2 (last node) and collapse thread 0 to verify auto-expand + wrap-around
        state.threads[0].expanded = false;
        state.selected_node = 2; // Root(2) while th0 is collapsed

        let (tx, _rx) = mpsc::channel();

        // 1. Press `/` and type "rust" -> auto-expands th0, highlights 3 matches: SubReply(0,0)=idx1, Root(1)=idx2, Root(2)=idx3
        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('/'),
            KeyModifiers::NONE,
        ));
        for ch in ['r', 'u', 's', 't'] {
            assert!(!handle_key_event(
                &mut state,
                &ctx,
                &tx,
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            ));
        }
        assert!(state.threads[0].expanded);
        assert_eq!(state.search_query, "rust");
        assert_eq!(state.search_match_count(), 3);
        assert_eq!(state.selected_node, 2); // first match >= origin(2) is Root(1) at idx 2

        // 2. Press Enter -> switches to SearchNav
        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Enter,
            KeyModifiers::NONE,
        ));
        assert!(matches!(state.mode, InputMode::SearchNav { .. }));
        assert_eq!(state.current_match_ordinal(), Some((2, 3)));

        // 3. Press `n` -> moves to match 3/3 (Root(2) at idx 3); next `n` wraps to match 1/3 (SubReply(0,0) at idx 1)
        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
        ));
        assert_eq!(state.current_match_ordinal(), Some((3, 3)));
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::Root { thread_idx: 2 })
        );

        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
        ));
        assert_eq!(state.current_match_ordinal(), Some((1, 3)));
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::SubReply {
                thread_idx: 0,
                sub_idx: 0
            })
        );

        // 4. Press `p` -> wraps back to match 3/3, another `p` -> match 2/3
        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('p'),
            KeyModifiers::NONE,
        ));
        assert_eq!(state.current_match_ordinal(), Some((3, 3)));
        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('p'),
            KeyModifiers::NONE,
        ));
        assert_eq!(state.current_match_ordinal(), Some((2, 3)));
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::Root { thread_idx: 1 })
        );

        // 5. Press Enter in SearchNav -> commits jump right onto Root(1) and clears search_query
        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Enter,
            KeyModifiers::NONE,
        ));
        assert!(matches!(state.mode, InputMode::Browse));
        assert!(state.search_query.is_empty());
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::Root { thread_idx: 1 })
        );

        // 6. Start a new search from Root(1) (idx 2), jump to Bob's sub-reply, then press Esc -> restores back to Root(1) (idx 2)
        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('/'),
            KeyModifiers::NONE,
        ));
        for ch in ['b', 'o', 'b'] {
            handle_key_event(&mut state, &ctx, &tx, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::SubReply {
                thread_idx: 0,
                sub_idx: 0
            })
        );
        handle_key_event(&mut state, &ctx, &tx, KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(state.mode, InputMode::SearchNav { .. }));
        assert!(!handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Esc,
            KeyModifiers::NONE,
        ));
        assert!(matches!(state.mode, InputMode::Browse));
        assert!(state.search_query.is_empty());
        assert_eq!(
            state.selected_node_ref(),
            Some(NodeRef::Root { thread_idx: 1 })
        );

        // 7. No-match Enter keeps user in Search with error toast; Esc cleanly returns to Browse
        handle_key_event(
            &mut state,
            &ctx,
            &tx,
            KeyCode::Char('/'),
            KeyModifiers::NONE,
        );
        for ch in ['z', 'z', 'z'] {
            handle_key_event(&mut state, &ctx, &tx, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        handle_key_event(&mut state, &ctx, &tx, KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(state.mode, InputMode::Search { .. }));
        assert!(state.active_toast().is_some_and(|t| t.is_error));
        handle_key_event(&mut state, &ctx, &tx, KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(state.mode, InputMode::Browse));
        assert!(state.search_query.is_empty());

        // 8. Highlight spans wrap match without slicing UTF-8
        let hl = highlight_query_spans("Rust 实现很丝滑", "rust", false, true);
        assert!(hl.contains("Rust"));
        assert!(hl.contains("\x1b["));
    }
}
