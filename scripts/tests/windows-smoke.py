#!/usr/bin/env python3
"""Windows-only isolated native setup/start/backup smoke; no real mail or TS traffic."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


def ps_string(value):
    return "'" + str(value).replace("'", "''") + "'"


def smoke(binary):
    if os.name != 'nt':
        raise RuntimeError('This check must run on real Windows')
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='windows-smoke-', dir=cache) as folder:
        root = Path(folder)
        shutil.copyfile(binary, root / 'web-ts.exe')
        (root / 'scripts').mkdir()
        shutil.copyfile(ROOT / 'scripts/windows-menu.ps1', root / 'scripts/windows-menu.ps1')
        (root / 'web/dist').mkdir(parents=True)
        (root / 'web/dist/index.html').write_text('<!doctype html><title>isolated fixture</title>')
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        values = {'url': f'http://localhost:{port}', 'public_ip': '', 'site_name': '测试社区', 'operator': '测试运营者', 'contact': 'demo@example.invalid', 'data_details': '隔离检查，无真实用户数据。', 'smtp_host': 'smtp.example.invalid', 'smtp_port': 465, 'smtp_user': 'demo@example.invalid', 'smtp_from': 'demo@example.invalid'}
        (root / 'fixture.json').write_text(json.dumps(values, ensure_ascii=False), encoding='utf-8-sig')
        executable = 'powershell.exe'
        environment = {name: value for name, value in os.environ.items() if name.lower() != 'psmodulepath'}
        prefix = f"$c=ConvertFrom-Json ([IO.File]::ReadAllText({ps_string(root / 'fixture.json')}));$h=@{{}};$c.PSObject.Properties|ForEach-Object{{$h[$_.Name]=$_.Value}};& {ps_string(root / 'scripts/windows-menu.ps1')} -Root {ps_string(root)} "
        def run(arguments):
            return subprocess.run([executable, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', prefix + arguments], cwd=root, env=environment, capture_output=True, timeout=45)
        setup = run("-Action setup -Configuration $h -SmtpPassword (ConvertTo-SecureString 'test-only-fake-password' -AsPlainText -Force)")
        if setup.returncode:
            raise AssertionError(setup.stderr.decode('utf-8', errors='replace'))
        config = tomllib.loads((root / 'config.local.toml').read_text(encoding='utf-8'))
        assert config['bind'] == f'127.0.0.1:{port}'
        assert config['smtp']['password_file'] == 'secrets/smtp-password'
        key = (root / 'secrets/master.key').read_bytes()
        assert len(key) == 64 and b'test-only-fake-password' not in setup.stdout
        assert run('-Action setup -Configuration $h').returncode != 0, 'Reinitialization must refuse existing data'
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with (root / 'gateway.log').open('wb') as log:
            gateway = subprocess.Popen([str(root / 'web-ts.exe'), 'serve', 'config.local.toml'], cwd=root, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 15
                while True:
                    try:
                        with opener.open(f'http://127.0.0.1:{port}/api/health', timeout=1) as response:
                            health = json.load(response)
                        break
                    except OSError:
                        if time.monotonic() > deadline:
                            raise AssertionError('Native Windows health check failed')
                        time.sleep(.2)
                assert health['version'] and health['smtp_ready']
                with opener.open(f'http://127.0.0.1:{port}/') as response:
                    assert b'isolated fixture' in response.read(), 'Native gateway must serve its packaged frontend'
                assert run('-Action status').returncode == 0, 'Status must use the configured loopback port'
                with opener.open(f'http://127.0.0.1:{port}/api/site') as response:
                    site_name = json.load(response)['site_name']
                    assert site_name == '测试社区', f'UTF-8 site settings must survive PowerShell piping: {site_name!r}'
                assert run('-Action backup').returncode != 0, 'Running gateway backup must be refused'
            finally:
                gateway.terminate()
                gateway.wait(timeout=10)
        backup = run('-Action backup')
        assert backup.returncode == 0, backup.stderr.decode('utf-8', errors='replace')
        assert list((root / 'backups').glob('*/secrets/master.key'))[0].read_bytes() == key
        assert (root / 'secrets/master.key').read_bytes() == key
        assert b'test-only-fake-password' not in (root / 'gateway.log').read_bytes()
        # Test rejecting a directory junction before setup/backup follows it.
        elsewhere = root / 'elsewhere'
        elsewhere.mkdir()
        junction = root / 'linked'
        subprocess.run(['cmd.exe', '/c', 'mklink', '/J', str(junction), str(elsewhere)], check=True, capture_output=True)
        try:
            assert run('-Action backup').returncode != 0, 'Junction must be rejected'
        finally:
            junction.rmdir()
        print('Native Windows UTF-8 setup, health, key preservation, stop-only backup and junction rejection passed.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    smoke(args.binary.resolve())
