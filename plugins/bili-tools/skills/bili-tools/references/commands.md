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
| `video` | `info`, `streams`, `subtitle`, `subtitle-upload`, `subtitle-list`, `subtitle-delete`, `summary`, `tags`, `related`, `danmaku`, `comments [--compact\|-i\|--root\|--raw]`, `comment (post\|like\|delete)`, `like`, `coin`, `triple`, `download`, `upload`, `append`, `replace`, `edit`, `topics`, `studio (ls\|show)` |
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
- `--topic <NAME>` / `--topic-id <ID>` / `--mission-id <ID>`: bind official Bilibili campaign/activity tag (also supported in `video edit`; query available activities with `bit video topics`).

## Subtitle Track Options (`video subtitle-upload` / `subtitle-list` / `subtitle-delete`)

```bash
bit video subtitle-upload <VIDEO> <FILE> --lang <CODE> [-p N] [--draft] [--sign] [--no-verify] [--dry-run]
bit video subtitle-list [VIDEO] [-p N] [--status all|draft|published|deleted] [--list-page N] [--page-size N]
bit video subtitle-delete <VIDEO> --id <SUBTITLE_ID> [--id ...] [-p N] [-y]
```

- **Part (`-p`, alias `--page` / `--part`)**: all three subcommands use `-p` for the part number; `--list-page` (not `--page`) paginates the account-wide listing.
- **Input**: `subtitle-upload` only accepts `.srt`; it is converted to BCC client-side (raw SRT is rejected by Bilibili).
- **Language (`--lang`)**: base ISO codes `en`, `zh`, `zh-Hant`, `ja`, `ko`. Only the listed aliases are accepted and normalized: `zh-Hans`/`zh-CN`→`zh`, `zh-TW`/`zh-HK`/`zh-MO`→`zh-Hant`, `en-US`→`en`, `ja-JP`→`ja`, `ko-KR`→`ko`. Any other value (e.g. `jp`, `zh-foo`, `en-GB`) fails with an `Input` error before any request.
- **Versioning**: subtitle tracks are bound to `cid` (part), and each upload adds a new version for the same `(cid, lang)`; old versions stay in the list.
- **Draft (`--draft`)**: sends `submit=0` (not submitted for review). Without it, `submit=1`.
- **Dry run (`--dry-run`)**: parses and converts only, zero network requests, no login required; JSON contains the full `bcc` object and entry/timeline stats.
- **Listing**: always uses `author/list`. With `VIDEO` it resolves the cid and filters by `oid` (all pages). Without `VIDEO` it scans all pages, applies `--status`, then pages the filtered result with `--list-page`/`--page-size`; JSON reports `matched_total` (matches overall) alongside `total` (returned here) and `account_total`.
- **Review delay**: after submit, anonymous playback waits for approval (a few minutes). Not visible anonymously ≠ failure.
- **Verification**: after `draft/save`, the command polls page 1 of `author/list` for a new `(cid, lang)` entry (the list is `mtime`-descending, so a new version is always there; server index lag measured ~40 s; default budget 90 s, wall-clock). If no new version appears, it fails instead of reporting a false success; transient poll errors are retried within that budget, and if every poll failed the underlying error is reported.
- **`--no-verify`**: skip that poll (useful for large batches). JSON reports `verified=false`; verify afterwards with `subtitle-list`.
- **Deletion**: soft delete; `status` first becomes `4` (visible in the list for a short window) and the server later drops the entry. Non-interactive or `--json` invocations require `-y`.

## JSON Error Envelope Kinds

`Input`, `Auth`, `Api`, `Network`, `Parse`, `Io`, `ExternalTool`, `Cancelled`.
