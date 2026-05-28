# (bili-tools)bt

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
brew tap QwerProg/bili-tools
brew install bt
```

> Homebrew 会从源码编译，首次需要 Rust 工具链，`brew` 会自动安装。

### Linux (AUR)

```bash
yay -S bili-tools       # 稳定版
yay -S bili-tools-git   # 最新 git 版
```

### Windows

#### Winget
```bash
winget install QwerProg.bt
```

#### Scoop

```powershell
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression
scoop bucket add QwerProg https://github.com/QwerProg/bili-tools
scoop install bt
# 日后升级：scoop update bt
```

#### 手动下载
从 [Releases](https://github.com/QwerProg/bili-tools/releases) 下载 `bt-x86_64-windows.zip`，解压后即可运行。

## 使用

```bash
bt start              # 开播（默认，交互式）
bt start -y           # 自动同意所有确认
bt start -r           # 清除登录并重新登录后开播
bt stop               # 下播
bt stop -d 30m        # 30分钟后下播（阻塞进程，Ctrl+C 取消）
bt status             # 查看直播状态
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
  start    开始直播 (默认支持交互式选择)
  stop     停止直播
  status   查看当前直播状态
  help     显示帮助信息
  version  显示版本号

提示：查看子命令参数请使用 `bt <command> -h`。例如 `bt start -h`。
```

## 交互流程

```
# 首次使用 — 扫码登录
✅ 需要登录，开始登录流程...
[终端弹出二维码]

# 登录后 — 正常开播
bt start
✅ 登录状态正常
📺 是否使用上次直播的分区？(回车/y=使用默认，n=选择新分区)
✅ 使用上次的分区: 自习室 - 372
📺 请输入新标题（直接回车保留原标题）:
ℹ️ 开始直播！
✅ RTMP地址: rtmp://live-push.bilivideo.com/live-bvc/
✅ 推流码: ?strea************...g=13
✅ 推流信息已写入 stream_info.txt
✅ 直播已开启，程序退出

# 已在播时运行 — 询问下播
bt start
✅ 登录状态正常
ℹ️ 检测到当前正在直播中
📺 是否关闭直播？(y/n): y
ℹ️ 准备关闭直播...
✅ 成功关闭直播
ℹ️ 直播统计信息:
ℹ️ 新增粉丝 : 3
ℹ️ 弹幕数 : 42
ℹ️ 直播时长 : 7200

# 查看状态 — 显示 B 站电视机 Logo
bt status
```

## 分区选择

使用上下箭头选择分区大类，Enter 确认后选择子分区。

## 技术栈

| 类别 | 依赖 |
|---|---|
| CLI 解析 | `clap 4`（derive 宏） |
| HTTP 请求 | `minreq`（轻量级，rustls TLS） |
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
    auth --> qr_login
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
```

```
src/
├── main.rs            # 入口与命令分发
├── api/
│   ├── client.rs      # 公共 User-Agent 常量
│   ├── passport.rs    # 二维码生成/轮询、room_id 查询
│   ├── live.rs        # 直播状态查询、分区查询、标题更新
│   └── area.rs        # 拉取全量分区列表
├── auth/
│   ├── qr_login.rs    # 扫码登录流程
│   ├── cookies.rs     # cookies.json 读写管理
│   └── session.rs     # 登录状态验证
├── live/
│   ├── manager.rs     # 开播/下播（调用 B站 API，写 stream_info.txt）
│   └── stats.rs       # 下播后拉取直播统计数据
├── ui/
│   ├── area_selector.rs  # dialoguer 两级分区选择器
│   └── prompts.rs        # 用户输入宏
└── utils/
    ├── qrcode.rs      # 终端 ASCII 二维码 + PNG 保存
    └── string.rs      # URL 参数解析、推流码打码
```

### 模块说明

**`main.rs`** — 解析 `clap` 子命令并分发：`start` / `stop` / `status`。

**`auth/`** — `qr_login.rs` 调用 Passport API 生成二维码，每 2 秒轮询一次扫码结果，成功后提取 `SESSDATA` 与 `bili_jct` 并保存到 `cookies.json`。`cookies.rs` 负责读写该文件，字段包括 `room_id`、`sessdata`、`csrf_token`、`live_key`。

**`api/`** — 所有对 B 站接口的原始请求，每个函数只做单一职责：发请求、解析 JSON、返回数据或错误。HTTP 层统一使用 `minreq`（同步），伪装成 Edge 130 浏览器 UA。

**`live/`** — `manager.rs` 负责开播（POST `startLive`，解析 RTMP 地址与推流码，写入 `stream_info.txt`，更新 `live_key`）和下播（POST `stopLive`）。`stats.rs` 在下播后调用 `StopLiveData` API 展示新增粉丝、弹幕数、在线峰值等统计。

**`ui/area_selector.rs`** — 使用 `dialoguer::Select` 实现两级分区选择器。

## 构建与发布

CI 由 GitHub Actions 驱动，推送 `v*` tag 时自动执行：

1. 在 `windows-latest` 编译 `x86_64-pc-windows-msvc` 可执行文件
2. 打包为 `bt-x86_64-windows.zip`，计算 SHA256，发布 GitHub Release
3. 自动向 `microsoft/winget-pkgs` 提交 PR 更新 winget 清单

Release 构建参数已做极限体积优化：

```toml
[profile.release]
codegen-units = 1
lto = "fat"
opt-level = "z"   # 体积优先
panic = "abort"
strip = "symbols"
```

## 设计亮点

- **极简依赖**：用 `minreq` 替代 `reqwest`，去掉异步运行时，整体为同步阻塞模型，逻辑直观
- **体积优化**：激进的 Release 配置使产出二进制尽可能小，适合直接分发单文件
- **跨平台分发**：Scoop、Winget、AUR、Homebrew、Cargo 均有支持
- **推流码安全**：默认打码（`prefix****...suffix`），`--show` 才显示完整推流码
- **非交互式友好**：`bt start --area ... --title ...` 组合可完全跳过交互，便于脚本调用

## 注意事项

- `cookies.json` 包含敏感凭证（SESSDATA、bili_jct），请勿泄露或提交到版本控制
- 推流信息写入 `stream_info.txt`，第一行 RTMP 地址，第二行完整推流码
- 高频调用接口可能触发风控，请合理使用
- 本项目仅供学习交流，禁止用于违反 B 站用户协议的行为
