---
name: bili-tools
description: >-
  Operate Bilibili via the `bit` CLI (bili-tools). Use when the user asks to
  inspect, search, summarize, download, or upload Bilibili videos or bangumi/PGC,
  read or post comments and sub-replies, manage favorites, watch-later, or
  watch history, check user profiles/dynamics/rankings, or manage Bilibili live
  streams, danmaku, and 24/7 recording.
---

# Bilibili CLI (`bit`) Skill

Use the `bit` command-line tool with `--json` to interact with Bilibili safely and deterministically.

## Pre-Flight Check

1. Verify `bit` is installed:
   ```bash
   command -v bit || cargo install --git https://github.com/qwerprog/bili-tools
   ```
2. Discover the exact CLI schema if unsure of a flag:
   ```bash
   bit --json --help
   ```
3. Check authentication state before any account-scoped or write operation:
   ```bash
   bit --json auth status
   ```
   - If `is_login` is `false` for a login-required action, instruct the user to run `bit auth login` interactively (QR scan cannot run inside non-TTY `--json`).

## Common Workflows

### 1. Resolve Any Link, ID, or Short URL
```bash
bit --json resolve "https://b23.tv/pigt3PQ"
```

### 2. Summarize or Inspect a Video / Bangumi (`BV`/`av`/`ep`/`ss`/`md`)
```bash
bit --json video info BV1hp4y1k7SV
bit --json video summary BV1hp4y1k7SV
bit --json video subtitle BV1hp4y1k7SV --format srt -o sub.srt
bit --json video tags BV1hp4y1k7SV
```

### 3. Read Comments & Sub-Replies ("楼中楼")
```bash
bit --json video comments BV1hp4y1k7SV --sort hot --page 1
# Expand sub-replies under a root comment RPID:
bit --json video comments BV1hp4y1k7SV --root <RPID> --page 1
```

### 4. Download Video, Audio, Danmaku, or Sidecars
```bash
# Full muxed MP4 with danmaku (.ass), subtitle, and cover:
bit --json video download BV1hp4y1k7SV -p 1 -Q 1080p60 -e h264 --danmaku --subtitle --cover
# Audio-only (.flac):
bit --json video download BV1hp4y1k7SV --audio-only -o audio.flac
```

### 5. Search, Rankings, User Profiles & Feeds
```bash
bit --json search video "Rust" --order click
bit --json search hot --limit 20
bit --json rank popular --page 1 --page-size 20
bit --json user info [UID]
bit --json feed --type video
```

### 6. Live Streaming, Danmaku & Recording
```bash
bit --json live info 76
bit --json live stream 76 -Q 10000
# Direct domestic mode (-D) bypasses local proxy/TUN for live start/stop:
bit -D live start -a 372 -s
bit --json -D live stop -y
# Record stream + time-synced .ass danmaku (24/7 watch mode with -w):
bit live record 76 --danmaku -w -o live.flv
```

### 7. Upload & Studio Management (Confirm Before Running)
```bash
bit --json video studio ls
bit --json video upload demo.mp4 --title "标题" --tid 171 --tag "工具,CLI"
bit --json video append <BVID> part2.mp4
```

## Detailed Reference

For complete command trees, output template tokens, quality/codec aliases, upload lines, and error kinds, read [commands.md](./references/commands.md).
