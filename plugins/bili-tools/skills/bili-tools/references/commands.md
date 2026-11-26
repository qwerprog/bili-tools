# `bit` Command & Option Reference

## Global Flags

| Flag | Description |
| :--- | :--- |
| `--json` | Machine-readable JSON envelope (`ok`, `data` / `error`) |
| `-q, --quiet` | Suppress decorative output |
| `-v, --verbose` | Verbose logging (`-v` info, `-vv` debug, `-vvv` trace) |
| `-y, --yes` | Auto-confirm destructive or interactive prompts |
| `--no-color` | Disable ANSI colors (equivalent to `NO_COLOR=1`) |
| `-D, --direct` | Domestic direct mode via IPv6 DoH + native IPv6 (bypasses proxy/TUN) |
| `-u, --cookie-file <PATH>` | Custom Cookie JSON path for multi-account isolation |

## Modules Overview

| Module | Subcommands / Usage |
| :--- | :--- |
| `resolve` | `bit resolve <ID\|URL\|b23.tv> [--no-network]` |
| `auth` | `login`, `status`, `logout`, `refresh`, `daily` |
| `live` | `start`, `stop`, `status`, `info`, `title`, `area`, `following`, `stream`, `danmaku`, `chat`, `send`, `record` |
| `search` | `video`, `user`, `live`, `hot` |
| `video` | `info`, `streams`, `subtitle`, `summary`, `tags`, `related`, `danmaku`, `comments`, `comment (post\|like\|delete)`, `like`, `coin`, `triple`, `download`, `upload`, `append`, `replace`, `studio (ls\|show)` |
| `user` | `info`, `videos`, `dynamics`, `follow`, `unfollow` |
| `feed` | `bit feed [--type all\|video\|article] [--offset <CURSOR>]` |
| `rank` | `video`, `popular` |
| `fav` | `list`, `show`, `export`, `add`, `remove`, `clean`, `create`, `delete` |
| `later` | `list`, `add`, `remove`, `clear` |
| `history` | `list`, `delete`, `clear` |
| `completions` | `bit completions [bash\|zsh\|fish\|powershell\|elvish] [--install]` |

## Download Options (`video download`, alias `dl`)

- **Semantic Qualities (`-Q, --quality`)**: `4k`, `1080p60`, `1080p+`, `1080p`, `720p60`, `720p`, `480p`, `360p` (or numeric IDs `120`, `116`, `80`, `64`, `32`, `16`).
- **Codecs (`-e, --codec`)**: `h264` (`avc`), `h265` (`hevc`), `av1`.
- **Output Template (`-o`) Variables**: `{title}`, `{bvid}`, `{aid}`, `{p}`, `{page}`, `{p_title}`, `{part}`, `{quality}`, `{codec}`.
- **Sidecars**: `--danmaku` (default `.ass`), `--subtitle`, `--cover`, `--audio-only` (`.flac`), `--video-only`, `--skip-mux`.

## Upload Options (`video upload`, alias `up`)

- `--line`: `bda2`, `qn`, `ws`, `tx`, `bldsa` (UPOS CDN line).
- `--tid <TID>`: Bilibili partition ID (e.g., `171` 电子竞技, `17` 单机游戏, `122` 野生技术协会).
- `--copyright <1|2>`: `1` original (default), `2` repost (requires `--source`).
- `--cover <IMAGE>` / `--cover43 <IMAGE>`: 16:9 main cover and optional 4:3 feed cover (local `jpg`/`png`/`webp` path or remote `https` URL).

## JSON Error Envelope Kinds

`Input`, `Auth`, `Api`, `Network`, `Parse`, `Io`, `ExternalTool`, `Cancelled`.
