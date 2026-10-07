# 构建与部署

当前提供开发工具和协议探针，尚无网站启动命令或生产发布包。下列命令的实际验证结果见docs/ACCEPTANCE.md。

## Windows开发

在项目根目录的PowerShell中执行：

```powershell
.\scripts\bootstrap-rust.ps1
.\scripts\prepare-vendor.ps1
.\scripts\with-tools.ps1 cargo check --workspace --locked
.\scripts\with-tools.ps1 cargo test --workspace --locked
.\scripts\with-tools.ps1 cargo fmt --package web-ts -- --check
```

需要Python 3可在命令路径使用。工具固定Rust 1.99.0 GNU和w64devkit 2.10.0（GCC 16.2.0），不修改系统PATH。为兼容中文目录及空格，使用Rust自带LLVM链接器、配套运行库和scripts/linker.py，将项目内链接路径转换为相对路径。不要对vendor运行全量格式化；仅格式化web-ts包。

## Linux开发

```sh
git clone --recurse-submodules https://github.com/PTPHAP/WebTS.git
cd WebTS
sh scripts/prepare-vendor.sh
cargo check --workspace --locked
cargo test --workspace --locked
```

需要Rustup及C编译器（SQLite以bundled模式编译），rust-toolchain.toml固定工具版本。Linux流程待CI及实际发布环境验证。

## 本地协议探针

这属于开发人员的协议验证工具，不是绕过网站登录的公开连接接口。它不发出语音，只检查握手、全局加密策略并统计收到的普通语音和耳语包。

```powershell
New-Item -ItemType Directory -Force secrets,data
.\scripts\with-tools.ps1 cargo build --locked
.\.cache\target\debug\web-ts.exe init-key secrets/master.key
.\.cache\target\debug\web-ts.exe create-identity secrets/probe.ini
.\.cache\target\debug\web-ts.exe probe 127.0.0.1:9987 secrets/probe.ini data/probe-report.json 10
```

只使用获授权的测试地址；示例localhost并未随项目自动启动服务器。密钥、身份和报告采用新建模式，已存在时拒绝覆盖。Unix新建秘密文件权限为0600；Windows请将secrets目录访问权限限制到部署账号。真实身份文件不要上传GitHub。

报告中的AES-128-EAX是固定协议库源码算法声明，不是独立密码审计或端到端加密证明。握手/收到包数量不能代替双向通话、耳语、权限拒绝或原生UID验证。

## 本地配置

以config.example.toml为模板复制为config.local.toml。真实配置已被Git忽略。

- servers：仅填写获授权的TS测试地址和端口，不运行任意地址网关。
- TS语音加密：服务器设为Globally on，测试频道使用Opus。
- SMTP：TLS发信服务、from、用户名及单独密码文件；不要在聊天或仓库粘贴密码。
- master_key_file：部署密钥文件，独立于数据库备份。
- RTC：公网IP和UDP范围；受限网络配置带认证TURN。

## 目录约束

便携Rust/Cargo/编译器在.tools/；依赖、编译输出和临时文件在.cache/；前端依赖在web/node_modules/。均不提交Git。

## 正式部署门槛

Linux发布包、Docker Compose、HTTPS/SMTP/TURN以及备份恢复须实际复现。当前尚未提供可运行生产版本。

不要对生产TS服务器压测。单服容量须满足实际许可；TS6测试版容量限制另行核实记录。
