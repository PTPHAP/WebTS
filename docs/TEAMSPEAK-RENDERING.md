# TeamSpeak 频道显示与音乐机器人

## 原生频道树与格式

频道仍按服务器提供的前置同级频道顺序显示。顶层频道名称中的 `[cspacer01]`、`[lspacer]`、`[rspacer]`、`[*spacer]` 和 `[spacer]` 原生分隔符按居中、左、右或重复图案显示；不修改频道名称、ID、权限或顺序，频道仍可选择、查看介绍和加入。

频道介绍支持嵌套与跨行 BBCode：`b`、`i`、`u`、`s`、`sup`、`sub`、`color`、`size`、`left`、`center`、`right`、`url`、`img`、`hr`、`list` / `[*]`、`table` / `tr` / `th` / `td`。聊天和成员介绍采用官方 SDK 的简单标签集合，不解释 HTML。字号限制在 8–48px，颜色只接受十六进制或颜色名。未知标签保留文字；限制长度、节点数量和嵌套深度。

```text
[center][size=24][color=#00CCFF][b]欢迎来到频道[/b][/color][/size]
[img]ts3image://weixin.jpg?channel=1&path=/[/img]
[/center]
[list=1][*]请遵守规则[*]祝你玩得开心[/list]
```

## 频道图片与安全

`ts3image://文件名?channel=频道ID&path=/目录/` 和包含 `filename` 的原生长链接，通过当前已连接服务器读取。URL 中的主机不会成为新的网络目标；提供 `serverUID` 时必须与当前服务器一致。只接受当前可见频道介绍实际引用的图片；目标频道也必须可见，TS 继续检查当前身份的文件下载权限和频道密码。

支持 PNG、JPEG、GIF 和 WebP；GIF 显示首帧。默认输入最多2MiB、边长2048px，限制解码内存并重编码成最多192KiB的 PNG，去除元数据与附带内容。每连接最多2个图片传输，默认10秒内8次读取、32张网页缓存，每个介绍最多8张图；超时及拒绝均显示原因，可点击重试。缓存只在当前连接内，断开清空。路径穿越、重复参数、服务器根目录、SVG、HTML 和任意 HTTP 代理请求会被拒绝。管理员可在后台调整图片与头像限制，范围和热加载说明见[配置教程](CONFIGURATION.md#8-图片加载与头像限制)。

外部 HTTP/HTTPS 图片显示明确的打开链接，默认不会请求外部图片服务器；打开会让对方看到你的 IP。这与官方 TS6 限制外部图片以防 IP 跟踪的安全方向一致。当前不支持任意远程图片自动加载。

经典 TS3 文件传输本身不提供语音加密，频道图片只能用于可公开内容；浏览器与本站间仍使用 HTTPS/WSS。不是 myTeamSpeak 云图片或通用文件管理器。

## 音乐机器人触发“未加密语音”

`ZHANGTIANYAO1/teamspeak-music-bot` 当前锁定的 `@honeybbq/teamspeak-client` 0.2.2 直接把音乐包标记为 `Unencrypted` 并使用会话固定签名，而不是执行 AES-EAX 加密。在开启全局加密的独立 TS3 上已复现；同一发送器改为加密后，100个 OpusMusic 包通过严格加密探针。该验证是协议数据包验证，不代表真人音质或官方客户端全量验收。

WebTS 继续要求 TS 全局语音加密。收到明文包时只丢弃这些包，不再退出整个服务器；每5秒最多提示一次，其他加密语音继续正常处理。普通语音、音乐与耳语使用相同检查，绝不把明文包转发到浏览器。关闭/未知全局加密策略和浏览器套件不合格仍停止连接。

**要听到这类机器人的音乐，发送端也必须真正加密。** WebTS 无法补救机器人至服务器这一段已发送的明文。

临时兼容工具仅修改完全匹配的 SDK 0.2.2 发布文件，固定 SHA-256；其他版本拒绝修改。维护者可以优先升级到已修复发送端加密的上游版本。临时修复时，下载仓库源码，在机器人所在服务器停止机器人后执行：

```bash
# 默认只检查，不修改；路径换成实际机器人目录
python3 scripts/fix-music-bot-encryption.py --bot-root /opt/teamspeak-music-bot
# 确认机器人已停止，再应用并重新启动机器人
python3 scripts/fix-music-bot-encryption.py --bot-root /opt/teamspeak-music-bot --apply
```

工具保留 `handler-C_JhqGTd.js.webts-backup`；重新安装依赖可能覆盖修复。Docker 内须在实际运行镜像/依赖中应用并保留修改，单独改宿主机无效。未经授权不会修改其他项目或容器。无需在 WebTS 提交机器人 WebUI 密码、Query 密钥或管理权限。

## 依据

- [官方 SDK BBCode 标签定义](https://github.com/teamspeak/ts3client-pluginsdk/blob/master/include/teamspeak/public_rare_definitions.h)
- [官方社区：原生分隔符](https://community.teamspeak.com/t/spacer-with-heading/50424)
- [官方社区：外部图片限制与 ts3image 用法](https://community.teamspeak.com/t/img-tag-not-working-in-channel-description-since-6-0-0-beta4/64502)
- [音乐机器人源码](https://github.com/ZHANGTIANYAO1/teamspeak-music-bot/blob/main/src/ts-protocol/client.ts)
- [发送协议 SDK](https://github.com/HoneyBBQ/teamspeak-js)
