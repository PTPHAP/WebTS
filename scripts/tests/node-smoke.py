"""Native node process: loopback, synthetic keys, no production TS/mail/storage."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


def smoke(binary):
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='node-native-', dir=cache) as folder:
        base = Path(folder)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        config = base / 'node.local.toml'
        config.write_text(f'''id="fixture-node"
bind="127.0.0.1:{port}"
main_url="http://127.0.0.1:{port}"
target="127.0.0.1:19987"
file_transfer_port=30033
key_file="pair.key"
max_connections=2
allow_insecure_localhost=true
[rtc]
public_ip=""
udp_min=55100
udp_max=55101
''', encoding='utf-8')
        hidden = {'creationflags': subprocess.CREATE_NO_WINDOW} if os.name == 'nt' else {}
        for args in [('init-key', 'pair.key'), ('check-node', str(config)), ('inspect-node', str(config))]:
            value = subprocess.run([str(binary), *args], cwd=base, capture_output=True, timeout=20, **hidden)
            assert value.returncode == 0, value.stderr.decode(errors='replace')
            assert (base / 'pair.key').read_bytes() not in value.stdout + value.stderr
        with (base / 'runtime.log').open('wb') as log:
            process = subprocess.Popen([str(binary), 'serve-node', str(config)], cwd=base, stdout=log, stderr=log, **hidden)
            try:
                for _ in range(60):
                    try:
                        with urllib.request.urlopen(f'http://127.0.0.1:{port}/health', timeout=1) as response:
                            assert json.load(response)['role'] == 'voice-node'
                        break
                    except OSError:
                        assert process.poll() is None, 'native node exited'
                        time.sleep(.1)
                else:
                    raise AssertionError('node startup timeout')
                for path, status in [('/node/rtc?owner=1', 401), ('/api/identities', 404)]:
                    try:
                        urllib.request.urlopen(f'http://127.0.0.1:{port}{path}', timeout=2)
                        raise AssertionError('unexpected access')
                    except urllib.error.HTTPError as error:
                        assert error.code == status
                assert not list(base.glob('*.db*'))
            finally:
                process.terminate()
                process.wait(timeout=15)
        assert b'panicked at' not in (base / 'runtime.log').read_bytes().lower()
        print('Native node startup, private API refusal, no DB and shutdown: PASS; no real TS voice.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    smoke(parser.parse_args().binary.resolve())
