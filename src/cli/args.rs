use crate::api::upload::{StudioArchiveStatus, UploadLine};
use clap::builder::styling::{AnsiColor, Color as ClapColor, RgbColor, Styles};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

pub fn bili_clap_styles() -> Styles {
    Styles::styled()
        .header(ClapColor::Rgb(RgbColor(251, 114, 153)).on_default().bold())
        .usage(ClapColor::Rgb(RgbColor(251, 114, 153)).on_default().bold())
        .literal(ClapColor::Rgb(RgbColor(0, 174, 236)).on_default().bold())
        .placeholder(ClapColor::Ansi(AnsiColor::BrightBlack).on_default())
        .valid(ClapColor::Ansi(AnsiColor::Green).on_default())
        .invalid(ClapColor::Ansi(AnsiColor::Red).on_default().bold())
        .error(ClapColor::Ansi(AnsiColor::Red).on_default().bold())
}

#[derive(Parser, Debug)]
#[command(
    name = "bit",
    author,
    version,
    about = "B站命令行工具箱 (bili-tools)",
    long_about = None,
    disable_help_subcommand = false,
    styles = bili_clap_styles(),
)]
pub struct Cli {
    /// 以 JSON 格式输出结果
    #[arg(long, global = true)]
    pub json: bool,

    /// 静默模式，不输出提示信息
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// 输出详细调试日志 (-v=info, -vv=debug, -vvv=trace；等价于 RUST_LOG)
    #[arg(short = 'v', long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// 自动确认交互式或危险操作
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// 禁用终端彩色输出 (等价于 NO_COLOR=1)
    #[arg(long, global = true)]
    pub no_color: bool,

    /// 指定自定义 Cookie 文件路径
    #[arg(short = 'u', long, global = true, value_name = "PATH")]
    pub cookie_file: Option<PathBuf>,

    /// 国内直连模式：通过公共 DoH 与原生 IPv6 绕过本机代理/TUN 虚拟网卡（不断代理即可开播）
    #[arg(short = 'D', long, global = true)]
    pub direct: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    #[allow(clippy::enum_variant_names)]
    #[value(name = "powershell")]
    PowerShell,
    Elvish,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginMethod {
    Qr,
    Tv,
    Sms,
    Password,
    Cookie,
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

    /// 视频操作（详情/流/字幕/总结/弹幕/评论/下载/互动）
    #[command(subcommand)]
    Video(VideoCommands),

    /// 用户主页、投稿、动态与关注管理
    #[command(subcommand)]
    User(UserCommands),

    /// 查看已关注 UP 主动态流
    Feed {
        /// 动态类型过滤 (all, video, pgc, article)
        #[arg(long = "type", value_enum, default_value_t = FeedType::All)]
        feed_type: FeedType,

        /// 分页游标 (来自上次返回的 offset)
        #[arg(long, value_name = "CURSOR")]
        offset: Option<String>,
    },

    /// 全站视频排行榜与热门视频
    #[command(subcommand)]
    Rank(RankCommands),

    /// 收藏夹管理
    #[command(subcommand)]
    Fav(FavCommands),

    /// 稍后再看管理
    #[command(subcommand)]
    Later(LaterCommands),

    /// 观看历史记录
    #[command(subcommand)]
    History(HistoryCommands),

    /// 解析 B站视频/番剧/直播间/用户/收藏夹 ID、URL 或 b23.tv 短链接
    Resolve {
        /// 输入的 B站 ID、URL 或短链接
        input: String,

        /// 禁用网络请求（不展开 b23.tv 短链接）
        #[arg(long)]
        no_network: bool,
    },

    /// 生成 shell 自动补全脚本
    Completions {
        /// 目标 shell 类型 (bash/zsh/fish/powershell/elvish；省略时自动检测 $SHELL)
        #[arg(value_enum)]
        shell: Option<Shell>,

        /// 自动安装到系统补全目录
        #[arg(long)]
        install: bool,

        /// 指定生成 Zsh 补全脚本（等价于位置参数 zsh）
        #[arg(long, hide = true, conflicts_with_all = ["shell", "bash", "fish", "powershell", "elvish"])]
        zsh: bool,

        /// 指定生成 Bash 补全脚本（等价于位置参数 bash）
        #[arg(long, hide = true, conflicts_with_all = ["shell", "zsh", "fish", "powershell", "elvish"])]
        bash: bool,

        /// 指定生成 Fish 补全脚本（等价于位置参数 fish）
        #[arg(long, hide = true, conflicts_with_all = ["shell", "zsh", "bash", "powershell", "elvish"])]
        fish: bool,

        /// 指定生成 PowerShell 补全脚本（等价于位置参数 powershell）
        #[arg(long, hide = true, conflicts_with_all = ["shell", "zsh", "bash", "fish", "elvish"])]
        powershell: bool,

        /// 指定生成 Elvish 补全脚本（等价于位置参数 elvish）
        #[arg(long, hide = true, conflicts_with_all = ["shell", "zsh", "bash", "fish", "powershell"])]
        elvish: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// 登录 B站 账号
    Login {
        /// 登录方式 (qr/tv/sms/password/cookie)
        #[arg(short = 'm', long, value_enum)]
        method: Option<LoginMethod>,

        /// 直接提供 SESSDATA (用于 --method cookie)
        #[arg(long)]
        sessdata: Option<String>,

        /// 直接提供 bili_jct / CSRF token (用于 --method cookie)
        #[arg(long)]
        bili_jct: Option<String>,

        /// 直接提供 UID / DedeUserID (用于 --method cookie)
        #[arg(long = "dede-user-id", visible_alias = "uid")]
        dede_user_id: Option<String>,
    },

    /// 查看当前登录状态和用户信息
    Status,

    /// 退出登录并清除凭证
    Logout,

    /// 刷新持久化 Cookie
    Refresh,

    /// 查询今日每日经验奖励并执行直播签到
    Daily,
}

pub fn parse_video_quality(raw: &str) -> std::result::Result<u32, String> {
    let s = raw.trim().to_ascii_lowercase();
    match s.as_str() {
        "best" | "max" | "orig" | "raw" | "8k" => Ok(127),
        "dolby" => Ok(126),
        "hdr" => Ok(125),
        "4k" | "uhd" | "2160p" => Ok(120),
        "1080p60" | "1080p+" => Ok(116),
        "1080phigh" => Ok(112),
        "1080p" | "fhd" | "bluray" => Ok(80),
        "720p60" => Ok(74),
        "720p" | "hd" => Ok(64),
        "480p" | "sd" => Ok(32),
        "360p" | "low" | "smooth" => Ok(16),
        _ => match s.parse::<u32>() {
            Ok(n) if n > 0 => Ok(n),
            _ => Err(format!(
                "无效的视频画质 '{raw}'；可选语义值: best/8k/dolby/hdr/4k/1080p60/1080p/720p/480p/360p 或正整数 QN 编号 (如 120/80/64)"
            )),
        },
    }
}

pub fn parse_live_quality(raw: &str) -> std::result::Result<u32, String> {
    let s = raw.trim().to_ascii_lowercase();
    match s.as_str() {
        "best" | "max" | "orig" | "raw" => Ok(10000),
        "dolby" => Ok(30000),
        "4k" | "uhd" | "2160p" => Ok(20000),
        "1080p60" | "1080p" | "fhd" | "bluray" => Ok(400),
        "720p" | "hd" | "super" => Ok(250),
        "480p" | "sd" => Ok(150),
        "360p" | "low" | "smooth" => Ok(80),
        _ => match s.parse::<u32>() {
            Ok(n) if n > 0 => Ok(n),
            _ => Err(format!(
                "无效的直播画质 '{raw}'；可选语义值: orig/best/4k/dolby/bluray(1080p)/720p/480p/smooth(360p) 或正整数 QN 编号 (如 10000/400/250/150/80)"
            )),
        },
    }
}

pub fn parse_audio_quality(raw: &str) -> std::result::Result<u32, String> {
    let s = raw.trim().to_ascii_lowercase();
    match s.as_str() {
        "best" | "max" | "hires" | "flac" | "lossless" => Ok(30251),
        "dolby" | "atmos" => Ok(30250),
        "192k" | "high" => Ok(30280),
        "132k" | "medium" | "normal" => Ok(30232),
        "64k" | "low" => Ok(30216),
        _ => match s.parse::<u32>() {
            Ok(n) if n > 0 => Ok(n),
            _ => Err(format!(
                "无效的音质 '{raw}'；可选语义值: best/flac(hires)/dolby/192k/132k/64k 或音频流 ID (30251/30250/30280/30232/30216)"
            )),
        },
    }
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
    },

    /// 停止直播
    Stop {
        /// 延迟下播 (如 30m, 1h30m, 90s)
        #[arg(short = 'd', long)]
        delay: Option<String>,
    },

    /// 查看当前直播状态
    Status,

    /// 查看直播间详细信息（不指定房间号则查看自己的直播间）
    Info {
        /// 直播间号、短号、直播间 URL 或 b23.tv 短链接
        #[arg(value_name = "ROOM")]
        room: Option<String>,
    },

    /// 修改直播间标题
    Title {
        /// 新的直播标题
        #[arg(value_name = "TITLE")]
        title: String,
    },

    /// 查看或搜索直播分区列表
    #[command(visible_alias = "areas")]
    Area {
        /// 分区搜索关键词
        #[arg(value_name = "KEYWORD")]
        keyword: Option<String>,
    },

    /// 查看已关注主播的开播状态
    Following {
        /// 分页页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 每页条数 (1-20)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=20))]
        page_size: u32,
    },

    /// 获取直播间拉流地址与可用清晰度
    #[command(visible_alias = "streams")]
    Stream {
        /// 直播间号、短号、直播间 URL 或 b23.tv 短链接
        #[arg(value_name = "ROOM")]
        room: String,

        /// 目标画质 (支持 orig/best/4k/1080p/bluray/720p/480p/smooth 或编号 10000/400/250/150/80)
        #[arg(short = 'Q', long, default_value = "orig", value_parser = parse_live_quality)]
        quality: u32,
    },

    /// 查看直播间弹幕（支持快照、-f 实时流或 -i 聊天室面板）
    Danmaku {
        /// 直播间号、短号、直播间 URL 或 b23.tv 短链接
        #[arg(value_name = "ROOM")]
        room: String,

        /// 返回弹幕条数上限 (1-50；快照模式默认 20，配合 -f 时达到上限自动退出)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=50))]
        limit: Option<u32>,

        /// 持续监听实时弹幕流 (WebSocket)
        #[arg(short = 'f', long, conflicts_with = "interactive")]
        follow: bool,

        /// 启动看发一体的终端交互式弹幕聊天室 (TUI)
        #[arg(short = 'i', long, conflicts_with = "follow")]
        interactive: bool,

        /// 隐藏观众进房提示消息
        #[arg(long)]
        no_enter: bool,
    },

    /// 进入看发一体的终端交互式直播弹幕聊天室（不填房间号默认进入自己的直播间）
    Chat {
        /// 直播间号、短号、直播间 URL 或 b23.tv 短链接（留空默认进入自己的直播间）
        #[arg(value_name = "ROOM")]
        room: Option<String>,

        /// 初始隐藏观众进房提示消息（可在界面内按 Ctrl+T 或输入 /enter 随时切换）
        #[arg(long)]
        no_enter: bool,

        /// 预加载最近历史弹幕条数 (0-50)
        #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u32).range(0..=50))]
        history: u32,
    },

    /// 向直播间发送弹幕
    Send {
        /// 直播间号、短号、直播间 URL 或 b23.tv 短链接
        #[arg(value_name = "ROOM")]
        room: String,

        /// 弹幕内容
        #[arg(value_name = "MESSAGE")]
        message: String,
    },

    /// 录制直播流到本地文件 (支持同步录制弹幕与开播自动蹲守，需要 ffmpeg)
    Record {
        /// 直播间号、短号、直播间 URL 或 b23.tv 短链接
        #[arg(value_name = "ROOM")]
        room: String,

        /// 输出文件路径或目录
        #[arg(short = 'o', long, value_name = "PATH")]
        output: Option<PathBuf>,

        /// 目标画质 (支持 orig/best/4k/1080p/bluray/720p/480p/smooth 或编号 10000/400/250/150/80)
        #[arg(short = 'Q', long, default_value = "orig", value_parser = parse_live_quality)]
        quality: u32,

        /// 单次录制时长 (如 30s, 5m, 1h30m)
        #[arg(short = 'd', long, value_name = "DURATION")]
        duration: Option<String>,

        /// 同步录制时间轴对齐的直播实时弹幕
        #[arg(long)]
        danmaku: bool,

        /// 同步弹幕保存格式 (ass 播放器直读 或 xml 标准格式)
        #[arg(long, value_enum, default_value_t = DownloadDanmakuFormat::Ass)]
        danmaku_format: DownloadDanmakuFormat,

        /// 无人值守模式：未开播时自动轮询蹲守，开播即录、断流自动重连切分段
        #[arg(short = 'w', long)]
        watch: bool,

        /// 蹲守模式下检测开播状态的轮询间隔秒数 (5-3600)
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(5..=3600))]
        interval: u64,

        /// 配合 --watch 使用：录完一场主播下播后即退出（默认持续蹲守下一场）
        #[arg(long)]
        once: bool,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchVideoOrder {
    /// 综合排序
    Totalrank,
    /// 最多点击
    Click,
    /// 最新发布
    Pubdate,
    /// 最多弹幕
    Dm,
    /// 最多收藏
    Stow,
}

impl SearchVideoOrder {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Totalrank => "totalrank",
            Self::Click => "click",
            Self::Pubdate => "pubdate",
            Self::Dm => "dm",
            Self::Stow => "stow",
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchUserOrder {
    /// 默认排序
    Default,
    /// 粉丝数由高到低
    Fans,
    /// 等级由高到低
    Level,
}

impl SearchUserOrder {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Default => "0",
            Self::Fans => "fans",
            Self::Level => "level",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum SearchCommands {
    /// 搜索视频
    Video {
        /// 搜索关键词
        keyword: String,

        /// 页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 排序方式
        #[arg(long, value_enum)]
        order: Option<SearchVideoOrder>,
    },

    /// 搜索用户
    User {
        /// 搜索关键词
        keyword: String,

        /// 页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 排序方式
        #[arg(long, value_enum)]
        order: Option<SearchUserOrder>,
    },

    /// 搜索直播间
    Live {
        /// 搜索关键词
        keyword: String,

        /// 页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
    },

    /// 获取当前热搜榜单
    Hot {
        /// 返回数量限制 (1-50)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=50))]
        limit: u32,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleFormat {
    Srt,
    Txt,
    Json,
}

impl SubtitleFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Txt => "txt",
            Self::Json => "json",
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DanmakuFormat {
    Xml,
    Ass,
    Txt,
    Json,
}

impl DanmakuFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xml => "xml",
            Self::Ass => "ass",
            Self::Txt => "txt",
            Self::Json => "json",
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentSort {
    /// 按热度排序
    Hot,
    /// 按时间排序
    Time,
}

impl CommentSort {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Hot => "2",
            Self::Time => "0",
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadCodec {
    #[value(alias = "h264", alias = "x264")]
    Avc,
    #[value(alias = "h265", alias = "x265")]
    Hevc,
    Av1,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadDanmakuFormat {
    Ass,
    Xml,
}

impl DownloadCodec {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Avc => "avc",
            Self::Hevc => "hevc",
            Self::Av1 => "av1",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum VideoCommentCommands {
    /// 发表视频评论或回复
    Post {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        #[arg(value_name = "VIDEO")]
        video: String,

        /// 评论内容
        #[arg(value_name = "MESSAGE")]
        message: String,

        /// 根评论 RPID（回复评论时使用）
        #[arg(long, value_name = "RPID")]
        root: Option<i64>,

        /// 父评论 RPID（回复二级评论时使用）
        #[arg(long, value_name = "RPID")]
        parent: Option<i64>,
    },

    /// 点赞或取消点赞视频评论
    Like {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        #[arg(value_name = "VIDEO")]
        video: String,

        /// 评论 RPID
        #[arg(value_name = "RPID", value_parser = clap::value_parser!(i64).range(1..))]
        rpid: i64,

        /// 取消点赞
        #[arg(long)]
        cancel: bool,
    },

    /// 删除视频评论
    Delete {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        #[arg(value_name = "VIDEO")]
        video: String,

        /// 评论 RPID
        #[arg(value_name = "RPID", value_parser = clap::value_parser!(i64).range(1..))]
        rpid: i64,
    },
}

#[derive(Subcommand, Debug)]
pub enum VideoStudioCommands {
    /// 查看创作中心后台稿件列表（含审核中、转码中、退回及已发布稿件，对标 biliup list）
    #[command(visible_alias = "ls")]
    List {
        /// 稿件状态筛选 (all, pubed, pubing, rejected)
        #[arg(long, value_enum, default_value_t = StudioArchiveStatus::All)]
        status: StudioArchiveStatus,

        /// 页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 每页条数 (1-50)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=50))]
        page_size: u32,
    },

    /// 查看创作中心单个稿件后台审核与分P转码详情（对标 biliup show）
    #[command(visible_alias = "info")]
    Show {
        /// 稿件 BV号、AV号或视频 URL
        #[arg(value_name = "VIDEO")]
        video: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum VideoCommands {
    /// 查看视频详细信息（含分P列表）
    Info {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,
    },

    /// 查看视频可用清晰度、编码与流地址
    #[command(visible_alias = "stream")]
    Streams {
        /// 视频 BV号、AV号、番剧 ep/ss/md、URL 或 b23.tv 短链接
        video: String,

        /// 分P编号 (从 1 开始；默认取 URL 中的 p 参数或 1)
        #[arg(short = 'p', long, value_parser = clap::value_parser!(u32).range(1..))]
        page: Option<u32>,
    },

    /// 获取并导出视频字幕 (SRT/TXT/JSON)
    Subtitle {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,

        /// 分P编号 (从 1 开始；默认取 URL 中的 p 参数或 1)
        #[arg(short = 'p', long, value_parser = clap::value_parser!(u32).range(1..))]
        page: Option<u32>,

        /// 字幕语言代码 (如 zh-Hans, en-US)
        #[arg(long, value_name = "CODE")]
        lang: Option<String>,

        /// 导出格式 (srt, txt, json)
        #[arg(long, value_enum, default_value_t = SubtitleFormat::Srt)]
        format: SubtitleFormat,

        /// 输出文件路径 (未指定则输出到标准输出)
        #[arg(short = 'o', long, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// 上传本地 SRT 字幕到指定分P（客户端转 BCC 后提交，支持草稿与离线转换检查）
    SubtitleUpload {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,

        /// 待上传的字幕文件（当前仅支持 SRT）
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// 字幕语言代码（如 zh, en, zh-Hant, ja, ko；zh-Hans/zh-CN/en-US 会自动归一化）
        #[arg(long, value_name = "CODE")]
        lang: String,

        /// 分P编号 (从 1 开始；默认取 URL 中的 p 参数或 1)
        #[arg(short = 'p', long, alias = "part", value_parser = clap::value_parser!(u32).range(1..))]
        page: Option<u32>,

        /// 仅保存草稿，不提交审核 (submit=0)
        #[arg(long)]
        draft: bool,

        /// 以自制字幕签名提交 (sign=1；默认 sign=0)
        #[arg(long)]
        sign: bool,

        /// 跳过提交后的字幕列表回查（大批量上传时使用，需自行用 subtitle-list 复核）
        #[arg(long)]
        no_verify: bool,

        /// 只做 SRT→BCC 解析与转换，不发起任何网络请求
        #[arg(long)]
        dry_run: bool,
    },

    /// 查询当前账号的字幕轨（默认全账号，指定视频与分P时只显示该分P）
    SubtitleList {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接（可选；指定后按分P过滤）
        #[arg(value_name = "VIDEO")]
        video: Option<String>,

        /// 分P编号 (从 1 开始；默认取 URL 中的 p 参数或 1)
        #[arg(short = 'p', long, alias = "part", value_parser = clap::value_parser!(u32).range(1..))]
        page: Option<u32>,

        /// 状态过滤 (all, draft, published, deleted)
        #[arg(long, value_enum, default_value_t = SubtitleStatusFilter::All)]
        status: SubtitleStatusFilter,

        /// 结果页码 (从 1 开始；指定 VIDEO 时该参数无效，会扫描全部分页并按 cid 过滤)
        #[arg(long = "list-page", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page_num: u32,

        /// 每页条数 (1-50)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=50))]
        page_size: u32,
    },

    /// 删除指定分P的字幕轨（软删除，条目 status 先变为 4，随后由服务端从列表移除）
    SubtitleDelete {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,

        /// 要删除的字幕 ID（可重复指定或一次传入多个，来自 subtitle-list 的 id_str）
        #[arg(long = "id", value_name = "SUBTITLE_ID", num_args = 1.., required = true)]
        ids: Vec<String>,

        /// 分P编号 (从 1 开始；默认取 URL 中的 p 参数或 1)
        #[arg(short = 'p', long, alias = "part", value_parser = clap::value_parser!(u32).range(1..))]
        page: Option<u32>,
    },

    /// 获取视频 AI 总结与大纲
    Summary {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,

        /// 分P编号 (从 1 开始；默认取 URL 中的 p 参数或 1)
        #[arg(short = 'p', long, value_parser = clap::value_parser!(u32).range(1..))]
        page: Option<u32>,
    },

    /// 查看视频标签
    Tags {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,
    },

    /// 查看相关推荐视频
    Related {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,
    },

    /// 获取并导出视频弹幕 (XML/ASS/TXT/JSON)
    Danmaku {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,

        /// 分P编号 (从 1 开始；默认取 URL 中的 p 参数或 1)
        #[arg(short = 'p', long, value_parser = clap::value_parser!(u32).range(1..))]
        page: Option<u32>,

        /// 导出格式 (xml, ass, txt, json)
        #[arg(long, value_enum, default_value_t = DanmakuFormat::Xml)]
        format: DanmakuFormat,

        /// 输出文件路径 (未指定则输出到标准输出)
        #[arg(short = 'o', long, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// 查看视频评论列表（默认多行树状排版，支持 --compact / -i 交互浏览 / --root 楼中楼）
    #[command(visible_alias = "replies")]
    Comments {
        /// 视频 BV号、AV号、番剧 ep/ss、URL 或 b23.tv 短链接
        video: String,

        /// 页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 排序方式 (hot, time)
        #[arg(long, value_enum, default_value_t = CommentSort::Hot)]
        sort: CommentSort,

        /// 指定根评论 RPID，展开查看该楼下的全部楼中楼二级回复
        #[arg(long, value_name = "RPID", value_parser = clap::value_parser!(i64).range(1..))]
        root: Option<i64>,

        /// 兼容参数：完整多行展示评论与内嵌楼中楼（现已成为默认行为）
        #[arg(long, conflicts_with = "compact")]
        full: bool,

        /// 使用紧凑单行表格模式显示评论列表（截断长内容）
        #[arg(long, conflicts_with_all = ["full", "interactive"])]
        compact: bool,

        /// 启动终端交互式评论浏览器 (TUI，支持浏览、展开楼中楼、翻页、点赞与回复)
        #[arg(short = 'i', long, conflicts_with_all = ["compact", "raw"])]
        interactive: bool,

        /// 配合 --json 返回 B 站原始未精简接口数据（含头像图层、装扮等冗余字段）
        #[arg(long)]
        raw: bool,
    },

    /// 发表、点赞或删除视频评论
    #[command(subcommand)]
    Comment(VideoCommentCommands),

    /// 点赞视频
    Like {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,

        /// 取消点赞
        #[arg(long)]
        cancel: bool,
    },

    /// 为视频投币
    Coin {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,

        /// 投币数量 (1 或 2)
        #[arg(short = 'n', long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=2))]
        num: u32,

        /// 同时点赞
        #[arg(long)]
        also_like: bool,
    },

    /// 一键三连 (点赞+投币+收藏)
    Triple {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,
    },

    /// 下载视频/音频流及弹幕、字幕、封面 (DASH 混流需要 ffmpeg)
    #[command(visible_alias = "dl")]
    Download {
        /// 视频 BV号、AV号、番剧 ep/ss/md、URL 或 b23.tv 短链接
        video: String,

        /// 分P选择 (如 1, 1,3, 2-4, ALL; 默认取 URL 中的 p 参数或 1)
        #[arg(short = 'p', long, value_name = "SPEC")]
        pages: Option<String>,

        /// 目标清晰度 (支持 best/8k/dolby/hdr/4k/1080p60/1080p/720p/480p/360p 或 QN 编号)
        #[arg(short = 'Q', long, value_name = "QN", default_value = "best", value_parser = parse_video_quality)]
        quality: u32,

        /// 目标音质 (支持 best/flac/dolby/192k/132k/64k 或音频流 ID)
        #[arg(long, value_name = "AQ", value_parser = parse_audio_quality)]
        audio_quality: Option<u32>,

        /// 偏好视频编码 (avc/h264, hevc/h265, av1)
        #[arg(short = 'e', long, value_enum, default_value_t = DownloadCodec::Avc)]
        codec: DownloadCodec,

        /// 输出文件或模板路径 (支持 {title},{bvid},{aid},{p},{page},{p_title},{part},{quality},{codec})
        #[arg(short = 'o', long, value_name = "TEMPLATE")]
        output: Option<String>,

        /// 仅下载音频流
        #[arg(long, conflicts_with = "video_only")]
        audio_only: bool,

        /// 仅下载视频流
        #[arg(long, conflicts_with = "audio_only")]
        video_only: bool,

        /// 跳过 ffmpeg 混流，保留原始分流文件
        #[arg(long)]
        skip_mux: bool,

        /// 同时下载弹幕文件 (默认保存为播放器可直接加载的 .ass 格式)
        #[arg(long)]
        danmaku: bool,

        /// 随附弹幕保存格式 (ass 或 xml)
        #[arg(long, value_enum, default_value_t = DownloadDanmakuFormat::Ass)]
        danmaku_format: DownloadDanmakuFormat,

        /// 同时下载字幕文件
        #[arg(long)]
        subtitle: bool,

        /// 同时下载视频封面
        #[arg(long)]
        cover: bool,
    },

    /// 向 B 站上传视频投稿（支持多P并发分块上传、线路优选、封面与定时发布，对标 biliup upload）
    #[command(visible_alias = "up")]
    Upload(Box<VideoUploadArgs>),

    /// 向已有视频稿件追加新的分P视频（对标 biliup append）
    Append {
        /// 目标已投递稿件的 BV号、AV号或视频 URL
        #[arg(value_name = "VIDEO")]
        video: String,

        /// 待追加上传的本地视频文件路径（支持一次追加多个分P）
        #[arg(value_name = "FILE", num_args = 1.., required = true)]
        files: Vec<PathBuf>,

        /// UPOS 上传线路 (auto, bda2, ws, qn, cos/tx, txa, alia, bldsa)
        #[arg(short = 'l', long, value_enum, default_value_t = UploadLine::Auto)]
        line: UploadLine,

        /// 单文件最大并发上传线程数 (1-16)
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=16))]
        limit: u32,
    },

    /// 替换已有稿件中指定分P的视频文件（保留 BV 号与播放数据，可同时修改标题；提交后重新审核）
    Replace {
        /// 目标已投递稿件的 BV号、AV号或视频 URL
        #[arg(value_name = "VIDEO")]
        video: String,

        /// 用于替换的本地视频文件路径
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// 要替换的分P序号（从 1 开始）
        #[arg(short = 'p', long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        part: u32,

        /// 同时修改稿件标题（最长 80 字）
        #[arg(short = 't', long)]
        title: Option<String>,

        /// UPOS 上传线路 (auto, bda2, ws, qn, cos/tx, txa, alia, bldsa)
        #[arg(short = 'l', long, value_enum, default_value_t = UploadLine::Auto)]
        line: UploadLine,

        /// 单文件最大并发上传线程数 (1-16)
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=16))]
        limit: u32,
    },

    /// 编辑已有稿件的元数据（标题、分区、标签、活动标签、简介、16:9/4:3封面、版权、动态文案、禁转声明等；未指定字段保持原值）
    Edit(Box<VideoEditArgs>),

    /// 查询或搜索 B 站创作中心可用的投稿话题与活动标签 (topic_id / mission_id)
    Topics {
        /// 按关键词搜索话题或活动名称（留空则按分区列出推荐活动话题）
        #[arg(short = 'k', long)]
        keyword: Option<String>,

        /// 按分区 ID 筛选推荐话题（未指定 --keyword 时生效，如 231, 171）
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        tid: Option<u32>,

        /// 仅显示绑定了官方征稿活动 (mission_id > 0) 的活动标签
        #[arg(long)]
        activity_only: bool,

        /// 分页页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 每页条数 (1-50)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=50))]
        page_size: u32,
    },

    /// 创作中心后台稿件列表与转码审核状态查询（对标 biliup list / show）
    #[command(visible_alias = "archives", subcommand)]
    Studio(VideoStudioCommands),
}

#[derive(clap::Args, Debug)]
pub struct VideoUploadArgs {
    /// 待上传的本地视频文件路径（传入多个文件自动合成为多P稿件）
    #[arg(value_name = "FILE", num_args = 1.., required = true)]
    pub files: Vec<PathBuf>,

    /// 稿件标题（留空默认取首个视频文件名，最长 80 字）
    #[arg(short = 't', long)]
    pub title: Option<String>,

    /// 投稿分区 ID（如 171=电子竞技, 17=单机游戏, 122=野生技术协会, 21=日常）
    #[arg(long, default_value_t = 171, value_parser = clap::value_parser!(u32).range(1..))]
    pub tid: u32,

    /// 视频标签（多个用英文或中文逗号分隔，最多 10 个；留空默认按标题生成）
    #[arg(long)]
    pub tag: Option<String>,

    /// 按名称搜索并绑定官方活动/话题标签（自动解析 topic_id 与 mission_id，并将活动名补入标签首位）
    #[arg(long, value_name = "NAME")]
    pub topic: Option<String>,

    /// 直接指定活动/话题 ID (topic_id)
    #[arg(long, value_name = "ID")]
    pub topic_id: Option<i64>,

    /// 直接指定官方征稿活动 ID (mission_id)
    #[arg(long, value_name = "ID")]
    pub mission_id: Option<i64>,

    /// 视频简介
    #[arg(long, default_value = "")]
    pub desc: String,

    /// 版权类型：1=自制（默认），2=转载（需配合 --source）
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
    pub copyright: u8,

    /// 转载来源网址（当 --copyright 2 时必填）
    #[arg(long, default_value = "")]
    pub source: String,

    /// 自定义封面（16:9 主封面，支持本地图片路径 jpg/png/webp 自动上传，或远程 https URL）
    #[arg(long, value_name = "IMAGE")]
    pub cover: Option<String>,

    /// 自定义 4:3 推荐封面（支持本地图片路径 jpg/png/webp 自动上传，或远程 https URL；留空由 B 站从主封面居中裁切）
    #[arg(long, value_name = "IMAGE")]
    pub cover43: Option<String>,

    /// 粉丝动态文案
    #[arg(long, default_value = "")]
    pub dynamic: String,

    /// 定时发布时间（支持 Unix 秒级时间戳，或相对偏移如 +4h / +2h30m）
    #[arg(long, value_name = "TIME")]
    pub dtime: Option<String>,

    /// 自制稿件启用“未经作者授权禁止转载”声明
    #[arg(long)]
    pub no_reprint: bool,

    /// UPOS 上传线路 (auto, bda2, ws, qn, cos/tx, txa, alia, bldsa)
    #[arg(short = 'l', long, value_enum, default_value_t = UploadLine::Auto)]
    pub line: UploadLine,

    /// 单文件最大并发上传线程数 (1-16)
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=16))]
    pub limit: u32,
}

#[derive(clap::Args, Debug)]
pub struct VideoEditArgs {
    /// 目标已投递稿件的 BV号、AV号或视频 URL
    #[arg(value_name = "VIDEO")]
    pub video: String,

    /// 修改稿件标题（最长 80 字）
    #[arg(short = 't', long)]
    pub title: Option<String>,

    /// 修改投稿分区 ID（如 231, 171, 122）
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub tid: Option<u32>,

    /// 修改视频标签（多个用英文或中文逗号分隔，最多 10 个）
    #[arg(long)]
    pub tag: Option<String>,

    /// 按名称搜索并绑定/切换活动标签（自动解析 topic_id 与 mission_id，并将活动名补入标签首位）
    #[arg(long, value_name = "NAME")]
    pub topic: Option<String>,

    /// 直接设置或清除活动/话题 ID（传 0 清除绑定；不传则保留原值）
    #[arg(long, value_name = "ID")]
    pub topic_id: Option<i64>,

    /// 直接设置或清除官方征稿活动 ID（传 0 清除绑定；不传则保留原值）
    #[arg(long, value_name = "ID")]
    pub mission_id: Option<i64>,

    /// 修改视频简介
    #[arg(long)]
    pub desc: Option<String>,

    /// 修改版权类型：1=自制，2=转载（改转载时若原稿无来源须同时指定 --source）
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=2))]
    pub copyright: Option<u8>,

    /// 修改转载来源网址
    #[arg(long)]
    pub source: Option<String>,

    /// 修改 16:9 主封面（支持本地图片路径 jpg/png/webp 自动上传，或远程 https URL）
    #[arg(long, value_name = "IMAGE")]
    pub cover: Option<String>,

    /// 修改 4:3 推荐封面（支持本地图片路径 jpg/png/webp 自动上传，或远程 https URL）
    #[arg(long, value_name = "IMAGE")]
    pub cover43: Option<String>,

    /// 修改粉丝动态文案
    #[arg(long)]
    pub dynamic: Option<String>,

    /// 修改自制稿件“未经作者授权禁止转载”声明（true/false）
    #[arg(long, value_name = "BOOL")]
    pub no_reprint: Option<bool>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleStatusFilter {
    /// 全部状态
    All,
    /// 草稿 (status=1)
    Draft,
    /// 已发布 (status=5)
    Published,
    /// 已删除 (status=4)
    Deleted,
}

impl SubtitleStatusFilter {
    /// 匹配 `author/list` 返回记录里的 `status` 字段
    pub fn matches(self, status: i64) -> bool {
        match self {
            Self::All => true,
            Self::Draft => status == 1,
            Self::Published => status == 5,
            Self::Deleted => status == 4,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserVideoOrder {
    /// 最新发布
    Pubdate,
    /// 最多播放
    Click,
    /// 最多收藏
    Stow,
}

impl UserVideoOrder {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Pubdate => "pubdate",
            Self::Click => "click",
            Self::Stow => "stow",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum UserCommands {
    /// 查看用户主页信息与粉丝关注数据（留空默认查询当前登录账号）
    Info {
        /// 用户 UID、空间 URL 或 b23.tv 短链接（可选，默认当前登录账号）
        #[arg(value_name = "USER")]
        user: Option<String>,
    },

    /// 查看用户投稿视频列表（留空默认查询当前登录账号）
    Videos {
        /// 用户 UID、空间 URL 或 b23.tv 短链接（可选，默认当前登录账号）
        #[arg(value_name = "USER")]
        user: Option<String>,

        /// 分页页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 每页条数 (1-50)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=50))]
        page_size: u32,

        /// 排序方式 (pubdate, click, stow)
        #[arg(long, value_enum, default_value_t = UserVideoOrder::Pubdate)]
        order: UserVideoOrder,

        /// 关键词过滤
        #[arg(long, value_name = "KW")]
        keyword: Option<String>,
    },

    /// 查看用户空间动态列表（留空默认查询当前登录账号）
    Dynamics {
        /// 用户 UID、空间 URL 或 b23.tv 短链接（可选，默认当前登录账号）
        #[arg(value_name = "USER")]
        user: Option<String>,

        /// 分页游标 (来自上次返回的 offset)
        #[arg(long, value_name = "CURSOR")]
        offset: Option<String>,
    },

    /// 关注指定用户
    Follow {
        /// 用户 UID、空间 URL 或 b23.tv 短链接
        #[arg(value_name = "USER")]
        user: String,
    },

    /// 取消关注指定用户
    Unfollow {
        /// 用户 UID、空间 URL 或 b23.tv 短链接
        #[arg(value_name = "USER")]
        user: String,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedType {
    /// 全部动态
    All,
    /// 视频动态
    Video,
    /// 番剧/影视动态
    Pgc,
    /// 专栏动态
    Article,
}

impl FeedType {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Video => "video",
            Self::Pgc => "pgc",
            Self::Article => "article",
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankType {
    /// 全站榜
    All,
    /// 新人榜
    Rookie,
    /// 原创榜
    Origin,
}

impl RankType {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Rookie => "rookie",
            Self::Origin => "origin",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum RankCommands {
    /// 查看全站或分区视频排行榜
    Video {
        /// 分区 TID (0=全站, 1=动画, 3=音乐, 4=游戏, 36=知识等)
        #[arg(long, default_value_t = 0, value_name = "TID")]
        rid: u32,

        /// 榜单类型 (all, rookie, origin)
        #[arg(long = "type", value_enum, default_value_t = RankType::All)]
        rank_type: RankType,
    },

    /// 查看当前综合热门视频
    Popular {
        /// 分页页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 每页条数 (1-50)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=50))]
        page_size: u32,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FavExportFormat {
    Json,
    Csv,
    #[value(alias = "txt")]
    Text,
}

impl FavExportFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Csv => "csv",
            Self::Text => "text",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum FavCommands {
    /// 查看收藏夹列表
    #[command(visible_alias = "ls")]
    List {
        /// 目标用户 UID 或空间 URL (默认当前登录账号)
        #[arg(long)]
        mid: Option<String>,
    },

    /// 查看收藏夹内视频
    Show {
        /// 收藏夹 ID (media_id)、ml 编号或收藏夹 URL
        #[arg(value_name = "MEDIA_ID")]
        media_id: String,

        /// 页码 (从 1 开始)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,

        /// 每页条数 (1-20)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=20))]
        page_size: u32,
    },

    /// 导出收藏夹全部内容
    Export {
        /// 收藏夹 ID (media_id)、ml 编号或收藏夹 URL
        #[arg(value_name = "MEDIA_ID")]
        media_id: String,

        /// 导出文件路径 (未指定则输出到标准输出)
        #[arg(short = 'o', long)]
        output: Option<String>,

        /// 导出格式 (json, csv, text)
        #[arg(long, value_enum, default_value_t = FavExportFormat::Json)]
        format: FavExportFormat,
    },

    /// 添加视频到收藏夹
    Add {
        /// 收藏夹 ID (media_id)、ml 编号或收藏夹 URL
        #[arg(value_name = "MEDIA_ID")]
        media_id: String,

        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        #[arg(value_name = "VIDEO")]
        video: String,
    },

    /// 从收藏夹移除视频
    #[command(visible_alias = "rm")]
    Remove {
        /// 收藏夹 ID (media_id)、ml 编号或收藏夹 URL
        #[arg(value_name = "MEDIA_ID")]
        media_id: String,

        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        #[arg(value_name = "VIDEO")]
        video: String,
    },

    /// 清理收藏夹中的失效视频
    Clean {
        /// 收藏夹 ID (media_id)、ml 编号或收藏夹 URL
        #[arg(value_name = "MEDIA_ID")]
        media_id: String,
    },

    /// 新建收藏夹
    Create {
        /// 收藏夹名称
        #[arg(value_name = "TITLE")]
        title: String,

        /// 设为私密收藏夹
        #[arg(long)]
        private: bool,
    },

    /// 删除收藏夹
    Delete {
        /// 收藏夹 ID (media_id)、ml 编号或收藏夹 URL
        #[arg(value_name = "MEDIA_ID")]
        media_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum LaterCommands {
    /// 查看稍后再看列表
    #[command(visible_alias = "ls")]
    List,

    /// 添加视频到稍后再看
    Add {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,
    },

    /// 从稍后再看移除指定视频
    #[command(visible_aliases = ["rm", "delete", "del"])]
    Remove {
        /// 视频 BV号、AV号、URL 或 b23.tv 短链接
        video: String,
    },

    /// 清空稍后再看列表
    Clear {
        /// 仅清理已观看过的视频
        #[arg(long)]
        viewed_only: bool,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryCategory {
    /// 全部类型
    All,
    /// 视频
    Archive,
    /// 直播
    Live,
    /// 专栏文章
    Article,
}

impl HistoryCategory {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Archive => "archive",
            Self::Live => "live",
            Self::Article => "article",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum HistoryCommands {
    /// 查看观看历史记录
    #[command(visible_alias = "ls")]
    List {
        /// 获取条数 (1-30)
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=30))]
        limit: u32,

        /// 分类筛选
        #[arg(short = 'c', long, value_enum)]
        category: Option<HistoryCategory>,

        /// 翻页游标 max (传入上一页最后一条的 oid)
        #[arg(long, value_name = "OID")]
        max: Option<i64>,

        /// 翻页时间戳游标 view_at (传入上一页最后一条的观看时间戳)
        #[arg(long, value_name = "TIMESTAMP")]
        view_at: Option<i64>,
    },

    /// 删除单条观看历史记录（支持视频 BV/av、直播 room:<ID>、专栏 cv<ID> 或原始 kid）
    #[command(visible_aliases = ["rm", "remove", "del"])]
    Delete {
        /// 目标历史记录标识（如 BV1xx / av123 / room:76 / cv12345 / archive_123）
        #[arg(value_name = "TARGET")]
        video: String,
    },

    /// 清空全部观看历史记录
    Clear,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_bare_bt_shows_welcome_without_error() {
        let cli = Cli::try_parse_from(["bit"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.json);
        assert!(!cli.quiet);
    }

    #[test]
    fn test_cli_parsing_subcommands() {
        // bit --json auth status
        let cli = Cli::try_parse_from(["bit", "--json", "auth", "status"]).unwrap();
        assert!(cli.json);
        assert!(!cli.quiet);
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthCommands::Status))
        ));

        // bit -q live start -a 123 -t "Test" -y
        let cli = Cli::try_parse_from([
            "bit", "-q", "live", "start", "-a", "123", "-t", "Test", "-y",
        ])
        .unwrap();
        assert!(!cli.json);
        assert!(cli.quiet);
        assert!(cli.yes);
        if let Some(Commands::Live(LiveCommands::Start { area, title, .. })) = cli.command {
            assert_eq!(area, Some(123));
            assert_eq!(title, Some("Test".to_string()));
        } else {
            panic!("expected LiveCommands::Start");
        }

        // bit search video "Rust" --page 2
        let cli = Cli::try_parse_from(["bit", "search", "video", "Rust", "--page", "2"]).unwrap();
        if let Some(Commands::Search(SearchCommands::Video { keyword, page, .. })) = cli.command {
            assert_eq!(keyword, "Rust");
            assert_eq!(page, 2);
        } else {
            panic!("expected SearchCommands::Video");
        }

        // bit video coin BV1xx -n 2 --also-like
        let cli = Cli::try_parse_from(["bit", "video", "coin", "BV1xx", "-n", "2", "--also-like"])
            .unwrap();
        if let Some(Commands::Video(VideoCommands::Coin {
            video,
            num,
            also_like,
        })) = cli.command
        {
            assert_eq!(video, "BV1xx");
            assert_eq!(num, 2);
            assert!(also_like);
        } else {
            panic!("expected VideoCommands::Coin");
        }

        // bit fav export 12345 -o /tmp/test.csv --format csv
        let cli = Cli::try_parse_from([
            "bit",
            "fav",
            "export",
            "12345",
            "-o",
            "/tmp/test.csv",
            "--format",
            "csv",
        ])
        .unwrap();
        if let Some(Commands::Fav(FavCommands::Export {
            media_id,
            output,
            format,
        })) = cli.command
        {
            assert_eq!(media_id, "12345");
            assert_eq!(output, Some("/tmp/test.csv".to_string()));
            assert_eq!(format, FavExportFormat::Csv);
        } else {
            panic!("expected FavCommands::Export");
        }

        // bit later clear --viewed-only
        let cli = Cli::try_parse_from(["bit", "later", "clear", "--viewed-only"]).unwrap();
        if let Some(Commands::Later(LaterCommands::Clear { viewed_only })) = cli.command {
            assert!(viewed_only);
        } else {
            panic!("expected LaterCommands::Clear");
        }

        // bit history list --limit 10 -c archive
        let cli = Cli::try_parse_from(["bit", "history", "list", "--limit", "10", "-c", "archive"])
            .unwrap();
        if let Some(Commands::History(HistoryCommands::List {
            limit, category, ..
        })) = cli.command
        {
            assert_eq!(limit, 10);
            assert_eq!(category, Some(HistoryCategory::Archive));
        } else {
            panic!("expected HistoryCommands::List");
        }
        assert!(Cli::try_parse_from(["bit", "history", "list", "-t", "archive"]).is_err());

        // bit completions zsh --install
        let cli = Cli::try_parse_from(["bit", "completions", "zsh", "--install"]).unwrap();
        if let Some(Commands::Completions {
            shell,
            install,
            zsh,
            ..
        }) = cli.command
        {
            assert!(matches!(shell, Some(Shell::Zsh)));
            assert!(install);
            assert!(!zsh);
        } else {
            panic!("expected Completions");
        }
    }

    #[test]
    fn test_legacy_top_level_commands_are_rejected() {
        assert!(Cli::try_parse_from(["bit", "start"]).is_err());
        assert!(Cli::try_parse_from(["bit", "stop"]).is_err());
        assert!(Cli::try_parse_from(["bit", "status"]).is_err());
    }

    #[test]
    fn test_clap_command_debug_assert() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn test_m1_global_flags_and_new_subcommands() {
        let cli = Cli::try_parse_from([
            "bit",
            "--no-color",
            "-y",
            "-u",
            "/tmp/custom.json",
            "resolve",
            "https://b23.tv/pigt3PQ",
            "--no-network",
        ])
        .unwrap();
        assert!(cli.no_color);
        assert!(cli.yes);
        assert_eq!(cli.cookie_file, Some(PathBuf::from("/tmp/custom.json")));
        assert!(matches!(
            cli.command,
            Some(Commands::Resolve { ref input, no_network: true }) if input == "https://b23.tv/pigt3PQ"
        ));

        let cli = Cli::try_parse_from([
            "bit",
            "auth",
            "login",
            "--method",
            "cookie",
            "--sessdata",
            "s1",
            "--bili-jct",
            "j1",
            "--dede-user-id",
            "42",
            "-u",
            "/tmp/after.json",
        ])
        .unwrap();
        assert_eq!(cli.cookie_file, Some(PathBuf::from("/tmp/after.json")));
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthCommands::Login {
                method: Some(LoginMethod::Cookie),
                sessdata: Some(ref s),
                bili_jct: Some(ref j),
                dede_user_id: Some(ref d),
            })) if s == "s1" && j == "j1" && d == "42"
        ));

        let daily = Cli::try_parse_from(["bit", "--json", "auth", "daily"]).unwrap();
        assert!(daily.json);
        assert!(matches!(
            daily.command,
            Some(Commands::Auth(AuthCommands::Daily))
        ));

        let ps = Cli::try_parse_from(["bit", "completions", "powershell"]).unwrap();
        assert!(matches!(
            ps.command,
            Some(Commands::Completions {
                shell: Some(Shell::PowerShell),
                install: false,
                ..
            })
        ));

        let elv = Cli::try_parse_from(["bit", "completions", "elvish"]).unwrap();
        assert!(matches!(
            elv.command,
            Some(Commands::Completions {
                shell: Some(Shell::Elvish),
                install: false,
                ..
            })
        ));
    }

    #[test]
    fn test_m2_live_refactored_and_new_subcommands() {
        // live title <TITLE> (single required positional; -t rejected; bare rejected)
        let cli = Cli::try_parse_from(["bit", "live", "title", "新标题"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Title { ref title })) if title == "新标题"
        ));
        assert!(Cli::try_parse_from(["bit", "live", "title"]).is_err());
        assert!(Cli::try_parse_from(["bit", "live", "title", "-t", "x"]).is_err());

        // live area [KEYWORD] (-k rejected)
        let cli = Cli::try_parse_from(["bit", "live", "area"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Area { keyword: None }))
        ));
        let cli = Cli::try_parse_from(["bit", "live", "area", "游戏"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Area { keyword: Some(ref k) })) if k == "游戏"
        ));
        assert!(Cli::try_parse_from(["bit", "live", "area", "-k", "游戏"]).is_err());

        // live following range validation
        assert!(
            Cli::try_parse_from([
                "bit",
                "live",
                "following",
                "--page",
                "1",
                "--page-size",
                "20"
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["bit", "live", "following", "--page", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "live", "following", "--page-size", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "live", "following", "--page-size", "21"]).is_err());

        // live info [ROOM]
        let cli = Cli::try_parse_from(["bit", "live", "info"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Info { room: None }))
        ));
        let cli =
            Cli::try_parse_from(["bit", "live", "info", "https://live.bilibili.com/76"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Info { room: Some(ref r) })) if r == "https://live.bilibili.com/76"
        ));

        // live stream <ROOM> [-Q/--quality] (global -q/--quiet does not collide)
        let cli = Cli::try_parse_from(["bit", "-q", "live", "stream", "76", "-Q", "400"]).unwrap();
        assert!(cli.quiet);
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Stream { ref room, quality: 400 })) if room == "76"
        ));

        // live danmaku <ROOM> [--limit 1..=50] [-f] [-i] [--no-enter]
        let cli = Cli::try_parse_from(["bit", "live", "danmaku", "76", "--limit", "50"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Danmaku {
                ref room,
                limit: Some(50),
                follow: false,
                interactive: false,
                no_enter: false,
            })) if room == "76"
        ));
        let cli_chat = Cli::try_parse_from(["bit", "live", "chat", "76", "--no-enter"]).unwrap();
        assert!(matches!(
            cli_chat.command,
            Some(Commands::Live(LiveCommands::Chat {
                room: Some(ref r),
                no_enter: true,
                history: 15,
            })) if r == "76"
        ));
        assert!(Cli::try_parse_from(["bit", "live", "danmaku", "76", "--limit", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "live", "danmaku", "76", "--limit", "51"]).is_err());
        assert!(Cli::try_parse_from(["bit", "live", "danmaku", "76", "-f", "-i"]).is_err());

        // live send <ROOM> <MESSAGE>
        let cli = Cli::try_parse_from(["bit", "live", "send", "76", "hello"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Send { ref room, ref message })) if room == "76" && message == "hello"
        ));

        // live record <ROOM> [-o] [-Q] [-d] [--danmaku] [--watch]
        let cli = Cli::try_parse_from([
            "bit",
            "live",
            "record",
            "76",
            "-o",
            "out.flv",
            "-Q",
            "400",
            "-d",
            "10s",
            "--danmaku",
            "-w",
            "--interval",
            "15",
            "--once",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Live(LiveCommands::Record {
                ref room,
                output: Some(ref out),
                quality: 400,
                duration: Some(ref dur),
                danmaku: true,
                watch: true,
                interval: 15,
                once: true,
                ..
            })) if room == "76" && out == &PathBuf::from("out.flv") && dur == "10s"
        ));
    }

    #[test]
    fn test_m2_search_fav_later_history_subcommands() {
        // search video/user ValueEnum & page/limit range validation
        let cli = Cli::try_parse_from([
            "bit", "search", "video", "rust", "--order", "click", "--page", "2",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Search(SearchCommands::Video {
                order: Some(SearchVideoOrder::Click),
                page: 2,
                ..
            }))
        ));
        assert!(Cli::try_parse_from(["bit", "search", "video", "rust", "--order", "bad"]).is_err());
        assert!(Cli::try_parse_from(["bit", "search", "video", "rust", "--page", "0"]).is_err());

        let cli =
            Cli::try_parse_from(["bit", "search", "user", "up", "--order", "default"]).unwrap();
        if let Some(Commands::Search(SearchCommands::User { order: Some(o), .. })) = cli.command {
            assert_eq!(o, SearchUserOrder::Default);
            assert_eq!(o.as_api_str(), "0");
        } else {
            panic!("expected SearchCommands::User");
        }
        assert!(Cli::try_parse_from(["bit", "search", "user", "up", "--order", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "search", "user", "up", "--page", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "search", "live", "lol", "--page", "0"]).is_err());

        assert!(Cli::try_parse_from(["bit", "search", "hot", "--limit", "1"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "search", "hot", "--limit", "50"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "search", "hot", "--limit", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "search", "hot", "--limit", "51"]).is_err());

        // fav CRUD & URL inputs & ValueEnum format & range checks
        let cli = Cli::try_parse_from([
            "bit",
            "fav",
            "list",
            "--mid",
            "https://space.bilibili.com/42",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Fav(FavCommands::List { mid: Some(ref m) })) if m == "https://space.bilibili.com/42"
        ));
        assert!(Cli::try_parse_from(["bit", "fav", "show", "ml100", "--page", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "fav", "show", "ml100", "--page-size", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "fav", "show", "ml100", "--page-size", "21"]).is_err());

        let cli =
            Cli::try_parse_from(["bit", "fav", "export", "ml100", "--format", "txt"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Fav(FavCommands::Export {
                format: FavExportFormat::Text,
                ..
            }))
        ));
        assert!(Cli::try_parse_from(["bit", "fav", "export", "ml100", "--format", "xml"]).is_err());

        assert!(Cli::try_parse_from(["bit", "fav", "add", "100", "BV1xx"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "fav", "remove", "100", "BV1xx"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "fav", "clean", "100"]).is_ok());
        let cli = Cli::try_parse_from(["bit", "fav", "create", "MyFav", "--private"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Fav(FavCommands::Create { ref title, private: true })) if title == "MyFav"
        ));
        assert!(Cli::try_parse_from(["bit", "fav", "delete", "100"]).is_ok());

        // later remove & history CRUD
        assert!(Cli::try_parse_from(["bit", "later", "remove", "BV1xx"]).is_ok());
        assert!(
            Cli::try_parse_from(["bit", "history", "list", "--limit", "1", "-c", "live"]).is_ok()
        );
        assert!(Cli::try_parse_from(["bit", "history", "list", "--limit", "30"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "history", "list", "--limit", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "history", "list", "--limit", "31"]).is_err());
        assert!(Cli::try_parse_from(["bit", "history", "list", "-c", "invalid"]).is_err());
        assert!(Cli::try_parse_from(["bit", "history", "delete", "BV1xx"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "history", "clear"]).is_ok());
    }

    #[test]
    fn test_m3_video_subcommands_validation_and_conflicts() {
        // video coin -n 1..=2
        assert!(Cli::try_parse_from(["bit", "video", "coin", "BV1xx", "-n", "1"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "video", "coin", "BV1xx", "-n", "2"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "video", "coin", "BV1xx", "-n", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "video", "coin", "BV1xx", "-n", "3"]).is_err());

        // video streams / subtitle / summary / tags / related
        let cli = Cli::try_parse_from(["bit", "video", "streams", "BV1xx", "-p", "2"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Video(VideoCommands::Streams { ref video, page: Some(2) })) if video == "BV1xx"
        ));
        assert!(Cli::try_parse_from(["bit", "video", "streams", "BV1xx", "-p", "0"]).is_err());

        let cli = Cli::try_parse_from([
            "bit", "video", "subtitle", "BV1xx", "--lang", "zh-Hans", "--format", "srt", "-o",
            "out.srt",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Video(VideoCommands::Subtitle {
                format: SubtitleFormat::Srt,
                ..
            }))
        ));
        assert!(Cli::try_parse_from(["bit", "video", "subtitle", "BV1xx", "-p", "0"]).is_err());
        assert!(
            Cli::try_parse_from(["bit", "video", "subtitle", "BV1xx", "--format", "vtt"]).is_err()
        );

        assert!(Cli::try_parse_from(["bit", "video", "summary", "BV1xx", "-p", "1"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "video", "summary", "BV1xx", "-p", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "video", "tags", "BV1xx"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "video", "related", "BV1xx"]).is_ok());

        // video danmaku / comments / comment
        let cli = Cli::try_parse_from([
            "bit", "video", "danmaku", "BV1xx", "-p", "2", "--format", "ass", "-o", "dm.ass",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Video(VideoCommands::Danmaku {
                page: Some(2),
                format: DanmakuFormat::Ass,
                ..
            }))
        ));
        assert!(
            Cli::try_parse_from(["bit", "video", "danmaku", "BV1xx", "--format", "bad"]).is_err()
        );
        assert!(Cli::try_parse_from(["bit", "video", "danmaku", "BV1xx", "-p", "0"]).is_err());

        let cli = Cli::try_parse_from([
            "bit", "video", "comments", "BV1xx", "--page", "2", "--sort", "time",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Video(VideoCommands::Comments {
                page: 2,
                sort: CommentSort::Time,
                ..
            }))
        ));
        assert_eq!(CommentSort::Hot.as_api_str(), "2");
        assert_eq!(CommentSort::Time.as_api_str(), "0");
        assert!(Cli::try_parse_from(["bit", "video", "comments", "BV1xx", "--page", "0"]).is_err());

        assert!(Cli::try_parse_from(["bit", "video", "comment", "post", "BV1xx", "nice"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "bit", "video", "comment", "like", "BV1xx", "123", "--cancel"
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["bit", "video", "comment", "like", "BV1xx", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "video", "comment", "delete", "BV1xx", "123"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "video", "comment", "delete", "BV1xx", "0"]).is_err());

        // video download: -Q vs global -q, --audio-only vs --video-only conflict, -e validation
        let cli = Cli::try_parse_from([
            "bit",
            "-q",
            "video",
            "download",
            "BV1xx",
            "-p",
            "1-2",
            "-Q",
            "80",
            "-e",
            "hevc",
            "-o",
            "{bvid}_p{page}.mp4",
            "--skip-mux",
            "--danmaku",
            "--subtitle",
            "--cover",
        ])
        .unwrap();
        assert!(cli.quiet);
        assert!(matches!(
            cli.command,
            Some(Commands::Video(VideoCommands::Download {
                quality: 80,
                codec: DownloadCodec::Hevc,
                skip_mux: true,
                danmaku: true,
                subtitle: true,
                cover: true,
                ..
            }))
        ));
        assert!(
            Cli::try_parse_from([
                "bit",
                "video",
                "download",
                "BV1xx",
                "--audio-only",
                "--video-only",
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["bit", "video", "download", "BV1xx", "-e", "vp9"]).is_err());
    }

    #[test]
    fn test_m4_user_feed_rank_parsing_and_validation() {
        // user info / videos / dynamics / follow / unfollow
        let cli_self = Cli::try_parse_from(["bit", "user", "info"]).unwrap();
        assert!(matches!(
            cli_self.command,
            Some(Commands::User(UserCommands::Info { user: None }))
        ));

        let cli = Cli::try_parse_from(["bit", "user", "info", "https://space.bilibili.com/123456"])
            .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::User(UserCommands::Info { user: Some(ref user) })) if user == "https://space.bilibili.com/123456"
        ));

        let cli = Cli::try_parse_from([
            "bit",
            "user",
            "videos",
            "123456",
            "--page",
            "2",
            "--page-size",
            "50",
            "--order",
            "click",
            "--keyword",
            "Rust",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::User(UserCommands::Videos {
                user: Some(ref user),
                page: 2,
                page_size: 50,
                order: UserVideoOrder::Click,
                keyword: Some(ref kw),
            })) if user == "123456" && kw == "Rust"
        ));
        assert_eq!(UserVideoOrder::Pubdate.as_api_str(), "pubdate");
        assert_eq!(UserVideoOrder::Click.as_api_str(), "click");
        assert_eq!(UserVideoOrder::Stow.as_api_str(), "stow");

        assert!(Cli::try_parse_from(["bit", "user", "videos", "123", "--page", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "user", "videos", "123", "--page-size", "0"]).is_err());
        assert!(
            Cli::try_parse_from(["bit", "user", "videos", "123", "--page-size", "51"]).is_err()
        );
        assert!(Cli::try_parse_from(["bit", "user", "videos", "123", "--order", "bad"]).is_err());

        let cli = Cli::try_parse_from([
            "bit",
            "user",
            "dynamics",
            "123456",
            "--offset",
            "9000000001",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::User(UserCommands::Dynamics {
                user: Some(ref user),
                offset: Some(ref off),
            })) if user == "123456" && off == "9000000001"
        ));

        assert!(Cli::try_parse_from(["bit", "user", "follow", "123456"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "user", "unfollow", "123456"]).is_ok());

        // feed
        let cli =
            Cli::try_parse_from(["bit", "feed", "--type", "video", "--offset", "cur1"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Feed {
                feed_type: FeedType::Video,
                offset: Some(ref off),
            }) if off == "cur1"
        ));
        assert_eq!(FeedType::All.as_api_str(), "all");
        assert_eq!(FeedType::Video.as_api_str(), "video");
        assert_eq!(FeedType::Pgc.as_api_str(), "pgc");
        assert_eq!(FeedType::Article.as_api_str(), "article");
        assert!(Cli::try_parse_from(["bit", "feed", "--type", "bogus"]).is_err());

        // rank video / popular
        let cli = Cli::try_parse_from(["bit", "rank", "video", "--rid", "36", "--type", "rookie"])
            .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Rank(RankCommands::Video {
                rid: 36,
                rank_type: RankType::Rookie,
            }))
        ));
        assert_eq!(RankType::All.as_api_str(), "all");
        assert_eq!(RankType::Rookie.as_api_str(), "rookie");
        assert_eq!(RankType::Origin.as_api_str(), "origin");
        assert!(Cli::try_parse_from(["bit", "rank", "video", "--type", "bogus"]).is_err());

        let cli =
            Cli::try_parse_from(["bit", "rank", "popular", "--page", "1", "--page-size", "10"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Rank(RankCommands::Popular {
                page: 1,
                page_size: 10,
            }))
        ));
        assert!(Cli::try_parse_from(["bit", "rank", "popular", "--page", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "rank", "popular", "--page-size", "0"]).is_err());
        assert!(Cli::try_parse_from(["bit", "rank", "popular", "--page-size", "51"]).is_err());
    }

    #[test]
    fn test_semantic_qualities_codec_aliases_and_subcommand_aliases() {
        // Semantic video & live qualities resolve without semantic inversion
        let dl = Cli::try_parse_from([
            "bit",
            "-vv",
            "video",
            "dl",
            "BV1xx",
            "-Q",
            "1080p",
            "-e",
            "h265",
            "--audio-quality",
            "flac",
        ])
        .unwrap();
        assert_eq!(dl.verbose, 2);
        assert!(matches!(
            dl.command,
            Some(Commands::Video(VideoCommands::Download {
                quality: 80,
                audio_quality: Some(30251),
                codec: DownloadCodec::Hevc,
                ..
            }))
        ));

        let dl_h264 = Cli::try_parse_from([
            "bit", "video", "download", "BV1xx", "-Q", "4k", "-e", "h264",
        ])
        .unwrap();
        assert!(matches!(
            dl_h264.command,
            Some(Commands::Video(VideoCommands::Download {
                quality: 120,
                codec: DownloadCodec::Avc,
                ..
            }))
        ));

        let live_st = Cli::try_parse_from(["bit", "live", "streams", "76", "-Q", "1080p"]).unwrap();
        assert!(matches!(
            live_st.command,
            Some(Commands::Live(LiveCommands::Stream { quality: 400, .. }))
        ));

        let live_smooth =
            Cli::try_parse_from(["bit", "live", "record", "76", "-Q", "smooth"]).unwrap();
        assert!(matches!(
            live_smooth.command,
            Some(Commands::Live(LiveCommands::Record { quality: 80, .. }))
        ));

        // --uid alias on auth login
        let login = Cli::try_parse_from([
            "bit",
            "auth",
            "login",
            "-m",
            "cookie",
            "--sessdata",
            "s",
            "--bili-jct",
            "j",
            "--uid",
            "10086",
        ])
        .unwrap();
        assert!(matches!(
            login.command,
            Some(Commands::Auth(AuthCommands::Login {
                dede_user_id: Some(ref uid),
                ..
            })) if uid == "10086"
        ));

        // ls / rm / remove aliases across fav, later, history
        assert!(Cli::try_parse_from(["bit", "fav", "ls"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "fav", "rm", "100", "BV1xx"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "later", "ls"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "later", "rm", "BV1xx"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "later", "delete", "BV1xx"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "bit",
                "history",
                "ls",
                "--max",
                "123",
                "--view-at",
                "1700000000"
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["bit", "history", "rm", "room:76"]).is_ok());
        assert!(Cli::try_parse_from(["bit", "history", "remove", "cv12345"]).is_ok());

        // video upload / append / studio / comments --root --full
        let up = Cli::try_parse_from([
            "bit",
            "video",
            "up",
            "p1.mp4",
            "p2.mp4",
            "-t",
            "录播合集",
            "--tid",
            "122",
            "--tag",
            "Rust,Linux",
            "--cover",
            "cover16x9.png",
            "--cover43",
            "cover4x3.png",
            "--topic",
            "影视神仙剪刀手",
            "--topic-id",
            "1105274",
            "--mission-id",
            "4071918",
            "-l",
            "tx",
            "--limit",
            "4",
        ])
        .unwrap();
        assert!(matches!(
            up.command,
            Some(Commands::Video(VideoCommands::Upload(ref args)))
                if args.files.len() == 2
                    && args.title.as_deref() == Some("录播合集")
                    && args.tid == 122
                    && args.cover.as_deref() == Some("cover16x9.png")
                    && args.cover43.as_deref() == Some("cover4x3.png")
                    && args.topic.as_deref() == Some("影视神仙剪刀手")
                    && args.topic_id == Some(1105274)
                    && args.mission_id == Some(4071918)
                    && args.line == UploadLine::Cos
                    && args.limit == 4
        ));
        assert!(
            Cli::try_parse_from(["bit", "video", "append", "BV1xx", "p3.mp4", "-l", "bda2"])
                .is_ok()
        );
        let rep = Cli::try_parse_from([
            "bit",
            "video",
            "replace",
            "BV1xx",
            "v2.mp4",
            "-p",
            "2",
            "-t",
            "新标题",
        ])
        .unwrap();
        assert!(matches!(
            rep.command,
            Some(Commands::Video(VideoCommands::Replace { part: 2, title: Some(ref t), .. })) if t == "新标题"
        ));
        assert!(
            Cli::try_parse_from(["bit", "video", "replace", "BV1xx", "v2.mp4", "-p", "0"]).is_err()
        );
        assert!(Cli::try_parse_from(["bit", "video", "replace", "BV1xx"]).is_err());
        let ed = Cli::try_parse_from([
            "bit",
            "video",
            "edit",
            "BV1xx",
            "-t",
            "改后标题",
            "--tid",
            "231",
            "--tag",
            "广播剧,双语",
            "--topic",
            "影视神仙剪刀手",
            "--topic-id",
            "1105274",
            "--mission-id",
            "4071918",
            "--desc",
            "新简介",
            "--cover",
            "c16x9.png",
            "--cover43",
            "c4x3.png",
            "--no-reprint",
            "true",
        ])
        .unwrap();
        assert!(matches!(
            ed.command,
            Some(Commands::Video(VideoCommands::Edit(ref args)))
                if args.video == "BV1xx"
                    && args.title.as_deref() == Some("改后标题")
                    && args.tid == Some(231)
                    && args.tag.as_deref() == Some("广播剧,双语")
                    && args.topic.as_deref() == Some("影视神仙剪刀手")
                    && args.topic_id == Some(1105274)
                    && args.mission_id == Some(4071918)
                    && args.desc.as_deref() == Some("新简介")
                    && args.cover.as_deref() == Some("c16x9.png")
                    && args.cover43.as_deref() == Some("c4x3.png")
                    && args.no_reprint == Some(true)
        ));
        assert!(Cli::try_parse_from(["bit", "video", "edit", "BV1xx", "--tid", "0"]).is_err());
        assert!(
            Cli::try_parse_from(["bit", "video", "edit", "BV1xx", "--copyright", "3"]).is_err()
        );
        let tp = Cli::try_parse_from([
            "bit",
            "video",
            "topics",
            "-k",
            "影视",
            "--tid",
            "231",
            "--activity-only",
            "--page",
            "2",
            "--page-size",
            "10",
        ])
        .unwrap();
        assert!(matches!(
            tp.command,
            Some(Commands::Video(VideoCommands::Topics {
                keyword: Some(ref k),
                tid: Some(231),
                activity_only: true,
                page: 2,
                page_size: 10,
            })) if k == "影视"
        ));
        assert!(
            Cli::try_parse_from(["bit", "video", "studio", "ls", "--status", "pubing"]).is_ok()
        );
        assert!(Cli::try_parse_from(["bit", "video", "archives", "show", "BV1xx"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "bit", "video", "comments", "BV1xx", "--root", "12345", "--full"
            ])
            .is_ok()
        );
        let c_tui = Cli::try_parse_from(["bit", "video", "comments", "BV1xx", "-i"]).unwrap();
        assert!(matches!(
            c_tui.command,
            Some(Commands::Video(VideoCommands::Comments {
                interactive: true,
                compact: false,
                raw: false,
                ..
            }))
        ));
        let c_cmp =
            Cli::try_parse_from(["bit", "video", "comments", "BV1xx", "--compact"]).unwrap();
        assert!(matches!(
            c_cmp.command,
            Some(Commands::Video(VideoCommands::Comments {
                compact: true,
                raw: false,
                ..
            }))
        ));
        let c_raw =
            Cli::try_parse_from(["bit", "--json", "video", "comments", "BV1xx", "--raw"]).unwrap();
        assert!(matches!(
            c_raw.command,
            Some(Commands::Video(VideoCommands::Comments { raw: true, .. }))
        ));
        assert!(
            Cli::try_parse_from(["bit", "video", "comments", "BV1xx", "--compact", "--full"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["bit", "video", "comments", "BV1xx", "--compact", "-i"]).is_err()
        );
        assert!(Cli::try_parse_from(["bit", "video", "comments", "BV1xx", "-i", "--raw"]).is_err());
    }
}
