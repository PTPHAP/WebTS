import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('release_check', Path(__file__).parents[1] / 'verify-release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
package_spec = importlib.util.spec_from_file_location('windows_package', Path(__file__).parents[1] / 'package-windows.py')
windows = importlib.util.module_from_spec(package_spec)
package_spec.loader.exec_module(windows)


class ReleaseTests(unittest.TestCase):
    def prepare(self, root):
        for kind, name in {'SOURCE': 'source.zip', 'LINUX': 'linux-x64.tar.gz', 'WINDOWS': 'windows-x64.zip', 'DOCKER': 'docker-linux-x64.tar.gz'}.items():
            path = root / ('webts-1.2.3-' + name)
            path.write_bytes(b'isolated artifact fixture')
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            (root / ('SHA256SUMS_' + kind)).write_text(f'{digest}  {path.name}\n')

    def test_complete_matching_release_and_no_private_extra_files(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.prepare(root)
            release.verify(root, 'v1.2.3', '1.2.3')
            (root / 'config.local.toml').write_text('not for release')
            with self.assertRaises(ValueError):
                release.verify(root, 'v1.2.3', '1.2.3')

    def test_refuse_wrong_version_missing_platform_or_tampered_package(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.prepare(root)
            with self.assertRaises(ValueError):
                release.verify(root, 'v1.2.4', '1.2.3')
            path = root / 'webts-1.2.3-windows-x64.zip'
            path.write_bytes(b'changed after checks')
            with self.assertRaises(ValueError):
                release.verify(root, 'v1.2.3', '1.2.3')
            path.unlink()
            with self.assertRaises(ValueError):
                release.verify(root, 'v1.2.3', '1.2.3')

    def test_windows_package_uses_public_allowlist_and_refuses_private_frontend(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            def write(name, data=b'fixture'):
                file = root / name
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_bytes(data)
            for name in windows.PUBLIC:
                write(name)
            write('server/Cargo.toml', b'[package]\nversion="1.2.3"\n')
            for name in ('index.html', 'crypto/webts_crypto.js', 'crypto/webts_crypto_bg.wasm', 'audio/0.4.1-gtcrn.wasm', 'audio/0.4.1-rnnoise.wasm', 'audio/0.4.1-rnnoise_simd.wasm', 'audio/gtcrn-0.4.1-strength1.js', 'audio/rnnoise-0.4.1-voice4.js'):
                write('web/dist/' + name)
            for name in ('react','react-dom','scheduler','qrcode','dijkstrajs'):
                write('web/node_modules/' + name + ('/LICENSE.md' if name=='dijkstrajs' else '/LICENSE'))
            write('licenses/audio/LICENSE'); write('vendor/tsclientlib/LICENSE')
            write('web-ts.exe', b'MZfixture'); write('data/user.db', b'private fixture')
            write('secrets/master.key', b'private fixture'); write('docs/HANDOFF.md', b'private fixture')
            with patch.object(windows.subprocess, 'check_output', return_value=b'docs/HANDOFF.md\0'):
                package = windows.package(root / 'web-ts.exe', root / 'output', root)
                with zipfile.ZipFile(package) as archive:
                    self.assertFalse(any(name.endswith(('user.db','master.key','HANDOFF.md')) for name in archive.namelist()))
                    self.assertTrue(archive.read('webts-1.2.3-windows-x64/scripts/windows-menu.ps1').startswith(b'\xef\xbb\xbf'))
                write('web/dist/config.local.toml', b'private fixture')
                with self.assertRaises(ValueError):
                    windows.package(root / 'web-ts.exe', root / 'refused', root)
                self.assertFalse((root / 'refused/webts-1.2.3-windows-x64.zip').exists())
