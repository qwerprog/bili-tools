#!/usr/bin/env bash
set -euo pipefail
# 使用已发布产物的实际校验和同步版本，避免发布 URL 与旧 hash 混用。
cd "$(dirname "$0")/.."
exec python3 scripts/sync-packages.py "$@"
