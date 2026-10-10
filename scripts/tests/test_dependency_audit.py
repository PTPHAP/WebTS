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
