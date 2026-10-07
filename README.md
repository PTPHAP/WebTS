<div align="center">

# WebTS

**把你的 TeamSpeak，带进浏览器。**

自托管 · 真实身份 · 权限继承 · 加密语音

[![License: MIT](https://img.shields.io/badge/License-MIT-8b5cf6.svg)](LICENSE)
[![Rust checks](https://github.com/PTPHAP/WebTS/actions/workflows/check.yml/badge.svg)](https://github.com/PTPHAP/WebTS/actions/workflows/check.yml)

[部署指南](docs/DEPLOYMENT.md) · [身份互用](docs/IDENTITIES.md) · [安全说明](docs/SECURITY.md) · [参与贡献](CONTRIBUTING.md)

</div>

---

WebTS 是面向 TeamSpeak 3 / 6 的开源网页客户端。用邮箱登录，管理自己的多个 TeamSpeak 身份，让社区成员通过浏览器加入已有服务器。

> **首版正在制作。** 当前公开代码包含身份与账号安全基础、协议探针；完整网页、邮件流程和双向语音还未交付。可用功能与启动方式将随验证结果更新。

## 为你的社区而设计

| 体验 | 首版目标 |
| --- | --- |
| 浏览器加入 | 中文界面、深浅主题，频道、聊天和成员同屏显示 |
| 一个账号，多个身份 | 邮箱注册与找回，导入、创建、命名、切换和导出身份 |
| 原有权限继续使用 | 以真实TS身份连接，权限由目标服务器决定 |
| 清晰的语音控制 | Opus、按键发言、语音激活、设备选择、静音、耳语 |
| 自己掌握数据 | 单进程Rust网关、SQLite、本地密钥，便于部署与备份 |

## 加密，讲清楚边界

身份私钥采用 **AES-256-GCM** 加密托管。网站运营者持有解密能力，账号密码重置不会改变TS身份。

浏览器至网关使用 HTTPS/WSS 与 WebRTC DTLS-SRTP；目标设计优先 AES-256-GCM，兼容下限 AES-128-GCM。网关至TeamSpeak使用原生协议加密，当前固定协议库为 **AES-128-EAX**。必须开启服务器全局语音加密，不能确认时拒绝语音。

这属于两段传输加密：网关能够接触语音内容。不会把它标成端到端加密或全链路AES-256。[了解安全边界 →](docs/SECURITY.md)

## 本地开发

```sh
git clone --recurse-submodules https://github.com/PTPHAP/WebTS.git
cd WebTS
sh scripts/prepare-vendor.sh
cargo test --workspace --locked
```

Windows开发、配置和协议探针见[部署指南](docs/DEPLOYMENT.md)。不要把身份文件、密钥、数据库或真实配置提交到仓库。

## 项目与许可

React · TypeScript · Rust · Tokio · Axum · SQLite · Opus · WebRTC

MIT许可，见[LICENSE](LICENSE)。第三方许可保留各自声明，见[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。WebTS是独立第三方客户端，与TeamSpeak官方无隶属关系。
