# Windows 安装与维护

[文档中心](README.md) · [配置说明](CONFIGURATION.md) · [支持范围](SUPPORT.md)

Windows 运行包面向 Windows 10/11 x64，网关在本机运行，用户通过浏览器访问。不需要 Rust、Node.js、Python 或 Docker。1.0.6 起提供经过原生构建和启动检查的运行包；请下载对应资产，不要把源码 ZIP 当作 Windows 运行包。

## 首次安装

1. 在 [Releases](https://github.com/PTPHAP/WebTS/releases) 选择同一正式版本的 `webts-版本-windows-x64.zip` 和 `SHA256SUMS_WINDOWS`。
2. 用 PowerShell 的 `Get-FileHash .\webts-版本-windows-x64.zip -Algorithm SHA256` 与校验文件比较。校验只能检查下载一致性，不能代替发布来源的可信性。
3. 解压到独立、本机 NTFS 的普通目录，例如 `D:\WebTS`；不要放入公开共享目录、同步盘、目录链接或其他项目。首次使用前先检查运行包中的脚本。
4. 双击 `webts.cmd`，选择“初始化”。输入网站地址、站名、运营者、公开联系方式、数据保留说明、默认 TS 公网地址以及 TLS SMTP 配置。授权码以隐藏输入保存，不写入命令参数。
5. 选择“启动”，保持网关窗口打开。按 Ctrl+C 正常停止。浏览器在本机使用 `http://localhost:8080`；选择其他本机端口时监听端口随地址变化。
6. 注册账号、点击邮件验证链接，停止网关后运行 `webts.cmd admin`，填写已验证账号邮箱。再次启动并登录，即可看到站点管理入口。

菜单也支持 `webts.cmd setup`、`start`、`status`、`admin`、`backup`。状态命令读取初始化配置的本机 IPv4 回环监听端口；手动改成其他监听地址时，自行检查对应 `/api/health`。

初始化限制目录访问为当前 Windows 用户和 SYSTEM；在之后启动时继续使用同一账号。管理命令不会安装系统服务或自行修改防火墙。目录中已有配置、数据库或主密钥时拒绝重复初始化，以免误覆盖。

## 对外开放

本机 HTTP 只能由本机访问。公开部署必须配置 HTTPS 代理、有效证书、域名和可达公网 IPv4。网关仍绑定 `127.0.0.1:8080`，代理转发 WebSocket；`public_url` 填实际 HTTPS 地址，`trusted_proxy` 只填实际代理的回环地址。代理模板见 `deploy/Caddyfile`，需按 Windows 文件路径和实际域名调整，不直接启动 Linux 容器配置。

手动在防火墙/NAT 开放网站端口及 `[rtc]` 的 UDP 范围（默认 40000–40100），`rtc.public_ip` 指向公网 IPv4。家用网络可能受 CGNAT、入站封锁或动态 IP 影响；网页能打开不等于 WebRTC UDP 可用。必要时配置带认证的 TURN。TLS SMTP 未配置时不能注册或找回，界面会明确报错。

默认 TS 目标必须公网可达、使用 Opus，并开启全局语音加密。网站管理员权限不替代 TeamSpeak 身份权限。公网 Windows 服务的实际证书、UDP、防火墙和长时间语音需要独立验收；长期社区部署优先选择已验证的 Linux 方案。

## 更新、备份与回退

本轮提供人工可检查的更新流程，不自动覆盖运行中的 Windows 程序：

1. 下载新版本及校验文件，验证后解压到另一个临时目录，先读更新记录。
2. 停止当前网关；运行 `webts.cmd backup`。工具在网关运行时拒绝复制 SQLite，备份包含 `data`、`secrets` 和 `config.local.toml`。
3. 备份移到受控的加密位置并检查可读性。保留原版本运行包。默认备份只覆盖安装目录中的数据；若手动将数据库或密钥改到外部路径，必须另外备份那些路径。
4. 替换程序 `web-ts.exe`、网页 `web/dist`、管理脚本、公开文档和许可。**保留 `data`、`secrets`、`config.local.toml` 和 `backups`，不要执行初始化。**
5. 启动，验证登录、原身份 UID、配置及小规模双向通话。跨数据库迁移版本回退必须同时恢复对应停服备份，不能只换旧程序。

丢失主密钥不能通过账号密码找回托管身份；数据库和原密钥必须成对保留。备份包含邮箱凭据和个人数据，不要上传仓库或公开分享。

## 开发者构建

普通部署者使用运行包。开发者工具链和源码步骤见 [部署文档](DEPLOYMENT.md)；发布流程固定 Rust 版本，在 Windows runner 构建原生 MSVC x64 程序，使用静态 C 运行库，并复用同一次构建中经过检查的网页与 WASM。包内附第三方许可。
