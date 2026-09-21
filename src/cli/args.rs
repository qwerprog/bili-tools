use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "bt",
    author,
    version,
    about = "B站命令行工具箱 (bili-tools)",
    long_about = None,
    disable_help_subcommand = false,
)]
pub struct Cli {
    /// 以 JSON 格式输出结果
    #[arg(long, global = true)]
    pub json: bool,

    /// 静默模式，不输出提示信息
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 账号与认证管理
    #[command(subcommand)]
    Auth(AuthCommands),

    /// 直播间管理与开播
    #[command(subcommand)]
    Live(LiveCommands),

    /// 内容与用户搜索
    #[command(subcommand)]
    Search(SearchCommands),

    /// 视频操作（详情/点赞/投币/三连）
    #[command(subcommand)]
    Video(VideoCommands),

    /// 收藏夹管理
    #[command(subcommand)]
    Fav(FavCommands),

    /// 稍后再看管理
    #[command(subcommand)]
    Later(LaterCommands),

    /// 观看历史记录
    #[command(subcommand)]
    History(HistoryCommands),

    /// 生成 shell 自动补全脚本
    Completions {
        /// 目标 shell 类型 (bash/zsh/fish)
        #[arg(value_enum)]
        shell: Shell,

        /// 自动安装到系统补全目录
        #[arg(long)]
        install: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// 登录 B站 账号
    Login,

    /// 查看当前登录状态和用户信息
    Status,

    /// 退出登录并清除凭证
    Logout,

    /// 刷新持久化 Cookie
    Refresh,
}

#[derive(Subcommand, Debug)]
pub enum LiveCommands {
    /// 开启直播
    Start {
        /// 指定直播分区 ID，跳过交互式选择
        #[arg(short = 'a', long)]
        area: Option<u32>,

        /// 指定直播标题，跳过输入
        #[arg(short = 't', long)]
        title: Option<String>,

        /// 显示完整推流码（不打码）
        #[arg(short = 's', long)]
        show: bool,

        /// 重新登录（清除 Cookie）
        #[arg(short = 'r', long)]
        relogin: bool,

        /// 所有确认默认选择 yes
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// 停止直播
    Stop {
        /// 延迟下播 (如 30m, 1h30m, 90s)
        #[arg(short = 'd', long)]
        delay: Option<String>,
    },

    /// 查看当前直播状态
    Status,

    /// 查看或修改直播间标题
    Title {
        /// 新的直播标题；若不提供则显示当前标题
        #[arg(short = 't', long)]
        title: Option<String>,

        /// 新的直播标题（位置参数，若不提供则显示当前标题）
        #[arg(value_name = "NEW_TITLE")]
        title_pos: Option<String>,
    },

    /// 查看或搜索直播分区列表
    Area {
        /// 分区搜索关键词
        #[arg(short = 'k', long)]
        keyword: Option<String>,

        /// 分区搜索关键词（位置参数）
        #[arg(value_name = "KEYWORD_ARG")]
        keyword_pos: Option<String>,
    },

    /// 查看已关注主播的开播状态
    Following {
        /// 分页页码
        #[arg(long, default_value = "1")]
        page: u32,

        /// 每页条数 (1-20)
        #[arg(long, default_value = "20")]
        page_size: u32,
    },
}

#[derive(Subcommand, Debug)]
pub enum SearchCommands {
    /// 搜索视频
    Video {
        /// 搜索关键词
        keyword: String,

        /// 页码 (从 1 开始)
        #[arg(long, default_value = "1")]
        page: u32,

        /// 排序方式 (totalrank=综合排序, click=最多点击, pubdate=最新发布, dm=最多弹幕, stow=最多收藏)
        #[arg(long)]
        order: Option<String>,
    },

    /// 搜索用户
    User {
        /// 搜索关键词
        keyword: String,

        /// 页码 (从 1 开始)
        #[arg(long, default_value = "1")]
        page: u32,

        /// 排序方式 (0=默认, fans=粉丝数, level=等级)
        #[arg(long)]
        order: Option<String>,
    },

    /// 搜索直播间
    Live {
        /// 搜索关键词
        keyword: String,

        /// 页码 (从 1 开始)
        #[arg(long, default_value = "1")]
        page: u32,
    },

    /// 获取当前热搜榜单
    Hot {
        /// 返回数量限制 (1-50)
        #[arg(long, default_value = "20")]
        limit: u32,
    },
}

#[derive(Subcommand, Debug)]
pub enum VideoCommands {
    /// 查看视频详细信息
    Info {
        /// 视频 BV号 或 AV号 (如 BV1xx... 或 av12345)
        video: String,
    },

    /// 点赞视频
    Like {
        /// 视频 BV号 或 AV号
        video: String,

        /// 取消点赞
        #[arg(long)]
        cancel: bool,
    },

    /// 为视频投币
    Coin {
        /// 视频 BV号 或 AV号
        video: String,

        /// 投币数量 (1 或 2)
        #[arg(short = 'n', long, default_value = "1")]
        num: u32,

        /// 同时点赞
        #[arg(long)]
        also_like: bool,
    },

    /// 一键三连 (点赞+投币+收藏)
    Triple {
        /// 视频 BV号 或 AV号
        video: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum FavCommands {
    /// 查看收藏夹列表
    List {
        /// 目标用户 mid (默认当前登录账号)
        #[arg(long)]
        mid: Option<i64>,
    },

    /// 查看收藏夹内视频
    Show {
        /// 收藏夹 ID (media_id)
        media_id: i64,

        /// 页码
        #[arg(long, default_value = "1")]
        page: u32,

        /// 每页条数 (1-20)
        #[arg(long, default_value = "20")]
        page_size: u32,
    },

    /// 导出收藏夹全部内容
    Export {
        /// 收藏夹 ID (media_id)
        media_id: i64,

        /// 导出文件路径 (未指定则输出到标准输出)
        #[arg(short = 'o', long)]
        output: Option<String>,

        /// 导出格式 (json, csv, text)
        #[arg(long, default_value = "json")]
        format: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum LaterCommands {
    /// 查看稍后再看列表
    List,

    /// 添加视频到稍后再看
    Add {
        /// 视频 BV号 或 AV号
        video: String,
    },

    /// 清空稍后再看列表
    Clear {
        /// 仅清理已观看过的视频
        #[arg(long)]
        viewed_only: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum HistoryCommands {
    /// 查看观看历史记录
    List {
        /// 获取条数 (1-30)
        #[arg(long, default_value = "20")]
        limit: u32,

        /// 分类筛选 (all=全部, archive=视频, live=直播, article=文章)
        #[arg(short = 't', long)]
        category: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_cli_parsing_subcommands() {
        // bt --json auth status
        let cli = Cli::try_parse_from(["bt", "--json", "auth", "status"]).unwrap();
        assert!(cli.json);
        assert!(!cli.quiet);
        assert!(matches!(cli.command, Commands::Auth(AuthCommands::Status)));

        // bt -q live start -a 123 -t "Test" -y
        let cli = Cli::try_parse_from(["bt", "-q", "live", "start", "-a", "123", "-t", "Test", "-y"]).unwrap();
        assert!(!cli.json);
        assert!(cli.quiet);
        if let Commands::Live(LiveCommands::Start { area, title, yes, .. }) = cli.command {
            assert_eq!(area, Some(123));
            assert_eq!(title, Some("Test".to_string()));
            assert!(yes);
        } else {
            panic!("expected LiveCommands::Start");
        }

        // bt search video "Rust" --page 2
        let cli = Cli::try_parse_from(["bt", "search", "video", "Rust", "--page", "2"]).unwrap();
        if let Commands::Search(SearchCommands::Video { keyword, page, .. }) = cli.command {
            assert_eq!(keyword, "Rust");
            assert_eq!(page, 2);
        } else {
            panic!("expected SearchCommands::Video");
        }

        // bt video coin BV1xx -n 2 --also-like
        let cli = Cli::try_parse_from(["bt", "video", "coin", "BV1xx", "-n", "2", "--also-like"]).unwrap();
        if let Commands::Video(VideoCommands::Coin { video, num, also_like }) = cli.command {
            assert_eq!(video, "BV1xx");
            assert_eq!(num, 2);
            assert!(also_like);
        } else {
            panic!("expected VideoCommands::Coin");
        }

        // bt fav export 12345 -o /tmp/test.csv --format csv
        let cli = Cli::try_parse_from(["bt", "fav", "export", "12345", "-o", "/tmp/test.csv", "--format", "csv"]).unwrap();
        if let Commands::Fav(FavCommands::Export { media_id, output, format }) = cli.command {
            assert_eq!(media_id, 12345);
            assert_eq!(output, Some("/tmp/test.csv".to_string()));
            assert_eq!(format, "csv");
        } else {
            panic!("expected FavCommands::Export");
        }

        // bt later clear --viewed-only
        let cli = Cli::try_parse_from(["bt", "later", "clear", "--viewed-only"]).unwrap();
        if let Commands::Later(LaterCommands::Clear { viewed_only }) = cli.command {
            assert!(viewed_only);
        } else {
            panic!("expected LaterCommands::Clear");
        }

        // bt history list --limit 10 -t archive
        let cli = Cli::try_parse_from(["bt", "history", "list", "--limit", "10", "-t", "archive"]).unwrap();
        if let Commands::History(HistoryCommands::List { limit, category }) = cli.command {
            assert_eq!(limit, 10);
            assert_eq!(category, Some("archive".to_string()));
        } else {
            panic!("expected HistoryCommands::List");
        }

        // bt completions zsh --install
        let cli = Cli::try_parse_from(["bt", "completions", "zsh", "--install"]).unwrap();
        if let Commands::Completions { shell, install } = cli.command {
            assert!(matches!(shell, Shell::Zsh));
            assert!(install);
        } else {
            panic!("expected Completions");
        }
    }

    #[test]
    fn test_legacy_top_level_commands_are_rejected() {
        assert!(Cli::try_parse_from(["bt", "start"]).is_err());
        assert!(Cli::try_parse_from(["bt", "stop"]).is_err());
        assert!(Cli::try_parse_from(["bt", "status"]).is_err());
    }
}
