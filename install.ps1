# bili-tools (bit) - Standalone installer for Windows
# Usage in PowerShell:
#   irm https://raw.githubusercontent.com/qwerprog/bili-tools/main/install.ps1 | iex
#   $env:BIT_VERSION="v1.0.3"; irm https://raw.githubusercontent.com/qwerprog/bili-tools/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$repo = "qwerprog/bili-tools"
$binaryName = "bit.exe"

function Write-Info($msg) {
    Write-Host "==> " -ForegroundColor Cyan -NoNewline
    Write-Host $msg -ForegroundColor White
}

function Write-Success($msg) {
    Write-Host "`u{2714} " -ForegroundColor Green -NoNewline
    Write-Host $msg -ForegroundColor White
}

function Write-WarnMsg($msg) {
    Write-Host "`u{26A0} " -ForegroundColor Yellow -NoNewline
    Write-Host $msg -ForegroundColor Yellow
}

# 1. 检测系统架构
if (-not [System.Environment]::Is64BitOperatingSystem) {
    throw "不支持 32 位操作系统，仅支持 64 位 (x64 / arm64) Windows。"
}

$arch = "amd64"
if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
    $arch = "arm64"
}

# 2. 获取版本号
$version = $env:BIT_VERSION
if (-not $version) {
    Write-Info "正在获取最新版本..."
    try {
        $req = [System.Net.WebRequest]::Create("https://github.com/$repo/releases/latest")
        $req.AllowAutoRedirect = $false
        $resp = $req.GetResponse()
        $loc = $resp.GetResponseHeader("Location")
        $resp.Close()
        if ($loc -match "/tag/([^/]+)$") {
            $version = $Matches[1]
        }
    } catch {
        try {
            $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers @{ "User-Agent" = "bili-tools-installer" }
            $version = $release.tag_name
        } catch {
            throw "无法获取最新版本号，请指定版本安装，例如: `$env:BIT_VERSION='v1.0.3'; irm ... | iex"
        }
    }
}

if (-not $version.StartsWith("v")) {
    $version = "v$version"
}

$pkgName = "bit-$version-windows-$arch.zip"
$downloadUrl = "https://github.com/$repo/releases/download/$version/$pkgName"

Write-Info "检测到运行环境: windows-$arch"
Write-Info "准备安装 bili-tools (bit) $version..."

# 3. 下载与解压
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

try {
    $zipPath = Join-Path $tempDir $pkgName
    Write-Info "正在下载 $downloadUrl..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath

    Write-Info "正在解压安装包..."
    Expand-Archive -Path $zipPath -DestinationPath $tempDir -Force

    $srcExe = Join-Path $tempDir $binaryName
    if (-not (Test-Path $srcExe)) {
        throw "在安装包中未找到可执行文件 '$binaryName'。"
    }

    # 4. 安装路径
    $installDir = if ($env:BIT_INSTALL_DIR) {
        $env:BIT_INSTALL_DIR
    } else {
        Join-Path $env:LOCALAPPDATA "Programs\bit"
    }

    if (-not (Test-Path $installDir)) {
        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    }

    $destExe = Join-Path $installDir $binaryName
    Copy-Item -Path $srcExe -Destination $destExe -Force

    Write-Success "bili-tools (bit) $version 已成功安装到: $destExe"

    # 5. PATH 环境变量设置
    $userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
    $pathParts = $userPath -split ";" | Where-Object { $_ -ne "" }

    if ($pathParts -notcontains $installDir) {
        Write-Info "正在将安装路径添加到用户 PATH 环境变量..."
        $newPath = ($pathParts + $installDir) -join ";"
        [System.Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        $env:Path = "$env:Path;$installDir"
        Write-Success "已将 $installDir 添加至系统 PATH。"
    }

    Write-Host ""
    Write-Host "执行 'bit --help' 即可开始使用！（若新终端中无法识别，请重启终端使环境变量生效）" -ForegroundColor Green
} finally {
    if (Test-Path $tempDir) {
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
