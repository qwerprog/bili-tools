#!/usr/bin/env bash
# bili-tools (bit) - Standalone installer for Linux & macOS
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/qwerprog/bili-tools/main/install.sh | bash
#   BIT_VERSION=v1.0.4 curl -fsSL ... | bash

set -euo pipefail

REPO="qwerprog/bili-tools"
BINARY_NAME="bit"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

info() {
    printf "${BLUE}==>${NC} ${BOLD}%s${NC}\n" "$1"
}

success() {
    printf "${GREEN}✔${NC} ${BOLD}%s${NC}\n" "$1"
}

warn() {
    printf "${YELLOW}⚠${NC} %s\n" "$1"
}

error() {
    printf "${RED}✖ 错误:${NC} %s\n" "$1" >&2
    exit 1
}

# 1. 检测操作系统
OS="$(uname -s)"
case "$OS" in
    Linux)
        PLATFORM="linux"
        EXT="tar.gz"
        ;;
    Darwin)
        PLATFORM="darwin"
        EXT="zip"
        ;;
    *)
        error "不支持的操作系统: $OS。请前往 https://github.com/$REPO/releases 手动下载预编译包。"
        ;;
esac

# 2. 检测系统架构
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        TARGET_ARCH="amd64"
        ;;
    aarch64|arm64)
        TARGET_ARCH="arm64"
        ;;
    *)
        error "不支持的系统架构: $ARCH。请从源码编译或前往 Releases 查看可用产物。"
        ;;
esac

# 3. 获取版本号
if [ -n "${BIT_VERSION:-}" ]; then
    VERSION="$BIT_VERSION"
    case "$VERSION" in
        v*) ;;
        *) VERSION="v$VERSION" ;;
    esac
else
    info "正在获取最新版本..."
    TAG="$(curl -sI "https://github.com/$REPO/releases/latest" 2>/dev/null | grep -i '^location:' | sed 's/.*tag\///' | tr -d '\r\n')"
    if [ -z "$TAG" ]; then
        TAG="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)"
    fi
    if [ -z "$TAG" ]; then
        error "无法获取最新版本号，您可以通过指定环境变量安装：BIT_VERSION=v1.0.4 bash install.sh"
    fi
    VERSION="$TAG"
fi

PKG_NAME="bit-${VERSION}-${PLATFORM}-${TARGET_ARCH}.${EXT}"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/${VERSION}/${PKG_NAME}"

info "检测到运行环境: ${PLATFORM}-${TARGET_ARCH}"
info "准备安装 bili-tools (${BINARY_NAME}) ${VERSION}..."

# 4. 依赖检查
if [ "$EXT" = "zip" ] && ! command -v unzip >/dev/null 2>&1; then
    error "解压 $PKG_NAME 需要 unzip 命令，请先安装 unzip。"
fi
if [ "$EXT" = "tar.gz" ] && ! command -v tar >/dev/null 2>&1; then
    error "解压 $PKG_NAME 需要 tar 命令，请先安装 tar。"
fi

# 5. 下载与解压
TMP_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

info "正在下载 ${DOWNLOAD_URL}..."
if command -v curl >/dev/null 2>&1; then
    curl -fL --progress-bar "$DOWNLOAD_URL" -o "$TMP_DIR/$PKG_NAME"
elif command -v wget >/dev/null 2>&1; then
    wget -q --show-progress -O "$TMP_DIR/$PKG_NAME" "$DOWNLOAD_URL"
else
    error "需要 curl 或 wget 工具以下载文件。"
fi

info "正在解压安装包..."
if [ "$EXT" = "zip" ]; then
    unzip -q "$TMP_DIR/$PKG_NAME" -d "$TMP_DIR"
else
    tar -xzf "$TMP_DIR/$PKG_NAME" -C "$TMP_DIR"
fi

if [ ! -f "$TMP_DIR/$BINARY_NAME" ]; then
    error "在安装包中未找到可执行文件 '$BINARY_NAME'。"
fi

# 6. 安装至目标路径
# 优先级: BIT_INSTALL_DIR > /usr/local/bin (若有写权限) > ~/.local/bin
if [ -n "${BIT_INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$BIT_INSTALL_DIR"
elif [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
else
    INSTALL_DIR="${HOME}/.local/bin"
fi

mkdir -p "$INSTALL_DIR"
install -m 755 "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"

success "bili-tools (${BINARY_NAME}) ${VERSION} 已成功安装到: ${INSTALL_DIR}/${BINARY_NAME}"

# 7. PATH 环境变量检查
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        warn "${INSTALL_DIR} 尚未包含在系统的 PATH 环境变量中。"
        echo "为了能在任意终端直接输入 '${BINARY_NAME}'，建议执行以下命令将其加入 PATH："
        echo ""
        CURRENT_SHELL="$(basename "${SHELL:-bash}")"
        case "$CURRENT_SHELL" in
            zsh)
                echo "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.zshrc"
                echo "  source ~/.zshrc"
                ;;
            fish)
                echo "  fish_add_path ${INSTALL_DIR}"
                ;;
            *)
                echo "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc"
                echo "  source ~/.bashrc"
                ;;
        esac
        echo ""
        ;;
esac

echo "执行 '${BINARY_NAME} --help' 即可开始使用！"
