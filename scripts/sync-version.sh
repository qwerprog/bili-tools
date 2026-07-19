#!/usr/bin/env bash
set -euo pipefail

# 从 Cargo.toml 读取版本号，同步到所有包管理器清单
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "→ 当前版本: $VERSION"

# Scoop
SCOOP_FILE="pkg/scoop/bt.json"
sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" "$SCOOP_FILE"
sed -i '' "s|/download/v[^/]*/|/download/v$VERSION/|g" "$SCOOP_FILE"
echo "✓ Scoop: $SCOOP_FILE"

# WinGet（本地校验用清单）
WINGET_FILE="pkg/winget/QwerProg.bt.installer.yaml"
sed -i '' "s/PackageVersion: .*/PackageVersion: $VERSION/" "$WINGET_FILE"
sed -i '' "s|/download/v[^/]*/|/download/v$VERSION/|g" "$WINGET_FILE"
echo "✓ WinGet: $WINGET_FILE"

# Homebrew
BREW_FILE="pkg/homebrew/Formula/bt.rb"
sed -i '' "s/version \"[^\"]*\"/version \"$VERSION\"/" "$BREW_FILE"
sed -i '' "s|/download/v[^/]*/|/download/v$VERSION/|g" "$BREW_FILE"
echo "✓ Homebrew: $BREW_FILE"

echo "→ 同步完成，hash 会在 CI 发布时自动替换"
