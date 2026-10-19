#!/usr/bin/env python3
"""Package Windows runtime using a public-file allowlist, not a directory snapshot."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = ('README.md', 'CHANGELOG.md', 'PRIVACY.md', 'TERMS.md', 'LICENSE',
          'THIRD_PARTY_NOTICES.md', 'config.example.toml', 'webts.cmd',
          'scripts/windows-menu.ps1', 'docs/WINDOWS.md', 'docs/DOCKER.md', 'docs/RELEASE-GATES.md')


def safe_files(folder, base):
    if folder.is_symlink() or (hasattr(folder.lstat(), 'st_file_attributes') and folder.lstat().st_file_attributes & 0x400):
        raise ValueError('Linked runtime root')
    folder = folder.resolve()
    for file in sorted(folder.rglob('*')):
        if file.is_symlink() or (hasattr(file.lstat(), 'st_file_attributes') and file.lstat().st_file_attributes & 0x400):
            raise ValueError('Linked runtime input')
        if file.is_file():
            if not file.resolve().is_relative_to(base.resolve()):
                raise ValueError('Runtime input outside its root')
            yield file


def package(binary, output, root=ROOT):
    version = tomllib.loads((root / 'server/Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    stem = f'webts-{version}-windows-x64'
    output.mkdir(parents=True, exist_ok=True)
    target = output / f'{stem}.zip'
    if target.exists():
        raise FileExistsError('Preserve existing runtime package')
    with binary.open('rb') as executable:
        magic = executable.read(2)
    if binary.is_symlink() or magic != b'MZ':
        raise ValueError('Expected a native Windows executable')
    dist = root / 'web/dist'
    for required in ('index.html', 'crypto/webts_crypto.js', 'crypto/webts_crypto_bg.wasm', 'audio/0.4.1-gtcrn.wasm', 'audio/0.4.1-rnnoise.wasm', 'audio/0.4.1-rnnoise_simd.wasm', 'audio/gtcrn-0.4.1-strength1.js', 'audio/rnnoise-0.4.1-voice5.js'):
        if not (dist / required).exists():
            raise ValueError(f'Incomplete frontend: {required}')
    inputs = [(binary, 'web-ts.exe')]
    inputs.extend((root / name, name) for name in PUBLIC)
    tracked = subprocess.check_output(['git', '-C', str(root), 'ls-files', '-z', 'docs', 'deploy']).decode().split('\0')
    # Only published, tracked documentation and deployment templates.
    inputs.extend((root / name, name) for name in tracked if name and not name.endswith(('HANDOFF.md', 'ACCEPTANCE.md', 'DECISIONS.md', 'GITHUB.md', 'PRIVACY-AUDIT.md')))
    inputs.extend((file, file.relative_to(root).as_posix()) for file in safe_files(dist, root))
    inputs.extend((file, file.relative_to(root).as_posix()) for file in safe_files(root / 'licenses', root))
    cargo = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo'))
    for registry in (cargo / 'registry/src').glob('*'):
        for crate in registry.iterdir():
            for file in safe_files(crate, crate):
                if len(file.relative_to(crate).parts) <= 2 and file.name.upper().startswith(('LICENSE', 'NOTICE', 'COPYING')):
                    inputs.append((file, f'licenses/rust/{crate.name}/{file.relative_to(crate).as_posix()}'))
    for name in ('react', 'react-dom', 'scheduler', 'qrcode', 'dijkstrajs'):
        file = root / f'web/node_modules/{name}/LICENSE'
        if not file.is_file():
            file = root / f'web/node_modules/{name}/LICENSE.md'
        if not file.is_file():
            raise ValueError('Install pinned frontend dependencies before packaging licenses')
        inputs.append((file, f'licenses/npm-{name}/LICENSE'))
    inputs.extend((file, f'licenses/tsclientlib/{file.relative_to(root / "vendor/tsclientlib").as_posix()}') for file in safe_files(root / 'vendor/tsclientlib', root) if len(file.relative_to(root / 'vendor/tsclientlib').parts) <= 2 and file.name.upper().startswith(('LICENSE', 'NOTICE', 'COPYING')))
    unique = {name: file for file, name in inputs}
    for name, file in unique.items():
        path = Path(name)
        if path.name in {'config.local.toml', 'node.local.toml', '.env', 'PROGRESS.md', 'PROJECT_STATE.md', 'ROADMAP.md', 'REQUIREMENTS.md'} or path.suffix in {'.key', '.pem', '.db', '.db-shm', '.db-wal', '.log'} or name.startswith(('secrets/', 'data/', 'backups/', '.cache/', '.tools/')):
            raise ValueError('Private file in runtime inputs')
        if file.is_symlink() or not file.is_file():
            raise ValueError(f'Invalid package input: {name}')
    with zipfile.ZipFile(target, 'x', zipfile.ZIP_DEFLATED) as archive:
        for name, file in unique.items():
            if file.is_symlink() or not file.is_file():
                raise ValueError(f'Invalid package input: {name}')
            data = file.read_bytes()
            if name.endswith('.ps1'):
                data = b'\xef\xbb\xbf' + data.decode('utf-8-sig').encode('utf-8')
            archive.writestr(f'{stem}/{name}', data)
    with target.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    (output / 'SHA256SUMS_WINDOWS').write_text(f'{digest}  {target.name}\n', encoding='utf-8')
    print(f'Windows runtime: {target.name}; SHA256: {digest}')
    return target


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/web-ts.exe')
    parser.add_argument('--output', type=Path, default=ROOT / '.cache/windows-release')
    args = parser.parse_args()
    package(args.binary.resolve(), args.output.resolve())
