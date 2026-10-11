#!/usr/bin/env python3
"""Reject incomplete, mis-versioned or corrupt runtime release assets."""
import hashlib
from pathlib import Path
import re
import sys
import tomllib


def verify(folder, tag, version):
    if not re.fullmatch(r'\d+\.\d+\.\d+', version) or tag != f'v{version}':
        raise ValueError('Release tag must match the source version')
    names = {
        'SOURCE': f'webts-{version}-source.zip',
        'LINUX': f'webts-{version}-linux-x64.tar.gz',
        'WINDOWS': f'webts-{version}-windows-x64.zip',
        'DOCKER': f'webts-{version}-docker-linux-x64.tar.gz',
    }
    expected = set(names.values()) | {f'SHA256SUMS_{name}' for name in names}
    if {p.name for p in folder.iterdir()} != expected:
        raise ValueError('Release must contain only all four packages and their checksums')
    for kind, name in names.items():
        checksum = folder / f'SHA256SUMS_{kind}'
        match = re.fullmatch(r'([a-f0-9]{64})  (?:\.cache/(?:source-release/|windows-release/)?)?([^\s/]+)\n?', checksum.read_text(encoding='utf-8'))
        if not match or match[2] != name:
            raise ValueError('Unexpected checksum path or format')
        file = folder / name
        if file.is_symlink() or checksum.is_symlink():
            raise ValueError('Linked release asset')
        with file.open('rb') as stream:
            if hashlib.file_digest(stream, 'sha256').hexdigest() != match[1]:
                raise ValueError('Package checksum mismatch')
    print('All four versioned packages verified; no private assets accepted.')


if __name__ == '__main__':
    version = tomllib.loads((Path(__file__).parents[1] / 'server/Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    verify(Path(sys.argv[1]), sys.argv[2], version)
