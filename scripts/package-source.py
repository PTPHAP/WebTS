#!/usr/bin/env python3
"""Package committed source and pinned submodules; never copy the working tree."""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PRIVATE = {'PROGRESS.md', 'PROJECT_STATE.md', 'ROADMAP.md', 'REQUIREMENTS.md',
           'docs/HANDOFF.md', 'docs/ACCEPTANCE.md', 'docs/DECISIONS.md',
           'docs/GITHUB.md', 'docs/PRIVACY-AUDIT.md', 'config.local.toml', '.env'}


def git(folder, *args):
    return subprocess.check_output(['git', '-C', str(folder), *args])


def package(output, version):
    if not re.fullmatch(r'[A-Za-z0-9.-]+', version):
        raise ValueError('Invalid version')
    output.mkdir(parents=True, exist_ok=True)
    stem = f'webts-{version}-source'
    archive = output / f'{stem}.zip'
    if archive.exists():
        raise FileExistsError('Preserve the existing archive')
    revisions = {}
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as bundle:
        def tree(folder, prefix='', expected=None):
            revision = git(folder, 'rev-parse', 'HEAD').decode().strip()
            if expected is not None and revision != expected:
                raise ValueError(f'Unpinned submodule: {prefix}')
            revisions[prefix or '.'] = revision
            with tarfile.open(fileobj=io.BytesIO(git(folder, 'archive', '--format=tar', revision))) as source:
                for entry in source:
                    name = prefix + entry.name
                    path = PurePosixPath(name)
                    if path.is_absolute() or '..' in path.parts:
                        raise ValueError('Unsafe archive path')
                    if name in PRIVATE or path.parts[0] in {'.git', '.cache', '.tools', 'secrets', 'data', 'target'} or (name.startswith('.env.') and name != '.env.example'):
                        raise ValueError(f'Private path in committed source: {name}')
                    if entry.isdir():
                        continue
                    if not entry.isfile():
                        raise ValueError(f'Unsupported source entry: {name}')
                    info = zipfile.ZipInfo(f'{stem}/{name}')
                    info.create_system = 3
                    info.compress_type = zipfile.ZIP_DEFLATED
                    info.external_attr = (0o100000 | entry.mode) << 16
                    bundle.writestr(info, source.extractfile(entry).read())
            for item in git(folder, 'ls-tree', '-rz', 'HEAD').split(b'\0'):
                if not item:
                    continue
                metadata, name = item.split(b'\t', 1)
                mode, _, commit = metadata.split()
                if mode == b'160000':
                    relative = name.decode()
                    child = (folder / relative).resolve()
                    if not child.is_relative_to(ROOT):
                        raise ValueError('Submodule outside project')
                    tree(child, prefix + relative + '/', commit.decode())

        tree(ROOT)
        bundle.writestr(f'{stem}/SOURCE_VERSION.json', json.dumps({'version': version, 'commits': revisions}, indent=2) + '\n')
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (output / 'SHA256SUMS_SOURCE').write_text(f'{digest}  {archive.name}\n', encoding='utf-8')
    print(f'Source only: {archive.name}; SHA256: {digest}; pinned trees: {len(revisions)}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / '.cache' / 'source-release')
    parser.add_argument('--version', default='1.0.1')
    options = parser.parse_args()
    package(options.output.resolve(), options.version)
