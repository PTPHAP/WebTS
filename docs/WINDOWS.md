# Windows 安装与维护

[文档中心](README.md) · [配置说明](CONFIGURATION.md) · [支持范围](SUPPORT.md)

Windows 运行包面向 Windows 10/11 x64，网关在本机运行，用户通过浏览器访问。不需要 Rust、Node.js、Python 或 Docker。1.0.6 起提供经过原生构建和启动检查的运行包；请下载对应资产，不要把源码 ZIP 当作 Windows 运行包。

## 一键下载与打开菜单

打开 **Windows PowerShell**，将下面整段命令一次粘贴执行。命令固定下载正式版 **1.0.6**，验证发布包 SHA-256，再打开现有中文管理菜单；不需要管理员权限。下载需要能够正常访问 GitHub，不会绕过 HTTPS 证书检查。

默认安装到当前用户的 `WebTS-1.0.6` 目录。目录已存在时停止，不覆盖已有安装；如需换位置，修改 `$Destination` 为独立的本机目录。安装前可先查看仓库中的 [中文菜单脚本](../scripts/windows-menu.ps1)。

```powershell
& {
    $ErrorActionPreference = 'Stop'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $Version = '1.0.6'
    $Destination = Join-Path $env:USERPROFILE "WebTS-$Version"
    if (Test-Path -LiteralPath $Destination) {
        throw '目标目录已存在，请使用新的独立目录；更新请按下方步骤操作。'
    }
    $Download = Join-Path ([IO.Path]::GetTempPath()) ('webts-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $Download | Out-Null
    $Archive = Join-Path $Download "webts-$Version-windows-x64.zip"
    $Url = "https://github.com/PTPHAP/WebTS/releases/download/v$Version/webts-$Version-windows-x64.zip"
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Archive
    $Expected = '060671a56bc286c17ff1d6b994e3cbde862c42bbfc6672dd6f22c120acfd7c3d'
    if ((Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Expected) {
        throw '校验失败，请停止安装，并重新从正式发布页下载。'
    }
    Expand-Archive -LiteralPath $Archive -DestinationPath $Destination
    $Root = Join-Path $Destination "webts-$Version-windows-x64"
    Set-Location -LiteralPath $Root
    Write-Host "校验通过，安装位置：$Root"
    Write-Host "下载文件：$Archive"
    & (Join-Path $Root 'webts.cmd')
}
```

上面的校验值仅适用于 1.0.6，与 [SHA256SUMS_WINDOWS](https://github.com/PTPHAP/WebTS/releases/download/v1.0.6/SHA256SUMS_WINDOWS) 一致。不要只修改版本号后沿用旧校验值。下载 ZIP 保留在命令显示的安装目录之外的临时目录中，不包含网站数据；初始化过程中输入的 SMTP 授权码保存在安装目录受限文件中。

如无法下载，先检查 GitHub 网络连接或使用下面的手动流程；不要关闭 TLS 验证。已部署网站更新请使用“更新、备份与回退”流程，不重复初始化。

## 手动下载与首次配置

1. 在 [Releases](https://github.com/PTPHAP/WebTS/releases) 选择同一正式版本的 `webts-版本-windows-x64.zip` 和 `SHA256SUMS_WINDOWS`。
2. 用 PowerShell 的 `Get-FileHash .\webts-版本-windows-x64.zip -Algorithm SHA256` 与校验文件比较。校验只能检查下载一致性，不能代替发布来源的可信性。
3. 解压到独立、本机 NTFS 的普通目录，例如 `D:\WebTS`；不要放入公开共享目录、同步盘、目录链接或其他项目。首次使用前先检查运行包中的脚本。
4. 双击 `webts.cmd`，选择“初始化”。使用上方命令时菜单会自动打开，从此步继续。按下面的表格填写基础配置。授权码以隐藏输入保存，不写入命令参数。
5. 选择“启动”，保持网关窗口打开。按 Ctrl+C 正常停止。浏览器在本机使用 `http://localhost:8080`；选择其他本机端口时监听端口随地址变化。
6. 注册账号、点击邮件验证链接，停止网关后运行 `webts.cmd admin`，填写已验证账号邮箱。再次启动并登录，即可看到站点管理入口。

菜单也支持 `webts.cmd setup`、`start`、`status`、`admin`、`backup`。状态命令读取初始化配置的本机 IPv4 回环监听端口；手动改成其他监听地址时，自行检查对应 `/api/health`。

初始化限制目录访问为当前 Windows 用户和 SYSTEM；在之后启动时继续使用同一账号。管理命令不会安装系统服务或自行修改防火墙。目录中已有配置、数据库或主密钥时拒绝重复初始化，以免误覆盖。

| 初始化项目 | 本机试用示例与说明 |
| --- | --- |
| 网站地址 | `http://localhost:8080`；公开使用改为真实 HTTPS 地址 |
| 站名、运营者与公开联系方式 | 站名默认 `WebTS`，默认协议同步使用站名；联系方式会向用户公开 |
| 数据处理说明 | 填真实部署地区、服务提供方、日志/备份期限与注销联系渠道 |
| 默认 TS | `ts.example.com:9987`，使用有权连接的公网目标；频道需 Opus，服务器全局语音加密开启 |
| 公网 IPv4 | 网关的实际公网地址；没有可达 UDP 的公网地址时，跨网络语音可能需要认证 TURN |
| SMTP 主机、端口 | 例如 `smtp.qq.com`、`465`，其他邮箱以服务商 TLS SMTP 配置为准 |
| 登录/发件邮箱 | 填有发信权限的邮箱；没有默认邮件账号 |
| SMTP 密码 | 使用邮箱服务提供的授权码，不是网站账号密码；隐藏输入 |

初始化后选择“启动”，在浏览器访问网站。网页注册并验证站长邮箱后，先按 Ctrl+C 停止网关，在安装目录的 PowerShell 执行 `./webts.cmd admin`，填写该邮箱；再执行 `./webts.cmd start`，重新登录即可进入 **站点管理**。管理员可设置 SMTP、默认 TS、自定义地址开关、外观和公告，网站角色不会提升 TS 权限。

## 对外开放

本机 HTTP 只能由本机访问。公开部署必须配置 HTTPS 代理、有效证书、域名和可达公网 IPv4。网关仍绑定 `127.0.0.1:8080`，代理转发 WebSocket；`public_url` 填实际 HTTPS 地址，`trusted_proxy` 只填实际代理的回环地址。代理模板见 `deploy/Caddyfile`，需按 Windows 文件路径和实际域名调整，不直接启动 Linux 容器配置。

手动在防火墙/NAT 开放网站端口及 `[rtc]` 的 UDP 范围（默认 40000–40100），`rtc.public_ip` 指向公网 IPv4。家用网络可能受 CGNAT、入站封锁或动态 IP 影响；网页能打开不等于 WebRTC UDP 可用。必要时配置带认证的 TURN。TLS SMTP 未配置时不能注册或找回，界面会明确报错。

默认 TS 目标必须公网可达、使用 Opus，并开启全局语音加密。网站管理员权限不替代 TeamSpeak 身份权限。公网 Windows 服务的实际证书、UDP、防火墙和长时间语音需要独立验收；长期社区部署优先选择已验证的 Linux 方案。

## 更新、备份与回退

采用人工可检查的更新流程，不自动覆盖运行中的 Windows 程序：

1. 下载新版本及校验文件，验证后解压到另一个临时目录，先读更新记录。
2. 停止当前网关；运行 `webts.cmd backup`。工具在网关运行时拒绝复制 SQLite，备份包含 `data`、`secrets` 和 `config.local.toml`。
3. 备份移到受控的加密位置并检查可读性。保留原版本运行包。默认备份只覆盖安装目录中的数据；若手动将数据库或密钥改到外部路径，必须另外备份那些路径。
4. 替换程序 `web-ts.exe`、网页 `web/dist`、管理脚本、公开文档和许可。**保留 `data`、`secrets`、`config.local.toml` 和 `backups`，不要执行初始化。**
5. 启动，验证登录、原身份 UID、配置及小规模双向通话。跨数据库迁移版本回退必须同时恢复对应停服备份，不能只换旧程序。

丢失主密钥不能通过账号密码找回托管身份；数据库和原密钥必须成对保留。备份包含邮箱凭据和个人数据，不要上传仓库或公开分享。

## 开发者构建

普通部署者使用运行包。开发者工具链和源码步骤见 [部署文档](DEPLOYMENT.md)；发布流程固定 Rust 版本，在 Windows runner 构建原生 MSVC x64 程序，使用静态 C 运行库，并复用同一次构建中经过检查的网页与 WASM。包内附第三方许可。
