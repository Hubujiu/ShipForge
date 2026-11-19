# GitHub 与 GitLab 仓库

在 Windows 打开「集成」，选择 GitHub、GitLab.com 或自建 GitLab。组织连接决定能够配对哪些仓库。成员在同一页连接自己的账号后，可以用个人身份回复来源 Issue 和同步处理人。[HiFox 的 GitHub 行为](https://help.hifox.com/docs/integration/github) · [HiFox 的 GitLab 行为](https://help.hifox.com/docs/integration/gitlab)

## 组织与个人授权

| 来源 | 服务部署和首次连接 | 身份与范围 |
| --- | --- | --- |
| GitHub | 注册 GitHub App，启用用户授权。服务端设置以下环境变量，管理员在「组织授权」中安装 App、选择仓库、完成用户授权，再提交浏览器回调网址和安装 ID。 | 服务校验 OAuth 用户能看到该安装、App ID 匹配，且该用户是安装账户、签名安装事件记录的安装者或 GitHub 组织 Owner。App 的 RSA 密钥签发 JWT 并换取短期安装令牌。 |
| GitLab.com | 注册 OAuth 应用、授予 `api`，登记回调网址。分别完成组织与个人 OAuth。 | 服务用 PKCE 校验授权码。组织令牌只能列出其具有 Maintainer 或更高权限的项目。授权方提供刷新令牌时，服务在访问令牌过期前刷新。 |
| 自建 GitLab | 组织管理员填写实例的 HTTPS 根地址和具有 `api` scope 的 PAT。成员随后在同一页输入自己的 PAT。 | 服务读取当前用户及 PAT 自省接口，检查所属用户、有效状态和 scope。Windows 显示凭据到期时间，临近到期可直接更新。组织服务所在网络须能访问此实例。 |

GitHub 配置 `SHIPFORGE_GITHUB_APP_ID`、`SHIPFORGE_GITHUB_APP_SLUG`、`SHIPFORGE_GITHUB_CLIENT_ID`、`SHIPFORGE_GITHUB_CLIENT_SECRET`、`SHIPFORGE_GITHUB_PRIVATE_KEY_FILE`、`SHIPFORGE_GITHUB_REDIRECT_URI`、`SHIPFORGE_GITHUB_WEBHOOK_SECRET`。为 App 配置 Issues 读写、Pull requests 读取、Contents 与 Metadata 读取，以及查询组织成员身份所需的 Members 读取权限。订阅 Issues、Issue comments、Pull requests、Push，保留安装与仓库范围变更事件。将 GitHub App Webhook 设为团队服务的 HTTPS 地址加 `/v1/forge/github`。[GitHub App 安装令牌](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-as-a-github-app-installation) · [安装后验证用户访问范围](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/about-the-setup-url)

GitLab.com 配置 `SHIPFORGE_GITLAB_CLIENT_ID`、`SHIPFORGE_GITLAB_CLIENT_SECRET`、`SHIPFORGE_GITLAB_REDIRECT_URI`。两个 OAuth 应用都可以登记服务的 `https://你的域名/v1/forge/oauth/callback`。授权成功后，复制浏览器地址栏里的完整网址到同一成员的 Windows 表单。每次申请有效十分钟，紧接着发起的新申请替换旧申请。[GitLab OAuth](https://docs.gitlab.com/api/oauth2/) · [自建 PAT 自省](https://docs.gitlab.com/api/personal_access_tokens/)

本机配置使用当前 Windows 用户的凭据存储。连接团队服务后，授权处理和凭据存放在服务主机。组织及个人凭据独立更新。同一组织、平台和实例的一个来源账户只可关联一个成员。同一成员连接另一个 GitLab 实例时，会替换原来的 GitLab 个人绑定。更换组织连接的实例或安装，先解除原组织连接。

## 配对仓库

选择当前空间，打开「集成 → 平台 → 选择代码仓库」，在仓库下点「配对到此空间」。管理员必须同时具有组织与空间管理权。入站配对可有多个，同一空间和平台可启用一个双向仓库。把私有空间内容发往公开仓库时，页面要求明确勾选确认。私有仓库默认启用来源回链，公开仓库默认关闭，PR/MR 与提交关联分别设开关。

GitLab 配对后复制 Webhook 路径和密钥，在该项目的 **Settings → Webhooks** 中填写完整 HTTPS 地址，勾选 Issue、Comment、Push 和 Merge request 事件。每个项目配对拥有独立密钥。GitHub 使用 App 的共享回调，由签名、安装 ID 与仓库 ID 共同确定目标。[GitLab 项目 Webhook](https://docs.gitlab.com/api/project_webhooks/) · [GitHub 事件载荷](https://docs.github.com/en/webhooks/webhook-events-and-payloads)

## Issue 与评论

- 配对后新建的来源 Issue 建成任务，既有 Issue 的编辑不会触发导入。开启分拣且满足空间来源与类型条件时，任务进入待分拣。已配对 Issue 的关闭、重开、标题、描述和处理人变化更新任务。已有机器端 `github-sync` 仍提供独立的显式历史拉取入口。
- 任务详情的「GitHub 来源」「GitLab.com 来源」「自建 GitLab 来源」展示仓库、Issue 号、原链接及来源评论。已绑定的作者显示对应成员。未知处理人的平台 ID 保存在任务上。解除任务或仓库配对会停止同步，并保留已有任务、来源记录与远端 Issue。
- 双向仓库的「回复到来源」只发送该回复。用普通任务评论框可继续内部讨论。「推送任务更改」显式发布标题、描述、开闭状态和可映射的处理人。「刷新来源」先读取已配对的 Issue。远端版本变化会阻止覆写，并提示先刷新。如果来源拒绝所选处理人，操作会报告冲突。
- 来源评论优先使用个人授权。个人授权缺失或来源明确拒绝个人权限时，使用组织 GitLab 身份或 GitHub App 机器人发送，回执显示身份。响应丢失时，重试原请求编号只核对目标 Issue 中的署名和评论标记。查不到确认时保留待确认状态。
- GitHub 对公开仓库 PR 检查作者关联。PR/MR 和 Commit 按任务编号关联；GitHub PR 合并仅记录活动。来源回链发送到对应 Issue 或 PR/MR。应用重新核验仓库访问范围后才加载来源事件或外发。GitHub 安装暂停、删除或移除仓库后，该配对停止。

## 验收范围

隔离回归启用临时 GitHub RSA 密钥、模拟用户 OAuth、GitLab.com 与自建 GitLab HTTP 服务，使用 Owner 和两名成员验证授权范围、身份冲突、历史边界、方向、回退身份、重复投递、关闭重开、凭据撤销及保留任务。Windows 隐藏 D3D 窗口覆盖三个平台入口，并在自建 GitLab 表单中实际输入令牌、选择项目、提交双向配对、从任务详情发送来源回复和解除授权。[HTTP 回归](../scripts/verify-forge.py) · [Windows 控件回归](../crates/shipforge-desktop/src/repository_ui_test.rs)

真实 GitHub 私有安装、企业组织审批、GitLab.com 真实租户与客户自建实例的网络路由仍待授权联调。选择一台在线 Agent 机器代理自建实例、组级共享 Webhook、组织外部身份管理目录和历史处理人回补仍待实现。任务改动向外发布采用显式推送。当前来源讨论针对 Issue；MR 讨论线程与按企业工作流映射 MR 状态仍待扩展。
