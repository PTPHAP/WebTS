#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ ${EUID} -eq 0 ]] || { echo '请用 root 运行，或在安装命令前加 sudo。'; exit 1; }
[[ -r /etc/os-release ]] || { echo '当前系统暂不支持。'; exit 1; }
source /etc/os-release
case "$ID:$VERSION_ID" in
  debian:12|ubuntu:24.04) ;;
  *) echo '自动安装支持 Debian 12、Ubuntu 24.04。其他 Linux 可按部署文档安装。'; exit 1 ;;
esac
root=/opt/webts
[[ ! -L "$root" ]] || { echo '安装目录不能是符号链接。'; exit 1; }
if [[ -e "$root" && ! -f "$root/.webts-managed" ]]; then
  echo '/opt/webts 已有其他文件，安装已停止，避免覆盖。'; exit 1
fi
if [[ -e /usr/local/bin/webts ]] && ! head -n 2 /usr/local/bin/webts | grep -q 'WebTS managed launcher'; then
  echo '/usr/local/bin/webts 已被其他程序占用，安装已停止。'; exit 1
fi
echo 'WebTS 中文安装 · 源码构建 · 保留现有数据库和部署密钥'
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y ca-certificates curl git python3
if ! command -v docker >/dev/null; then
  apt-get install -y docker.io
  systemctl enable --now docker
fi
docker info >/dev/null || { echo 'Docker 未运行，请先启动 Docker 后重试。'; exit 1; }
if ! docker compose version >/dev/null 2>&1; then
  # Docker's documented plugin location; do not replace an existing Engine.
  case $(uname -m) in x86_64|aarch64) asset="docker-compose-linux-$(uname -m)" ;; *) echo '当前架构请先安装 Docker Compose V2。'; exit 1 ;; esac
  plugin=/usr/local/lib/docker/cli-plugins/docker-compose
  [[ ! -e "$plugin" ]] || { echo '已有 Compose 插件无法运行，请修复它；不会覆盖。'; exit 1; }
  task_download=$(mktemp -d)
  trap 'rm -f "$task_download/$asset" "$task_download/$asset.sha256"; rmdir "$task_download"' EXIT
  base=https://github.com/docker/compose/releases/download/v2.39.4
  curl -fsSL "$base/$asset" -o "$task_download/$asset"
  curl -fsSL "$base/$asset.sha256" -o "$task_download/$asset.sha256"
  (cd "$task_download"; sha256sum -c "$asset.sha256")
  install -d -m 755 /usr/local/lib/docker/cli-plugins
  install -m 755 "$task_download/$asset" "$plugin"
  docker compose version
  rm -f "$task_download/$asset" "$task_download/$asset.sha256"
  rmdir "$task_download"
  trap - EXIT
fi
if [[ -f "$root/current/scripts/webts.py" ]]; then
  echo '检测到已有 WebTS，进入保留数据的更新流程。'
  if docker image inspect webts:managed >/dev/null 2>&1; then
    exec python3 "$root/current/scripts/webts.py" update
  else
    exec python3 "$root/current/scripts/webts.py" setup
  fi
fi
install -d -m 700 "$root" "$root/releases"
printf 'WebTS managed installation\n' > "$root/.webts-managed"
candidate=$(mktemp -d "$root/releases/source-XXXXXXXX")
git -c http.sslVerify=true clone --depth 1 --recurse-submodules --shallow-submodules https://github.com/PTPHAP/WebTS.git "$candidate"
test -f "$candidate/scripts/webts.py" || { echo '下载的版本缺少中文管理器，请稍后重试。'; exit 1; }
ln -s "$candidate" "$root/current"
cat > /usr/local/bin/webts <<'SH'
#!/bin/sh
# WebTS managed launcher
exec python3 /opt/webts/current/scripts/webts.py "$@"
SH
chmod 755 /usr/local/bin/webts
echo '源码下载完成；进入中文基础配置。SMTP 授权码不会显示或写入命令历史。'
exec webts setup
