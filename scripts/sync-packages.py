#!/usr/bin/env python3
"""Update package manifests from Cargo.toml and the release SHA256SUMS."""
import argparse
import json
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
base = f'https://github.com/QwerProg/bili-tools/releases/download/v{version}'
for platform, ext in [('windows', 'zip'), ('macos', 'zip'), ('linux', 'tar.gz')]:
    for arch in ['x86_64', 'arm64']:
        if f'bt-{arch}-{platform}.{ext}' not in hashes:
            raise ValueError(f'Missing {arch} {platform} checksum')

scoop = json.loads((ROOT / 'pkg/scoop/bt.json').read_text())
scoop['version'] = version
scoop['architecture'] = {
    arch: {'url': f'{base}/{file}', 'hash': hashes[file]}
    for arch, file in [('64bit', 'bt-x86_64-windows.zip'), ('arm64', 'bt-arm64-windows.zip')]
}
for path in ['pkg/scoop/bt.json', 'bucket/bt.json']:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(scoop, ensure_ascii=False, indent=2) + '\n')

brew = ROOT / 'pkg/homebrew/Formula/bt.rb'
text = brew.read_text()
text = re.sub(r'version "[^"]+"', f'version "{version}"', text)
text = re.sub(r'/download/v[^/]+/', f'/download/v{version}/', text)
for arch in ['x86_64', 'arm64']:
    text = re.sub(rf'(bt-{arch}-macos.zip"\s+sha256 ")[^"]+', lambda m: m[1] + hashes[f'bt-{arch}-macos.zip'], text)
brew.write_text(text)

winget = ROOT / 'pkg/winget/QwerProg.bt.installer.yaml'
text = winget.read_text()
text = re.sub(r'PackageVersion: .*', f'PackageVersion: {version}', text)
start = text.index('Installers:')
end = text.index('ManifestType:', start)
installers = 'Installers:\n'
for arch, file in [('x64', 'bt-x86_64-windows.zip'), ('arm64', 'bt-arm64-windows.zip')]:
    installers += f'''  - Architecture: {arch}
    InstallerUrl: {base}/{file}
    InstallerType: zip
    NestedInstallerType: portable
    NestedInstallerFiles:
      - RelativeFilePath: bt.exe
        PortableCommandAlias: bt
    InstallerSha256: {hashes[file]}
'''
winget.write_text(text[:start] + installers + text[end:])

winget_dir = ROOT / f'pkg/winget-pkgs/manifests/q/QwerProg/bt/{version}'
winget_dir.mkdir(parents=True, exist_ok=True)
installer = f"PackageIdentifier: QwerProg.bt\nPackageVersion: {version}\n" + installers + "ManifestType: installer\nManifestVersion: 1.9.0\n"
(winget_dir / 'QwerProg.bt.installer.yaml').write_text(installer)
for filename in ['QwerProg.bt.locale.en-US.yaml', 'QwerProg.bt.yaml']:
    template = (ROOT / 'pkg/winget-pkgs/manifests/q/QwerProg/bt/0.1.1' / filename).read_text()
    template = re.sub(r'PackageVersion: .*', f'PackageVersion: {version}', template)
    if 'locale' in filename:
        template = template.replace('ManifestType:', f'ReleaseNotesUrl: https://github.com/QwerProg/bili-tools/releases/tag/v{version}\nManifestType:')
    (winget_dir / filename).write_text(template)

aur = ROOT / 'pkg/aur-bin/PKGBUILD'
aur.parent.mkdir(parents=True, exist_ok=True)
aur.write_text(f'''# Maintainer: QwerProg
pkgname=bili-tools-bin
pkgver={version}
pkgrel=1
pkgdesc="B站直播开播工具 — 命令行一键开播/下播 (预编译二进制版)"
arch=('x86_64' 'aarch64')
url="https://github.com/QwerProg/bili-tools"
license=('MIT')
depends=('gcc-libs')
provides=('bili-tools')
conflicts=('bili-tools' 'bili-tools-git')
options=('!debug')
source_x86_64=("bt-${{pkgver}}-x86_64-linux.tar.gz::https://github.com/QwerProg/bili-tools/releases/download/v${{pkgver}}/bt-x86_64-linux.tar.gz")
source_aarch64=("bt-${{pkgver}}-arm64-linux.tar.gz::https://github.com/QwerProg/bili-tools/releases/download/v${{pkgver}}/bt-arm64-linux.tar.gz")
sha256sums_x86_64=('{hashes['bt-x86_64-linux.tar.gz']}')
sha256sums_aarch64=('{hashes['bt-arm64-linux.tar.gz']}')
package() {{
  install -Dm755 "${{srcdir}}/bt" "${{pkgdir}}/usr/bin/bt"
}}
''')
print(f'Updated Scoop, Homebrew, WinGet and AUR manifests to {version}')
