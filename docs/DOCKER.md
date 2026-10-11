# Docker 安装

[文档中心](README.md) · [中文一键安装](INSTALL.md) · [配置说明](CONFIGURATION.md)

推荐 Debian 12 / Ubuntu 24.04 x64 和 Docker Compose v2。镜像使用 Linux host 网络保留实际 UDP 候选。Docker Desktop 的 host 网络需单独启用并验证；本项目不把 Windows Desktop 容器网络视为已通过语音验收。[Docker host 网络说明](https://docs.docker.com/engine/network/drivers/host/)

## 使用预构建镜像

1.0.6 起提供经过构建、启动和完整源码构建检查的镜像归档。每个发布版本提供：

- `webts-版本-docker-linux-x64.tar.gz`：已通过构建和启动检查的镜像归档；不是普通源码包。
- `SHA256SUMS_DOCKER`：对应归档的 SHA-256。
- `webts-版本-source.zip`：完整固定版本源码，包含 Compose、公开配置模板、文档和协议子模块。

从 [Releases](https://github.com/PTPHAP/WebTS/releases) 下载同一版本文件，分别验证镜像与源码校验，再在独立目录操作。以下用 `1.0.6` 示范，实际版本以下载页为准。

```sh
VERSION=1.0.6
sha256sum "webts-$VERSION-docker-linux-x64.tar.gz"
# 比较 SHA256SUMS_DOCKER；完整源码也比较 SHA256SUMS_SOURCE。
docker load -i "webts-$VERSION-docker-linux-x64.tar.gz"
unzip "webts-$VERSION-source.zip"
cd "webts-$VERSION-source"
cp .env.example .env
printf '\nWEBTS_VERSION=%s\n' "$VERSION" >> .env
cp deploy/config.production.toml config.local.toml
sudo install -d -m 700 -o 10001 -g 10001 data secrets
# 填写 .env 域名，以及配置中的 HTTPS 地址、SMTP、TS、公网 IPv4。
docker compose run --rm --no-deps -v "$PWD/secrets:/app/secrets:rw" webts init-key /app/secrets/master.key
# secrets/smtp-password 填授权码，让 UID 10001 可读，权限 0600。
docker compose up -d --no-build webts caddy
docker compose logs --tail 50 webts
```

初始化命令仅针对首次部署，已有数据库必须保留原主密钥，不得重新生成。镜像归档已经包含网页、WASM、本地降噪资源及第三方许可，不下载访问者或部署者数据。

如果宿主机已有代理占用 80/443，只启动 `webts` 并接入既有代理；不要直接启动额外 Caddy 抢占端口。开放完整 RTC UDP 范围并允许网关访问 TS 语音 UDP。域名、证书、SMTP 和对象存储由部署者配置，没有预设账号。

## 从源码构建

保持原有 [中文引导安装](INSTALL.md)，也可手动：

```sh
git clone --recurse-submodules https://github.com/PTPHAP/WebTS.git
cd WebTS
cp .env.example .env
cp deploy/config.production.toml config.local.toml
sudo install -d -m 700 -o 10001 -g 10001 data secrets
# 先按配置教程填写以上文件。
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
