"""Real systemd acceptance, restricted to a disposable GitHub Ubuntu runner."""
import argparse
import getpass
import importlib.util
import os
from pathlib import Path
import pwd
import shutil
import socket
import subprocess
import tempfile
from unittest.mock import patch

REPO=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('node_linux',REPO/'scripts/node-linux.py')
node=importlib.util.module_from_spec(spec);spec.loader.exec_module(node)


def smoke(package):
    if os.environ.get('GITHUB_ACTIONS')!='true' or os.geteuid()!=0 or not Path('/run/systemd/system').is_dir():raise RuntimeError('Only an isolated root GitHub systemd runner may perform OS mutations')
    if any(p.exists() for p in (node.ROOT,node.UNIT,node.COMMAND)):raise RuntimeError('Preserve existing node installation; runner is not isolated')
    try:pwd.getpwnam('webts-node')
    except KeyError:pass
    else:raise RuntimeError('Preserve existing service account')
    account=None
    with tempfile.TemporaryDirectory(prefix='node-linux-os-',dir=REPO/'.cache') as folder:
        stage=Path(folder);stem=package.name.removesuffix('.tar.gz');node.extract(package,stage,stem);source=stage/stem;node.manifest(source)
        try:
            node.ROOT.mkdir(mode=0o750);(node.ROOT/'secrets').mkdir(mode=0o700)
            config=(source/'node.example.toml').read_text()
            with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
            config=config.replace('127.0.0.1:8095',f'127.0.0.1:{port}').replace('https://web.example.com','https://127.0.0.1:1').replace('203.0.113.10','1.1.1.1')
            (node.ROOT/'node.local.toml').write_text(config);key=node.ROOT/'secrets/node-pair.key';key.write_text('12'*32);key.chmod(0o600)
            for _ in range(2):
                # Real configuration, backup, systemd user, process and HTTP. Only human answers are scripted.
                with patch.object(node,'ask',side_effect=lambda label,default='':'Y' if label.startswith('继续') else str(default)),patch.object(getpass,'getpass',return_value=''):
                    node.install(source)
                account=pwd.getpwnam('webts-node');node.preflight_service()
                assert key.read_text()=='12'*32
                assert key.stat().st_uid==0 and key.stat().st_gid==account.pw_gid and key.stat().st_mode&0o777==0o640
                assert (node.ROOT/'backups').stat().st_mode&0o777==0o700
                pid=int(node.run('systemctl','show','webts-node.service','--property=MainPID','--value',cwd='/'))
                assert (Path('/proc')/str(pid)/'status').read_text().split('Uid:')[1].split()[0]==str(account.pw_uid)
                assert not list(node.ROOT.glob('*.db*'))
            assert len(list((node.ROOT/'backups').iterdir()))==2
            def invalid_ports(label,default=''):
                if label.startswith('自定义'):return 'Y'
                if label.startswith('网页语音UDP起始'):return '1000'
                if label.startswith('网页语音UDP结束'):return '1099'
                return str(default)
            with patch.object(node,'ask',side_effect=invalid_ports),patch.object(getpass,'getpass',return_value=''):
                try:node.install(source)
                except ValueError as error:assert '1024' in str(error)
                else:raise AssertionError('Privileged UDP must be rejected before stopping the running service')
            assert int(node.run('systemctl','show','webts-node.service','--property=MainPID','--value',cwd='/'))==pid
            assert len(list((node.ROOT/'backups').iterdir()))==2
            subprocess.run(['webts-node','check'],check=True)
            print('NATIVE LINUX PASS: systemd, dedicated user, read-only keys, real process/HTTP, repeat upgrade and preserved key. No real TS voice or cloud firewall acceptance.')
        finally:
            if node.UNIT.exists():
                assert node.UNIT.read_text().startswith(node.MARKER)
                subprocess.run(['systemctl','disable','--now','webts-node.service'],check=True)
                node.UNIT.unlink();subprocess.run(['systemctl','daemon-reload'],check=True)
            if node.COMMAND.exists():
                assert node.COMMAND.read_text().startswith('#!/bin/sh\n'+node.MARKER);node.COMMAND.unlink()
            if node.ROOT.exists():
                assert node.ROOT==Path('/opt/webts-node') and not node.ROOT.is_symlink();shutil.rmtree(node.ROOT)
            if account is not None:
                assert pwd.getpwnam('webts-node').pw_uid==account.pw_uid;subprocess.run(['userdel','webts-node'],check=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--package',type=Path,required=True)
    smoke(parser.parse_args().package.resolve())
