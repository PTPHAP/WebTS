# 安全与信任边界

## 必须保持的约束

- 验证邮箱、Argon2id密码哈希、可撤销安全Cookie；找回令牌短时且单次使用。
- 重置密码或退出全部设备立即撤销登录，并取消相应的活动TS连接。
- 每次身份读取和使用都检查当前登录用户归属；不存在“只填UID即可绑定”。
- 私钥密文使用AES-256-GCM认证加密，关联账号与身份元数据；部署主密钥不进数据库/Git/镜像。
- 服务端托管意味着站点运营者或被攻破的网关可能使用私钥。文件持有者可以取得对应TS身份权限。
- 禁止记录私钥、密码、恢复令牌、聊天内容和音频。协议库的原始命令/包调试输出在正式运行中关闭。
- WebRTC允许AES-GCM、禁止明文降级；TS服务器必须全局语音加密，普通音频和耳语同样验证。
- TS加密算法依实际协议实现验证，不把AES-128-EAX写成AES-256；网关是两段加密的终止点，可接触音频。
- 未配置SMTP时注册和找回不能假装发送成功。HTTP仅用于明确允许的localhost开发。

## 数据备份

数据库和主密钥分别保管。恢复必须同时具备正确主密钥；已有数据库缺密钥时应报错，不自动生成替代密钥破坏可恢复性。

## 安全报告

发布前补充私下报告渠道。请勿在公开Issue上传身份文件、密钥、数据库、恢复邮件链接或真实凭据。

## 依据

- [OWASP密码存储](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html)
- [OWASP账号找回](https://cheatsheetseries.owasp.org/cheatsheets/Forgot_Password_Cheat_Sheet.html)
- [OWASP加密存储](https://cheatsheetseries.owasp.org/cheatsheets/Cryptographic_Storage_Cheat_Sheet.html)
- [TeamSpeak身份说明](https://support.teamspeak.com/hc/en-us/articles/360002711518-How-does-the-TeamSpeak-3-user-Authentication-work)
