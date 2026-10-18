#!/usr/bin/env python3
"""Build node-only preview archives from an explicit public-file allowlist."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PREVIEW = 5
PUBLIC = ('node.example.toml', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'docs/NODE-INSTALL.md',
          'docs/VOICE-NODES.md',
          'docs/SECURITY-REVIEW-VOICE-NODES.md')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_file(file):
    if file.is_symlink() or not file.is_file():
        raise ValueError(f'Expected a regular file: {file.name}')
    for parent in (file, *file.parents):
        if getattr(parent.lstat(), 'st_file_attributes', 0) & 0x400:
            raise ValueError('Linked package input')
    return file.read_bytes()


def source_manifest():
    changed = subprocess.check_output(['git', '-C', str(ROOT), 'diff', '--name-only', '-z']).decode().split('\0')
    new = subprocess.check_output(['git', '-C', str(ROOT), 'ls-files', '--others', '--exclude-standard', '-z']).decode().split('\0')
    entries = {}
    for name in sorted(set(changed + new), key=lambda name: (name.casefold(), name)):
        file = ROOT / name
        if name and file.is_file() and not file.is_symlink():
            # Normalize text only; public source inputs contain no binaries.
            entries[name] = digest(read_file(file).replace(b'\r\n', b'\n'))
    return entries


def package(binary, output, platform, build_target=None, runtime=None):
    data = read_file(binary)
    if platform == 'windows-x64':
        pe = int.from_bytes(data[60:64], 'little')
        if data[:2] != b'MZ' or data[pe:pe+6] != b'PE\0\0\x64\x86':
            raise ValueError('Expected Windows x64 PE')
    elif data[:5] != b'\x7fELF\x02' or int.from_bytes(data[18:20], 'little') != 62:
        raise ValueError('Expected Linux x64 ELF')
    version = tomllib.loads((ROOT / 'server/Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    stem = f'webts-node-{version}-preview.{PREVIEW}-{platform}'
    suffix = '.zip' if platform == 'windows-x64' else '.tar.gz'
    output.mkdir(parents=True, exist_ok=True)
    target = output / (stem + suffix)
    checksum = output / (target.name + '.sha256')
    if target.exists() or checksum.exists():
        raise FileExistsError('Preserve existing package and checksum')
    files = {name: read_file(ROOT / name) for name in PUBLIC}
    quick = ('Windows：解压到临时专用文件夹，双击 **开始安装.cmd**，按中文说明填写安装位置、主站网址、公网地址和配对码。'
             '自动备份、停启本节点、设置专用防火墙和开机任务；可选自定义端口，无需手写配置。\n\n'
             if platform == 'windows-x64' else
             'Linux（Ubuntu 24.04 x64 / Python 3.11+ / systemd）：解压后运行 `sudo sh webts-node install`，按中文向导安装；之后 `sudo webts-node update` 检查升级。不会自动修改未知防火墙。\n\n')
    files['README.md'] = ('# WebTS 同机语音节点 · 开发预览\n\n' + quick +
                          '完整说明：[中文安装教程](docs/NODE-INSTALL.md)。\n\n'
                          '此包不是独立网站；主站也需包含节点支持并完成配对。'
                          '真实 TS 通话、Windows Server 和 Linux 真机验收仍待完成。\n').encode()
    files['web-ts.exe' if platform == 'windows-x64' else 'web-ts'] = data
    helpers = ('webts-node.cmd', '开始安装.cmd', 'scripts/node.ps1', 'scripts/node-install.ps1', 'scripts/node-update.ps1') if platform == 'windows-x64' else ('webts-node', 'scripts/node.sh', 'scripts/node-linux.py', 'deploy/webts-node.service')
    for name in helpers:
        value = read_file(ROOT / name)
        if name.endswith('.ps1'):
            value = b'\xef\xbb\xbf' + value.decode('utf-8-sig').encode('utf-8')
        if name.endswith('.sh'):
            value = value.replace(b'\r\n', b'\n')
        files[name] = value
    lock = tomllib.loads((ROOT / 'Cargo.lock').read_text(encoding='utf-8'))
    crates = {f"{p['name']}-{p['version']}" for p in lock['package'] if p.get('source', '').startswith('registry+')}
    cargo = Path(os.environ.get('CARGO_HOME', ROOT / '.tools/cargo'))
    for registry in (cargo / 'registry/src').glob('*'):
        for crate in registry.iterdir():
            if crate.name not in crates or not crate.is_dir():
                continue
            candidates = list(crate.iterdir())
            for child in list(candidates):
                if child.is_dir() and not child.is_symlink():
                    candidates.extend(child.iterdir())
            for file in candidates:
                if file.is_file() and file.name.upper().startswith(('LICENSE', 'NOTICE', 'COPYING')):
                    files[f'licenses/rust/{crate.name}/{file.relative_to(crate).as_posix()}'] = read_file(file)
    for folder, prefix in ((ROOT / 'licenses', 'licenses'), (ROOT / 'vendor/tsclientlib', 'licenses/tsclientlib')):
        candidates = folder.rglob('*') if folder.name == 'licenses' else folder.glob('*/LICENSE*')
        for file in candidates:
            if file.is_file():
                files[f'{prefix}/{file.relative_to(folder).as_posix()}'] = read_file(file)
        if folder.name != 'licenses':
            for file in folder.glob('LICENSE*'):
                if file.is_file():
                    files[f'{prefix}/{file.name}'] = read_file(file)
    sources = source_manifest()
    build_target = build_target or ('x86_64-pc-windows-gnu' if platform == 'windows-x64' else 'x86_64-unknown-linux-gnu')
    if (platform == 'windows-x64') != ('windows' in build_target):
        raise ValueError('Build target does not match package platform')
    info = {
        'version': version + f'-node-preview.{PREVIEW}', 'platform': platform,
        'base_commit': subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
        'working_tree': 'uncommitted changes; base_commit is not the full source revision' if sources else 'committed source; protocol patch is included in repository',
        'source_changes_sha256': digest(json.dumps(sources, ensure_ascii=False, sort_keys=True).encode()),
        'binary_sha256': digest(data),
        'toolchain': 'rustc 1.99.0; Cargo.lock',
        'target': build_target,
        'build_runtime': runtime or ('Windows x64 / local GNU build' if platform == 'windows-x64' else 'local Zig build; glibc >= 2.36'),
        'acceptance': 'Development preview. See review docs for individual checks; real TS voice, Windows Server and capacity require acceptance. Installer OS changes require isolated-machine verification.',
    }
    files['BUILD-INFO.json'] = (json.dumps(info, ensure_ascii=False, indent=2) + '\n').encode()
    for name in files:
        path = PurePosixPath(name)
        if path.is_absolute() or '..' in path.parts or '\\' in name or path.suffix in {'.key', '.pem', '.db', '.log'} or path.name in {'PROGRESS.md', 'PROJECT_STATE.md', 'config.local.toml', 'node.local.toml'}:
            raise ValueError('Private or unsafe archive path')
    names = sorted(files, key=lambda name: (name.casefold(), name))
    files['SHA256SUMS'] = ''.join(f'{digest(files[name])}  {name}\n' for name in names).encode()
    names = sorted(files, key=lambda name: (name.casefold(), name))
    if suffix == '.zip':
        with zipfile.ZipFile(target, 'x', zipfile.ZIP_DEFLATED) as archive:
            for name in names:
                item = zipfile.ZipInfo(f'{stem}/{name}', (2026, 10, 18, 0, 0, 0))
                item.compress_type = zipfile.ZIP_DEFLATED
                item.external_attr = 0o100644 << 16
                archive.writestr(item, files[name])
    else:
        with target.open('xb') as stream, gzip.GzipFile(fileobj=stream, mode='wb', mtime=0, filename='') as zipped, tarfile.open(fileobj=zipped, mode='w') as archive:
            for name in names:
                item = tarfile.TarInfo(f'{stem}/{name}')
                item.size = len(files[name])
                item.mode = 0o755 if name in {'web-ts', 'webts-node', 'scripts/node.sh'} else 0o644
                archive.addfile(item, io.BytesIO(files[name]))
    with target.open('rb') as stream:
        checksum.write_text(f'{hashlib.file_digest(stream, "sha256").hexdigest()}  {target.name}\n', encoding='utf-8')
    print(json.dumps({'package': str(target), 'bytes': target.stat().st_size, **info}, ensure_ascii=False))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--platform', choices=('windows-x64', 'linux-x64'), required=True)
    parser.add_argument('--target', choices=('x86_64-pc-windows-gnu','x86_64-pc-windows-msvc','x86_64-unknown-linux-gnu'))
    parser.add_argument('--runtime', help='Actual build environment, recorded without claiming runtime acceptance')
    parser.add_argument('--output', type=Path, default=ROOT / '.cache/edge-node-20261018/packages')
    args = parser.parse_args()
    package(args.binary.absolute(), args.output.absolute(), args.platform, args.target, args.runtime)
