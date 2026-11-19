use crate::ui::theme::{
    BILI_CYAN, BILI_PINK, color_enabled, gradient_pink_to_cyan, terminal_width,
};
use crate::utils::string::pad_to_width;
use crossterm::style::{Color, Stylize};
use std::io::Write;

const TV_FULL: [&str; 8] = [
    r"      \       /    ",
    r"  ╭────┴─────┴────╮",
    r"  │ ╭───────────╮ │",
    r"  │ │  \     /  │ │",
    r"  │ │     w     │ │",
    r"  │ ╰───────────╯ │",
    r"  ╰────┬─────┬────╯",
    r"       ▀     ▀     ",
];

const WORDMARK_LINES: [&str; 3] = [
    "██▀▀█▄ ██ ██    ██      ▀▀██▀▀ ▄█▀▀█▄ ▄█▀▀█▄ ██    ▄█▀▀▀▀",
    "██▀▀█▄ ██ ██    ██ ▀▀▀▀   ██   ██  ██ ██  ██ ██     ▀▀▀█▄",
    "██▄▄█▀ ██ ██▄▄▄ ██        ██   ▀█▄▄█▀ ▀█▄▄█▀ ██▄▄▄ ▄▄▄▄█▀",
];

const TV_COMPACT: [&str; 5] = [
    r"    \     /  ",
    r"  ╭──┴───┴──╮",
    r"  │  \   /  │",
    r"  │    w    │",
    r"  ╰──┬───┬──╯",
];

/// 根据终端列宽与彩色开关生成自适应 ASCII/Unicode Logo 横幅
pub fn render_banner(cols: usize, enable_color: bool) -> String {
    let version = env!("CARGO_PKG_VERSION");

    if cols >= 79 {
        let mut lines = Vec::with_capacity(8);
        for (idx, tv_part) in TV_FULL.iter().enumerate() {
            let tv_rendered = if enable_color {
                tv_part.with(BILI_PINK).bold().to_string()
            } else {
                (*tv_part).to_string()
            };

            let line = match idx {
                0 | 4 | 7 => tv_rendered.trim_end().to_string(),
                1..=3 => {
                    let wm = gradient_pink_to_cyan(WORDMARK_LINES[idx - 1], enable_color);
                    format!("{tv_rendered}   {wm}")
                }
                5 => {
                    let tagline = if enable_color {
                        format!(
                            "{} {} {}",
                            "Bilibili CLI Toolbox".with(BILI_CYAN).bold(),
                            "·".dark_grey(),
                            "一站式 B 站命令行工具箱".white()
                        )
                    } else {
                        "Bilibili CLI Toolbox · 一站式 B 站命令行工具箱".to_string()
                    };
                    format!("{tv_rendered}   {tagline}")
                }
                6 => {
                    let meta = if enable_color {
                        format!(
                            "{} {} {} {}",
                            format!("bit v{version}").with(BILI_PINK).bold(),
                            "·".dark_grey(),
                            "Live · Video · Search · Fav · History".dark_grey(),
                            "· Rust".dark_grey()
                        )
                    } else {
                        format!("bit v{version} · Live · Video · Search · Fav · History · Rust")
                    };
                    format!("{tv_rendered}   {meta}")
                }
                _ => tv_rendered,
            };
            lines.push(line);
        }
        lines.join("\n")
    } else {
        // 窄屏终端（如分屏窗口 < 79 列）自动切换迷你小电视横幅，绝不断行或截断
        let mut lines = Vec::with_capacity(5);
        for (idx, tv_part) in TV_COMPACT.iter().enumerate() {
            let tv_rendered = if enable_color {
                tv_part.with(BILI_PINK).bold().to_string()
            } else {
                (*tv_part).to_string()
            };
            let line = match idx {
                0 | 4 => tv_rendered.trim_end().to_string(),
                1 => {
                    let title = gradient_pink_to_cyan("BILI-TOOLS", enable_color);
                    let ver = if enable_color {
                        format!("v{version}").dark_grey().to_string()
                    } else {
                        format!("v{version}")
                    };
                    format!("{tv_rendered}   {title} {ver}")
                }
                2 => {
                    let sub = if enable_color {
                        "一站式 B 站命令行工具箱".with(BILI_CYAN).to_string()
                    } else {
                        "一站式 B 站命令行工具箱".to_string()
                    };
                    format!("{tv_rendered}   {sub}")
                }
                3 => {
                    let mods = if enable_color {
                        "直播 · 视频 · 搜索 · 收藏 · 历史".dark_grey().to_string()
                    } else {
                        "直播 · 视频 · 搜索 · 收藏 · 历史".to_string()
                    };
                    format!("{tv_rendered}   {mods}")
                }
                _ => tv_rendered,
            };
            lines.push(line);
        }
        lines.join("\n")
    }
}

/// 向标准输出打印自适应 Logo 横幅（对管道提前关闭如 `| head` 安全忽略 BrokenPipe）
pub fn print_banner() {
    if crate::cli::output::is_quiet() || crate::cli::output::is_json() {
        return;
    }
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", render_banner(terminal_width(), color_enabled()));
    let _ = writeln!(out);
}

/// 渲染顶层欢迎页与分组帮助指南文本
pub(crate) fn render_welcome_help(cols: usize, color: bool) -> String {
    let mut out = String::with_capacity(2560);
    use std::fmt::Write as _;

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", render_banner(cols, color));
    let _ = writeln!(out);

    let heading = |text: &str| -> String {
        if color {
            text.with(BILI_PINK).bold().to_string()
        } else {
            text.to_string()
        }
    };
    let cmd_style = |name: &str, width: usize| -> String {
        let padded = pad_to_width(name, width);
        if color {
            padded.with(BILI_CYAN).bold().to_string()
        } else {
            padded
        }
    };
    let dim = |text: &str| -> String {
        if color {
            text.dark_grey().to_string()
        } else {
            text.to_string()
        }
    };

    let _ = writeln!(out, "  {}", heading("用法 Usage"));
    if color {
        let _ = writeln!(
            out,
            "    {} {} {}",
            "bit".with(BILI_PINK).bold(),
            "[全局选项]".dark_grey(),
            "<命令> [子命令] [参数]".with(Color::White)
        );
    } else {
        let _ = writeln!(out, "    bit [全局选项] <命令> [子命令] [参数]");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "  {}", heading("核心模块 Commands"));
    let commands = [
        ("resolve", "通用标识解析（BV / AV / EP / SS / MD / 各类 URL / b23.tv 短链接）"),
        ("auth", "账号认证管理（登录 / 状态 / 刷新 Cookie / 每日经验与直播签到）"),
        ("live", "直播间场控与互动（开播 / 下播 / 弹幕聊天室 / 拉流 / 录制）"),
        ("search", "全站搜索与榜单（实时热搜榜 / 搜索视频 / 用户 / 直播间）"),
        ("video", "视频全链路操作（详情 / 流 / 字幕 / 总结 / 弹幕 / 评论 / 下载 / 互动）"),
        ("user", "用户主页与关系（用户详情 / 投稿列表 / 空间动态 / 关注与取关）"),
        ("feed", "关注动态时间线（支持全部 / 视频 / 番剧 / 专栏筛选与游标翻页）"),
        ("rank", "全站排行榜与热门（分区视频榜 / 综合热门视频）"),
        ("fav", "收藏夹管理（列表 / 浏览 / 导出 / 增删视频 / 清理失效 / 新建与删除）"),
        ("later", "稍后再看管理（列表 / 添加 / 移除 / 清理已观看）"),
        ("history", "观看历史记录（支持 -c 按视频 / 直播 / 专栏筛选与单条/清空删除）"),
        ("completions", "生成并安装 Bash / Zsh / Fish / PowerShell / Elvish 补全脚本"),
    ];
    for (name, desc) in commands {
        let _ = writeln!(out, "    {}  {}", cmd_style(name, 13), desc);
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "  {}", heading("全局选项 Global Options"));
    let flags = [
        ("--json", "以结构化 JSON 输出结果（便于脚本与 AI Agent 调用）"),
        ("-q, --quiet", "静默模式，隐藏装饰与过程提示信息"),
        ("-y, --yes", "自动确认交互式或危险操作"),
        ("--no-color", "禁用终端彩色输出（等价于 NO_COLOR=1）"),
        ("-u, --cookie-file <PATH>", "指定自定义 Cookie 文件路径"),
        ("-h, --help", "显示帮助信息（支持子命令，如 `bit live -h`）"),
        ("-V, --version", "显示当前版本号"),
    ];
    for (flag, desc) in flags {
        let _ = writeln!(out, "    {}  {}", cmd_style(flag, 25), desc);
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "  {}", heading("快捷示例 Quick Start"));
    let examples = [
        ("bit auth login", "终端扫码登录 B 站账号"),
        ("bit live start", "交互式选择分区与标题并一键获取 OBS 推流码"),
        ("bit live chat", "打开看发一体的终端实时直播弹幕聊天室 (TUI)"),
        ("bit video download BV1xx...", "下载视频与弹幕字幕（DASH 混流需 ffmpeg）"),
        ("bit user info 293793435", "查看指定 UP 主主页信息与粉丝数据"),
        ("bit rank popular", "查看 B 站当前综合热门视频"),
        ("bit search hot", "查看 B 站实时热搜榜单"),
    ];
    for (ex, comment) in examples {
        let padded = pad_to_width(ex, 30);
        let rendered_ex = if color {
            padded.with(Color::White).to_string()
        } else {
            padded
        };
        let _ = writeln!(out, "    {}  {}", rendered_ex, dim(&format!("# {comment}")));
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  {}",
        dim("提示: 输入 `bit <命令> --help` 查看各模块的详细子命令与参数说明。")
    );

    out
}

/// 打印顶层欢迎页与分组彩色帮助指南（用于空参 `bit` 或 `bit --help`）
pub fn print_welcome_help() {
    let out = render_welcome_help(terminal_width(), color_enabled());
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(out.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::string::display_width;

    #[test]
    fn full_banner_fits_standard_80_col_terminal_without_ansi_when_disabled() {
        let banner = render_banner(80, false);
        assert!(!banner.contains("\x1b["));
        for line in banner.lines() {
            assert!(
                display_width(line) <= 80,
                "line exceeded 80 cols ({}): {}",
                display_width(line),
                line
            );
        }
    }

    #[test]
    fn compact_banner_fits_narrow_pane() {
        let banner = render_banner(60, false);
        assert!(!banner.contains("\x1b["));
        for line in banner.lines() {
            assert!(
                display_width(line) <= 52,
                "compact line exceeded 52 cols ({}): {}",
                display_width(line),
                line
            );
        }
    }

    #[test]
    fn welcome_help_lists_all_twelve_modules_without_ansi_when_disabled() {
        let help = render_welcome_help(80, false);
        assert!(!help.contains("\x1b["));
        for module in [
            "resolve",
            "auth",
            "live",
            "search",
            "video",
            "user",
            "feed",
            "rank",
            "fav",
            "later",
            "history",
            "completions",
        ] {
            assert!(help.contains(module), "missing module {module} in welcome help");
        }
        assert!(help.contains("--json"));
        assert!(help.contains("--cookie-file"));
    }
}
