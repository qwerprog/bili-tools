use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn mask_rtmp_code(code: &str) -> String {
    if code.len() <= 10 {
        return code.to_string();
    }
    let prefix = &code[..6];
    let suffix = &code[code.len() - 4..];
    let masked_length = code.len() - 10;
    format!(
        "{}{}...{}",
        prefix,
        "*".repeat(masked_length.min(12)),
        suffix
    )
}

/// 计算字符串在终端中的显示列宽（CJK 宽字符计为 2 列）
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// 按照终端显示宽度截断字符串。如果超出 max_width，则截断并在末尾添加单宽省略号 `…`
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    if display_width(s) <= max_width {
        return s.to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = display_width(ellipsis);
    let target_width = max_width.saturating_sub(ellipsis_width);

    let mut current_width = 0;
    let mut result = String::new();

    for c in s.chars() {
        let char_width = UnicodeWidthChar::width(c).unwrap_or(0);
        if current_width + char_width > target_width {
            break;
        }
        current_width += char_width;
        result.push(c);
    }

    result.push_str(ellipsis);
    result
}

/// 按照终端显示宽度向右填充空格，使字符串占据 target_width 列
pub fn pad_to_width(s: &str, target_width: usize) -> String {
    let w = display_width(s);
    if w >= target_width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(target_width - w))
    }
}

/// 将秒数格式化为时长 (hh:mm:ss 或 mm:ss)
pub fn format_duration(seconds: i64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

/// 将 Unix 时间戳格式化为北京时间 (UTC+8) 字符串
pub fn format_timestamp(ts: i64) -> String {
    if ts <= 0 {
        return "-".to_string();
    }
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| {
            let beijing = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
            dt.with_timezone(&beijing)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "-".to_string())
}

/// 将数量格式化为带 "万" 单位的简洁字符串
pub fn format_num(val: &serde_json::Value) -> String {
    if let Some(n) = val.as_i64() {
        if n >= 10_000 {
            format!("{:.1}万", n as f64 / 10_000.0)
        } else {
            n.to_string()
        }
    } else if let Some(s) = val.as_str() {
        if let Ok(n) = s.parse::<i64>() {
            if n >= 10_000 {
                format!("{:.1}万", n as f64 / 10_000.0)
            } else {
                s.to_string()
            }
        } else {
            s.to_string()
        }
    } else {
        "-".to_string()
    }
}

/// 剥离所有 HTML 标签并转换常用转义实体
pub fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;

    for c in input.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' && in_tag {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }

    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_width_and_padding() {
        // "你好" is 2 CJK characters = 4 display cells
        assert_eq!(display_width("你好"), 4);
        assert_eq!(display_width("hello"), 5);

        let padded = pad_to_width("你好", 10);
        assert_eq!(display_width(&padded), 10);
        assert_eq!(padded, "你好      ");
    }

    #[test]
    fn test_truncate_to_width() {
        // "中文字符串测试" has width 14
        let truncated = truncate_to_width("中文字符串测试", 8);
        assert!(display_width(&truncated) <= 8);
        assert!(truncated.ends_with('…'));

        // Short string not truncated
        assert_eq!(truncate_to_width("abc", 10), "abc");
    }

    #[test]
    fn test_format_timestamp() {
        // 1603285176 is 2020-10-21 20:59:36 UTC+8
        assert_eq!(format_timestamp(1603285176), "2020-10-21 20:59");
        assert_eq!(format_timestamp(0), "-");
        assert_eq!(format_timestamp(-10), "-");
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(
            strip_html_tags("<em class=\"keyword\">Rust</em> &amp; &lt;C++&gt;"),
            "Rust & <C++>"
        );
        assert_eq!(
            strip_html_tags("<span style=\"color:red\">Title&#39;s</span>"),
            "Title's"
        );
    }
}
