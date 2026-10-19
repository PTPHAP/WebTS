# Docker 安装

[文档中心](README.md) · [中文一键安装](INSTALL.md) · [配置说明](CONFIGURATION.md)

推荐 Debian 12 / Ubuntu 24.04 x64 和 Docker Compose v2。镜像使用 Linux host 网络保留实际 UDP 候选。Docker Desktop 的 host 网络需单独启用并验证；本项目不把 Windows Desktop 容器网络视为已通过语音验收。[Docker host 网络说明](https://docs.docker.com/engine/network/drivers/host/)

## 一键中文引导安装

适合第一次部署。先准备解析到服务器的域名、可达公网 IPv4、TLS SMTP 和获授权的 TeamSpeak 目标。在 **Debian 12 / Ubuntu 24.04** 的 root 终端粘贴：

```bash
apt-get update && apt-get install -y curl ca-certificates && bash -c 'f=$(mktemp) && curl -fsS --proto "=https" --tlsv1.2 -H "Accept: application/vnd.github.raw+json" "https://api.github.com/repos/PTPHAP/WebTS/contents/install.sh?ref=v1.1.1" -o "$f" && bash "$f"; r=$?; rm -f "$f"; exit "$r"'
```

非 root 先执行 `sudo -i`。安装器补齐缺少的 Docker/Compose，下载最新正式版及固定子模块，通过中文引导配置并**从源码构建镜像**。不是预构建镜像的一键导入，首次构建会较慢。安装目录为 `/opt/webts`；输入 `webts` 打开菜单，支持修改邮箱/TS、备份、状态和更新。

按引导填写域名、公网 IPv4、站名、运营信息、SMTP 和默认 TS。云安全组/防火墙放行 TCP 80/443、UDP 40000–40100，并允许网关向目标 TS 的语音 UDP 端口出站；安装器不代改防火墙。已有代理占用网站端口时会保留代理，需自行添加 HTTPS/WebSocket 转发。

访问 `https://你的域名`，注册并完成邮箱验证。在服务器运行 `webts admin`，填写已验证邮箱，再重新登录进入 **站点管理**。完整字段与命令见 [中文安装及配置引导](INSTALL.md)。

## 使用预构建镜像手动安装

1.0.6 起提供经过构建、启动和完整源码构建检查的镜像归档。每个发布版本提供：

- `webts-版本-docker-linux-x64.tar.gz`：已通过构建和启动检查的镜像归档；不是普通源码包。
- `SHA256SUMS_DOCKER`：对应归档的 SHA-256。
- `webts-版本-source.zip`：完整固定版本源码，包含 Compose、公开配置模板、文档和协议子模块。

适合已有 Docker Engine / Compose v2，希望省去编译的 Linux x64 服务器。此流程不会安装 `webts` 管理菜单，使用 Compose 命令维护。以下固定使用 [1.1.1 维护版本](https://github.com/PTPHAP/WebTS/releases/tag/v1.1.1)，在 root 终端、新的独立工作目录执行；需要 `curl`、`unzip` 和 `sha256sum`。

先下载、校验并导入镜像：

```bash
(
set -euo pipefail
VERSION=1.1.1
mkdir "webts-$VERSION-install"
cd "webts-$VERSION-install"
BASE="https://github.com/PTPHAP/WebTS/releases/download/v$VERSION"
curl -fL "$BASE/webts-$VERSION-docker-linux-x64.tar.gz" -o "webts-$VERSION-docker-linux-x64.tar.gz"
curl -fL "$BASE/SHA256SUMS_DOCKER" -o SHA256SUMS_DOCKER
curl -fL "$BASE/webts-$VERSION-source.zip" -o "webts-$VERSION-source.zip"
curl -fL "$BASE/SHA256SUMS_SOURCE" -o SHA256SUMS_SOURCE
# 镜像校验文件带构建目录前缀，核对时只移除此路径前缀。
sed 's#  .cache/#  #' SHA256SUMS_DOCKER | sha256sum -c -
sha256sum -c SHA256SUMS_SOURCE
docker load -i "webts-$VERSION-docker-linux-x64.tar.gz"
unzip "webts-$VERSION-source.zip"
)
```

失败时停止处理，不使用未通过校验的文件。随后进入源码包中的部署目录，准备配置：

```sh
VERSION=1.1.1
cd "webts-$VERSION-install/webts-$VERSION-source"
cp .env.example .env
printf '\nWEBTS_VERSION=%s\n' "$VERSION" >> .env
cp deploy/config.production.toml config.local.toml
install -d -m 700 -o 10001 -g 10001 data secrets
chown 10001:10001 config.local.toml
chmod 600 config.local.toml
```

编辑 `.env` 和 `config.local.toml`，不要保留示例地址：

| 位置 | 必填内容 |
| --- | --- |
| `.env` 的 `WEBTS_DOMAIN` | 域名，例如 `voice.example.com`；A 记录指向网关公网 IPv4 |
| `.env` 的 `WEBTS_VERSION` | `1.1.1`，与导入镜像的标签一致 |
| 配置的 `public_url` | 真实 `https://voice.example.com`，用于邮件链接、Cookie 与同源校验 |
| 配置的 `bind` / `trusted_proxy` | 自带代理时沿用回环监听与回环代理；其他代理填真实可信来源，不能信任任意来源 |
| `[[servers]]` | 名称及 `address`，例如 `ts.example.com:9987`；目标需全局语音加密与 Opus |
| `[smtp]` | TLS 主机、端口、登录邮箱、获授权的发件邮箱，密码文件保留 `secrets/smtp-password` |
| `[rtc].public_ip` | 网关可达公网 IPv4；UDP 默认 40000–40100 |

保存配置后，在上述部署目录执行。如果已有代理占用 80/443，最后的启动命令只填 `webts`，先在已有代理配置 HTTPS/WebSocket 转发，再访问网站。

```bash
# 仅首次安装生成主密钥。
docker compose run --rm --no-deps -v "$PWD/secrets:/app/secrets:rw" webts init-key /app/secrets/master.key
# 隐藏输入 SMTP 授权码，不把授权码放在命令历史里。
install -m 600 -o 10001 -g 10001 /dev/null secrets/smtp-password
read -r -s -p 'SMTP 授权码：' SMTP_PASSWORD; printf '\n'
printf '%s' "$SMTP_PASSWORD" > secrets/smtp-password
unset SMTP_PASSWORD
docker compose up -d --no-build webts caddy
docker compose logs --tail 50 webts
```

上面的授权码输入使用 Bash。首次在网页注册并验证站长后，按下方命令授予网站管理员权限，再进入后台完善站名、运营资料、隐私联系、图标和公告；好友私信还需要 [配置私有 S3 兼容桶](OBJECT-STORAGE.md)。

初始化命令仅针对首次部署，已有数据库必须保留原主密钥，不得重新生成。镜像归档已经包含网页、WASM、本地降噪资源及第三方许可，不下载访问者或部署者数据。

如果宿主机已有代理占用 80/443，只启动 `webts` 并接入既有代理；不要直接启动额外 Caddy 抢占端口。开放完整 RTC UDP 范围并允许网关访问 TS 语音 UDP。域名、证书、SMTP 和对象存储由部署者配置，没有预设账号。

## 从源码构建

保持原有 [中文引导安装](INSTALL.md)，也可在 root 终端手动构建：

```sh
git clone --recurse-submodules https://github.com/PTPHAP/WebTS.git
cd WebTS
cp .env.example .env
cp deploy/config.production.toml config.local.toml
install -d -m 700 -o 10001 -g 10001 data secrets
chown 10001:10001 config.local.toml
chmod 600 config.local.toml
# 先按配置教程填写以上文件，并按上节隐藏输入的方法创建 secrets/smtp-password。
docker compose build webts
docker compose run --rm --no-deps -v "$PWD/secrets:/app/secrets:rw" webts init-key /app/secrets/master.key
docker compose up -d webts caddy
```

完整源码 ZIP 可用于构建；GitHub 自动生成的 Source code 压缩包不包含子模块，不能替代完整源码资产。

## 站长和升级

完成邮箱注册与验证后：

```sh
docker compose exec webts /usr/local/bin/web-ts grant-admin /app/config.local.toml owner@example.com
```

`owner@example.com` 是占位邮箱。管理员登录后配置站点、SMTP、服务器列表、对象存储和通知；网站角色不提升 TS 权限。

升级前停服并备份数据库、原主密钥与配置，保留旧镜像。导入并验证新版本，在 `.env` 修改 `WEBTS_VERSION` 后执行 `docker compose up -d --no-build webts`。不要覆盖数据卷；数据库发生迁移时回退需恢复匹配的停服备份。完整备份恢复说明见 [部署与维护](DEPLOYMENT.md)。
