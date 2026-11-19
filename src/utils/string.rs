use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn mask_rtmp_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let len = chars.len();
    if len == 0 {
        return String::new();
    }
    if len <= 4 {
        return "*".repeat(len.max(3));
    }
    if len <= 10 {
        let prefix: String = chars[..2].iter().collect();
        let suffix: String = chars[len - 2..].iter().collect();
        let masked_len = len.saturating_sub(4).max(3);
        return format!("{prefix}{}...{suffix}", "*".repeat(masked_len));
    }
    let prefix: String = chars[..6].iter().collect();
    let suffix: String = chars[len - 4..].iter().collect();
    let masked_length = len - 10;
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
    if max_width == 0 {
        return String::new();
    }
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

/// 反转义常见 HTML 字符实体（如 B 站评论中的 `&#34;`、`&#39;`、`&quot;`、`&amp;`），单遍扫描避免二次解码
pub fn unescape_html_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'&'
            && let Some(rel_semi) = input[i..].find(';')
            && rel_semi <= 10
        {
            let ent = &input[i + 1..i + rel_semi];
            let decoded = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" | "#34" | "#x22" | "#X22" => Some('"'),
                "apos" | "#39" | "#x27" | "#X27" => Some('\''),
                "nbsp" | "#160" => Some(' '),
                _ if ent.starts_with('#') => {
                    let code = if let Some(hex) = ent[1..]
                        .strip_prefix('x')
                        .or_else(|| ent[1..].strip_prefix('X'))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        ent[1..].parse::<u32>().ok()
                    };
                    code.and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                i += rel_semi + 1;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
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

    unescape_html_entities(&out)
}

/// 清理文件名中的非法路径字符和控制字符（用于视频下载输出模板插值）
pub fn sanitize_filename(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
            sanitized.push('_');
        } else {
            sanitized.push(c);
        }
    }
    let trimmed = sanitized.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if trimmed.is_empty() {
        "video".to_string()
    } else {
        trimmed.to_string()
    }
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
    fn test_truncate_to_width_zero_and_one() {
        assert_eq!(truncate_to_width("abc", 0), "");
        assert_eq!(truncate_to_width("中文", 0), "");
        assert_eq!(truncate_to_width("abc", 1), "…");
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

    #[test]
    fn test_mask_rtmp_code_short_standard_and_utf8() {
        assert_eq!(mask_rtmp_code(""), "");
        assert_eq!(mask_rtmp_code("abc"), "***");
        assert_eq!(mask_rtmp_code("abcd"), "****");

        let short_secret = mask_rtmp_code("secret");
        assert_ne!(short_secret, "secret");
        assert!(short_secret.contains('*'));
        assert!(short_secret.starts_with("se"));
        assert!(short_secret.ends_with("et"));

        let ten = mask_rtmp_code("1234567890");
        assert_ne!(ten, "1234567890");
        assert!(ten.starts_with("12"));
        assert!(ten.ends_with("90"));
        assert!(ten.contains('*'));

        let standard = mask_rtmp_code("1234567890abcdef");
        assert!(standard.starts_with("123456"));
        assert!(standard.ends_with("cdef"));
        assert!(standard.contains("******..."));

        // Multi-byte UTF-8 must not panic on byte slicing
        let utf8_short = mask_rtmp_code("秘密推流码测试字符");
        assert!(utf8_short.starts_with("秘密"));
        assert!(utf8_short.ends_with("字符"));
        assert!(utf8_short.contains('*'));

        let utf8_long = mask_rtmp_code("一二三四五六七八九十一二三四");
        assert!(utf8_long.starts_with("一二三四五六"));
        assert!(utf8_long.ends_with("一二三四"));
        assert!(utf8_long.contains('*'));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("A/B:C*?<>|\"test"), "A_B_C______test");
        assert_eq!(sanitize_filename("hello\x00\n\tworld"), "hello___world");
        assert_eq!(sanitize_filename("   ...  "), "video");
        assert_eq!(sanitize_filename(". ."), "video");
        assert_eq!(sanitize_filename(".."), "video");
        assert_eq!(sanitize_filename("v1.0 release"), "v1.0 release");
        assert_eq!(sanitize_filename("正常视频标题 123"), "正常视频标题 123");
    }
}
