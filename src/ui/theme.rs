use crossterm::style::{Color, Stylize};
use dialoguer::console::{Style, style};
use dialoguer::theme::ColorfulTheme;
use std::io::IsTerminal;

/// B 站品牌粉 (#FB7299)
pub const BILI_PINK: Color = Color::Rgb {
    r: 251,
    g: 114,
    b: 153,
};

/// B 站品牌蓝 (#00AEEC)
pub const BILI_CYAN: Color = Color::Rgb {
    r: 0,
    g: 174,
    b: 236,
};

fn env_no_color_disabled() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// 检测当前标准输出是否应当启用 ANSI 彩色输出
/// 遵循 --no-color、NO_COLOR 规范与非 TTY 管道自动降级
pub fn color_enabled() -> bool {
    if crate::cli::output::is_no_color() {
        return false;
    }
    if env_no_color_disabled() {
        return false;
    }
    if matches!(std::env::var("TERM").as_deref(), Ok("dumb")) {
        return false;
    }
    std::io::stdout().is_terminal()
}

/// 检测标准错误流是否启用彩色输出
pub fn stderr_color_enabled() -> bool {
    if crate::cli::output::is_no_color() {
        return false;
    }
    if env_no_color_disabled() {
        return false;
    }
    if matches!(std::env::var("TERM").as_deref(), Ok("dumb")) {
        return false;
    }
    std::io::stderr().is_terminal()
}

/// 获取当前终端列宽（回退默认 88 列）
pub fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(88)
}

/// 对单行文本应用从 B 站粉 (#FB7299) -> 过渡紫 -> B 站蓝 (#00AEEC) 的平滑水平渐变
pub fn gradient_pink_to_cyan(text: &str, enable_color: bool) -> String {
    if !enable_color || text.is_empty() {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let total = chars.len().max(1);
    let mut out = String::with_capacity(text.len() * 16);

    for (idx, ch) in chars.into_iter().enumerate() {
        if ch == ' ' {
            out.push(' ');
            continue;
        }
        let t = if total <= 1 {
            0.0
        } else {
            idx as f32 / (total - 1) as f32
        };
        let (r, g, b) = sample_bili_gradient(t);
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m{ch}"));
    }
    out.push_str("\x1b[0m");
    out
}

fn sample_bili_gradient(t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        let local = t * 2.0;
        lerp_rgb((251, 114, 153), (168, 132, 232), local)
    } else {
        let local = (t - 0.5) * 2.0;
        lerp_rgb((168, 132, 232), (0, 174, 236), local)
    }
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let r = (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t).round() as u8;
    let g = (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t).round() as u8;
    let bl = (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t).round() as u8;
    (r, g, bl)
}

/// 用指定颜色渲染文本（非彩色环境下原样返回）
pub fn paint(text: &str, color: Color) -> String {
    if color_enabled() {
        text.with(color).to_string()
    } else {
        text.to_string()
    }
}

/// 用加粗+颜色渲染文本（非彩色环境下原样返回）
pub fn paint_bold(text: &str, color: Color) -> String {
    if color_enabled() {
        text.with(color).bold().to_string()
    } else {
        text.to_string()
    }
}

/// 用指定颜色渲染 stderr 文本（非彩色环境下原样返回）
pub fn paint_stderr(text: &str, color: Color) -> String {
    if stderr_color_enabled() {
        text.with(color).to_string()
    } else {
        text.to_string()
    }
}

/// 暗灰弱化文本
pub fn muted(text: &str) -> String {
    paint(text, Color::DarkGrey)
}

/// 定制 B 站主题风格的 dialoguer 交互配色
pub fn bili_dialoguer_theme() -> ColorfulTheme {
    if !stderr_color_enabled() {
        return ColorfulTheme {
            prompt_prefix: style("?".to_string()).for_stderr(),
            prompt_suffix: style("›".to_string()).for_stderr(),
            success_prefix: style("✔".to_string()).for_stderr(),
            success_suffix: style("·".to_string()).for_stderr(),
            error_prefix: style("✖".to_string()).for_stderr(),
            active_item_prefix: style("❯".to_string()).for_stderr(),
            inactive_item_prefix: style(" ".to_string()).for_stderr(),
            active_item_style: Style::new().for_stderr(),
            inactive_item_style: Style::new().for_stderr(),
            values_style: Style::new().for_stderr(),
            defaults_style: Style::new().for_stderr(),
            prompt_style: Style::new().for_stderr(),
            error_style: Style::new().for_stderr(),
            hint_style: Style::new().for_stderr(),
            checked_item_prefix: style("✔".to_string()).for_stderr(),
            unchecked_item_prefix: style(" ".to_string()).for_stderr(),
            picked_item_prefix: style("❯".to_string()).for_stderr(),
            unpicked_item_prefix: style(" ".to_string()).for_stderr(),
        };
    }

    ColorfulTheme {
        prompt_prefix: style("?".to_string()).for_stderr().magenta().bright().bold(),
        prompt_suffix: style("›".to_string()).for_stderr().black().bright(),
        success_prefix: style("✔".to_string()).for_stderr().green().bold(),
        success_suffix: style("·".to_string()).for_stderr().black().bright(),
        error_prefix: style("✖".to_string()).for_stderr().red().bold(),
        active_item_prefix: style("❯".to_string()).for_stderr().cyan().bright().bold(),
        inactive_item_prefix: style(" ".to_string()).for_stderr(),
        active_item_style: Style::new().for_stderr().cyan().bright().bold(),
        values_style: Style::new().for_stderr().magenta().bright(),
        defaults_style: Style::new().for_stderr().black().bright(),
        prompt_style: Style::new().for_stderr().bold(),
        ..ColorfulTheme::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ResetNoColorGuard;
    impl Drop for ResetNoColorGuard {
        fn drop(&mut self) {
            crate::cli::output::set_no_color(false);
        }
    }

    #[test]
    fn no_color_disables_stdout_and_stderr_colors() {
        let _lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _reset = ResetNoColorGuard;

        crate::cli::output::set_no_color(true);
        assert!(!color_enabled());
        assert!(!stderr_color_enabled());
        assert_eq!(paint_stderr("ok", Color::Green), "ok");
        let _theme = bili_dialoguer_theme();
    }

    #[test]
    fn empty_no_color_env_var_does_not_force_disable_color() {
        let _env_lock = crate::api::client::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _out_lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        struct Guard(Option<std::ffi::OsString>);
        impl Drop for Guard {
            fn drop(&mut self) {
                crate::cli::output::set_no_color(false);
                unsafe {
                    match &self.0 {
                        Some(v) => std::env::set_var("NO_COLOR", v),
                        None => std::env::remove_var("NO_COLOR"),
                    }
                }
            }
        }
        let _g = Guard(std::env::var_os("NO_COLOR"));
        crate::cli::output::set_no_color(false);
        unsafe {
            std::env::set_var("NO_COLOR", "");
        }
        assert!(!env_no_color_disabled());

        unsafe {
            std::env::set_var("NO_COLOR", "1");
        }
        assert!(env_no_color_disabled());
        assert!(!color_enabled());
        assert!(!stderr_color_enabled());
    }
}
