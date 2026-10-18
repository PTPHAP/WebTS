#!/usr/bin/env python3
"""Chinese node installer/updater. Python 3.11+, systemd; never operates TeamSpeak."""
import getpass
import hashlib
import ipaddress
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request

ROOT = Path('/opt/webts-node')
UNIT = Path('/etc/systemd/system/webts-node.service')
COMMAND = Path('/usr/local/bin/webts-node')
MARKER = '# Managed by WebTS node installer v1\n'
SOURCE = Path(__file__).resolve().parents[1]
PUBLIC = {'web-ts', 'webts-node', 'scripts', 'docs', 'deploy', 'licenses', 'LICENSE',
          'README.md', 'THIRD_PARTY_NOTICES.md', 'node.example.toml', 'SHA256SUMS', 'BUILD-INFO.json',
          'node.local.toml', 'secrets', 'backups'}


def regular(path):
    for item in (path, *path.parents):
        if item.is_symlink():
            raise ValueError('拒绝链接路径：' + str(item))
    info = path.stat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise ValueError('需要独立普通文件：' + str(path))
    return path


def directory(root):
    for item in (root, *root.parents):
        if item.is_symlink():
            raise ValueError('安装目录不能含链接')
    if root.exists():
        for child in root.iterdir():
            if child.name not in PUBLIC:
                raise ValueError('节点目录混有其他文件，未操作：' + child.name)
        for child in root.rglob('*'):
            if child.is_symlink() or (child.is_file() and child.stat().st_nlink != 1):
                raise ValueError('节点目录含链接或硬链接')


def manifest(root):
    directory(root)
    entries = {}
    for line in regular(root / 'SHA256SUMS').read_text().splitlines():
        match = re.fullmatch(r'([a-f0-9]{64})  ([^\r\n]+)', line)
        if not match:
            raise ValueError('包内校验清单格式错误')
        digest, name = match.groups()
        path = PurePosixPath(name)
        if path.is_absolute() or path.as_posix()!=name or any(p in {'', '.', '..', 'secrets', 'backups', 'node.local.toml'} for p in path.parts) or '\\' in name or ':' in name or name in entries:
            raise ValueError('校验清单包含私有、重复或越界路径')
        file = regular(root / name)
        if file.stat().st_size > 268435456 or hashlib.sha256(file.read_bytes()).hexdigest() != digest:
            raise ValueError('包内文件校验失败：' + name)
        entries[name] = digest
    if not {'web-ts', 'scripts/node-linux.py', 'webts-node', 'node.example.toml', 'BUILD-INFO.json'} <= entries.keys():
        raise ValueError('节点包缺少必需文件')
    return entries


def run(*args, cwd=ROOT, data=None):
    result = subprocess.run(args, cwd=cwd, input=data, capture_output=True, timeout=45)
    if result.returncode:
        # Do not echo stdin, which may contain a one-time pairing code.
        raise RuntimeError(args[0] + ' 执行失败：' + result.stderr.decode(errors='replace')[:1500])
    return result.stdout


def ask(label, default=''):
    answer = input(label + (f' [{default}]' if default else '') + '：').strip()
    return answer or str(default)


def preflight_service():
    for path in (UNIT, COMMAND):
        prefix=('#!/bin/sh\n' if path==COMMAND else '')+MARKER
        if path.exists() and (regular(path).stat().st_uid != 0 or not path.read_text().startswith(prefix)):
            raise ValueError('已有同名服务或命令不属于此安装器，未覆盖：' + str(path))
    if UNIT.exists() and ('\nExecStart='+str(ROOT/'web-ts')+' serve-node '+str(ROOT/'node.local.toml')+'\n') not in UNIT.read_text():raise ValueError('节点服务启动目标已被修改，未接管')
    loaded=subprocess.run(['systemctl','show','webts-node.service','--property=FragmentPath','--value'],capture_output=True,timeout=10).stdout.decode().strip()
    if loaded and loaded!=str(UNIT):raise ValueError('已有其他位置的同名节点服务，未覆盖：'+loaded)
    if UNIT.exists():
        drops = run('systemctl', 'show', 'webts-node.service', '--property=DropInPaths', '--value', cwd='/').strip()
        if drops:
            raise ValueError('节点服务有额外覆盖配置，请先人工核对，未停止服务')
        pid = int(run('systemctl', 'show', 'webts-node.service', '--property=MainPID', '--value', cwd='/').strip() or b'0')
        if pid:
            proc = Path('/proc') / str(pid)
            if Path(os.readlink(proc / 'exe')) != ROOT / 'web-ts' or (proc / 'cmdline').read_bytes().split(b'\0')[:3] != [os.fsencode(ROOT / 'web-ts'), b'serve-node', os.fsencode(ROOT / 'node.local.toml')]:
                raise ValueError('同名服务进程归属不符，未停止它')


def install(source):
    import pwd
    entries = manifest(source)
    directory(ROOT)
    preflight_service()
    if ROOT.exists() and any(p.stat().st_uid != 0 for p in (ROOT, *ROOT.rglob('*'))):
        raise ValueError('安装目录并非完全由root维护，未接管已有文件')
    # Verify runtime compatibility BEFORE stopping the existing process.
    binary = regular(source / 'web-ts')
    binary.chmod(0o755)
    run(str(binary),'inspect-node',str(source/'node.example.toml'),cwd=source)
    old = {}
    if (ROOT / 'node.local.toml').exists():
        old = json.loads(run(str(binary), 'inspect-node', str(ROOT / 'node.local.toml')))
        if (ROOT/old['key_file']).absolute()!=ROOT/'secrets/node-pair.key':raise ValueError('自动安装只支持原目录secrets/node-pair.key，未改动其他密钥')
        if int(old['bind'].rsplit(':',1)[1])<1024:raise ValueError('低权限节点控制监听须使用1024以上端口；不会为此授予额外系统权限')
    try:
        existing=pwd.getpwnam('webts-node')
        if existing.pw_uid==0 or existing.pw_uid>=1000 or existing.pw_dir!=str(ROOT) or existing.pw_shell not in {'/usr/sbin/nologin','/sbin/nologin','/bin/false'}:raise ValueError('已有webts-node账号不是专用系统账号，未停止节点')
    except KeyError:pass
    main = ask('① WebTS主站HTTPS根网址', old.get('main_url') or '')
    url = urllib.parse.urlsplit(main)
    if url.scheme != 'https' or not url.hostname or url.username or url.password or url.query or url.fragment or url.path not in {'', '/'}:
        raise ValueError('主站需要有效HTTPS根网址，不能含凭据、路径或参数')
    public = ask('② 当前TS机器的公网IPv4，不能填127.0.0.1', old.get('public_ip', ''))
    if not ipaddress.IPv4Address(public).is_global:
        raise ValueError('请填当前TS机器的真实公网IPv4')
    node_id = ask('③ 主站后台对应服务器的节点标识', old.get('id', 'community-node'))
    if not re.fullmatch(r'[A-Za-z0-9_-]{1,64}', node_id):
        raise ValueError('节点标识格式无效')
    def port(label, default):
        n = int(ask(label, default))
        if not 1 <= n <= 65535:
            raise ValueError('端口范围为1—65535')
        return n
    ts = int(old.get('target', '127.0.0.1:9987').rsplit(':', 1)[1])
    file_port = old.get('file_transfer_port', 30033)
    low, high = old.get('udp_min', 50000), old.get('udp_max', 50099)
    if ask('自定义本机TS/文件/网页语音端口？Y/N', 'N').lower() == 'y':
        ts, file_port = port('TS语音UDP端口', ts), port('TS文件TCP端口', file_port)
        low, high = port('网页语音UDP起始端口', low), port('网页语音UDP结束端口', high)
    if high < low or high - low + 1 < old.get('max_connections', 50):
        raise ValueError('UDP范围不足以容纳连接上限')
    if low<1024:raise ValueError('低权限节点的网页语音UDP须从1024以上开始；TS目标端口不受此限制')
    code = getpass.getpass('④ 主站生成的一次性配对码（已有配对且未换主站/标识可留空）：')
    if not code and (not old or not (ROOT/'secrets/node-pair.key').is_file() or old.get('id') != node_id or (old.get('main_url') or '').rstrip('/') != main.rstrip('/')):
        raise ValueError('首次安装或更换主站/节点标识必须配对')
    if code and not re.fullmatch('[a-fA-F0-9]{64}', code):
        raise ValueError('配对码格式错误')
    print(f'仅管理 {ROOT}、webts-node.service 和维护命令；不会操作TeamSpeak。\n'
          f'[当前TS机器] 云安全组及主机防火墙需要入站 UDP {low}—{high}；出站主站 TCP {url.port or 443}。\n'
          '[WebTS主站] 保留现有HTTPS，无须新增这段UDP。[用户设备] 无须手动开端口。\n'
          'Linux防火墙可能由ufw/firewalld/nftables或云平台接管，本安装器不改未知规则；控制HTTP保持本机回环。\n'
          '升级会中断本节点网页通话，失败保留备份并停止节点，不自动放宽权限或启动未知旧程序。')
    if ask('继续安装/升级？Y/N', 'N').lower() != 'y':
        print('已取消，未停止节点。')
        return
    ROOT.mkdir(mode=0o750, exist_ok=True)
    backups = ROOT / 'backups';backups.mkdir(mode=0o700, exist_ok=True);backups.chmod(0o700)
    backup = Path(tempfile.mkdtemp(prefix=time.strftime('%Y%m%d-%H%M%S-'), dir=backups))
    for child in ROOT.iterdir():
        if child.name != 'backups':
            if child.is_dir(): shutil.copytree(child, backup / child.name)
            else: shutil.copy2(child, backup / child.name)
    if UNIT.exists():
        shutil.copy2(UNIT, backup / 'service.backup')
        run('systemctl', 'stop', 'webts-node.service', cwd='/')
    for name in [*entries, 'SHA256SUMS']:
        destination = ROOT / name;destination.parent.mkdir(parents=True, exist_ok=True)
        if source / name != destination:
            shutil.copyfile(source / name, destination)
        destination.chmod(0o755 if name in {'web-ts', 'webts-node'} else 0o644)
    config = ROOT / 'node.local.toml';secrets = ROOT / 'secrets';secrets.mkdir(mode=0o750, exist_ok=True)
    if not config.exists(): shutil.copyfile(ROOT / 'node.example.toml', config)
    key = secrets / 'node-pair.key'
    if not key.exists(): run(str(ROOT / 'web-ts'), 'init-key', str(key))
    config.chmod(0o600);key.chmod(0o600)
    payload = json.dumps(dict(id=node_id, main_url=main, public_ip=public, code=code, ts_port=ts,
                              file_transfer_port=file_port, udp_min=low, udp_max=high)).encode()
    run(str(ROOT / 'web-ts'), 'configure-node', str(config), data=payload)
    del code, payload
    run(str(ROOT / 'web-ts'), 'check-node', str(config))
    try: account = pwd.getpwnam('webts-node')
    except KeyError:
        run('useradd', '--system', '--home-dir', str(ROOT), '--shell', '/usr/sbin/nologin', 'webts-node', cwd='/')
        account = pwd.getpwnam('webts-node')
    if account.pw_uid == 0 or account.pw_uid >= 1000 or account.pw_dir != str(ROOT) or account.pw_shell not in {'/usr/sbin/nologin', '/sbin/nologin', '/bin/false'}:
        raise ValueError('已有webts-node账号并非专用系统账号，请人工核对')
    for path in (ROOT, secrets): os.chown(path, 0, account.pw_gid);path.chmod(0o750)
    for path in (config, key): os.chown(path, 0, account.pw_gid);path.chmod(0o640)
    # Native configure validation rejects any custom key path before serving.
    inspected = json.loads(run(str(ROOT / 'web-ts'), 'inspect-node', str(config)))
    if (ROOT / inspected['key_file']).absolute() != key:
        raise ValueError('自动安装只支持secrets/node-pair.key；未启动节点')
    unit = regular(ROOT / 'deploy/webts-node.service').read_text()
    UNIT.write_text(MARKER + unit);UNIT.chmod(0o644)
    COMMAND.write_text('#!/bin/sh\n'+MARKER+'exec /opt/webts-node/webts-node "$@"\n');COMMAND.chmod(0o755)
    run('systemctl', 'daemon-reload', cwd='/');run('systemctl', 'enable', '--now', 'webts-node.service', cwd='/')
    healthy = False
    for _ in range(30):
        try:
            preflight_service()
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            with opener.open('http://' + inspected['bind'] + '/health', timeout=1) as response:
                healthy = json.load(response).get('role') == 'voice-node'
            if healthy: break
        except (OSError, ValueError): pass
        time.sleep(.25)
    if not healthy: raise RuntimeError('节点健康检查失败，请运行 webts-node status；保留备份：' + str(backup))
    print('安装完成。保留配置/密钥，专用低权限服务已启动。备份：' + str(backup))
    print(f'请在当前TS机器放行入站UDP {low}—{high}，到主站检查节点在线，再测双向语音。本机健康不代表公网语音已通过。')


class OfficialRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        check_url(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def check_url(url):
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != 'https' or parts.port not in {None, 443} or parts.username or parts.password or parts.hostname not in {'api.github.com', 'github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com'}:
        raise ValueError('拒绝非官方HTTPS更新目标')


def download(url, limit):
    check_url(url)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), OfficialRedirect())
    with opener.open(urllib.request.Request(url, headers={'User-Agent': 'WebTS-Node-Updater/1'}), timeout=30) as response:
        data = response.read(limit + 1)
    if len(data) > limit: raise ValueError('下载文件超过大小上限')
    return data


def extract(archive, destination, stem):
    with tarfile.open(archive) as package:
        members = package.getmembers()
        if len(members) > 12000: raise ValueError('更新包文件数量超限')
        seen, total = set(), 0
        for member in members:
            path = PurePosixPath(member.name);total += member.size
            if not member.isfile() or not member.name.startswith(stem + '/') or path.is_absolute() or '..' in path.parts or '\\' in member.name or ':' in member.name or member.name in seen or total > 536870912:
                raise ValueError('更新包含链接、重复、越界路径或超大文件')
            seen.add(member.name)
            file = destination / member.name;file.parent.mkdir(parents=True, exist_ok=True)
            with package.extractfile(member) as source, file.open('xb') as output: shutil.copyfileobj(source, output)
            file.chmod(0o755 if file.name in {'web-ts', 'webts-node'} else 0o644)


def update():
    release = json.loads(download('https://api.github.com/repos/PTPHAP/WebTS/releases/latest', 2097152))
    tag = release.get('tag_name', '')
    if release.get('draft') or release.get('prerelease') or not re.fullmatch(r'v\d+\.\d+\.\d+', tag): raise ValueError('正式发布版本无效')
    assets = [a['name'] for a in release['assets'] if re.fullmatch('webts-node-' + re.escape(tag[1:]) + r'-preview\.\d+-linux-x64\.tar\.gz', a['name'])]
    if len(assets) != 1: raise ValueError('正式发布尚未提供唯一Linux节点包，保留现有节点')
    name = assets[0];base = 'https://github.com/PTPHAP/WebTS/releases/download/' + tag + '/'
    checksum = download(base + name + '.sha256', 1024).decode()
    match = re.fullmatch('([a-f0-9]{64})  ' + re.escape(name) + '\n?', checksum)
    data = download(base + name, 268435456)
    if not match or hashlib.sha256(data).hexdigest() != match[1]: raise ValueError('更新包摘要不匹配，未停止节点')
    with tempfile.TemporaryDirectory(prefix='webts-node-update-') as folder:
        stage = Path(folder);archive = stage / name;archive.write_bytes(data);del data
        stem = name.removesuffix('.tar.gz');extract(archive, stage, stem);source = stage / stem;manifest(source)
        info = json.loads((source / 'BUILD-INFO.json').read_text())
        preview = re.search(r'-preview\.(\d+)-', name)[1]
        if info.get('platform') != 'linux-x64' or info.get('version') != tag[1:] + '-node-preview.' + preview or not re.fullmatch('[a-f0-9]{40}', info.get('base_commit', '')) or info.get('working_tree') != 'committed source; protocol patch is included in repository': raise ValueError('更新包构建来源或平台不符')
        print('已校验官方包：' + name + '，源码：' + info['base_commit'])
        # Keep the maintenance lock and visible prompts in this process.
        install(source)


def main():
    import fcntl
    os.umask(0o077)
    action = sys.argv[1] if len(sys.argv) > 1 else 'help'
    if action not in {'install', 'update', 'status', 'check', 'start', 'stop', 'restart'}:
        print('WebTS节点：sudo ./webts-node install；安装后：sudo webts-node update | status | check | start | stop | restart')
        return
    if os.geteuid() != 0: raise ValueError('请使用sudo运行节点维护命令')
    if sys.platform != 'linux' or not Path('/run/systemd/system').is_dir(): raise ValueError('需要使用systemd的Linux；不会自动安装依赖或修改其他服务')
    lock_path = Path('/run/webts-node-install.lock')
    fd = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as lock:
        if os.fstat(lock.fileno()).st_uid != 0 or os.fstat(lock.fileno()).st_nlink != 1: raise ValueError('节点维护锁归属不符')
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if action == 'install': install(SOURCE)
        elif action == 'update': update()
        elif action == 'check':
            directory(ROOT);print(run(str(ROOT / 'web-ts'), 'check-node', str(ROOT / 'node.local.toml')).decode())
        else:
            preflight_service()
            subprocess.run(['systemctl', action, 'webts-node.service'], check=True)


if __name__ == '__main__':
    try: main()
    except Exception as error:
        print('节点操作未完成：' + str(error) + '\n没有操作TeamSpeak或其他服务。升级中断后请保留原目录/backups，不要清空密钥或公开控制端口。', file=sys.stderr)
        sys.exit(1)
