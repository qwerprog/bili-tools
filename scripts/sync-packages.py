#!/usr/bin/env python3
"""Update package manifests from Cargo.toml and GitHub release digests (or SHA256SUMS)."""
import argparse
from datetime import date
import json
import re
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument(
    '--checksums',
    type=Path,
    required=False,
    help='Optional local SHA256SUMS file; defaults to reading digests via `gh release view`',
)
args = parser.parse_args()
version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
hashes = {}
if args.checksums:
    for line in args.checksums.read_text().splitlines():
        digest, name = line.split()
        if not re.fullmatch(r'[a-fA-F0-9]{64}', digest):
            raise ValueError(f'Invalid SHA256 for {name}')
        hashes[name.lstrip('*')] = digest.lower()
else:
    raw = subprocess.check_output(
        ['gh', 'release', 'view', f'v{version}', '--json', 'assets'],
        cwd=ROOT,
        text=True,
    )
    for asset in json.loads(raw).get('assets', []):
        digest = asset.get('digest') or ''
        if digest.startswith('sha256:'):
            hex_digest = digest.removeprefix('sha256:').lower()
            if re.fullmatch(r'[a-f0-9]{64}', hex_digest):
                hashes[asset['name']] = hex_digest
base = f'https://github.com/qwerprog/bili-tools/releases/download/v{version}'
for platform, ext in [('windows', 'zip'), ('darwin', 'zip'), ('linux', 'tar.gz')]:
    for arch in ['amd64', 'arm64']:
        pkg_name = f'bit-v{version}-{platform}-{arch}.{ext}'
        if pkg_name not in hashes:
            raise ValueError(f'Missing {pkg_name} checksum')

brew = ROOT / 'pkg/homebrew/Formula/bt.rb'
brew.write_text(f'''class Bt < Formula
  desc "B站工具箱 — 命令行多功能工具与一键开播/下播"
  homepage "https://github.com/qwerprog/bili-tools"
  version "{version}"
  license "MIT"

  on_intel do
    url "{base}/bit-v{version}-darwin-amd64.zip"
    sha256 "{hashes[f'bit-v{version}-darwin-amd64.zip']}"
  end

  on_arm do
    url "{base}/bit-v{version}-darwin-arm64.zip"
    sha256 "{hashes[f'bit-v{version}-darwin-arm64.zip']}"
  end

  def install
    bin.install "bit"
  end

  test do
    assert_match "{version}", shell_output("#{{bin}}/bit --version")
  end
end
''')

today = date.today().isoformat()
installer_entries = 'Installers:\n'
for arch, file in [
    ('x64', f'bit-v{version}-windows-amd64.zip'),
    ('arm64', f'bit-v{version}-windows-arm64.zip'),
]:
    installer_entries += f'''  - Architecture: {arch}
    InstallerUrl: {base}/{file}
    InstallerSha256: {hashes[file]}
'''

installer_manifest = f'''# yaml-language-server: $schema=https://aka.ms/winget-manifest.installer.1.12.0.schema.json

PackageIdentifier: QwerProg.bili-tools
PackageVersion: {version}
InstallerLocale: en-US
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
  - RelativeFilePath: bit.exe
    PortableCommandAlias: bit
ReleaseDate: {today}
{installer_entries}ManifestType: installer
ManifestVersion: 1.12.0
'''

locale_manifest = f'''# yaml-language-server: $schema=https://aka.ms/winget-manifest.defaultLocale.1.12.0.schema.json

PackageIdentifier: QwerProg.bili-tools
PackageVersion: {version}
PackageLocale: en-US
Publisher: QwerProg
PublisherUrl: https://github.com/qwerprog/bili-tools
PublisherSupportUrl: https://github.com/qwerprog/bili-tools/issues
PackageName: bili-tools
PackageUrl: https://github.com/qwerprog/bili-tools
License: MIT
LicenseUrl: https://github.com/qwerprog/bili-tools/blob/main/LICENSE
ShortDescription: B站工具箱 (Bilibili CLI toolbox)
Moniker: bit
Tags:
  - bilibili
  - live
  - stream
  - cli
  - toolbox
ReleaseNotesUrl: https://github.com/qwerprog/bili-tools/releases/tag/v{version}
ManifestType: defaultLocale
ManifestVersion: 1.12.0
'''

version_manifest = f'''# yaml-language-server: $schema=https://aka.ms/winget-manifest.version.1.12.0.schema.json

PackageIdentifier: QwerProg.bili-tools
PackageVersion: {version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.12.0
'''

(ROOT / 'pkg/winget').mkdir(parents=True, exist_ok=True)
(ROOT / 'pkg/winget/QwerProg.bili-tools.installer.yaml').write_text(installer_manifest)
winget_dir = ROOT / f'pkg/winget-pkgs/manifests/q/QwerProg/bili-tools/{version}'
winget_dir.mkdir(parents=True, exist_ok=True)
(winget_dir / 'QwerProg.bili-tools.installer.yaml').write_text(installer_manifest)
(winget_dir / 'QwerProg.bili-tools.locale.en-US.yaml').write_text(locale_manifest)
(winget_dir / 'QwerProg.bili-tools.yaml').write_text(version_manifest)

aur = ROOT / 'pkg/aur-bin/PKGBUILD'
aur.parent.mkdir(parents=True, exist_ok=True)
aur.write_text(f'''# Maintainer: QwerProg
pkgname=bili-tools-bin
pkgver={version}
pkgrel=1
pkgdesc="A multi-functional CLI tool for Bilibili (precompiled binary)"
arch=('x86_64' 'aarch64')
url="https://github.com/qwerprog/bili-tools"
license=('MIT')
depends=('gcc-libs')
provides=('bili-tools')
conflicts=('bili-tools' 'bili-tools-git')
options=('!debug')
source_x86_64=("bit-v${{pkgver}}-linux-amd64.tar.gz::https://github.com/qwerprog/bili-tools/releases/download/v${{pkgver}}/bit-v${{pkgver}}-linux-amd64.tar.gz")
source_aarch64=("bit-v${{pkgver}}-linux-arm64.tar.gz::https://github.com/qwerprog/bili-tools/releases/download/v${{pkgver}}/bit-v${{pkgver}}-linux-arm64.tar.gz")
sha256sums_x86_64=('{hashes[f'bit-v{version}-linux-amd64.tar.gz']}')
sha256sums_aarch64=('{hashes[f'bit-v{version}-linux-arm64.tar.gz']}')
package() {{
  install -Dm755 "${{srcdir}}/bit" "${{pkgdir}}/usr/bin/bit"
}}
''')
print(f'Updated Homebrew, WinGet and AUR manifests to {version}')
