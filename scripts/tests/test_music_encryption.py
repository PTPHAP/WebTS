import hashlib
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location('music_fix', pathlib.Path(__file__).parents[1] / 'fix-music-bot-encryption.py')
music_fix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(music_fix)

class MusicEncryptionTests(unittest.TestCase):
    def test_unknown_protocol_file_is_never_modified(self):
        with self.assertRaisesRegex(ValueError, '摘要'):
            music_fix.patched(b'unrecognized SDK file')

    def test_transform_removes_plaintext_flag_and_fake_signature_and_requires_handshake(self):
        source = ('typeFlagged: r.Voice | i.Unencrypted,\n' + music_fix.OLD).encode()
        expected = music_fix.EXPECTED
        try:
            music_fix.EXPECTED = hashlib.sha256(source).hexdigest()
            output = music_fix.patched(source).decode()
            self.assertNotIn('i.Unencrypted', output)
            self.assertNotIn('fakeSignature', output)
            self.assertIn('cryptoInitComplete', output)
            self.assertIn('encrypt(r.Voice, n, a, l, s, false, false)', output)
            with self.assertRaises(ValueError):
                music_fix.patched(output.encode())
        finally:
            music_fix.EXPECTED = expected

if __name__ == '__main__':
    unittest.main()
