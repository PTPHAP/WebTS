# 部署与运行

[文档中心](README.md) · [一条命令安装](INSTALL.md) · [配置教程](CONFIGURATION.md) · [隐私政策](../PRIVACY.md)

WebTS 0.1为可启动的预览版。先在独立环境小规模验证，再向社区开放。TS3/TS6原生客户端兼容、真实邮件送达和容量目标需另行验收。

推荐通过 [一条命令安装与中文 `webts` 管理菜单](INSTALL.md) 完成首次部署、邮箱与站长权限配置。以下保留手动部署方法。

## 准备

- 一个指向Linux主机的HTTPS域名；反向代理需支持WebSocket。2核/4GB为容量测试参考，不是已测容量承诺。
- 可验证证书的TLS SMTP账号：465为隐式TLS，其他端口STARTTLS。未配置时注册和找回会明确失败。
- 获授权的TS服务器，语音加密设为 **Globally on**，默认及测试频道使用 **Opus**。
- 公网IPv4与UDP端口范围；默认40000–40100，每网页连接使用一个UDP端口。

网站不会安装或修改现有TS。用户权限来自真实身份，不使用站长Query账号代操作。

当前协议库拒绝回环及私有网络目标。`servers.address` 请填写获授权的 TS 公网域名或公网 IP 和 UDP 端口；同机部署也需使用实际可达的公网地址。网关自身的 `bind` 仍可使用回环地址，由 HTTPS 代理转发。

## Linux构建包

从[自动构建](https://github.com/PTPHAP/WebTS/actions/workflows/check.yml)成功运行中下载 `webts-1.0.0-linux-x64`。解压ZIP后计算tar包SHA-256，与SHA256SUMS对应行比较；只有构建通过才生成包。

```sh
sha256sum webts-1.0.0-linux-x64.tar.gz
tar -xzf webts-1.0.0-linux-x64.tar.gz
cd webts-1.0.0-linux-x64
umask 077
mkdir -p data secrets
./web-ts init-key secrets/master.key
cp deploy/config.production.toml config.local.toml
# 编辑域名、TS、SMTP、公网IP；SMTP密码单独写入secrets/smtp-password。
./web-ts serve config.local.toml
```

运行目录必须为包根目录。默认网关只监听127.0.0.1:8080，交由HTTPS代理公开。已有数据库必须恢复原密钥；init-key不会覆盖文件。

systemd部署：创建专用webts用户，将包放入/opt/webts，给该用户data写权限、secrets读权限，配置文件只读。调整并安装deploy/webts.service。服务收到SIGTERM时取消活动TS连接后退出。

## Docker Compose（Linux）

采用host网络保留UDP候选地址，不适用于Docker Desktop常规Windows网络。已有80/443服务时复用现有HTTPS代理，为新网站单独增加域名规则，避免抢占端口。

旧首个 Release 包已撤下，使用一条命令安装或下方递归克隆源码。不建议使用 GitHub 自动生成的 “Source code” 归档，它不包含协议子模块。构建会应用随仓库补丁，没有预设账号、数据库、部署密钥或真实邮箱配置。

```sh
git clone --recurse-submodules https://github.com/PTPHAP/WebTS.git
cd WebTS
cp .env.example .env
cp deploy/config.production.toml config.local.toml
# WEBTS_DOMAIN与public_url必须对应；填写真实TS与SMTP。
sudo install -d -m 700 -o 10001 -g 10001 data secrets
docker compose build webts
docker run --rm -v "$PWD/secrets:/app/secrets" webts:1.0.0 init-key /app/secrets/master.key
# 填写SMTP密码文件，让UID10001可读；文件权限0600。
docker compose up -d webts caddy
docker compose logs --tail 50 webts
```

Caddy自动申请公开HTTPS证书，DNS与80/443必须可达。数据库卷可写、密钥卷只读，应用以非root运行。容器内存上限不是性能目标已通过的证明。

| 配置 | 含义 |
| --- | --- |
| public_url | 地址栏中实际HTTPS站点，含非标准端口时也写端口；用于同源验证 |
| bind | 网关监听IP与端口，HTTPS代理一般使用回环地址 |
| trusted_proxy | 只填写实际代理IP，才信任其X-Real-IP限流信息 |
| servers | 获授权的目标白名单；每物理目标只使用一个ID，避免地址别名重复连接 |
| smtp | TLS主机、端口、用户名、发件人和独立密码文件 |
| rtc.public_ip | 浏览器可访问的公网IPv4，与防火墙及NAT映射一致 |
| rtc.udp_min/max | 完整开放UDP范围，容量不得小于max_connections |
| master_key_file | 身份密文的独立主密钥，不能随意更换 |

防火墙开放网站80/443及RTC UDP范围，允许网关向配置的TS UDP目标发包。TS服务器也需允许网关访问语音端口。不要对不属于自己的服务器压测。

## 网站管理员与热加载

先通过邮箱验证建立站长账号，再在网关所在主机执行一次：

```sh
./web-ts grant-admin config.local.toml owner@example.com
# Docker：docker compose exec webts /app/web-ts grant-admin /app/config.local.toml owner@example.com
```

此命令只提升已验证的现有账号。普通注册不会自动成为管理员，网站角色不会增加 TS 权限。登录后点击顶部“站点管理”，设置 SMTP、服务器列表和默认项，并选择是否允许已登录用户连接自定义公网地址。自定义连接初始关闭；首次账号注册仍需在部署配置中提供 SMTP。

后台保存需要复核当前管理员密码。SMTP 授权码不会回显，留空保持原值；更换 SMTP 主机或账号时必须填写新授权码。后台配置使用部署主密钥进行 AES-256-GCM 加密，保存在 SQLite，重启后仍生效，并优先于初始配置文件中的邮箱和服务器列表。

保存后新邮件、服务器选择和连接策略立即生效，不需要重启。已经发送中的邮件用原配置完成；关闭自定义连接、删除服务器或修改服务器地址会取消对应活动连接。监听地址、HTTPS站点、媒体端口、部署密钥等基础配置仍通过配置文件管理，修改这些项目需要重启网关。

自定义地址仅支持公网域名/IP与可选 UDP 端口（默认9987）。网关解析后检查公网地址并固定实际连接目标，拒绝回环、内网、链路本地、元数据地址等；同一 UID 到同一实际地址不能通过预设/自定义别名重复连接。服务器仍需全局语音加密与 Opus。

## 带认证的TURN

默认优先直连网关UDP。受限网络可使用Coturn，deploy/turnserver.example.conf只是模板，不能原样用于公网。

1. 生成至少32字节随机密钥，写入secrets/turn-secret；同一值填写secrets/turnserver.conf的static-auth-secret。
2. 配置TURN域名、公网IP与可信TLS证书，保存secrets/turn-cert.pem和turn-key.pem。
3. 网关设置turn_url（例如turns:turn.example.com:5349?transport=tcp）和turn_secret_file。
4. 启动docker compose --profile turn up -d，开放5349/TCP、3478/TCP/UDP和49160–49259/UDP。

网页只取得1小时有效的HMAC凭据，不接触共享密钥。模板屏蔽私有网络作为中继目标；网关需提供可达公网候选。不要直接移除所有限制。长期连接的TURN认证刷新仍需验收。

## 本地开发

Node固定22.20.0，Rust固定1.99.0，依赖版本由web/package-lock.json及Cargo.lock固定。

Windows，在本项目根目录：

```powershell
.\scripts\bootstrap-rust.ps1
.\scripts\prepare-vendor.ps1
.\scripts\start.ps1
```

打开http://localhost:8080。启动脚本复制配置样例，但SMTP和TS需真实填写；没有演示账号或伪造服务器。修改配置后重启。所有编译工具与缓存位于本项目，兼容中文目录及空格。

Linux源码：

```sh
sh scripts/prepare-vendor.sh
cd web && npm ci && npm run build && cd ..
cargo test --workspace --locked
cargo build --release --package web-ts --locked
umask 077
mkdir -p data secrets
./target/release/web-ts init-key secrets/master.key
cp config.example.toml config.local.toml
./target/release/web-ts serve config.local.toml
```

HTTP仅用于明确允许的localhost开发，且仅监听回环地址。远程调试可通过SSH转发到自己的localhost，不能直接公开开发HTTP端口。

## 备份、恢复与升级

先停止网关，再备份整个data目录，避免遗漏SQLite WAL变更。主密钥另存于不同受控位置；数据库与密钥均应加密备份并限制访问。SMTP/TURN凭据分别备份。

恢复：停止实例，放回数据库与**原主密钥**，检查部署用户权限，启动后验证测试身份UID与连接。恢复后撤销旧会话或让用户退出全部设备。丢失主密钥不能用账号密码或邮件找回TS私钥。首版暂无在线密钥轮换，禁止生成新密钥替换旧密钥。

升级：停服务、分离备份数据库和密钥，替换程序与web/dist，保留配置、data、secrets。先跑独立账号的登录、身份与通话检查，再开放用户访问。预览版升级前检查版本说明和迁移要求。

## 故障排查

- 邮件失败：检查TLS证书、发件人授权、端口和密码文件。队列接受不等于收件箱已送达，日志不记录邮箱/凭据/令牌。
- 来源403：public_url必须匹配地址栏协议、域名和端口；代理不要改写Origin。
- 重复连接：退出旧网页并等待释放；同一UID与目标不允许重复网页连接。
- TS连接：网页区分UDP超时、服务器密码错误、封禁、防刷、容量及身份等级。UDP超时不代表密码错误；TS服务器看到的来源是网关IP，需要允许该IP访问目标语音UDP端口。较高身份等级请先在原生客户端提高后重新导入。后台预设名称只是标签，实际地址以“站点管理”为准。
- 显示昵称：需要3–30个字符，与身份名称独立；过短身份名会在默认昵称后补上“ · WebTS”，过长名称只在默认昵称中截取，身份名称与UID不改变。
- `Permission denied` / 麦克风权限被拒绝：检查浏览器地址栏的网站麦克风权限及系统麦克风隐私设置，允许后重新连接。也可以选择“仅收听”，不申请麦克风权限；这类提示不能直接判定为TS身份权限错误。默认自由发言不设置声音阈值；按键发言在语音设备设置中选择。
- 无音频：服务器Globally on、频道Opus、麦克风授权；开放完整UDP范围，填写公网IP，必要时启用认证TURN。点击“启用收听”解除浏览器暂停。
- 被踢出/断网：网页停止连接，需手动重连，不会在账号撤销后自动重连。

协议探针web-ts probe（命令用法直接运行程序查看）仅检查握手、加密与接收包数，不替代双向通话、耳语、权限和原生客户端兼容验收。
