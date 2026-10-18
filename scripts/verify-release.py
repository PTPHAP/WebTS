#!/usr/bin/env python3
"""Reject incomplete, mis-versioned or corrupt runtime release assets."""
import hashlib
import importlib.util
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
    spec = importlib.util.spec_from_file_location('node_package', Path(__file__).with_name('package-node.py'))
    node = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(node)
    nodes = [f'webts-node-{version}-preview.{node.PREVIEW}-{platform}{suffix}' for platform, suffix in [('windows-x64','.zip'),('linux-x64','.tar.gz')]]
    expected.update(nodes)
    expected.update(name + '.sha256' for name in nodes)
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
    for name in nodes:
        file = folder / name
        checksum = folder / (name + '.sha256')
        if file.is_symlink() or checksum.is_symlink():
            raise ValueError('Linked node release asset')
        match = re.fullmatch(r'([a-f0-9]{64})  '+re.escape(name)+r'\n?', checksum.read_text(encoding='utf-8'))
        with file.open('rb') as stream:
            if not match or hashlib.file_digest(stream,'sha256').hexdigest() != match[1]:
                raise ValueError('Node package checksum mismatch')
    print('Four core packages and two explicitly preview node packages verified; no private assets accepted.')


if __name__ == '__main__':
    version = tomllib.loads((Path(__file__).parents[1] / 'server/Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    verify(Path(sys.argv[1]), sys.argv[2], version)
