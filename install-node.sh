#!/usr/bin/env bash
# Node only. Download the versioned maintenance code, then its verified release package.
set -euo pipefail
umask 077
[[ ${EUID} -eq 0 ]] || { echo '请在TS机器使用root运行本命令；不会安装WebTS主站。'; exit 1; }
command -v curl >/dev/null || { echo '需要curl，请先用系统包管理器安装curl和ca-certificates。'; exit 1; }
command -v python3 >/dev/null || { echo '需要Python 3.10+；支持Debian 12、Ubuntu 22.04或更新的x64系统。尚未修改节点。'; exit 1; }
[[ -t 0 ]] || { echo '请在交互终端运行安装命令，不能把脚本管道直接传给bash。'; exit 1; }
python3 - <<'PY'
import platform, sys
from pathlib import Path
if sys.platform != 'linux' or platform.machine() != 'x86_64':
    sys.exit('此节点包支持Linux x64；Windows请使用PowerShell入口。尚未修改节点。')
libc, version = platform.libc_ver()
if sys.version_info < (3, 10) or libc != 'glibc' or tuple(map(int, version.split('.')[:2])) < (2, 35):
    sys.exit('需要Python 3.10+及glibc 2.35+（Debian 12、Ubuntu 22.04或更新版本）；不要强行替换系统库。尚未修改节点。')
if not Path('/run/systemd/system').is_dir():
    sys.exit('需要正常运行systemd的Linux主机，非普通容器。尚未修改节点。')
PY
stage=$(mktemp -d /tmp/webts-node-bootstrap-XXXXXXXX)
trap 'rm -f -- "$stage/node-linux.py"; rmdir -- "$stage"' EXIT
echo 'WebTS同机语音节点 · 正在下载中文安装器（不安装主站，不操作TeamSpeak）'
curl --fail --silent --show-error --proto '=https' --tlsv1.2 --max-time 60 --max-filesize 1048576 \
  -H 'Accept: application/vnd.github.raw+json' \
  'https://api.github.com/repos/PTPHAP/WebTS/contents/scripts/node-linux.py?ref=v1.1.3' -o "$stage/node-linux.py"
python3 "$stage/node-linux.py" update
