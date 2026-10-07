# 续开发交接

最后更新：2026-10-07。

## 阅读顺序

README.md → PROJECT_STATE.md → PROGRESS.md → REQUIREMENTS.md → ROADMAP.md → docs/ACCEPTANCE.md。

所有工作限定本项目目录，不探索或修改D:\幻时镜工作台中的其他项目，也不修改全局工具安装。

## 当前现场

- 公开仓库PTPHAP/WebTS，main起始历史来自用户原始提交；origin已设置。恢复时核实远端head及PROJECT_STATE.md发布状态，避免重复上传。
- 协议vendor子模块固定提交，raw-audio补丁仅改Cargo.toml、lib.rs、sync.rs；patches/raw-audio.patch保存可重现补丁。子模块呈dirty是已应用该补丁的预期状态。
- 项目内工具已安装，中文目录构建使用scripts/with-tools.ps1及linker.py，不改系统PATH，不迁移工程。Rust构建、10项测试、项目Clippy、格式检查已通过。
- 首批源码d51a154的Ubuntu 24.04 GitHub CI已全步骤通过，运行37641254191；后续代码改动仍需对应验证。
- server/src有identity/vault/db/password/config/probe与CLI实现。尚无React网站、HTTP认证、SMTP、WebRTC桥接或活动连接撤销，不能当作完整功能。
- .cache/servers含独立TS3 3.13.8和TS6 beta13.1服务文件，未启动；需要遵守对应服务许可与授权测试条件。
- 实际CLI验证产生的测试密钥/身份只在忽略的.cache/tmp，不输出或上传文件内容。构建日志位于.cache/check.log、tests.log、clippy.log、build.log；不作为真实语音证明。

## 下一步操作

1. 核验GitHub上传与Linux CI结果；保留用户仓库初始MIT署名和历史，不强推。
2. 用已获授权的独立测试服务验证握手、全局加密拒绝与变化断开，再实现Opus/WebRTC双向桥接。
3. 继续实现HTTP认证、SMTP、同源/限流、会话与活动TS连接取消，密码计算使用有界阻塞池。
4. 原生TS3/TS6身份导入与UID、双向语音、耳语、权限拒绝必须真实验证后才扩大完整网页。
5. 每次阶段变化同步状态/进度/路线/交接/验收，后续更新按照用户新要求调整。
