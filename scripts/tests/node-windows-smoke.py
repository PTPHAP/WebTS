"""Verify/extract a public node ZIP and run system PowerShell 5.1 fixtures."""
import argparse
import hashlib
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def smoke(package, native_os=False):
    if os.name != 'nt':
        raise RuntimeError('Real Windows is required')
    if native_os and os.environ.get('GITHUB_ACTIONS') != 'true':
        raise RuntimeError('OS mutation acceptance is restricted to an isolated CI runner')
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='node-windows-', dir=cache) as folder:
        base = Path(folder)
        with zipfile.ZipFile(package) as archive:
            names = archive.namelist()
            if not names or len(names) != len(set(names)):
                raise ValueError('Duplicate or empty archive')
            prefix = names[0].split('/')[0] + '/'
            payload = {}
            for name in names:
                path = PurePosixPath(name)
                if not name.startswith(prefix) or path.is_absolute() or '..' in path.parts or '\\' in name or path.suffix in {'.key','.pem','.db','.log'}:
                    raise ValueError('Unsafe node package path')
                relative = name[len(prefix):]
                if relative.startswith(('secrets/','data/','.cache/','.tools/','backups/')) or path.name in {'node.local.toml','config.local.toml','PROGRESS.md','PROJECT_STATE.md','HANDOFF.md'}:
                    raise ValueError('Private node input')
                payload[relative] = archive.read(name)
            for line in payload['SHA256SUMS'].decode().splitlines():
                expected, name = line.split('  ', 1)
                if hashlib.sha256(payload[name]).hexdigest() != expected:
                    raise ValueError('Corrupt node input')
            archive.extractall(base / 'payload')
        package_root = base / 'payload' / prefix[:-1]
        fixture_name = 'node-installer-native.ps1' if native_os else 'node-installer.ps1'
        fixture = base / fixture_name
        fixture.write_text((ROOT / 'scripts/tests' / fixture_name).read_text(encoding='utf-8-sig'), encoding='utf-8-sig')
        scratch = base / 'scratch'
        scratch.mkdir()
        powershell = Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
        result = subprocess.run([str(powershell), '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', str(fixture), '-PackageRoot', str(package_root), '-Scratch', str(scratch)], capture_output=True, timeout=240, creationflags=subprocess.CREATE_NO_WINDOW)
        print(result.stdout.decode(errors='replace'))
        if result.returncode:
            raise RuntimeError(result.stderr.decode(errors='replace'))
        if not native_os:
            checks=base/'node-update-checks.ps1'
            checks.write_text((ROOT/'scripts/tests/node-update-checks.ps1').read_text(encoding='utf-8-sig'),encoding='utf-8-sig')
            result=subprocess.run([str(powershell),'-NoProfile','-ExecutionPolicy','Bypass','-File',str(checks),'-PackageRoot',str(package_root),'-Scratch',str(scratch),'-IsolatedModules'],capture_output=True,timeout=60,creationflags=subprocess.CREATE_NO_WINDOW)
            print(result.stdout.decode(errors='replace'))
            if result.returncode:raise RuntimeError(result.stderr.decode(errors='replace'))


if __name__ == '__main__':
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    sys.stderr.reconfigure(encoding='utf-8', errors='replace')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--native-os', action='store_true')
    args = parser.parse_args()
    smoke(args.package.resolve(), args.native_os)
