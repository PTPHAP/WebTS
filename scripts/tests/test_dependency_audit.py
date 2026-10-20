import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('dependency_audit', Path(__file__).parents[1] / 'audit-dependencies.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class AdvisoryTests(unittest.TestCase):
    def test_roots_advisory_exception_requires_exact_reviewed_reexport_and_patched_parent(self):
        item = {'package': {'ecosystem': 'crates.io', 'name': 'webpki-roots'}, 'version': '0.26.11'}
        checksum = '521bc38abb08001b01866da9f51eb7c5d647a19260e00054a8c7fd5f9e57f7a9'
        parent_checksum = '1c1c8e20c8a38fc7d65abfe74eb7280e4aa667e8eec8bc84996d2874810748d9'
        registry = 'registry+https://github.com/rust-lang/crates.io-index'
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for parent, digest, allowed in [('1.0.10', checksum, True), ('1.0.9', checksum, False), ('1.0.10', '0' * 64, False)]:
                with self.subTest(parent=parent, checksum=digest):
                    (root / 'Cargo.lock').write_text(f'[[package]]\nname="webpki-roots"\nversion="0.26.11"\nsource="{registry}"\nchecksum="{digest}"\ndependencies=["webpki-roots {parent}"]\n[[package]]\nname="webpki-roots"\nversion="{parent}"\nsource="{registry}"\nchecksum="{parent_checksum}"\n')
                    self.assertEqual(audit.patched_root_reexport(root, item, 'RUSTSEC-2026-0381'), allowed)
                    self.assertFalse(audit.patched_root_reexport(root, item, 'RUSTSEC-OTHER'))
                    with patch.object(audit, 'coordinates', return_value=[item]), patch.object(audit, 'query', side_effect=[[{'vulns': [{'id': 'control'}]}], [{'vulns': [{'id': 'RUSTSEC-2026-0381'}]}]]), patch('sys.stdout', new_callable=io.StringIO):
                        self.assertEqual(audit.audit(root), 0 if allowed else 1)

    def test_only_public_lockfile_coordinates_are_sent(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'web').mkdir()
            (root / 'Cargo.lock').write_text('[[package]]\nname="public-crate"\nversion="1.2.3"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n[[package]]\nname="private-vendor"\nversion="0.1.0"\n')
            (root / 'web/package-lock.json').write_text(json.dumps({'packages': {'': {'name': 'private-app', 'version': '1.0.0'}, 'node_modules/@scope/example': {'version': '2.0.0'}}}))
            queries = audit.coordinates(root)
            self.assertEqual(len(queries), 2)
            self.assertNotIn('private-', json.dumps(queries))
            self.assertEqual(queries[1]['package']['name'], '@scope/example')

    def test_incomplete_advisory_pages_are_not_accepted_as_clean(self):
        for response in [{'results': []}, {'results': [{'next_page_token': 'more'}]}]:
            with patch.object(audit.urllib.request, 'urlopen', return_value=io.BytesIO(json.dumps(response).encode())):
                with self.assertRaises(ValueError):
                    audit.query([{'package': {'ecosystem': 'npm', 'name': 'fixture'}, 'version': '1.0.0'}])

    def test_positive_control_failure_and_real_findings_fail_the_gate(self):
        with patch.object(audit, 'query', return_value=[{}]):
            with self.assertRaises(ValueError): audit.audit(Path('.'))
        with patch.object(audit, 'coordinates', return_value=[{'package': {'ecosystem': 'npm', 'name': 'fixture'}, 'version': '1.0.0'}]), patch.object(audit, 'query', side_effect=[[{'vulns': [{'id': 'control'}]}], [{'vulns': [{'id': 'GHSA-fixture'}]}]]), patch('sys.stdout', new_callable=io.StringIO):
            self.assertEqual(audit.audit(Path('.')), 1)


if __name__ == '__main__':
    unittest.main()
