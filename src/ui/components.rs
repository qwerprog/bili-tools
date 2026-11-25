use crate::ui::theme::{
    BILI_CYAN, BILI_PINK, color_enabled, muted, paint, paint_bold, terminal_width,
};
use crate::utils::string::{display_width, truncate_to_width};
use crossterm::style::Color;
use unicode_width::UnicodeWidthChar;

/// 按终端显示列宽对长文本自动换行（保留原有换行并确保每行不超过 max_width）
pub fn wrap_to_width(s: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(2);
    let mut lines = Vec::new();
    for raw_line in s.split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut w = 0usize;
        for ch in raw_line.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw > max_width && !current.is_empty() {
                lines.push(current);
                current = String::new();
                w = 0;
            }
            current.push(ch);
            w += cw;
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// 计算水平滚动输入窗口可见文本与光标列偏移（保证多字节中文下光标始终可见）
pub fn slice_input_window(chars: &[char], cursor_idx: usize, max_cols: usize) -> (String, usize) {
    if chars.is_empty() {
        return (String::new(), 0);
    }
    let cursor_idx = cursor_idx.min(chars.len());
    let mut start = 0usize;

    // 保证 start..cursor_idx 的显示列宽不超过 max_cols
    loop {
        let w: usize = chars[start..cursor_idx]
            .iter()
            .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        if w <= max_cols.saturating_sub(2) || start >= cursor_idx {
            break;
        }
        start += 1;
    }

    let mut out = String::new();
    let mut used = 0usize;
    let mut cursor_col = 0usize;
    for (i, &ch) in chars[start..].iter().enumerate() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw > max_cols {
            break;
        }
        if start + i == cursor_idx {
            cursor_col = used;
        }
        out.push(ch);
        used += cw;
    }
    if cursor_idx >= start + out.chars().count() {
        cursor_col = used;
    }
    (out, cursor_col)
}

/// 单元格对齐方式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// 表格单元格（先按纯文本计算 CJK 宽度补齐，再施加颜色，彻底避免 ANSI 转义序列破坏列对齐）
#[derive(Clone, Debug)]
pub struct Cell {
    pub text: String,
    pub color: Option<Color>,
    pub bold: bool,
}

impl Cell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
            bold: false,
        }
    }

    pub fn colored(text: impl Into<String>, color: Color) -> Self {
        Self {
            text: text.into(),
            color: Some(color),
            bold: false,
        }
    }

    pub fn bold_colored(text: impl Into<String>, color: Color) -> Self {
        Self {
            text: text.into(),
            color: Some(color),
            bold: true,
        }
    }

    pub fn muted(text: impl Into<String>) -> Self {
        Self::colored(text, Color::DarkGrey)
    }

    pub fn cyan(text: impl Into<String>) -> Self {
        Self::colored(text, BILI_CYAN)
    }

    pub fn pink(text: impl Into<String>) -> Self {
        Self::colored(text, BILI_PINK)
    }

    fn render_padded(&self, width: usize, align: Align, enable_color: bool) -> String {
        let truncated = truncate_to_width(&self.text, width);
        let w = display_width(&truncated);
        let pad = width.saturating_sub(w);

        let styled = if enable_color {
            use crossterm::style::Stylize;
            match (self.color, self.bold) {
                (Some(c), true) => truncated.with(c).bold().to_string(),
                (Some(c), false) => truncated.with(c).to_string(),
                (None, true) => truncated.bold().to_string(),
                (None, false) => truncated,
            }
        } else {
            truncated
        };

        match align {
            Align::Left => format!("{styled}{}", " ".repeat(pad)),
            Align::Right => format!("{}{styled}", " ".repeat(pad)),
        }
    }
}

/// 圆角信息卡片组件（用于展示账号详情、直播状态面板、推流信息、视频详情等）
pub struct InfoCard {
    title: String,
    badge: Option<(String, Color)>,
    rows: Vec<(String, Cell)>,
}

impl InfoCard {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            badge: None,
            rows: Vec::new(),
        }
    }

    pub fn with_badge(mut self, text: impl Into<String>, color: Color) -> Self {
        self.badge = Some((text.into(), color));
        self
    }

    pub fn add_row(mut self, key: impl Into<String>, value: Cell) -> Self {
        self.rows.push((key.into(), value));
        self
    }

    pub fn add_plain_row(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.add_row(key, Cell::plain(value))
    }

    pub fn render(&self, max_term_width: usize, enable_color: bool) -> String {
        let label_width = self
            .rows
            .iter()
            .map(|(k, _)| display_width(k))
            .max()
            .unwrap_or(6)
            .max(4);

        let max_val_width = self
            .rows
            .iter()
            .map(|(_, v)| display_width(&v.text))
            .max()
            .unwrap_or(16);

        let header_plain_width = display_width(&self.title)
            + self
                .badge
                .as_ref()
                .map(|(b, _)| display_width(b) + 3)
                .unwrap_or(0)
            + 4;

        // 内部内容区宽度（不含左右边框 `│ ` 和 ` │`）
        let desired_inner = (label_width + 3 + max_val_width)
            .max(header_plain_width)
            .max(38);
        let cap_inner = max_term_width.saturating_sub(6).max(30);
        let inner_width = desired_inner.min(cap_inner).max(label_width + 3 + 8);
        let val_width = inner_width.saturating_sub(label_width + 3).max(8);

        let border = |s: &str| -> String {
            if enable_color {
                use crossterm::style::Stylize;
                s.dark_grey().to_string()
            } else {
                s.to_string()
            }
        };

        let mut out = Vec::with_capacity(self.rows.len() + 2);

        // 顶部标题边框: ╭─ Title · Badge ──────────╮
        // 若窄终端下标题 + 徽标超出 inner_width + 2，先截断纯文本，确保顶底边框与正文列宽严格一致
        let (title_disp, badge_disp) = if let Some((b_txt, b_col)) = &self.badge {
            let avail = (inner_width + 2).saturating_sub(6); // "─ " + " " + "· " + " "
            let badge_max = (avail / 2).max(avail.saturating_sub(display_width(&self.title)));
            let b_disp = truncate_to_width(b_txt, badge_max);
            let title_max = avail.saturating_sub(display_width(&b_disp));
            let t_disp = truncate_to_width(&self.title, title_max);
            (t_disp, Some((b_disp, *b_col)))
        } else {
            let title_max = (inner_width + 2).saturating_sub(3); // "─ " + " "
            (truncate_to_width(&self.title, title_max), None)
        };

        let mut header_used = 1 + 1 + display_width(&title_disp) + 1; // "─ " + title + " "
        let title_str = if enable_color {
            use crossterm::style::Stylize;
            title_disp.as_str().with(BILI_PINK).bold().to_string()
        } else {
            title_disp
        };

        let badge_segment = if let Some((b_txt, b_col)) = badge_disp {
            header_used += 2 + display_width(&b_txt) + 1; // "· " + badge + " "
            if enable_color {
                use crossterm::style::Stylize;
                format!("{} {} ", "·".dark_grey(), b_txt.as_str().with(b_col).bold())
            } else {
                format!("· {b_txt} ")
            }
        } else {
            String::new()
        };

        let total_top_dashes = (inner_width + 2).saturating_sub(header_used);
        out.push(format!(
            "  {}{}{}{}{}",
            border("╭─ "),
            title_str,
            " ",
            badge_segment,
            border(&format!("{}╮", "─".repeat(total_top_dashes)))
        ));

        for (key, cell) in &self.rows {
            let k_pad = label_width.saturating_sub(display_width(key));
            let key_rendered = if enable_color {
                use crossterm::style::Stylize;
                format!("{}{}", " ".repeat(k_pad), key.as_str().dark_grey())
            } else {
                format!("{}{}", " ".repeat(k_pad), key)
            };
            let blank_key = " ".repeat(label_width);
            let row_used = label_width + 3 + val_width;
            let extra_pad = inner_width.saturating_sub(row_used);

            let wrapped = wrap_to_width(&cell.text, val_width);
            for (line_idx, seg) in wrapped.into_iter().enumerate() {
                let seg_cell = Cell {
                    text: seg,
                    color: cell.color,
                    bold: cell.bold,
                };
                let val_rendered = seg_cell.render_padded(val_width, Align::Left, enable_color);
                let left_col = if line_idx == 0 {
                    &key_rendered
                } else {
                    &blank_key
                };
                out.push(format!(
                    "  {} {left_col}   {val_rendered}{} {}",
                    border("│"),
                    " ".repeat(extra_pad),
                    border("│")
                ));
            }
        }

        out.push(format!(
            "  {}",
            border(&format!("╰{}╯", "─".repeat(inner_width + 2)))
        ));

        out.join("\n")
    }

    pub fn print(&self) {
        if crate::cli::output::is_quiet() || crate::cli::output::is_json() {
            return;
        }
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{}", self.render(terminal_width(), color_enabled()));
    }
}

/// 现代无竖线对齐表格定义列
pub struct Column {
    pub header: &'static str,
    pub width: usize,
    pub align: Align,
}

impl Column {
    pub const fn left(header: &'static str, width: usize) -> Self {
        Self {
            header,
            width,
            align: Align::Left,
        }
    }

    pub const fn right(header: &'static str, width: usize) -> Self {
        Self {
            header,
            width,
            align: Align::Right,
        }
    }
}

/// 打印现代分区标题栏（如：`✦ 搜索视频 · "Rust"`  `共 100 条 · 第 1 页`）
pub fn print_section_header(title: &str, subtitle: Option<&str>) {
    if crate::cli::output::is_quiet() || crate::cli::output::is_json() {
        return;
    }
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    let icon = paint_bold("✦", BILI_PINK);
    let title_styled = paint_bold(title, Color::White);
    match subtitle {
        Some(sub) if !sub.is_empty() => {
            let _ = writeln!(
                stdout,
                "  {icon} {title_styled}  {}",
                muted(&format!("({sub})"))
            );
        }
        _ => {
            let _ = writeln!(stdout, "  {icon} {title_styled}");
        }
    }
}

/// 按终端最大列宽自适应压缩表格各列宽度（优先收缩最宽的左对齐文本列，保证不超出窄屏折行）
pub fn compute_responsive_widths(columns: &[Column], max_term_width: usize) -> Vec<usize> {
    let mut widths: Vec<usize> = columns.iter().map(|c| c.width).collect();
    if columns.is_empty() {
        return widths;
    }

    let min_widths: Vec<usize> = columns
        .iter()
        .map(|c| display_width(c.header).max(6).min(c.width))
        .collect();

    let overhead = 2 + columns.len().saturating_sub(1) * 2;
    let target_budget = max_term_width.saturating_sub(overhead);

    while widths.iter().sum::<usize>() > target_budget {
        // 优先压缩可收缩余量最大的列（余量相同时优先压缩左对齐文本列）
        let best_idx = (0..columns.len())
            .filter(|&i| widths[i] > min_widths[i])
            .max_by_key(|&i| {
                let slack = widths[i] - min_widths[i];
                let align_prio = usize::from(columns[i].align == Align::Left);
                (slack, align_prio, widths[i])
            });

        let Some(idx) = best_idx else {
            break;
        };
        widths[idx] -= 1;
    }

    widths
}

/// 渲染并打印对齐数据表格
pub fn print_table(columns: &[Column], rows: &[Vec<Cell>]) {
    if crate::cli::output::is_quiet() || crate::cli::output::is_json() {
        return;
    }
    use std::io::{IsTerminal, Write};
    let enable_color = color_enabled();
    let effective_widths = if std::io::stdout().is_terminal() {
        compute_responsive_widths(columns, terminal_width())
    } else {
        columns.iter().map(|c| c.width).collect()
    };
    let mut stdout = std::io::stdout().lock();

    // 表头
    let header_cells: Vec<String> = columns
        .iter()
        .zip(effective_widths.iter())
        .map(|(col, &w)| {
            Cell::bold_colored(col.header, BILI_PINK).render_padded(w, col.align, enable_color)
        })
        .collect();
    if writeln!(stdout, "  {}", header_cells.join("  ")).is_err() {
        return;
    }

    // 细横线分隔符
    let rule_width: usize =
        effective_widths.iter().sum::<usize>() + columns.len().saturating_sub(1) * 2;
    if writeln!(
        stdout,
        "  {}",
        paint(&"─".repeat(rule_width), Color::DarkGrey)
    )
    .is_err()
    {
        return;
    }

    // 数据行
    for row in rows {
        let rendered_cols: Vec<String> = columns
            .iter()
            .zip(effective_widths.iter())
            .enumerate()
            .map(|(i, (col, &w))| {
                let cell = row.get(i).cloned().unwrap_or_else(|| Cell::plain("-"));
                cell.render_padded(w, col.align, enable_color)
            })
            .collect();
        if writeln!(stdout, "  {}", rendered_cols.join("  ")).is_err() {
            return;
        }
    }
}

/// 渲染现代单行倒计时进度条
pub fn render_countdown_bar(elapsed_secs: u64, total_secs: u64, enable_color: bool) -> String {
    let remaining = total_secs.saturating_sub(elapsed_secs);
    let h = remaining / 3600;
    let m = (remaining % 3600) / 60;
    let s = remaining % 60;

    let ratio = if total_secs == 0 {
        1.0
    } else {
        (elapsed_secs as f64 / total_secs as f64).clamp(0.0, 1.0)
    };
    let bar_width = 18usize;
    let filled = (ratio * bar_width as f64).round() as usize;
    let empty = bar_width.saturating_sub(filled);
    let pct = (ratio * 100.0).round() as u32;

    let filled_bar = "━".repeat(filled);
    let empty_bar = "─".repeat(empty);
    let time_str = format!("{h:02}:{m:02}:{s:02}");

    if enable_color {
        use crossterm::style::Stylize;
        format!(
            "  {} {} {}{} {:>3}%  {} {}  {}",
            "⏳".with(BILI_PINK),
            "定时下播".bold(),
            filled_bar.with(BILI_PINK),
            empty_bar.dark_grey(),
            pct,
            "剩余".dark_grey(),
            time_str.with(BILI_CYAN).bold(),
            "(Ctrl+C 取消)".dark_grey()
        )
    } else {
        format!("  ⏳ 定时下播 [{filled_bar}{empty_bar}] {pct:>3}%  剩余 {time_str}  (Ctrl+C 取消)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_padding_handles_cjk_and_ansi_cleanly() {
        let cell = Cell::colored("已看完", Color::Green);
        // 无颜色模式下精确占 10 列（"已看完" 6 列 + 4 个空格）
        let plain = cell.render_padded(10, Align::Left, false);
        assert_eq!(display_width(&plain), 10);
        assert_eq!(plain, "已看完    ");

        // 启用颜色时末尾依然精确补齐 4 个空格，不被 ANSI 转义码吞掉空格
        let colored = cell.render_padded(10, Align::Left, true);
        assert!(colored.ends_with("    "));
    }

    #[test]
    fn info_card_borders_have_uniform_width() {
        let card = InfoCard::new("账号信息")
            .with_badge("有效", Color::Green)
            .add_plain_row("用户", "测试UP主")
            .add_plain_row("UID", "12345678")
            .add_plain_row("等级", "Lv.6");

        let rendered = card.render(80, false);
        let widths: Vec<usize> = rendered.lines().map(display_width).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "widths={widths:?}\n{rendered}"
        );
    }

    #[test]
    fn info_card_narrow_terminal_maintains_uniform_border_width() {
        let card = InfoCard::new("📅 每日经验与直播签到")
            .with_badge("今日已获 40/65 经验", Color::Red)
            .add_plain_row("每日登录", "✔ 已完成 (+5)");

        let rendered = card.render(36, false);
        let widths: Vec<usize> = rendered.lines().map(display_width).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "narrow InfoCard border width mismatch: widths={widths:?}\n{rendered}"
        );
    }

    #[test]
    fn compute_responsive_widths_shrinks_wide_tables_to_fit_80_cols() {
        let cols = [
            Column::left("类型", 6),
            Column::left("标识", 14),
            Column::left("标题", 34),
            Column::left("UP主/主播", 16),
            Column::left("进度", 10),
            Column::left("观看时间", 16),
        ];
        // 原总宽 = 2 + (6+14+34+16+10+16) + 5*2 = 108 列
        let adapted = compute_responsive_widths(&cols, 80);
        let total = 2 + adapted.iter().sum::<usize>() + (cols.len() - 1) * 2;
        assert_eq!(total, 80);
        assert_eq!(adapted[0], 6); // 窄列保持不变
    }
}
