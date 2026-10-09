#!/usr/bin/env python3
"""Chinese Linux operator menu; secrets never enter argv or public source."""
import getpass
from contextlib import closing
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time

ROOT = Path('/opt/webts')


def run(args, *, text=None, capture=False):
    return subprocess.run(args, input=text, text=True, capture_output=capture, check=True)


def compose(*args, text=None, capture=False):
    command = ['docker', 'compose'] if subprocess.run(['docker', 'compose', 'version'], capture_output=True).returncode == 0 else ['docker-compose']
    return run(command + ['-p', 'webts-managed', '-f', str(ROOT/'compose.json'), *args], text=text, capture=capture)


def private_write(path, text, owner=False):
    if path.is_symlink():
        raise ValueError('私有文件不能是符号链接')
    with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=path.parent, delete=False) as file:
        file.write(text)
        temp = Path(file.name)
    try:
        os.chmod(temp, 0o600)
        if owner:
            os.chown(temp, 10001, 10001)
        os.replace(temp, path)
    finally:
        temp.unlink(missing_ok=True)


def ask(label, default='', validate=lambda value: bool(value)):
    while True:
        value = input(f'{label}' + (f' [{default}]' if default else '') + '：').strip() or str(default)
        try:
            if validate(value):
                return value
        except (ValueError, TypeError):
            pass
        print('输入无效，请重新填写。')


def host(value):
    return bool(re.fullmatch(r'(?=.{1,253}$)[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?', value)) and '..' not in value


def domain(value):
    return host(value) and '.' in value and not re.fullmatch(r'[0-9.]+', value)


def email(value):
    return len(value) <= 254 and bool(re.fullmatch(r'[^\s@]+@[^\s@]+\.[^\s@]+', value))


def public_ip(value):
    ip = ipaddress.ip_address(value)
    return ip.version == 4 and ip.is_global


def target(value):
    from urllib.parse import urlsplit
    if any(c in value for c in '/@?#\\') or any(c.isspace() for c in value):
        return False
    parsed = urlsplit('ts3://' + value)
    if not parsed.hostname or not 0 < (parsed.port or 9987) <= 65535:
        return False
    try:
        return ipaddress.ip_address(parsed.hostname).is_global
    except ValueError:
        return host(parsed.hostname) and '.' in parsed.hostname


def port(value):
    return value.isdecimal() and 1024 <= int(value) <= 65535


def state():
    return json.loads((ROOT/'setup.json').read_text(encoding='utf-8'))


def save(config):
    # JSON-quoted strings are valid TOML basic strings; no shell interpolation.
    quote = lambda value: json.dumps(value, ensure_ascii=False)
    lines = [f'bind = {quote("127.0.0.1:"+str(config["port"]))}',
             f'public_url = {quote("https://"+config["domain"])}',
             'database = "data/web-ts.db"', 'master_key_file = "secrets/master.key"',
             'web_dir = "web/dist"', 'max_connections = 50',
             'trusted_proxy = ["127.0.0.1", "::1"]', 'allow_insecure_localhost = false',
             '[[servers]]', 'id = "community"', 'name = "默认 TeamSpeak"',
             f'address = {quote(config["server"])}', '[smtp]',
             f'host = {quote(config["smtp_host"])}', f'port = {config["smtp_port"]}',
             f'username = {quote(config["smtp_user"])}',
             'password_file = "secrets/smtp-password"', f'from = {quote(config["smtp_from"])}',
             '[rtc]', f'public_ip = {quote(config["ip"])}', 'udp_min = 40000', 'udp_max = 40100']
    private_write(ROOT/'config.local.toml', '\n'.join(lines)+'\n', True)
    service = {'image':'webts:managed', 'restart':'unless-stopped', 'network_mode':'host',
               'volumes':[f'{ROOT}/config.local.toml:/app/config.local.toml:ro', f'{ROOT}/data:/app/data', f'{ROOT}/secrets:/app/secrets:ro'],
               'read_only':True, 'tmpfs':['/tmp:size=32m,mode=1777'], 'security_opt':['no-new-privileges:true'], 'cap_drop':['ALL'], 'mem_limit':'1g'}
    services = {'webts': service}
    if config['proxy'] == 'auto':
        private_write(ROOT/'Caddyfile', f'{config["domain"]} {{\n encode zstd gzip\n header Strict-Transport-Security "max-age=31536000"\n reverse_proxy 127.0.0.1:{config["port"]} {{\n header_up X-Real-IP {{remote_host}}\n }}\n}}\n')
        services['caddy'] = {'image':'caddy:2.10.2-alpine', 'restart':'unless-stopped', 'network_mode':'host',
                             'volumes':[f'{ROOT}/Caddyfile:/etc/caddy/Caddyfile:ro', 'caddy_data:/data', 'caddy_config:/config']}
    private_write(ROOT/'compose.json', json.dumps({'version':'3.8','services':services,'volumes':{'caddy_data':{},'caddy_config':{}}}, ensure_ascii=False, indent=2)+'\n')
    private_write(ROOT/'setup.json', json.dumps(config, ensure_ascii=False, indent=2)+'\n')


def smtp_values(previous=None):
    previous = previous or {}
    from email.utils import parseaddr
    values = {'host':ask('SMTP 主机', previous.get('host','smtp.qq.com'), host),
              'port':int(ask('SMTP TLS 端口', str(previous.get('port',465)), lambda v: v.isdecimal() and 1<=int(v)<=65535)),
              'username':ask('SMTP 登录邮箱', previous.get('username',''), email),
              'from':ask('发件邮箱', parseaddr(previous.get('from',''))[1], email)}
    while True:
        password = getpass.getpass('SMTP 授权码/密码（隐藏输入，留空保持原值）：')
        if not password and previous.get('password') and values['host']==previous.get('host') and values['username']==previous.get('username'):
            password = previous['password']
        if password and len(password) <= 1024 and '\n' not in password and '\r' not in password:
            values['password'] = password
            return values
        print('首次配置必须填写授权码，不能包含换行。')


def tool(command, *, text=None, capture=False):
    return compose('run', '--rm', '-T', '--no-deps', 'webts', command, '/app/config.local.toml', text=text, capture=capture)


def settings():
    # get-settings includes credentials: captured in memory, never shown or logged.
    return json.loads(tool('get-settings', capture=True).stdout)


def health():
    import urllib.request
    config = state()
    for _ in range(30):
        try:
            with urllib.request.urlopen(f'http://127.0.0.1:{config["port"]}/api/health', timeout=2) as response:
                result=json.load(response)
                if result.get('ok') and result.get('smtp_ready'):
                    print('网关已就绪；SMTP 已配置（实际送达请用注册邮件验证）。')
                    return
        except (OSError, ValueError):
            pass
        time.sleep(1)
    raise ValueError('网关未就绪，请用 webts logs 检查；不要跳过 HTTPS 证书验证。')


def backup():
    parent=ROOT/'backups';parent.mkdir(mode=0o700,exist_ok=True)
    folder=Path(tempfile.mkdtemp(prefix=time.strftime('%Y%m%d-%H%M%S')+'-',dir=parent))
    if (ROOT/'data/web-ts.db').exists():
        with closing(sqlite3.connect(ROOT/'data/web-ts.db')) as source, closing(sqlite3.connect(folder/'web-ts.db')) as dest:
            source.backup(dest)
    for name in ['config.local.toml','setup.json','compose.json','Caddyfile']:
        if (ROOT/name).exists():
            shutil.copy2(ROOT/name, folder/name)
    shutil.copytree(ROOT/'secrets', folder/'secrets')
    print('备份已保存：'+str(folder)+'；请将密钥和数据库另行分开加密备份。')
    return folder


def setup():
    if (ROOT/'setup.json').exists():
        print('已有配置，保留它们并继续构建启动。')
        run(['docker','build','-t','webts:managed',str(ROOT/'current')])
        compose('up','-d','--no-build');health()
        return
    config = {'domain':ask('网站域名（提前解析到本机公网 IP）', validate=domain),
              'ip':ask('本机公网 IPv4', validate=public_ip),
              'port':int(ask('网关本机端口', '8080', port)),
              'server':ask('默认 TS 公网地址:UDP端口', validate=target),
              'admin_email':ask('站长邮箱（仍需网页注册并验证）', validate=email)}
    with socket.socket() as check:
        try:check.bind(('127.0.0.1',config['port']))
        except OSError:raise ValueError('网关端口已占用，请重新运行安装并选择其他本机端口。') from None
    occupied = []
    for number in [80,443]:
        with socket.socket() as check:
            if check.connect_ex(('127.0.0.1',number)) == 0:
                occupied.append(number)
    if occupied:
        print('80/443 已有服务，保留它们；使用现有反向代理接入此域名。')
        config['proxy']='external'
    else:
        config['proxy']=ask('HTTPS：1 自动证书 / 2 使用现有代理', '1', lambda v:v in {'1','2'})
        config['proxy']='auto' if config['proxy']=='1' else 'external'
    smtp = smtp_values()
    config.update(smtp_host=smtp['host'],smtp_port=smtp['port'],smtp_user=smtp['username'],smtp_from=smtp['from'])
    for name in ['data','secrets']:
        path=ROOT/name;path.mkdir(mode=0o700,exist_ok=True);os.chown(path,10001,10001)
    if (ROOT/'data/web-ts.db').exists() and not (ROOT/'secrets/master.key').exists():
        raise ValueError('已有数据库但缺少原密钥，拒绝创建替代密钥。')
    if not (ROOT/'secrets/master.key').exists():
        private_write(ROOT/'secrets/master.key', os.urandom(32).hex(), True)
    private_write(ROOT/'secrets/smtp-password',smtp['password'],True)
    save(config)
    print('开始构建固定依赖源码，首次可能需要数分钟。')
    run(['docker','build','-t','webts:managed',str(ROOT/'current')])
    compose('up','-d','--no-build');health()
    print('网站：https://'+config['domain'])
    print('先在网页用站长邮箱注册并验证，再运行 webts admin 授予网站管理权限。')
    print('需要放行 TCP 80/443 和 UDP 40000–40100；TS 要开启全局语音加密与 Opus。')
    if config['proxy']=='external':
        print(f'请在现有 HTTPS 代理将该域名转发到 127.0.0.1:{config["port"]}，保留 Origin 并覆盖 X-Real-IP。')


def configure(kind):
    config=state()
    original=settings() if kind!='config' else None
    if kind=='config':
        config['domain']=ask('网站域名',config['domain'],domain)
        config['ip']=ask('公网 IPv4',config['ip'],public_ip)
        number=int(ask('网关本机端口',str(config['port']),port))
        if number!=config['port']:
            with socket.socket() as check:
                try:check.bind(('127.0.0.1',number))
                except OSError:raise ValueError('新端口已占用，配置未修改。') from None
        config['port']=number
    else:
        updated=json.loads(json.dumps(original))
        if kind=='smtp':
            updated['smtp']=smtp_values(updated['smtp'])
            smtp=updated['smtp'];config.update(smtp_host=smtp['host'],smtp_port=smtp['port'],smtp_user=smtp['username'],smtp_from=smtp['from'])
        else:
            address=ask('默认 TS 公网地址:UDP端口',config['server'],target)
            servers=updated['servers'];entry=next((s for s in servers if s['id']=='community'),None)
            if entry is None:
                entry={'id':'community','name':'默认 TeamSpeak','address':address};servers.append(entry)
            entry['address']=address;updated['default_server']='community'
            updated['allow_custom']=ask('允许已登录用户连接自定义公网地址？1 是 / 2 否','1' if updated['allow_custom'] else '2',lambda v:v in {'1','2'})=='1'
            config['server']=address
    folder=backup();compose('stop','webts')
    try:
        if kind!='config':tool('set-settings',text=json.dumps(updated),capture=True)
        if kind=='smtp':private_write(ROOT/'secrets/smtp-password',updated['smtp']['password'],True)
        save(config)
        compose('up','-d','--no-build','--force-recreate');health()
    except Exception:
        for name in ['config.local.toml','setup.json','compose.json','Caddyfile']:
            if (folder/name).exists():shutil.copy2(folder/name,ROOT/name)
        if (folder/'secrets/smtp-password').exists():shutil.copy2(folder/'secrets/smtp-password',ROOT/'secrets/smtp-password')
        if original is not None:tool('set-settings',text=json.dumps(original),capture=True)
        compose('up','-d','--no-build','--force-recreate')
        raise ValueError('配置未能启动，已恢复原配置；账号/身份/密钥未替换。') from None
    print('配置已生效；终端配置会重启本实例，网页“站点管理”仍可热加载邮箱/服务器。')


def admin():
    config=state();address=ask('授予站长权限的已验证邮箱',config['admin_email'],email)
    compose('run','--rm','-T','--no-deps','webts','grant-admin','/app/config.local.toml',address)
    print('重新登录后点击顶部“站点管理”；网站管理权限不增加 TS 身份权限。')


def update():
    candidate=Path(tempfile.mkdtemp(prefix='source-',dir=ROOT/'releases'))
    run(['git','-c','http.sslVerify=true','clone','--depth','1','--recurse-submodules','--shallow-submodules','https://github.com/PTPHAP/WebTS.git',str(candidate)])
    run(['docker','build','-t','webts:managed-candidate',str(candidate)])
    folder=backup();old=(ROOT/'current').resolve()
    rollback='webts:rollback-'+folder.name
    run(['docker','tag','webts:managed',rollback])
    compose('stop','webts')
    pointer=ROOT/'next';pointer.unlink(missing_ok=True);pointer.symlink_to(candidate);os.replace(pointer,ROOT/'current')
    run(['docker','tag','webts:managed-candidate','webts:managed'])
    try:
        compose('up','-d','--no-build','--force-recreate');health()
    except Exception:
        pointer.symlink_to(old);os.replace(pointer,ROOT/'current')
        run(['docker','tag',rollback,'webts:managed']);compose('up','-d','--no-build','--force-recreate')
        raise ValueError('更新启动失败，已回退程序；配置、身份和密钥未被替换。') from None
    print('更新完成，原账号、身份、配置和密钥已保留。')


COMMANDS=['setup','config','smtp','server','admin','start','stop','restart','status','logs','backup','update']


def execute(command):
    if command=='setup':setup()
    elif command in {'config','smtp','server'}:configure(command)
    elif command=='admin':admin()
    elif command=='backup':backup()
    elif command=='update':update()
    elif command=='start':compose('up','-d','--no-build');health()
    elif command=='stop':compose('stop')
    elif command=='restart':compose('restart');health()
    elif command=='status':compose('ps')
    elif command=='logs':compose('logs','--tail','80','webts')
    else:raise ValueError('未知命令；输入 webts 打开中文菜单。')


def main():
    if len(sys.argv)>1 and sys.argv[1] in {'help','--help','-h'}:
        print('WebTS 中文管理：webts ['+' | '.join(COMMANDS)+']；不带参数打开菜单。')
        return
    if os.geteuid()!=0 or not (ROOT/'.webts-managed').is_file():
        raise ValueError('请用 root/sudo 操作已由安装器管理的 /opt/webts。')
    import fcntl
    with (ROOT/'.manage.lock').open('w') as lock:
        try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        except BlockingIOError:raise ValueError('另一个 WebTS 配置/更新正在运行。') from None
        if len(sys.argv)>1:
            execute(sys.argv[1]);return
        while True:
            print('\nWebTS 中文管理\n1 基础配置  2 邮箱配置  3 默认TS/自定义连接\n4 站长权限  5 启动  6 停止  7 重启\n8 状态  9 日志  10 备份  11 更新  0 退出')
            selected=ask('选择','0',lambda v:v.isdecimal() and 0<=int(v)<=11)
            if selected=='0':return
            command=['config','smtp','server','admin','start','stop','restart','status','logs','backup','update'][int(selected)-1]
            try:execute(command)
            except (ValueError,subprocess.CalledProcessError):print('操作失败，原数据保留；请检查输入或用 webts logs 查看服务状态。')


if __name__=='__main__':
    os.umask(0o077)
    try:main()
    except (KeyboardInterrupt,EOFError):print('\n已取消。')
    except (ValueError,OSError,subprocess.CalledProcessError) as error:
        # Captured stdout may include SMTP credentials; never print exception payloads.
        print(str(error) if isinstance(error,ValueError) else '操作失败，请检查 Docker、网络和文件权限；凭据不会回显。',file=sys.stderr)
        sys.exit(1)
