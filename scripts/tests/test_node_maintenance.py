import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('node_linux',ROOT/'scripts/node-linux.py')
node=importlib.util.module_from_spec(spec);spec.loader.exec_module(node)


def archive(path,items):
    with tarfile.open(path,'w:gz') as out:
        for name,data,kind in items:
            member=tarfile.TarInfo(name);member.type=kind;member.size=len(data)
            out.addfile(member,io.BytesIO(data))


class NodeMaintenanceTests(unittest.TestCase):
    def setUp(self):
        (ROOT/'.cache').mkdir(exist_ok=True)

    def test_official_downloads_reject_plaintext_credentials_and_external_redirects(self):
        for url in ['http://github.com/x','https://github.com.evil.invalid/x','https://example.invalid/x','https://user:secret@github.com/x','https://github.com:444/x']:
            with self.assertRaises(ValueError):node.check_url(url)
        node.check_url('https://release-assets.githubusercontent.com/example?synthetic=1')

    def test_linux_extract_refuses_links_duplicate_paths_and_oversized_files(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'.cache') as folder:
            root=Path(folder);out=root/'output';out.mkdir();file=root/'test.tar.gz'
            for items in [[('../escape',b'x',tarfile.REGTYPE)], [('node/../escape',b'x',tarfile.REGTYPE)], [('node/link',b'',tarfile.SYMTYPE)], [('node/a',b'a',tarfile.REGTYPE),('node/a',b'a',tarfile.REGTYPE)], [('other/a',b'x',tarfile.REGTYPE)]]:
                archive(file,items)
                with self.assertRaises(ValueError):node.extract(file,out,'node')
                for p in out.rglob('*'):
                    if p.is_file():p.unlink()
            self.assertFalse((root/'escape').exists())

    def test_linux_update_checks_archive_manifest_and_source_before_installing(self):
        version='1.2.3';name=f'webts-node-{version}-preview.5-linux-x64.tar.gz';stem=name.removesuffix('.tar.gz')
        with tempfile.TemporaryDirectory(dir=ROOT/'.cache') as folder:
            root=Path(folder);package=root/name
            files={n:b'public fixture' for n in ['web-ts','scripts/node-linux.py','webts-node','node.example.toml']}
            files['BUILD-INFO.json']=json.dumps(dict(platform='linux-x64',version=version+'-node-preview.5',base_commit='1'*40,working_tree='committed source; protocol patch is included in repository')).encode()
            files['SHA256SUMS']=''.join(f'{hashlib.sha256(v).hexdigest()}  {k}\n' for k,v in files.items()).encode()
            archive(package,[(stem+'/'+k,v,tarfile.REGTYPE) for k,v in files.items()]);data=package.read_bytes()
            release=json.dumps(dict(tag_name='v'+version,draft=False,prerelease=False,assets=[dict(name=name)])).encode()
            digest=f'{hashlib.sha256(data).hexdigest()}  {name}\n'.encode()
            def download(url,limit):return release if url.endswith('/latest') else digest if url.endswith('.sha256') else data
            with patch.object(node,'download',side_effect=download),patch.object(node,'install') as install:
                node.update();self.assertEqual(install.call_count,1)
            with patch.object(node,'download',side_effect=lambda u,n:release if u.endswith('/latest') else b'0'*64+b'  '+name.encode()+b'\n' if u.endswith('.sha256') else data),patch.object(node,'install') as install:
                with self.assertRaises(ValueError):node.update()
                install.assert_not_called()


if __name__=='__main__':unittest.main()
