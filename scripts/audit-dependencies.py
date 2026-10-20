#!/usr/bin/env python3
"""Check public lockfile package coordinates against OSV; no private data sent."""
import json
from pathlib import Path
import sys
import tomllib
import urllib.request


def coordinates(root):
    queries = {}
    cargo = tomllib.loads((root / 'Cargo.lock').read_text(encoding='utf-8'))
    for item in cargo['package']:
        if item.get('source', '').startswith('registry+'):
            queries[('crates.io', item['name'], item['version'])] = None
    npm = json.loads((root / 'web/package-lock.json').read_text(encoding='utf-8'))
    for path, item in npm['packages'].items():
        if '/node_modules/' not in '/' + path or not item.get('version') or item.get('link'):
            continue
        name = item.get('name') or path.rsplit('node_modules/', 1)[1]
        queries[('npm', name, item['version'])] = None
    return [{'package': {'ecosystem': e, 'name': n}, 'version': v} for e, n, v in sorted(queries)]


def query(queries):
    request = urllib.request.Request(
        'https://api.osv.dev/v1/querybatch',
        data=json.dumps({'queries': queries}).encode(),
        headers={'Content-Type': 'application/json', 'User-Agent': 'WebTS-dependency-check'},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        raw = response.read(4 * 1024 * 1024 + 1)
    if len(raw) > 4 * 1024 * 1024:
        raise ValueError('OSV response exceeds limit')
    results = json.loads(raw)['results']
    if len(results) != len(queries) or any(r.get('next_page_token') for r in results):
        raise ValueError('OSV response incomplete; do not treat as a clean audit')
    return results


def patched_root_reexport(root, item, advisory):
    # 0.26.11 contains no certificate data: its only export forwards parent v1.
    # Pin this reviewed case; other advisories, shims or bundles still fail closed.
    if item != {'package': {'ecosystem': 'crates.io', 'name': 'webpki-roots'}, 'version': '0.26.11'} or advisory != 'RUSTSEC-2026-0381':
        return False
    packages = tomllib.loads((root / 'Cargo.lock').read_text(encoding='utf-8'))['package']
    registry = 'registry+https://github.com/rust-lang/crates.io-index'
    shim = any(p['name'] == 'webpki-roots' and p['version'] == '0.26.11'
               and p.get('source') == registry
               and p.get('checksum') == '521bc38abb08001b01866da9f51eb7c5d647a19260e00054a8c7fd5f9e57f7a9'
               and p.get('dependencies') == ['webpki-roots 1.0.10'] for p in packages)
    patched = any(p['name'] == 'webpki-roots' and p['version'] == '1.0.10'
                  and p.get('source') == registry
                  and p.get('checksum') == '1c1c8e20c8a38fc7d65abfe74eb7280e4aa667e8eec8bc84996d2874810748d9' for p in packages)
    return shim and patched


def audit(root):
    # Ensure the service really reports a known affected version.
    control = query([{'package': {'ecosystem': 'crates.io', 'name': 'time'}, 'version': '0.2.22'}])
    if not control[0].get('vulns'):
        raise ValueError('OSV positive control failed')
    queries = coordinates(root)
    found = False
    for start in range(0, len(queries), 100):
        group = queries[start:start + 100]
        for item, result in zip(group, query(group), strict=True):
            for vulnerability in result.get('vulns', []):
                if patched_root_reexport(root, item, vulnerability['id']):
                    print('Reviewed RUSTSEC-2026-0381: webpki-roots 0.26.11 re-exports patched 1.0.10; no bundled roots in shim.')
                    continue
                found = True
                print(f"{item['package']['ecosystem']} {item['package']['name']}@{item['version']}: {vulnerability['id']}")
    print(f'Checked {len(queries)} public registry package versions; local/vendor code and container images require separate review.')
    return 1 if found else 0


if __name__ == '__main__':
    try:
        sys.exit(audit(Path(__file__).resolve().parent.parent))
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f'Dependency advisory check failed: {error}', file=sys.stderr)
        sys.exit(2)
