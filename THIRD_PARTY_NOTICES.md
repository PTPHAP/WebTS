# 第三方声明

## tsclientlib

原项目ReSpeak/tsclientlib（Flakebi及贡献者），本项目固定使用Moepchi维护分支。

- 来源：https://github.com/Moepchi/tsclientlib/tree/webspeak3
- 固定提交：2e7794928f18d5aef2da5b48f420f9c801b1112c
- 许可：MIT OR Apache-2.0，本项目复用按MIT条款，保留vendor中的完整版权与许可文件。
- 本地修改：patches/raw-audio.patch，开放原始Opus包，禁止网关模式启动不可取消的自动身份等级计算，让网关检查后拒绝未加密音频，并修复导入ASN.1身份时丢失私钥前导零的问题；不修改加密算法。

其声明子模块同样保留来源与许可。锁文件内各平台依赖的版本及包作者许可声明见[依赖清单](docs/DEPENDENCIES.md)，共414个包（包含本项目）。发布二进制前仍需复核实际打包许可文本与NOTICE。

本项目为第三方兼容客户端，不代表TeamSpeak官方产品、授权或背书。


## 浏览器降噪

固定 @sapphi-red/web-noise-suppressor 0.4.1（MIT），底层 @shiguredo/rnnoise-wasm 2022.2.0（Apache-2.0）、RNNoise 2022.1.0（BSD-3-Clause），SIMD检测 wasm-feature-detect 1.9.0（Apache-2.0）。完整原文与作者归属保存在 [licenses/audio](licenses/audio/ATTRIBUTION.md)，发布包和Docker均包含。WebTS补充Worklet初始化成功/错误消息；可选人声模式使用现有模型人声概率，在降噪后应用10ms回看、句尾保留和渐变，未修改底层WASM模型。

键盘增强复用相同固定包的 GTCRN：@sapphi-red/gtcrn-wasm 0.0.3 与 GTCRN 模型（MIT），PFFFT/FFTPACK（UCAR许可）；生成器 onnx2c 的原文许可与作者表也随包保存。来源提交及完整许可见上述音频归属目录。GTCRN 内部16kHz语音处理、48kHz接口重新采样；不将其描述为回声消除算法。

tsclientlib额外补丁限制头像TCP只访问已验证TS服务器IP、仅处理未过期且单次匹配的主动请求，设置连接超时；不修改加密算法。MD5仅用于TS原生头像版本标识，不用于密码或安全签名。
