#!/usr/bin/env python3
"""Update package manifests from Cargo.toml and the release SHA256SUMS."""
import argparse
from datetime import date
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--checksums', type=Path, required=True, help='Downloaded release SHA256SUMS')
args = parser.parse_args()
version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
hashes = {}
for line in args.checksums.read_text().splitlines():
    digest, name = line.split()
    if not re.fullmatch(r'[a-fA-F0-9]{64}', digest):
        raise ValueError(f'Invalid SHA256 for {name}')
    hashes[name.lstrip('*')] = digest.lower()
base = f'https://github.com/qwerprog/bili-tools/releases/download/v{version}'
for platform, ext in [('windows', 'zip'), ('macos', 'zip'), ('linux', 'tar.gz')]:
    for arch in ['x86_64', 'arm64']:
        if f'bt-{arch}-{platform}.{ext}' not in hashes:
            raise ValueError(f'Missing {arch} {platform} checksum')

brew = ROOT / 'pkg/homebrew/Formula/bt.rb'
text = brew.read_text()
text = re.sub(r'version "[^"]+"', f'version "{version}"', text)
text = re.sub(r'/download/v[^/]+/', f'/download/v{version}/', text)
for arch in ['x86_64', 'arm64']:
    text = re.sub(rf'(bt-{arch}-macos.zip"\s+sha256 ")[^"]+', lambda m: m[1] + hashes[f'bt-{arch}-macos.zip'], text)
brew.write_text(text)

today = date.today().isoformat()
installer_entries = 'Installers:\n'
for arch, file in [('x64', 'bt-x86_64-windows.zip'), ('arm64', 'bt-arm64-windows.zip')]:
    installer_entries += f'''  - Architecture: {arch}
    InstallerUrl: {base}/{file}
    InstallerSha256: {hashes[file]}
'''

installer_manifest = f'''# yaml-language-server: $schema=https://aka.ms/winget-manifest.installer.1.12.0.schema.json

PackageIdentifier: QwerProg.bt
PackageVersion: {version}
InstallerLocale: en-US
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
  - RelativeFilePath: bt.exe
    PortableCommandAlias: bt
ReleaseDate: {today}
{installer_entries}ManifestType: installer
ManifestVersion: 1.12.0
'''

locale_manifest = f'''# yaml-language-server: $schema=https://aka.ms/winget-manifest.defaultLocale.1.12.0.schema.json

PackageIdentifier: QwerProg.bt
PackageVersion: {version}
PackageLocale: en-US
Publisher: QwerProg
PublisherUrl: https://github.com/qwerprog/bili-tools
PublisherSupportUrl: https://github.com/qwerprog/bili-tools/issues
PackageName: bt
PackageUrl: https://github.com/qwerprog/bili-tools
License: MIT
LicenseUrl: https://github.com/qwerprog/bili-tools/blob/main/LICENSE
ShortDescription: B站工具箱 (Bilibili CLI toolbox)
Moniker: bt
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

PackageIdentifier: QwerProg.bt
PackageVersion: {version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.12.0
'''

(ROOT / 'pkg/winget/QwerProg.bt.installer.yaml').write_text(installer_manifest)
winget_dir = ROOT / f'pkg/winget-pkgs/manifests/q/QwerProg/bt/{version}'
winget_dir.mkdir(parents=True, exist_ok=True)
(winget_dir / 'QwerProg.bt.installer.yaml').write_text(installer_manifest)
(winget_dir / 'QwerProg.bt.locale.en-US.yaml').write_text(locale_manifest)
(winget_dir / 'QwerProg.bt.yaml').write_text(version_manifest)

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
source_x86_64=("bt-${{pkgver}}-x86_64-linux.tar.gz::https://github.com/qwerprog/bili-tools/releases/download/v${{pkgver}}/bt-x86_64-linux.tar.gz")
source_aarch64=("bt-${{pkgver}}-arm64-linux.tar.gz::https://github.com/qwerprog/bili-tools/releases/download/v${{pkgver}}/bt-arm64-linux.tar.gz")
sha256sums_x86_64=('{hashes['bt-x86_64-linux.tar.gz']}')
sha256sums_aarch64=('{hashes['bt-arm64-linux.tar.gz']}')
package() {{
  install -Dm755 "${{srcdir}}/bt" "${{pkgdir}}/usr/bin/bt"
}}
''')
print(f'Updated Homebrew, WinGet and AUR manifests to {version}')
