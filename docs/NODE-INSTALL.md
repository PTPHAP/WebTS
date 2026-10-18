# WebTS 同机语音节点 · 反向连接安装

**Windows 1.1.0-node-preview.4 开发预览**，增加双击中文安装、自定义语音端口和升级检查。兼容已部署的preview.2反向主站；旧preview.1公网节点HTTPS模式不适用。真实TS通话、Windows Server和长期容量仍须验收，不是稳定发行认证。

节点装在TS同一台机器，主动WSS连接现有主站；浏览器加密语音直达节点，再经本机连接TS。用户无需安装插件。节点能接触当前TS身份私钥和声音，须由可信运营者管理。

下载：[Windows节点ZIP](https://github.com/PTPHAP/WebTS/releases/download/v1.1.0/webts-node-1.1.0-preview.4-windows-x64.zip) · [Windows校验文件](https://github.com/PTPHAP/WebTS/releases/download/v1.1.0/webts-node-1.1.0-preview.4-windows-x64.zip.sha256) · [Linux节点归档](https://github.com/PTPHAP/WebTS/releases/download/v1.1.0/webts-node-1.1.0-preview.4-linux-x64.tar.gz) · [同版发布说明](https://github.com/PTPHAP/WebTS/releases/tag/v1.1.0)。附件只在完整CI通过后发布；不要用其他版本的程序搭配此安装器。

## 地址和端口

**先分清两台机器：WebTS主站负责网站；TS节点机器负责语音。** 新增公网语音规则加在**TS节点机器**，不是主站。

| 哪台机器 | 方向与端口 | 是否需要新增 |
| --- | --- | --- |
| TS节点服务器的云安全组 | 入站 UDP 50000–50099 | 需要；已有同等规则则不重复添加 |
| 同一台TS节点服务器的Windows防火墙 | 入站 UDP 50000–50099，仅节点程序 | 双击安装器自动处理 |
| WebTS主站服务器 | 入站 TCP 443 | 沿用网站现有HTTPS，不为此节点再新增UDP范围 |
| TS节点服务器 | 出站到主站HTTPS端口（默认TCP443） | 默认允许出站时无需新增规则 |
| 用户手机、电脑 | 不手动开放入站端口 | 无需配置 |

以上语音范围以节点已有配置为准，安装窗口会显示实际值。主站使用非443的HTTPS时，节点出站跟随该实际端口。节点控制TCP8095只用于回环；不为本次节点向公网放行。TS原来UDP9987和文件TCP30033的规则保持；同机节点通过回环访问它们，不要求新增公网规则。

| 配置位置 | 内容 | 用途 |
| --- | --- | --- |
| 节点 main_url | https://你的WebTS主站域名 | 主动连出到现有主站 |
| 节点 bind | 127.0.0.1:8095 | 本机控制监听，禁止公网开放 |
| 节点 target | 127.0.0.1:9987 | 本机TS实际UDP端口 |
| 节点 file_transfer_port | 30033 | 本机TS实际文件TCP端口 |
| 节点 [rtc].public_ip | TS机器真实公网IPv4 | 浏览器语音地址，不能填127.0.0.1或域名 |
| 节点 [rtc].udp_min/udp_max | 50000 / 50099 | 云安全组及系统防火墙放行此UDP范围 |
| TS机器出站 | 主站443/TCP及必要DNS/时间服务 | 已认证WSS，不需要节点公网80/443 |

127.0.0.1指当前机器自己。TS目标和控制监听使用回环地址，浏览器直达使用公网IP。用户能看到语音IP，不公开管理入口不等于隐藏服务器IP，机器认证也不能防DDoS。UDP受限网络可能需要另配经过认证的TURN，须核对实际中继位置。

节点不需要自己的域名、HTTPS证书或Caddy；主站原有HTTPS仍须有效。地区和服务商要求仍由部署者确认，本架构不构成备案义务判断。不要改变其他网站、TS、Query或整机安全策略。

## 1. 主站管理员准备

先更新自有主站至同批源码，备份配置，为每节点生成独立随机密钥，不复用master.key：

```sh
./web-ts init-key secrets/community-node.key
```

限制为主站服务账号可读；Docker使用只读挂载，不能打进镜像。在已有服务器上添加绑定，不重复添加ID、不覆盖整份配置：

```toml
[[servers]]
id = "community"
name = "我的TeamSpeak"
address = "ts.example.com:9987"

[[voice_nodes]]
id = "community-node"
server = "community"
reverse = true
key_file = "secrets/community-node.key"
```

后台服务器ID和地址须与绑定一致，变化时明确拒绝，不自动改回香港。绑定属于启动配置，安排维护重启自有WebTS生效；未绑定服务器保持原路线。

后台“基础配置 → 同机语音节点”重验管理员密码，生成5分钟一次性配对码。不要放进参数、聊天、截图、日志或Git。重新生成使该节点旧码失效；退出、撤销会话、封禁或取消管理员权限使未领取码失效。已成功配对不依赖管理员后续登录；撤销节点信任须停止节点、轮换两侧密钥并重启主站。

## 2. Windows Server / Windows x64

### 推荐：双击安装，不用敲命令或编辑配置

1. 核对ZIP与旁边.sha256，把**整个包**解压到本地专用临时目录。不要直接在ZIP里运行，不要与网站源码或其他服务混放。
2. 双击 **`开始安装.cmd`**（双击 `webts-node.cmd` 也一样），在Windows提权提示中由维护者确认。这是中文问答窗口，不需要输入命令。
3. 安装位置默认 `C:\WebTS-Node`，升级时选择原节点目录。填写主站网址、公网IPv4或解析到本机的域名。已有正确配置会作为默认值，回车沿用。TS默认UDP9987、文件TCP30033，网页语音默认UDP50000–50099；可选自定义TS端口及网页语音UDP起止范围，界面分别说明用途。范围至少容纳节点配置的连接上限，修改后云安全组也须同步。节点ID沿用包内或已有配置，必须与主站绑定一致。
4. 确认安装摘要。向导自动核对包内文件、备份配置和密钥、识别并停止**本目录的节点**，然后提示去主站后台生成配对码并隐藏输入。不会让你先手动停止节点，不会按名称批量杀进程。
5. 自动检查配置、设置专用读取权限、放行**此节点程序对应的语音UDP入站范围**、创建低权限开机任务并启动节点。窗口显示本机检查结果及仍需在云安全组放行的准确规则。

向导只管理此专用目录；发现其他文件、链接、同名但归属不同的任务，或其他程序占用控制端口时会停止并说明原因。既有节点进程必须使用本目录程序及完整配置路径；无法安全确认的旧启动方式不会被强行结束。配对失败保持节点停止，旧文件备份保留在 `backups`，重新双击可重试，不需要改配置文件。首次必须输入配对码；已有成功配对且主站密钥未变化时可回车保留。

节点任务名为 `WebTS-Node`，使用Windows内置 **LOCAL SERVICE**（S-1-5-19），非管理员，不保存你的服务器密码。该账号仅获得节点目录执行/读取及配置、配对密钥读取权限，没有写权限。LOCAL SERVICE是共享系统身份，其他以同一身份运行的本机程序也在这一信任边界内；如需每服务独立身份隔离，按手动方式配置专用账号。备份目录不授予该账号。只备份可归属的任务XML，不保存旧任务密码。

**云安全组无法仅靠本机程序自动修改。** 默认添加Windows防火墙的程序专用入站 `UDP 50000–50099`，云控制台也须放行相同入站UDP范围；8095/TCP仍只监听回环，不需要公开80/443或新增广州HTTPS。若云安全组已放行，无需重复添加。受限出站环境需管理员单独允许主站HTTPS和正常DNS；向导不放宽整机出站策略。

这是单机本地检查，不证明主站在线或公网语音成功。安装后查看主站后台节点在线，再选择绑定该节点的服务器进行小规模双向通话验收。没有自动更新主站、切换默认服务器或创建云安全组。

### 维护命令（可选）

双击入口适用于普通安装；需要手动维护时仍保留：

```powershell
.\webts-node.cmd install    # 与双击开始安装相同
.\webts-node.cmd init       # 初始化，不覆盖配置/密钥
.\webts-node.cmd configure  # 旧手动配对；先自行停止节点
.\webts-node.cmd check
.\webts-node.cmd start      # 手动前台运行；不要与开机任务同时启动
```

本地Windows11检查与CI Windows Server系统检查分别记录；CI新增真实任务、LOCAL SERVICE和防火墙验收步骤，最终结果以对应提交CI为准，不能替代目标服务器通话验收。手动使用专用运行账号时，任务程序为 `C:\WebTS-Node\web-ts.exe`，参数 `serve-node "C:\WebTS-Node\node.local.toml"`，起始目录 `C:\WebTS-Node`；自行授予配置和密钥读取，维护账号保留写权限。程序是控制台节点，不直接用 `sc create` 注册为Windows服务。

旧包迁移：把新版解压到**另一个临时专用文件夹**并双击安装，安装位置选原节点目录。向导校验新版、备份并停止可确认归属的旧节点后，自动复制新版公开程序、脚本和文档，**保留原 `node.local.toml`、`secrets`、`backups`**；不必先覆盖运行中的EXE。旧节点单独创建的Caddy与80/443规则，只有确认独占后才手动停用；安装器不操作它们、不清空防火墙或回滚整机安全策略。原TS域名不变。

## 3. Linux x64

本轮GitHub节点包在Ubuntu24.04构建，按glibc2.39及以上部署；不适用于Alpine/musl或未经验证的旧glibc。BUILD-INFO记录具体构建环境。旧本地Zig预览的glibc2.36基线不适用于新的CI包；Debian12等环境可从源码构建或使用经过验证的容器，不能仅换标签冒充兼容。Linux真实TS通话仍须现场验收。

```sh
sha256sum webts-node-1.1.0-preview.4-linux-x64.tar.gz
# 核对后解压到新目录，不覆盖旧密钥
tar -xzf webts-node-1.1.0-preview.4-linux-x64.tar.gz
cd webts-node-1.1.0-preview.4-linux-x64
sh scripts/node.sh init
# 编辑node.local.toml，核对节点ID、TS/文件端口与UDP范围
sh scripts/node.sh configure
sh scripts/node.sh check
sh scripts/node.sh start
```

不使用sh -x运行向导。初始化umask077，普通用户可前台运行。systemd先创建webts-node专用用户/组，目录root:webts-node/750、配置与密钥root:webts-node/640、密钥父目录750、程序750。核对deploy/webts-node.service再安装服务，不同时运行前台和服务。向导由维护账号执行，运行账号不需写权限。

Docker普通bridge内127.0.0.1是容器自身。Linux用本批镜像时须核对host网络、最小权限和配置/密钥只读挂载；Windows Docker Desktop不在本批验收范围。

## 4. 上线门槛

1. 主站后台控制通道在线，8095仅回环，无节点公网HTTPS。在线不代表声音已通过。
2. 临时身份真实连接TS，核对UID、原生权限、头像和频道图；TS启用全局语音加密。
3. 原生TS与网页双向语音、耳语，记录实际加密套件及ICE选中地址，确认媒体没经香港TURN绕路。
4. 手机/电脑、退出、账号撤销、VPN/网络切换、主站/节点重启能回收身份并恢复。黑洞清理有超时，不是瞬时。
5. 测量延迟、丢包和资源，再逐步扩大。本批未完成50连接/10路语音或长期容量验收。

节点不复制账号库，连接身份仅在运行内存使用；宿主机和运营者仍属信任边界。图片限制暂独立配置[image_limits]，不随主站热加载。包仅含公开程序、模板、教程及许可证，不含部署配置、用户数据、密钥或进度。详情见[技术说明](VOICE-NODES.md)和[审查说明](SECURITY-REVIEW-VOICE-NODES.md)。
