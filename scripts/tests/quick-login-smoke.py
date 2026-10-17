#!/usr/bin/env python3
"""Real loopback gateway + isolated browser contexts; synthetic accounts, no SMTP/S3/TS."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import sqlite3
import subprocess
import tempfile
import time
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
PHC = '$argon2id$v=19$m=19456,t=2,p=1$d2VidHMtcmVjb3ZlcnktZml4dHVyZQ$QdYth8OS47ITfJxfAFr7xVqSMXwuCq38zqgwGmtyzyE'

def smoke(binary, output, browsers):
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    assert cache.resolve().is_relative_to(ROOT)
    output.mkdir(parents=True, exist_ok=True)
    assert output.resolve().is_relative_to(cache.resolve()), 'Proof output must stay in project .cache'
    assert (ROOT/'web/dist/index.html').is_file(), 'Build the current frontend first'
    version = tomllib.loads((ROOT/'server/Cargo.toml').read_text())['package']['version']
    with tempfile.TemporaryDirectory(prefix='quick-login-', dir=cache) as folder:
        work = Path(folder).resolve()
        assert work.is_relative_to(cache.resolve())
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        origin = f'http://127.0.0.1:{port}'
        literal = lambda p: json.dumps(str(p).replace('\\', '/'), ensure_ascii=False)
        config = work/'config.toml'
        config.write_text(f'bind="127.0.0.1:{port}"\npublic_url="{origin}"\ndatabase={literal(work/"data.db")}\nmaster_key_file={literal(work/"master.key")}\nweb_dir={literal(ROOT/"web/dist")}\nmax_connections=2\nallow_insecure_localhost=true\n[rtc]\npublic_ip=""\nudp_min=41020\nudp_max=41023\n', encoding='utf-8')
        for args in [('init-key',str(work/'master.key')),('get-settings',str(config))]:
            result = subprocess.run([str(binary),*args], cwd=work, capture_output=True, timeout=30)
            assert result.returncode == 0, 'Synthetic native initialization failed'
        with contextlib.closing(sqlite3.connect(work/'data.db')) as db:
            with db:
                for owner in [1,2]:
                    db.execute('INSERT INTO users(id,email,password_hash,verified,is_admin) VALUES(?,?,?,1,?)',(owner,f'quick-{owner}@example.invalid',PHC,int(owner==1)))
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with (output/'gateway.log').open('wb') as log:
            process = subprocess.Popen([str(binary),'serve',str(config)], cwd=work, stdout=log, stderr=log)
            try:
                deadline = time.monotonic()+20
                while True:
                    assert process.poll() is None, 'Loopback gateway stopped unexpectedly'
                    try:
                        health = json.load(opener.open(origin+'/api/health', timeout=2))
                        assert health['ok'] and not health['smtp_ready'] and health['version']==version
                        break
                    except OSError:
                        assert time.monotonic()<deadline, 'Loopback gateway did not start'
                        time.sleep(.1)
                results=[]
                for browser in browsers:
                    env=os.environ.copy()
                    run=subprocess.run(['node',str(ROOT/'web/tests/browser/quick-login-real.mjs'),origin,str(output),browser],cwd=ROOT,env=env,capture_output=True,text=True,encoding='utf-8',timeout=180)
                    if run.returncode:
                        failure=re.sub(r'#qr=[a-f0-9.]+','#qr=<REDACTED>',run.stdout+'\n'+run.stderr)
                        (output/f'{browser}-failure.log').write_text(failure,encoding='utf-8')
                        raise AssertionError(f'Real {browser} login flow failed; see private fixture log')
                    results.append(json.loads(run.stdout))
                with contextlib.closing(sqlite3.connect(work/'data.db')) as db:
                    assert db.execute('PRAGMA quick_check').fetchone()==('ok',)
                    assert not db.execute('PRAGMA foreign_key_check').fetchall()
                    assert db.execute('SELECT count(*) FROM quick_logins').fetchone()==(0,)
                    assert db.execute('SELECT count(*) FROM sessions s JOIN session_details d ON d.hash=s.hash WHERE d.public_device=1').fetchone()==(0,)
                proof={'real_gateway_http':True,'synthetic_accounts_only':True,'smtp_s3_ts_used':False,'sqlite_checks_passed':True,'pending_grants':0,'public_sessions_left':0,'version':version,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'browsers':results,'physical_phone_acceptance':False}
                (output/'proof.json').write_text(json.dumps(proof,indent=2),encoding='utf-8')
                print(json.dumps(proof))
            finally:
                process.terminate()
                try:process.wait(timeout=10)
                except subprocess.TimeoutExpired:process.kill();process.wait(timeout=10)

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--binary',required=True,type=Path)
    parser.add_argument('--output',type=Path,default=ROOT/'.cache/quick-login-smoke')
    parser.add_argument('--browsers',nargs='+',choices=['chromium','firefox','webkit','mobile-webkit'],default=['chromium'])
    args=parser.parse_args()
    smoke(args.binary.resolve(),args.output.resolve(),args.browsers)
