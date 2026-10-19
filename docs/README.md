# WebTS 使用与安装

先选自己要做的事，不需要把所有文档读一遍。

| 我要做什么 | 打开这一页 |
| --- | --- |
| 登录网站，连接频道，设置麦克风 | [使用说明](CLIENT.md) |
| 搭建WebTS网站 | [Linux / Docker](INSTALL.md) · [Windows](WINDOWS.md) |
| 已有网站，给TS服务器装语音节点 | [节点安装](NODE-INSTALL.md) |
| 改邮箱、TS地址、域名或后台配置 | [配置教程](CONFIGURATION.md) |
| 管理账号、封禁或发送通知 | [账号管理](ACCOUNT-ADMIN.md) · [信件通知](NOTIFICATIONS.md) |
| 发帖子、加好友或配置消息存储 | [社区](COMMUNITY.md) · [好友](FRIENDS.md) · [对象存储](OBJECT-STORAGE.md) |

网站服务器输入`webts`打开管理菜单；TS节点机器使用`webts-node`。两者安装位置与用途不同，普通网页使用者无需安装。

<details>
<summary>查看全部文档与开发资料</summary>

| 文档 | 内容 |
| --- | --- |
| [社区节日主题](SEASONS.md) | 内置主题预览与发布、成员偏好、深浅色、更新及回退 |
| [账号安全中心](ACCOUNT-SECURITY.md) | 本人登录会话、密码复核、退出其他设备、隐私及升级兼容 |
| [社区功能与诊断](COMMUNITY.md) | 社区首页、候选版帖子/评论/配图/举报、图片裁剪、账号举报、通知偏好、收藏与邀请二维码、TS 聊天工具、连接诊断 |
| [安装教程](INSTALL.md) | 一条命令、中文引导、证书、邮箱验证、站长开通、`webts` 菜单 |
| [配置教程](CONFIGURATION.md) | 域名/IP/端口、SMTP、默认 TS、自定义连接、网页后台热加载、TURN |
| [Docker 安装](DOCKER.md) | 一键中文引导、预构建镜像下载与校验、Compose、站长权限与更新 |
| [Windows 安装](WINDOWS.md) | 一次粘贴下载并校验运行包、中文初始化、HTTPS、停服备份与更新 |
| [同机语音节点安装](NODE-INSTALL.md) | Windows双击安装、可选UDP范围、主站配对、Linux原生包；按服务器区分端口 |
| [语音节点审查](SECURITY-REVIEW-VOICE-NODES.md) | 信任边界、安装回归、系统检查、真实语音与容量待验收范围 |
| [发布验收门槛](RELEASE-GATES.md) | 发布包检查、E2EE 转正式条件与后续顺序 |
| [1.1.0登录增量复核](SECURITY-REVIEW-1.1.0.md) | 审查范围、不变量、候选问题修复、测试证据与尚未完成的验收 |
| [部署与维护](DEPLOYMENT.md) | Docker、systemd、构建、反向代理、备份恢复与 Windows 开发 |
| [备份与恢复验收](BACKUP-RECOVERY.md) | 一致性快照、原身份恢复、旧会话撤销、隔离验收与私信灾备边界 |
| [支持范围与常见问题](SUPPORT.md) | 平台、浏览器、TS3/TS6、已实现功能与验证边界 |
| [语音与头像](CLIENT.md) | 降噪、回声、按键发言、连接统计与服务器头像 |
| [界面与自由布局](WORKSPACE.md) | 多栏/紧凑视图、模块排序、调宽、账号同步与动画 |
| [原生频道显示与音乐](TEAMSPEAK-RENDERING.md) | spacer、BBCode、ts3image、音乐机器人加密兼容 |
| [首页与个人资料](HOME-PROFILE.md) | 首页、登录入口、图片公告、固定头像与 TS 资料同步 |
| [AFK 与账号管理](ACCOUNT-ADMIN.md) | 离开状态互通、封禁/解封、强制退出、网站管理员与操作记录 |
| [联合观赛](WATCH.md) | 免费直播链接、公开标题/封面、观赛语音、手动赛况及签名接口 |
| [信件与通知](NOTIFICATIONS.md) | 管理员专页、HTML/Markdown、裁剪图片、站点信箱、更新邮件订阅与撤回 |
| [好友与加密私信](FRIENDS.md) | 社区资料、悬浮/停靠好友、设备密钥、双棘轮、阅后即焚 |
| [雨云与对象存储配置](OBJECT-STORAGE.md) | S3 API、Region默认值、读写删除检测、同桶修正和迁移保护 |
| [身份互用](IDENTITIES.md) | UID、导入/导出、多身份与 TS 原生客户端 |
| [隐私政策](../PRIVACY.md) | 收集的数据、用途、接收方、保留与用户控制 |
| [安全说明](SECURITY.md) | 加密、信任边界、会话、权限与部署密钥 |
| [架构](ARCHITECTURE.md) | 浏览器、网关、TS、数据库与接口的关系 |
| [依赖](DEPENDENCIES.md) | 工具链、锁文件、协议库与补丁 |
| [第三方许可](../THIRD_PARTY_NOTICES.md) | 依赖、音频组件与原始许可 |
| [贡献指南](../CONTRIBUTING.md) | 本地检查、变更约定与反馈 |
| [更新记录](../CHANGELOG.md) | 面向使用者的功能更新 |

| [便捷登录与陌生环境](QUICK-LOGIN.md) | 用户名、手机扫码与公共设备 |

</details>

[返回项目首页](../README.md) · [查看已发布版本](https://github.com/PTPHAP/WebTS/releases/tag/v1.1.1)
