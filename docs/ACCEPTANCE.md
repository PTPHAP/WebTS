# 验收记录

当前阶段：协议验证及基础实现。未进行真实 TS3/TS6 原生客户端验收，不代表可生产交付。

必须记录具体版本、设备、服务器配置、加密套件和测试时间。演示、自动负载、真实原生客户端通话分别记载。

| 必须通过的项目 | 状态 |
| --- | --- |
| Rust基础构建 | Windows Rust 1.99.0 check/build通过；Ubuntu 24.04 Linux CI通过 |
| 前端构建 | 尚未建立前端，不记为通过 |
| 身份创建/导入/导出UID一致 | 库内INI及规范表示往返测试通过；原生客户端导入未验证 |
| 账号、恢复令牌、身份归属和密文安全测试 | 10项自动测试通过；HTTP/邮件/活动连接流程未实现 |
| 真 SMTP 注册与密码找回邮件 | 等待 SMTP 配置 |
| TS3 双向语音、耳语、强制加密、拒绝权限 | 等待独立测试服务器 |
| TS6 双向语音、耳语、强制加密、拒绝权限 | 等待独立测试服务器 |
| Chrome/Edge/Firefox 原生客户端互通 | 待执行 |
| 50 连接/10 发言/30 分钟性能 | 待执行 |
| 公网 HTTPS、受限网络 TURN | 待执行 |
| GitHub开发源码 | 首批提交d51a154已上传，远端head核验一致；未创建正式发布标签 |
| Linux发布包、容器 | 待实现，未验收 |

## 2026-10-07 · 本地基础验证

环境：Windows，项目目录D:\幻时镜工作台\Web TS；Rust 1.99.0 GNU、w64devkit 2.10.0/GCC 16.2.0、Rust配套LLVM链接器、Python 3。依赖固定于Cargo.lock，协议子模块提交见THIRD_PARTY_NOTICES.md。

- cargo check --workspace --locked --offline：通过。
- cargo build --workspace --locked --offline：通过。
- cargo test --workspace --locked --offline：10 passed、0 failed，执行约1.16秒（不含首次编译）。
- cargo clippy --package web-ts --no-deps --locked --offline -- -D warnings：项目代码通过。
- cargo fmt --package web-ts -- --check：通过。
- raw-audio补丁反向检查：通过；仅改动3个必要协议库文件。
- 实际CLI运行：创建密钥、重复创建拒绝覆盖且文件校验值不变、创建身份、无效身份拒绝且未生成成功报告，均通过。

测试覆盖：真实随机身份UID/计数往返、无效与纯公钥导入拒绝、重复UID去重、跨账号读取/修改/删除拒绝、默认身份唯一及删除回退、密文篡改/错密钥/账号或行元数据替换拒绝、随机nonce不同、找回链接过期/用途/重复使用、重置撤销旧会话且保留身份、旧密码验证与重置竞态、未验证会话拒绝、注册旧链接失效、Argon2id随机盐/错误密码/异常内存参数拒绝、HTTP开发监听范围与重复目标拒绝。

上游tsproto-packets产生13条future_incompatible同名方法警告，当前编译通过。未修整无关第三方代码。密码哈希将来必须经有界阻塞计算队列调用；当前尚无HTTP调用路径。

不包含真实SMTP、原生客户端、WebRTC协商、语音延迟或容量证明。探针仅接收与统计包，不实现双向发言。

## 2026-10-07 · Linux自动检查

代码提交：`d51a1545841224a959f11e6a2ef6c588d756048a`。环境：GitHub Actions ubuntu-24.04，Rust 1.99.0。

[检查运行37641254191](https://github.com/PTPHAP/WebTS/actions/runs/37641254191)已核验job及全部step为success：固定工具链安装、递归子模块、协议补丁、格式、cargo check、cargo test、项目Clippy。Linux基础构建门槛通过；尚未制作和验收正式Linux发布包。

性能参考：Linux 2 核/4GB，网关 RSS ≤1GB，平均 CPU ≤1.5核，RTT ≤30ms时语音延迟 P95 ≤300ms。TS6 单服按实际许可容量验收；50 是多目标网关总容量。
