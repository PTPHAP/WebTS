# GitHub仓库创建与发布

## 已确认仓库

用户已建立公开仓库：[PTPHAP/WebTS](https://github.com/PTPHAP/WebTS)，默认分支main，MIT许可。沿用用户选择的WebTS名称及LICENSE中的Nicole Reeyn署名。

本地origin已关联该仓库。远端初始提交为`0f62154aed24485cd43a8c91760be96a3ef4605a`，包含README与LICENSE；后续提交在此基础上增加项目内容，不覆盖历史。

## 创建时的建议项（历史说明）

- Repository name：web-ts
- Description：自托管的 TS3/TS6 网页客户端，支持加密语音、邮箱账号、多身份管理与权限继承。开发中。
- Visibility：Public
- Initialize README：不勾选
- Add .gitignore：None
- Choose a license：None（创建空仓库；本地LICENSE已提供实际MIT许可，首次推送时上传）

[打开GitHub新建仓库](https://github.com/new)。仓库所有者选择自己的账号。若web-ts名称已存在，不覆盖原仓库，先核对已有内容。

MIT允许使用、修改、再分发和商业使用，需保留版权及许可声明；项目按现状提供。第三方协议库及依赖保留各自许可，不将其作者成果改署为本项目。

## 发布门槛

首次源码发布可以标记为开发中，不能冒称完整正式版本。只上传源代码、必要文档、补丁和锁文件；不上传.tools、.cache、身份.ini、数据库、部署密钥、真实本地配置及发信凭据。

推送前检查暂存文件和秘密泄露，确认补丁可重现。只上传开发快照，不创建正式发布标签；发布成功以远端提交核验为准。实际发布状态见PROJECT_STATE.md与PROGRESS.md。

## 官方参考

- [创建仓库](https://docs.github.com/en/repositories/creating-and-managing-repositories/creating-a-new-repository)
- [MIT说明](https://choosealicense.com/licenses/mit/)
