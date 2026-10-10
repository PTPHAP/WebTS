<div align="center">

![WebTS](docs/images/webts-banner.svg)

**把熟悉的 TeamSpeak 频道，带进浏览器。**

自托管 · 邮箱账号 · 多个真实身份 · 权限继承 · 加密语音

[![MIT](https://img.shields.io/badge/License-MIT-8b5cf6?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Backend-Rust-e8795a?style=flat-square&logo=rust)](server)
[![TypeScript](https://img.shields.io/badge/Frontend-TypeScript-3178c6?style=flat-square&logo=typescript&logoColor=white)](web)
[![React](https://img.shields.io/badge/UI-React-61dafb?style=flat-square&logo=react&logoColor=white)](web)
[![Checks](https://github.com/PTPHAP/WebTS/actions/workflows/check.yml/badge.svg)](https://github.com/PTPHAP/WebTS/actions/workflows/check.yml)

[安装教程](docs/INSTALL.md) · [配置教程](docs/CONFIGURATION.md) · [使用说明](docs/CLIENT.md) · [支持范围](docs/SUPPORT.md) · [好友加密私信](docs/FRIENDS.md) · [隐私政策](PRIVACY.md)

</div>

---

WebTS 是独立开源的 **TeamSpeak 网页客户端与协议网关**。成员通过邮箱登录、创建或导入 TeamSpeak 身份，在浏览器里加入已有频道；网站以该身份连接目标服务器，继承它原有的权限。部署者掌握自己的账号数据库、密钥、邮件服务和默认服务器配置。

> **1.0.1 正式源码版本。** 核心发布范围是自托管账号、TS3 网关与网页工作区；好友 E2EE、TS6 和移动语音继续标为实验支持。 已有真实 TS3 协议联调、浏览器双向语音及 Linux 安装/更新检查。TS6、移动语音、各浏览器完整兼容、TURN 长连接及容量指标仍在验收中。[查看支持与验证范围](docs/SUPPORT.md)。

## 界面预览

![WebTS 中文深色界面](docs/images/webts-preview.jpg)

<sub>使用专门测试账号与频道的界面预览；实际频道、成员和权限由所连接的服务器决定。</sub>

## 一条命令开始

**Debian 12 / Ubuntu 24.04**，服务器 root 终端：

```bash
apt-get update && apt-get install -y curl ca-certificates && bash -c 'f=$(mktemp) && curl -fsSL https://raw.githubusercontent.com/PTPHAP/WebTS/main/install.sh -o "$f" && bash "$f"; r=$?; rm -f "$f"; exit "$r"'
```

中文引导会先配置站名、公开运营资料，再收集域名、公网 IP、邮箱和默认 TS 地址，从源码构建并启动。安装后输入 **`webts`** 打开中文管理菜单。首次构建需要时间；部署前准备 HTTPS 域名、TLS SMTP 和开启全局语音加密的 TS 服务器。

| 我要做什么 | 从这里开始 |
| --- | --- |
| 第一次安装、申请证书、开通站长 | [安装教程](docs/INSTALL.md) |
| 配置域名、邮箱、TS 地址、热加载和 TURN | [配置教程](docs/CONFIGURATION.md) |
| Docker / systemd / 手动部署、备份恢复 | [部署与维护](docs/DEPLOYMENT.md) |
| 登录、导入身份、语音设置、头像 | [身份互用](docs/IDENTITIES.md) · [语音与头像](docs/CLIENT.md) |
| 判断设备或某项功能是否支持 | [支持范围与常见问题](docs/SUPPORT.md) |
| 设置首页图片公告与个人资料 | [首页与个人资料](docs/HOME-PROFILE.md) |
| 单击预览、调整布局与创建频道 | [工作区与频道管理](docs/WORKSPACE.md) |
| 查看频道格式、图片与音乐机器人兼容 | [TeamSpeak 渲染与音乐](docs/TEAMSPEAK-RENDERING.md) |
| 设置 AFK、管理网站账号和封禁 | [AFK 与账号管理](docs/ACCOUNT-ADMIN.md) |
| 好友、双棘轮私信、阅后即焚及私有S3配置 | [好友与加密存储](docs/FRIENDS.md) |
| 雨云Region怎么填、检测存储配置或处理迁移限制 | [对象存储配置与排错](docs/OBJECT-STORAGE.md) |
| 发布维护通知与更新邮件 | [信件与通知](docs/NOTIFICATIONS.md) |
| 了解数据如何处理 | [隐私政策](PRIVACY.md) · [安全边界](docs/SECURITY.md) |

完整导航见 [文档中心](docs/README.md)。[正式发布页](https://github.com/PTPHAP/WebTS/releases/tag/v1.0.1)提供完整源码 ZIP 和 SHA256 校验，含固定协议子模块；GitHub 自动生成的 Source code 归档不含子模块，请优先使用完整源码包。

## 已提供的功能

| 模块 | 功能 |
| --- | --- |
| 账号 | 邮箱验证、密码登录与协议主动确认、可选记住登录、单次链接找回、退出当前或全部设备 |
| 身份 | 导入、创建、命名、默认项、切换、删除、TS3 兼容导出；显示 UID，各身份权限独立 |
| 频道 | 按服务器实际顺序显示频道树，成员、介绍、密码频道与切换；支持安全的有限 BBCode |
| TS 聊天 | 频道/服务器聊天、独立原生私聊窗口、明显的消息与戳一戳提示音和提醒；两段传输加密 |
| 好友私信（实验） | 好友码/请求/屏蔽、安全码核对、可选 Olm v1 双棘轮 E2EE、自动设备密钥和可选口令备份；好友在线/频道位置、隐私开关和加入频道；默认普通聊天无需设备身份或对方额外同意，离线加密存入对象存储，内容AES-256-GCM；与Signal协议不同 |
| 临时图片与表情 | 内置贴纸、裁剪图片、自定义表情；好友消息密文保存在私有 S3 桶，默认7天和可选阅后即焚 |
| 手机界面 | 底部导航，频道、语音聊天、资料和好友独立页面；桌面好友支持悬浮/停靠，导航按钮与过渡动画，支持减少动态效果 |
| 站点信件 | 右上角信箱、未读与已读、HTML/Markdown通知、裁剪图片、管理员专页发布/撤回；可选订阅更新邮件 |
| 语音 | Opus 双向通话、自由发言与自动静默停发、默认 V 的可自定义按键发言 |
| 音频处理 | 本地 GTCRN AI 降噪、RNNoise 轻量档、降噪强度、键鼠抑制；设备支持的回声消除/增益及实际状态 |
| AFK 离开 | 原生离开状态与留言互通、暂停开麦、保留收听；自动重连恢复，成功进入/返回时播放提示音 |
| 设备与收听 | 输入/输出选择、静音、停止收听、仅收听、成员独立音量、频道与成员耳语 |
| 个人资料与头像 | 社区昵称、账号头像和介绍统一用于顶部及好友资料，不向好友暴露邮箱；按开关同步到当前 TS 身份，显示原生成员服务器头像 |
| 权限操作 | 依当前 TS 身份权限移动/踢出成员，创建、编辑或删除空频道 |
| 连接恢复 | 持续退避重连、恢复断线前实际频道；可手动停止，遵守撤权与加密策略 |
| 站点管理 | SMTP/服务器配置热加载；账号搜索、状态筛选、详情、临时/永久封禁、解封、强制退出、管理员角色、备注与操作记录 |
| 部署管理 | 中文 `webts` 菜单，服务状态、日志、配置、备份及保留数据更新 |
| 账号偏好 | 跨设备同步主题、音频/提示音、开麦键、布局、阅后即焚开关和时长；设备及密码留在本机 |
| 界面 | 独立首页与登录页、后台编辑图片公告；中文优先、深浅主题、桌面三栏与手机响应式布局 |

严格“仅保留人声”默认关闭，避免吞掉轻声；降噪效果受设备和环境影响。按键发言仅在网页获得焦点时响应。手机语音为试验支持；通用文件传输、屏幕共享、完整组权限编辑器、myTeamSpeak 云头像和旧 Speex/CELT 编码暂未提供。

## 用什么开发

| 层次 | 语言与技术 | 用途 |
| --- | --- | --- |
| 网页 | **TypeScript · React · Vite · CSS** | 界面、账号、频道与成员交互 |
| 网关 | **Rust · Tokio · Axum** | 单进程 HTTP / WebSocket、会话与 TS 连接 |
| 持久化 | **SQLite · rusqlite** | 账号、身份密文、会话、后台设置密文 |
| TS 协议 | **固定版本 tsclientlib / tsproto** | 真实身份认证、权限、消息、Opus 与头像互通 |
| 浏览器语音 | **WebRTC · Opus · AudioWorklet · WebAssembly** | 加密音频传输、本地处理与设备控制 |
| 本地降噪 | **GTCRN · RNNoise** | 在访问者设备上运行，不接入云降噪服务 |
| 认证与存储保护 | **Argon2id · AES-256-GCM** | 密码哈希、身份与后台敏感设置加密 |
| 邮件 | **SMTP over TLS · lettre** | 验证、找回与用户订阅的更新通知 |
| 安装与运维 | **Bash · Python 3 · Docker Compose · Caddy** | 中文引导、管理命令、反向代理与 HTTPS |

依赖版本由锁文件、固定工具链和子模块提交约束。[依赖说明](docs/DEPENDENCIES.md) · [架构说明](docs/ARCHITECTURE.md) · [第三方许可](THIRD_PARTY_NOTICES.md)。

## 安全与隐私

浏览器 → 网关：HTTPS/WSS + WebRTC DTLS-SRTP，优先 **AES-256-GCM**，兼容下限 **AES-128-GCM**。网关 → TeamSpeak：必须开启全局原生语音加密，当前协议库使用 **AES-128-EAX**。不满足策略时停止语音。

**这是两段传输加密，网关能接触语音和解密托管身份；不是端到端加密。** 网站管理员不会因此获得 TS 权限，但掌握部署密钥的运营者具有身份解密能力，只向可信站点托管身份。

默认不录制语音，也不把 TS 聊天历史写入 WebTS 数据库；好友私信在私有对象存储中临时加密保存，也不接入广告或访客统计脚本。SMTP、目标 TS、可选 TURN、站点代理和备份仍有各自的数据处理边界；头像在经典 TS3 文件传输链路不享有语音加密。[阅读完整隐私政策](PRIVACY.md)。

## 开发与贡献

```sh
git clone --recurse-submodules https://github.com/PTPHAP/WebTS.git
cd WebTS
sh scripts/prepare-vendor.sh
cd web && npm ci && npm run build && cd ..
cargo build --release --package web-ts --locked
mkdir -p secrets data
cargo run --release --locked -- init-key secrets/master.key
cp config.example.toml config.local.toml
# 按配置教程填写本地配置，再启动
cargo run --release --locked -- serve config.local.toml
```

Windows 工具链、测试与配置细节见 [部署指南](docs/DEPLOYMENT.md)。欢迎通过 [Issues](https://github.com/PTPHAP/WebTS/issues) 提交不含个人资料的反馈；贡献前阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。不要上传密码、身份文件、邮箱链接、数据库或实际部署配置。

---

<div align="center">

**WebTS · 让声音相聚**

[更新记录](CHANGELOG.md) · [文档中心](docs/README.md) · [MIT License](LICENSE) · [PTPHAP](https://github.com/PTPHAP)

<sub>独立第三方开源项目，与 TeamSpeak 官方无隶属关系。TeamSpeak 及相关标识归其权利人所有。</sub>

</div>


站点外观、图标、站内隐私政策/免责声明和安全HTML页脚见[自定义教程](docs/SITE-CUSTOMIZATION.md)；与原生客户端的差距和验收门槛见[适配清单](docs/CLIENT-COMPATIBILITY.md)。

好友私信需站长在后台“加密临时存储”配置私有S3兼容桶，未配置时禁用发送。普通聊天无需私信设备身份，临时消息可在同一账号的其他设备读取；端到端模式一次一台活动私信设备，不提供多设备历史同步。详见[加密边界和设置](docs/FRIENDS.md)。
