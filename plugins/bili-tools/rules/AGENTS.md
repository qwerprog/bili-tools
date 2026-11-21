# bili-tools (`bit`) Agent Rules

- **Structured Output**: Always pass `--json` when invoking `bit` programmatically so outputs and errors follow the `{ "ok": true, "data": ... }` / `{ "ok": false, "error": { "kind", "message" } }` envelope.
- **Non-Interactive Safety**: Never pass `-y` / `--yes` blindly for destructive or public side-effect commands (`video upload`, `video append`, `video replace`, `video comment post/delete`, `fav clean/delete`, `history clear`, `later clear`, `live start/stop`) without explicit user confirmation first.
- **Credential Protection**: Never read, print, or transmit raw `SESSDATA`, `bili_jct`, `cookies.json`, or `stream_info.txt` contents unless the user explicitly asks to inspect a masked summary.
- **Proxy / Direct Mode**: If Bilibili live start or domestic API requests fail due to overseas/proxy IP restrictions, retry with `-D` / `--direct` without restarting the user's proxy.
