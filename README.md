# bili-tools

B站直播开播工具，命令行一键开播/下播。

## 安装

### Cargo (通用)

```bash
# 首次需安装 Rust 工具链
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 克隆并编译安装
git clone https://github.com/QwerProg/bili-tools.git --depth=1
cd bili-tools
cargo install --path .
```

编译安装到 `~/.cargo/bin/bt`，确认该目录在 PATH 中即可。

### macOS (Homebrew)

```bash
brew install QwerProg/bili-tools/bt
```

### Arch Linux (AUR)

```bash
# 使用 yay
yay -S bili-tools-bin

# 或使用 paru
paru -S bili-tools-bin
```

### Windows

#### Winget
```bash
winget install QwerProg.bt
```

#### Scoop

```powershell
# 安装 Scoop
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression

# 添加 bucket
scoop bucket add QwerProg https://github.com/QwerProg/bili-tools

# 安装
scoop install bt

# 日后升级
scoop update bt
```

#### 手动下载
从 [Releases](https://github.com/QwerProg/bili-tools/releases) 下载 `bt-x86_64-windows.zip`，解压后即可运行。

## 使用

```bash
bt start                      # 开播（默认，交互式）
bt start -y                   # 自动同意所有确认
bt start -r                   # 清除登录并重新登录后开播
bt start -a 398 -t "标题" -s   # 快捷参数
bt stop                       # 下播
bt stop -d 30m                # 30分钟后下播（阻塞进程，Ctrl+C 取消）
bt status                     # 查看直播状态
bt completions zsh --install  # 安装 Tab 补全
```

### 开播参数

| 参数 | 说明 |
|---|---|
| `-a, --area <ID>` | 指定分区 ID，跳过交互式选择 |
| `-t, --title <标题>` | 指定直播标题，跳过输入 |
| `-s, --show` | 显示完整推流码（默认打码） |
| `-r, --relogin` | 清除登录并重新登录后开播 |

```bash
# 非交互式一键开播
bt start --area 398 --title "晚上随便播会儿" --show
```

### 完整参数

```
Usage: bt <COMMAND> [OPTION]

Commands:
  start        开始直播 (默认支持交互式选择)
  stop         停止直播
  status       查看当前直播状态
  completions  生成 shell 补全脚本
  help         显示帮助信息
  version      显示版本号

提示：查看子命令参数请使用 `bt <command> -h`。例如 `bt start -h`。
```

## 交互流程

```
# 首次使用 — 扫码登录
✔ 选择一种登录方式 · 扫码登录（Web，推荐）
· 开始B站二维码登录流程...
[终端弹出二维码]
✅ 二维码已保存到 qrcode.png
· 等待用户处理...

# 登录后 — 正常开播
bt start
✅ 登录状态正常
✔ 是否使用上次直播的分区？ · yes
✅ 使用上次的分区: 自习室 - 372
✔ 请输入新标题（回车保留原标题） · 晚上随便播会儿

🎬 直播已开启
  推流地址  rtmp://live-push.bilivideo.com/live-bvc/
  推流码    ?strea************...g=13
  ·  推流信息已写入 ~/.config/bt/stream_info.txt

# 已在播时运行 — 询问下播
bt start
· 检测到当前正在直播中
✔ 检测到当前正在直播中，是否关闭？ · yes
· 准备关闭直播...
🎬 直播已关闭
  统计
  新增粉丝  3
  弹幕数量  42
  直播时长  7200
  最大在线  18
  累计观看  156
  粉丝勋章  2
  金仓鼠    0

# 查看状态 — 显示 B 站电视机 Logo
bt status
```

### 登录方式

默认使用 Web 扫码登录；菜单保留 TV 扫码作为备用，也支持短信、账号密码和浏览器链接登录。扫码等待约 3 分钟后超时，二维码过期时请重新登录。所有 HTTP 请求设置 30 秒超时。

Linux/macOS 上，数据目录权限为 `0700`，Cookie 和推流信息文件为 `0600`；访问数据目录时会收紧旧版本文件的权限。凭据采用临时文件写入后替换，避免写入中断损坏原文件。

开播成功后的推流文件或统计标识保存失败、下播成功后的统计查询失败，均单独显示警告，不会将已完成的开播/下播报告为失败。

## 分区选择

使用上下箭头选择分区大类，Enter 确认后选择子分区。

## 技术栈

| 类别 | 依赖 |
|---|---|
| CLI 解析 | `clap 4`（derive 宏） |
| HTTP 请求 | `minreq`；Web 登录轮询使用 `ureq` 保留多条 Cookie 响应头（rustls TLS） |
| 序列化 | `serde` + `serde_json` |
| 终端 UI | `dialoguer` |
| 二维码 | `qrcode` + `image` |
| 日志 | `log` + `env_logger` |
| 时间 | `chrono` |
| 错误处理 | `thiserror` |
| Rust 版本 | Edition 2024 |

## 架构

```mermaid
graph TD
    main --> auth
    main --> api
    main --> live
    main --> ui

    auth --> cookies
    auth --> login
    auth --> session

    api --> passport
    api --> live_api[live]
    api --> area
    api --> client

    live --> manager
    live --> stats

    ui --> area_selector
    ui --> prompts

    utils --> qrcode_util[qrcode]
    utils --> string

    start_cmd["start --relogin"] --> auth
```

```
src/
├── main.rs               # 入口与命令分发
├── api/
│   ├── client.rs         # 公共 User-Agent 常量
│   ├── passport.rs       # 二维码生成/轮询、room_id 查询
│   ├── live.rs           # 直播状态查询、分区查询、标题更新
│   └── area.rs           # 拉取全量分区列表
├── auth/
│   ├── login.rs          # 登录流程（含账号密码/短信/扫码/浏览器）
│   ├── cookies.rs        # cookies.json 读写管理
│   └── session.rs        # 登录状态验证
├── live/
│   ├── manager.rs        # 开播/下播（调用 B站 API，写 stream_info.txt）
│   └── stats.rs          # 下播后拉取直播统计数据
├── ui/
│   ├── area_selector.rs  # dialoguer 两级分区选择器
│   └── prompts.rs        # 输出宏
└── utils/
    ├── qrcode.rs         # 终端 ASCII 二维码 + PNG 保存
    └── string.rs         # 推流码打码
```

### 模块说明

| 模块路径 | 主要职责与核心逻辑 |
| :--- | :--- |
| **`main.rs`** | 解析 `clap` 命令行参数并进行子命令路由分发（`start` / `stop` / `status` / `completions` 等）；`start` 支持 `--relogin` 与 `-y` 自动确认；`stop` 支持 `-d` 倒计时下播并自动隐藏终端光标。 |
| **`auth/`** | 统一管理认证流程。`login.rs` 提供扫码、账号密码、短信及浏览器等多模式登录；`cookies.rs` 负责保存与读写 `cookies.json`（含 `room_id`、`sessdata`、`csrf_token`、`live_key`）；`session.rs` 处理凭证有效性检查。 |
| **`api/`** | 封装对 B 站官方接口的原始 HTTP 请求。基于 `minreq` 同步 HTTP 库与 Edge 130 伪装 User-Agent，实现凭证获取、房间信息查询、推流开启/关闭及分区数据同步。 |
| **`live/`** | 直播全生命周期管理。`manager.rs` 处理开播推流码获取、写入 `stream_info.txt` 以及下播操作；`stats.rs` 在下播后调用 `StopLiveData` 统计接口，格式化输出时长、弹幕、粉丝增量等数据。 |
| **`ui/`** | 命令行交互界面组件。`area_selector.rs` 采用 `dialoguer::Select` 实现优雅的两级分区选择器；`prompts.rs` 提供统一的控制台彩色输出辅助宏。 |
| **`utils/`** | 通用工具库。`qrcode.rs` 负责终端 ASCII 二维码渲染与本地 PNG 图片生成；`string.rs` 提供敏感推流码脱敏显示；`paths.rs` 管理跨平台数据存放路径（`~/.config/bt/` 或 `%APPDATA%/bt/`）。 |

## 构建与发布

CI 由 GitHub Actions 驱动，推送 `v*` tag 时自动执行多平台交叉编译与发布：

1. **Windows** (`windows-latest`)：构建 `x86_64` (x64) 与 `aarch64` (ARM64) 二进制，并分别打包为 `bt-x86_64-windows.zip` 和 `bt-arm64-windows.zip`。
2. **macOS** (`macos-latest`)：构建 `x86_64` (Intel) 与 `aarch64` (Apple Silicon) 二进制，分别打包为 `bt-x86_64-macos.zip` 和 `bt-arm64-macos.zip`。
3. **Linux** (`ubuntu-latest`)：构建 `x86_64` 与 `aarch64` 二进制，分别打包为 `bt-x86_64-linux.tar.gz` 和 `bt-arm64-linux.tar.gz`。
4. **统一发布**：六个平台构建成功后，使用 `docs/releases/v版本号.md` 发布说明生成 Release，上传完整 SHA256SUMS。
5. **包管理器更新**：运行同步脚本后推送 Scoop、Homebrew、AUR 清单，并向 `microsoft/winget-pkgs` 提交更新 PR。

Release 构建参数已做极限体积优化，并静态链接 MSVC 运行时，**无需额外安装 VC++ Redistributable**：

```toml
[profile.release]
codegen-units = 1
lto = "fat"
opt-level = "z"   # 体积优先
panic = "abort"
strip = "symbols"
```

### 版本号更新

版本号以 `Cargo.toml` 为准，同步脚本会自动更新所有包管理器清单：

```bash
# 1. 修改 Cargo.toml 中的 version
# 2. 运行同步脚本
./scripts/sync-version.sh --checksums /path/to/SHA256SUMS
```

该脚本根据发布包的 SHA256SUMS，同步更新 Scoop（含 bucket 入口）、Homebrew、WinGet 和 AUR 二进制包的版本、下载地址和校验和。必须先完成发布包构建。

## 设计亮点

- **同步请求**：常规接口使用 `minreq`，Web 登录轮询使用 `ureq` 完整读取多条 `Set-Cookie`，无需异步运行时
- **体积优化**：激进的 Release 配置使产出二进制尽可能小，适合直接分发单文件
- **跨平台分发**：Scoop、Winget、AUR、Homebrew、Cargo 均有支持
- **推流码安全**：默认打码（`prefix****...suffix`），`--show` 才显示完整推流码
- **非交互式友好**：`bt start --area ... --title ...` 组合可完全跳过交互，便于脚本调用

## 注意事项

- `cookies.json` 包含敏感凭证（SESSDATA、bili_jct），存储在系统数据目录（macOS/Linux: `~/.config/bt/`，Windows: `%APPDATA%/bt/`），请勿泄露或提交到版本控制
- 推流信息写入 `stream_info.txt`（同数据目录），第一行 RTMP 地址，第二行完整推流码
- 高频调用接口可能触发风控，请合理使用
- 本项目仅供学习交流，禁止用于违反 B 站用户协议的行为
