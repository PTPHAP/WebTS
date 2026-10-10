"""Real isolated GitHub-runner installation; never run on a personal server."""
import os
from pathlib import Path
import pty
import select
import subprocess
import time
import json
import urllib.request

if os.environ.get('GITHUB_ACTIONS')!='true' or os.geteuid()!=0:
    raise SystemExit('Only an isolated root GitHub Actions runner may run this fixture')
assert not Path('/opt/webts').exists(), 'Preserve an existing installation'
import re
assert re.fullmatch(r'[0-9a-f]{40}', os.environ.get('GITHUB_SHA',''))
os.environ['WEBTS_REF'] = os.environ['GITHUB_SHA']

def interactive(args,replies,timeout=900):
    pid,fd=pty.fork()
    if pid==0:os.execvp(args[0],args)
    transcript='';position=0;until=time.monotonic()+timeout
    try:
        while time.monotonic()<until:
            if select.select([fd],[],[],1)[0]:
                try:data=os.read(fd,65536)
                except OSError:break
                if not data:break
                transcript+=data.decode('utf-8',errors='replace')
                if position<len(replies) and replies[position][0] in transcript:
                    os.write(fd,(replies[position][1]+'\n').encode());position+=1
                    transcript=''
            waited,status=os.waitpid(pid,os.WNOHANG)
            if waited:
                assert os.waitstatus_to_exitcode(status)==0,transcript[-2000:]
                assert position==len(replies)
                return
        waited,status=os.waitpid(pid,os.WNOHANG)
        if not waited:
            os.kill(pid,15);os.waitpid(pid,0)
            raise AssertionError('Installer timeout: '+transcript[-2000:])
        assert os.waitstatus_to_exitcode(status)==0,transcript[-2000:]
        assert position==len(replies)
    finally:os.close(fd)

interactive(['bash','install.sh'],[
    ('网站域名','voice.example.com'),('本机公网 IPv4','8.8.8.8'),
    ('网关本机端口','18080'),('默认 TS 公网地址','ts.example.com:9987'),
    ('站长邮箱','owner@example.com'),('HTTPS：','2'),
    ('站点名称','安装验收站点'),('公开运营者名称','合成测试运营者'),
    ('公开隐私联系','privacy@example.com'),('公开部署地区','独立 CI 环境，运行结束删除测试数据'),
    ('SMTP 主机','smtp.example.com'),('SMTP TLS 端口','465'),
    ('SMTP 登录邮箱','mailer@example.com'),('发件邮箱','mailer@example.com'),
    ('SMTP 授权码/密码','installer-dummy-password'),
])
def public_site():
    with urllib.request.urlopen('http://127.0.0.1:18080/api/site',timeout=10) as response:
        return json.load(response)

site=public_site()
assert site['site_name']=='安装验收站点' and site['operator']=='合成测试运营者'
assert site['contact']=='privacy@example.com' and '{{site_name}}' in site['privacy_policy'] and '{{site_name}}' in site['terms']
assert 'smtp' not in site and 'admin_email' not in site
key=Path('/opt/webts/secrets/master.key').read_bytes()
subprocess.run(['webts','status'],check=True)
subprocess.run(['webts'],input='0\n',text=True,check=True)
interactive(['webts','server'],[('默认 TS 公网地址','ts.changed.example.com:9988'),('允许已登录用户','1')],timeout=120)
interactive(['webts','site'],[('站点名称','更新验收站点'),('公开运营者名称','合成运营者'),('公开隐私联系','privacy@example.com'),('公开部署地区','独立验收环境')],timeout=120)
assert public_site()['site_name']=='更新验收站点'
subprocess.run(['webts','backup'],check=True)
subprocess.run(['webts','update'],check=True,timeout=900)
assert Path('/opt/webts/secrets/master.key').read_bytes()==key
assert public_site()['site_name']=='更新验收站点'
subprocess.run(['webts','stop'],check=True)
print('Real installer download/build/start, Chinese menu, backend server config, backup and data-preserving update passed.')
