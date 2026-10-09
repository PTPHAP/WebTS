"""Real isolated GitHub-runner installation; never run on a personal server."""
import os
from pathlib import Path
import pty
import select
import subprocess
import time

if os.environ.get('GITHUB_ACTIONS')!='true' or os.geteuid()!=0:
    raise SystemExit('Only an isolated root GitHub Actions runner may run this fixture')
assert not Path('/opt/webts').exists(), 'Preserve an existing installation'

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

interactive(['bash','-c','f=$(mktemp) && curl -fsSL https://raw.githubusercontent.com/PTPHAP/WebTS/main/install.sh -o "$f" && bash "$f"; r=$?; rm -f "$f"; exit "$r"'],[
    ('网站域名','voice.example.com'),('本机公网 IPv4','8.8.8.8'),
    ('网关本机端口','18080'),('默认 TS 公网地址','ts.example.com:9987'),
    ('站长邮箱','owner@example.com'),('HTTPS：','2'),
    ('SMTP 主机','smtp.example.com'),('SMTP TLS 端口','465'),
    ('SMTP 登录邮箱','mailer@example.com'),('发件邮箱','mailer@example.com'),
    ('SMTP 授权码/密码','installer-dummy-password'),
])
key=Path('/opt/webts/secrets/master.key').read_bytes()
subprocess.run(['webts','status'],check=True)
subprocess.run(['webts'],input='0\n',text=True,check=True)
interactive(['webts','server'],[('默认 TS 公网地址','ts.changed.example.com:9988'),('允许已登录用户','1')],timeout=120)
subprocess.run(['webts','backup'],check=True)
subprocess.run(['webts','update'],check=True,timeout=900)
assert Path('/opt/webts/secrets/master.key').read_bytes()==key
subprocess.run(['webts','stop'],check=True)
print('Real installer download/build/start, Chinese menu, backend server config, backup and data-preserving update passed.')
