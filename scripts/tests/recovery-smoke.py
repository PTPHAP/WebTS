#!/usr/bin/env python3
"""Isolated recovery using a trusted native binary and synthetic data; no mail or TS."""
import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
# Public test credential only. Generated offline; no additional Python dependency is needed.
PASSWORD = 'recovery-fixture-password'
PHC = '$argon2id$v=19$m=19456,t=2,p=1$d2VidHMtcmVjb3ZlcnktZml4dHVyZQ$QdYth8OS47ITfJxfAFr7xVqSMXwuCq38zqgwGmtyzyE'


def smoke(binary):
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    assert cache.resolve().is_relative_to(ROOT), 'Fixture cache must remain inside this project'
    with tempfile.TemporaryDirectory(prefix='recovery-smoke-', dir=cache) as temp:
        base = Path(temp).resolve()
        assert base.is_relative_to(cache.resolve()), 'Fixture cleanup must remain inside the generated cache'
        source, restored = base / 'source', base / 'restored'
        for root in (source, restored):
            root.mkdir(mode=0o700)
            (root / 'data').mkdir()
            (root / 'web').mkdir()
            (root / 'web/index.html').write_text('<title>Synthetic recovery fixture</title>')
        (source / 'secrets').mkdir(mode=0o700)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        origin = f'http://localhost:{port}'
        endpoint = f'http://127.0.0.1:{port}/api'
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def command(root, args, data=None, success=True):
            result = subprocess.run([str(binary), *args], input=data, cwd=root, capture_output=True, timeout=30)
            assert (result.returncode == 0) == success, 'Native recovery command failed its expected outcome'
            return result.stdout

        def api(path, body=None, cookie='', status=200, raw=False):
            request = urllib.request.Request(endpoint + path, data=None if body is None else json.dumps(body).encode(), headers={'Origin': origin, 'Content-Type': 'application/json', 'Cookie': cookie})
            try:
                response = opener.open(request, timeout=10)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                assert response.status == status, f'Unexpected status on fixture API {path}'
                data = response.read()
                return (data if raw else json.loads(data)), response.headers

        def login(email):
            policies, _ = api('/policies')
            _, headers = api('/auth/login', {'email': email, 'password': PASSWORD, 'accept_policies': True, 'policy_version': policies['version']})
            return headers['Set-Cookie'].split(';', 1)[0]

        @contextlib.contextmanager
        def running(root):
            with (root / 'gateway.log').open('ab') as log:
                process = subprocess.Popen([str(binary), 'serve', 'config.local.toml'], cwd=root, stdout=log, stderr=log)
                try:
                    deadline = time.monotonic() + 15
                    while True:
                        assert process.poll() is None, 'Isolated gateway stopped before becoming ready'
                        try:
                            health, _ = api('/health')
                            site, _ = api('/site')
                            assert health['ok'] and not health['smtp_ready']
                            assert site['site_name'] == 'Recovery fixture'
                            break
                        except OSError:
                            if time.monotonic() > deadline:
                                raise AssertionError('Isolated gateway did not become ready') from None
                            time.sleep(.1)
                    yield
                finally:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=10)

        config = f'bind="127.0.0.1:{port}"\npublic_url="{origin}"\ndatabase="data/web-ts.db"\nmaster_key_file="secrets/master.key"\nweb_dir="web"\nmax_connections=2\nallow_insecure_localhost=true\n[rtc]\npublic_ip=""\nudp_min=41000\nudp_max=41001\n'
        (source / 'config.local.toml').write_text(config, encoding='utf-8')
        command(source, ['init-key', 'secrets/master.key'])
        settings = json.loads(command(source, ['get-settings', 'config.local.toml']))
        settings['home']['site_name'] = 'Recovery fixture'
        command(source, ['set-settings', 'config.local.toml'], json.dumps(settings).encode())
        with contextlib.closing(sqlite3.connect(source / 'data/web-ts.db')) as db:
            for user_id, email in [(1, 'owner@example.invalid'), (2, 'other@example.invalid')]:
                db.execute('INSERT INTO users(id,email,password_hash,verified) VALUES(?,?,?,1)', (user_id, email, PHC))
            db.execute('INSERT INTO email_tokens VALUES(?,?,?,?)', (hashlib.sha256(b'fixture-old-reset-token').hexdigest(), 1, 'reset', int(time.time()) + 600))
            db.commit()
        spec = importlib.util.spec_from_file_location('recovery_manager_fixture', ROOT / 'scripts/webts.py')
        manager = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(manager)
        manager.ROOT = source  # Only the generated directory; never /opt/webts or a real installation.
        with running(source):
            old_cookie = login('owner@example.invalid')
            created, _ = api('/identities', {'name': 'Synthetic recovery identity', 'is_default': True}, old_cookie)
            identity_id = created['id']
            identities, _ = api('/identities', cookie=old_cookie)
            uid = identities['identities'][0]['uid']
            assert (source / 'data/web-ts.db-wal').stat().st_size > 0
            with contextlib.redirect_stdout(io.StringIO()):
                backup = manager.backup()
            api('/identities', {'name': 'After snapshot'}, old_cookie)
        shutil.copyfile(backup / 'web-ts.db', restored / 'data/web-ts.db')
        shutil.copyfile(backup / 'config.local.toml', restored / 'config.local.toml')
        shutil.copytree(backup / 'secrets', restored / 'secrets')
        assert (restored / 'secrets/master.key').read_bytes() == (source / 'secrets/master.key').read_bytes()
        with contextlib.closing(sqlite3.connect(restored / 'data/web-ts.db')) as db:
            assert db.execute('PRAGMA quick_check').fetchone() == ('ok',)
            assert not db.execute('PRAGMA foreign_key_check').fetchall()
            assert db.execute('SELECT count(*) FROM identities').fetchone() == (1,)
            assert db.execute('SELECT count(*) FROM sessions').fetchone()[0] > 0
            db.execute('PRAGMA foreign_keys=ON')
            db.execute('BEGIN IMMEDIATE')
            db.execute('DELETE FROM sessions')
            db.execute('DELETE FROM email_tokens')
            db.commit()
            assert db.execute('SELECT count(*) FROM session_details').fetchone() == (0,)
            assert db.execute('SELECT count(*) FROM email_tokens').fetchone() == (0,)
        command(restored, ['init-key', 'secrets/master.key'], success=False)
        (restored / 'secrets/wrong.key').write_text('00' * 32)
        (restored / 'wrong.toml').write_text(config.replace('secrets/master.key', 'secrets/wrong.key'), encoding='utf-8')
        command(restored, ['get-settings', 'wrong.toml'], success=False)
        (restored / 'missing.toml').write_text(config.replace('secrets/master.key', 'secrets/missing.key'), encoding='utf-8')
        command(restored, ['get-settings', 'missing.toml'], success=False)
        assert not (restored / 'secrets/missing.key').exists()
        assert json.loads(command(restored, ['get-settings', 'config.local.toml'])) == settings
        with running(restored):
            api('/me', cookie=old_cookie, status=401)
            cookie = login('owner@example.invalid')
            rows, _ = api('/identities', cookie=cookie)
            assert len(rows['identities']) == 1 and rows['identities'][0]['uid'] == uid
            exported, _ = api(f'/identities/{identity_id}/export', {'password': PASSWORD}, cookie, raw=True)
            imported, _ = api('/identities/import', {'name': 'Restored duplicate', 'identity': exported.decode()}, cookie)
            assert imported['id'] == identity_id, 'Decrypted identity must retain its UID and deduplicate'
            other_cookie = login('other@example.invalid')
            api(f'/identities/{identity_id}/export', {'password': PASSWORD}, other_cookie, status=400)
        for root in (source, restored):
            assert PASSWORD.encode() not in (root / 'gateway.log').read_bytes()
        assert exported not in (restored / 'data/web-ts.db').read_bytes()
        assert (restored / 'secrets/master.key').read_bytes() == (source / 'secrets/master.key').read_bytes()
    print('Recovery PASS: consistent WAL snapshot, original UID/export/import, settings/key preservation, old session revocation, wrong/missing key rejection and cross-account denial. Synthetic data only; no TS, SMTP, S3 or E2EE disaster acceptance.')


if __name__ == '__main__':
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    smoke(parser.parse_args().binary.resolve(strict=True))
