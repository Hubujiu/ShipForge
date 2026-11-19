# 工单与即时通信连接器

ShipForge 把工单事件和普通用户文字消息转换成当前空间的任务。下列样例全部使用合成数据。[GitLab、Jira、飞书、Slack 和 Telegram 样例](tests/fixtures/inbound.json) · [Linear、Asana、Teams、Discord、企业微信和钉钉样例](tests/fixtures/inbound-extended.json)

| 配置中的 provider | 接收内容 | 外部对象标识 | 投递去重 | 可选来源限制 |
| --- | --- | --- | --- | --- |
| `gitlab` | 创建、更新、关闭、重开普通 Issue，支持 UTC 旧日期格式和 ISO 8601 | 项目数字 ID + 项目内 IID | relay 传入上游重试保持一致的 `event_id` | `fields.project_id`、`fields.repository`（命名空间/仓库） |
| `jira` | Jira Cloud 创建或更新 Issue，提取摘要、文本或 ADF 描述、状态类别 | Issue 数字 ID，按连接器区分实例 | relay 从 `X-Atlassian-Webhook-Identifier` 传入 `event_id` | `fields.project_key` |
| `feishu` 或 `lark` | v2 `im.message.receive_v1` 用户文字消息 | 会话 ID + 消息 ID | `header.event_id` | `fields.chat_id` |
| `slack` | Events API 的普通 `message` 回调 | 工作区 ID + 频道 ID + 消息时间戳 | 信封 `event_id` | `fields.workspace_id`、`fields.channel_id` |
| `telegram` | Bot API 的私聊、群组或超级群组普通用户文字消息 | 聊天 ID + 消息编号 | `update_id` | `fields.chat_id` |
| `linear` | `type=Issue` 的创建与更新，提取 Markdown 描述与工作流状态 | Issue UUID | relay 把 `Linear-Delivery` 传作 `event_id` | `fields.organization_id`、`fields.team_id` |
| `asana` | 官方 GET Task 响应，或单条已签名变更事件配对 GET Task 响应 | 任务 GID | relay 提供每次投递或快照的稳定 `event_id` | `fields.workspace_id`、`fields.project_id` |
| `teams` | Teams Bot Framework `msteams` 用户 `message` Activity | 对话 ID + Activity ID | 自动按对话和 Activity 去重 | `fields.tenant_id`、`fields.chat_id` |
| `discord` | Gateway `MESSAGE_CREATE` 的用户普通消息和文字回复 | 频道 ID + 消息 Snowflake | 消息 Snowflake | `fields.guild_id`、`fields.channel_id` |
| `wecom` | 企业应用已解密明文回调中的普通文字消息，使用与 XML 同名的 JSON 字段 | 企业、应用与消息 ID | 自动按企业、应用和消息 ID 去重 | `fields.corp_id`、`fields.app_id` |
| `dingtalk` | 已验证的机器人用户文字回调，支持 Stream CALLBACK 信封及其 SDK 解码后的 data | 会话 ID + 消息 ID | `msgId` | `fields.corp_id`、`fields.chat_id` |

这些适配器支持预览、入站创建和重投去重。GitLab、Jira、Linear 和 Asana 还支持来源工单更新、关闭及重开。独立聊天消息的任务正文保留完整文本，标题取首个非空行的前 160 个字符。五种即时通信适配器能将后续消息写入同一任务。Slack 还支持原生签名回调和线程内回写。其他平台可由接入方映射至通用的 `connector.ingest` 契约。GitHub Issues 的单独实现见 CLI 手册。

来源字段遵循供应商公开规范。[GitLab Issue events](https://docs.gitlab.com/user/project/integrations/webhook_events/#issue-events) · [Jira Cloud webhooks](https://developer.atlassian.com/cloud/jira/platform/webhooks/) · [飞书接收消息事件](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive) · [Slack message events](https://docs.slack.dev/reference/events/message/) · [Telegram Bot API](https://core.telegram.org/bots/api#update)

Linear 使用 webhook 的 Issue 数据。Asana 的 webhook 只含变更提示；中继将批量事件拆成单条，获取同一 GID 的任务后发送 `payload: {"events":[event],"task":{"data":task}}`。轮询时可直接发送 `payload: {"data":task}`。读取任务时请取 `gid,resource_type,name,notes,completed,modified_at`，设置来源限制时再取 `workspace.gid,memberships.project.gid`。[Linear Webhooks](https://linear.app/developers/webhooks) · [Asana Webhooks](https://developers.asana.com/docs/webhooks-guide) · [Asana Get a task](https://developers.asana.com/reference/gettask)

Teams 使用 relay 验证过的 Bot Framework Activity，发送方需要有用户角色或 Entra 用户标识。Discord 使用经授权的 Gateway 会话，获取正文需要应用具备相应的 Message Content 权限。企业微信的 relay 验签并解密 XML 后，将原消息字段作为 JSON 传入。钉钉使用 SDK 认证后的回调数据或已验证的 HTTP 机器人文本。[Microsoft Teams 会话](https://learn.microsoft.com/en-us/microsoftteams/platform/bots/build-conversational-capability) · [Discord Gateway Events](https://docs.discord.com/developers/events/gateway-events#message-create) · [企业微信消息格式](https://developer.work.weixin.qq.com/document/path/90239) · [钉钉机器人回调模型](https://github.com/open-dingtalk/dingtalk-stream-sdk-go/blob/main/chatbot/model.go)

## 使用

1. 在目标空间创建连接器，例如 `{"op":"create","kind":"connector","id":"support","title":"支持群","fields":{"provider":"slack","channel_id":"C3","enabled":true}}`。来源限制字段使用字符串，包括 Telegram 的负数群 ID。
2. 让已验证上游投递的 relay 或本机操作员提交 `{"op":"provider.preview","space":"default","id":"support","provider":"slack","payload":{...}}`。将 `payload` 替换为样例中的实际事件对象。预览返回对象与投递标识、版本、标题、描述和关闭状态。上游载荷含投递 ID 时，显式传入的 `event_id` 必须与其一致。
3. 使用相同参数调用 `provider.ingest`。需要目标空间的管理员权限，服务端可将 relay 的凭据限定到单个空间和连接器。相同投递 ID 与内容重试会返回原结果。新投递按源版本识别延迟更新，通过外部对象标识找到同一个任务。若本地与源同时改动某一字段，结果中的 `action` 为 `conflict`，管理员可重放并指定 `resolve:"source"` 或 `resolve:"local"`。有效更新同时提交任务和同步记录。
4. 受保密控制的 GitLab Issue、设置安全级别的 Jira Issue、图片及不支持的消息子类型会返回错误。普通入站排除机器人。Slack 原生回调可确认本应用自己的投递。由管理员选择可以复制到目标空间任务列表的来源，核对预览后启用 relay。

下面在独立的本机目录送入一个 Teams 样例。最后两次调用返回同一个任务 ID。可在 PowerShell 7 的仓库根目录运行。

```powershell
$workspace = Join-Path $env:TEMP ("ShipForge-provider-" + [guid]::NewGuid().ToString())
$sample = (Get-Content -Raw -Encoding utf8 crates/shipforge-core/tests/fixtures/inbound-extended.json | ConvertFrom-Json) | Where-Object provider -eq teams
@{op='create';kind='connector';id='teams-demo';title='Teams';fields=@{provider='teams'}} | ConvertTo-Json -Compress | shipforge --data-dir $workspace request
$q = @{op='provider.preview';id='teams-demo';provider='teams';payload=$sample.payload}
$q | ConvertTo-Json -Depth 32 -Compress | shipforge --data-dir $workspace request
$q.op = 'provider.ingest'
$q | ConvertTo-Json -Depth 32 -Compress | shipforge --data-dir $workspace request
$q | ConvertTo-Json -Depth 32 -Compress | shipforge --data-dir $workspace request
shipforge --data-dir $workspace list task
```

## 话题、代码仓库与其他来源

### 将同一话题绑定到任务

| 来源 | 归入同一任务的依据 |
| --- | --- |
| Slack | 同一工作区、频道以及 `event.thread_ts`。首条消息的 `ts` 就是根标识。即使回复先于根消息抵达，也只创建一个任务。 |
| Telegram | 同一聊天的论坛话题 `message_thread_id`，或 `reply_to_message.message_id`。 |
| 飞书与 Lark | 同一租户与会话的 `root_id`，缺省时使用 `parent_id`。 |
| Microsoft Teams | 同一租户与对话的 `replyToId`。 |
| Discord | 同一公会、频道内的 `message_reference.message_id`（回复类型 19）。 |

第一个事件创建任务，或由管理员先将话题绑定到现有共享任务。后续消息写为评论，并保留上游消息 ID、来源作者 ID（若提供）和话题。明确的根消息可直接定位，只有父消息 ID 的协议可沿已入库的父消息定位。企业微信和钉钉样例仅提供会话与单条消息 ID，每条独立消息创建任务。[Telegram 消息结构](https://core.telegram.org/bots/api#message) · [飞书接收消息](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive) · [Teams 消息回复](https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/conversations/conversation-messages) · [Discord 消息引用](https://docs.discord.com/developers/resources/message#message-reference-structure)

使用 `provider.preview` 检查输出中的 `thread`。将同一请求的 `op` 改为 `im.bind`、设置 `task_id` 即可预先绑定。解绑使用 `im.unbind` 和相同的任务、连接器与样例。解绑会取消该话题待发记录，再次绑定前的新回复会被忽略。当前空间的管理员才能操作绑定与出站队列。私有任务不能绑定，入站机器人消息和已导入的评论不会回发。

### Slack 原生双向配置

1. 在 Slack 创建应用，安装到已授权的工作区并将机器人加入目标频道。选择所需的入站事件，例如公开频道的 `message.channels`（`channels:history`）或私有频道的 `message.groups`（`groups:history`）。出站使用 `chat:write`。为自动核销网络中断时的投递，可另订阅 `message_metadata_posted` 和 `metadata.message:read`，应用 manifest 的 `metadata_subscriptions` 限定为自己的 `app_id` 和 `shipforge_delivery` 事件类型。[Slack HTTP 事件订阅](https://docs.slack.dev/apis/events-api/using-http-request-urls/) · [Slack 元数据订阅](https://docs.slack.dev/messaging/message-metadata/)
2. 通过空间管理员账号建立连接器。下面的 ID 均为合成值。工作区、频道、应用和机器人均填写 Slack 原始 ID，使用机器人令牌调用 `auth.test` 可核对工作区和机器人身份。专用入站连接器可设置 `outbound` 为 `false`。[Slack auth.test](https://docs.slack.dev/reference/methods/auth.test/)

```json
{"op":"create","space":"default","kind":"connector","id":"support","title":"支持频道","fields":{"provider":"slack","enabled":true,"outbound":true,"workspace_id":"T1","channel_id":"C1","app_id":"A1","bot_id":"B1"}}
```

3. 在团队服务主机运行 `shipforge-server --data-dir <团队数据目录> token issue --actor owner --scope hook --space default --resource connector --binding support`。把返回的 token 放入服务进程环境变量 `SUPPORT_HOOK_TOKEN`，Slack 应用的 Signing Secret 放入 `SUPPORT_SIGNING_SECRET`。将下面的数组保存为 `slack-routes.json`，其中只填写变量名。多条路由可指向不同的 ShipForge 空间，同一 Slack 应用共用签名密钥变量，每个工作区与频道组合只映射一次。

```json
[{"space":"default","connector":"support","workspace_id":"T1","channel_id":"C1","app_id":"A1","signing_secret_env":"SUPPORT_SIGNING_SECRET","hook_token_env":"SUPPORT_HOOK_TOKEN"}]
```

4. 运行 `shipforge-server --data-dir <团队数据目录> serve --slack-config slack-routes.json`。让 TLS 反向代理保留请求原始正文和签名头。Slack 应用的 Events API Request URL 填 `https://<团队服务>/v1/slack/apps/A1/events`。服务依据已验签的应用、工作区和频道路由，并对每次投递重新检查该空间的 hook 凭据、成员权限和连接器配置。单连接器也可以填 `/v1/slack/default/support`。签名使用原始请求字节和 HMAC-SHA256，服务器接受前后五分钟的时间戳。重复事件原子返回已有结果。[Slack 签名规范](https://docs.slack.dev/authentication/verifying-requests-from-slack/)
5. 在有该空间写入凭据与管理员角色的工作机，将 Slack bot token 设置为 `SHIPFORGE_SLACK_BOT_TOKEN`。确认工作机指向同一团队服务，运行 `shipforge-worker slack-deliver --space default --connector support` 执行一批。持续处理可增加 `--watch`，其他工作区令牌可用 `--token-env <变量名>` 指定。机器先调用 `auth.test` 检查工作区与 bot 身份，再调用 `chat.postMessage`，将共享任务的标题和状态变动发到原 `thread_ts`。每条消息最多 3,999 个字符，使用纯文本并关闭链接预览。[Slack chat.postMessage](https://docs.slack.dev/reference/methods/chat.postMessage/)
6. 在任务详情使用“添加评论”进行团队内部讨论。发送到 Slack 时选择“回复到 Slack 话题”及具体话题。CLI 先提交 `{"op":"im.threads","task_id":"<任务 ID>"}` 查看可写目标，再传 `{"op":"im.reply","id":"<连接器 ID>","task_id":"<任务 ID>","thread":<上一步返回的 thread 对象>,"body":"已收到，明天交付。","request_id":"reply-001"}`。需要当前任务的编辑权限。相同请求重放只写一条评论和一次投递，并只发往选中的话题。[HiFox 的渠道评论](https://help.hifox.com/docs/integration/github)

7. 需要从卡片改派人员时，在 Slack App 的 Interactivity & Shortcuts 启用 Interactivity，Request URL 填事件订阅使用的同一公开地址。然后在 ShipForge 的 Slack 连接器启用「将任务活动回写 Slack 线程」和「允许从 Slack 卡片改派处理人」，并在「外部账号到空间成员映射」填写 Slack 用户 ID 与本空间成员 ID 的对应关系。操作人和候选处理人都需要映射。新建话题绑定后，工作机会投递带原生人员单选框的任务卡片。已有绑定会在后续标题、状态或人员变更时收到新卡片。[Slack 交互回调](https://docs.slack.dev/reference/interaction-payloads/block_actions-payload/) · [Slack 单选框](https://docs.slack.dev/reference/block-kit/block-elements/select-menu-element/)

卡片操作按实际点击者的空间权限执行。选一人替换整组人员，原有 Agent 或小队继续执行。服务仅接受授权应用发给已绑定话题且已确认送达的卡片，核对工作区、频道、消息、话题、机器人及点击者。成员被移除、改派已由他人完成、关闭卡片或解绑时，旧操作无法改写任务。同一点击重试保持同一任务修订。[HiFox 的 Slack 任务卡片](https://help.hifox.com/docs/integration/slack)

管理员可从 CLI 设置卡片。以下示例假设连接器已配置 Slack 原始标识，并且 `alex` 和 `mei` 已加入当前空间。

```json
{"op":"update","space":"default","kind":"connector","id":"support","fields":{"outbound":true,"slack_assignment":true,"assignee_map":{"U01ALEX":"alex","U02MEI":"mei"}}}
```

出站与任务修改使用同一 SQLite 事务。同一话题按顺序投递。HTTP 429 按 `Retry-After` 延后，明确的拒绝记为 `failed`。超时、5xx、工作进程领走消息后异常退出都可能发生在 Slack 接收成功之后，此时记录进入 `uncertain`。已签名的本应用消息或元数据回执与应用、机器人、工作区、频道、话题和投递 ID 匹配后，会自动核销。[Slack 限流规则](https://docs.slack.dev/apis/web-api/rate-limits/) · [Slack 元数据回执](https://docs.slack.dev/reference/events/message_metadata_posted/)

管理员可用 `{"op":"im.outbox","id":"support"}` 查看队列。检查原话题发现消息已发出时，使用 `{"op":"im.retry","id":"support","delivery_id":"<投递 ID>","decision":"confirm","remote_id":"<Slack 消息 ts>"}`。在频道确认尚未送达并修复权限或连接后，使用同一操作的 `decision:"resend"` 解锁重投。`im.claim` 每次领取一条返回 60 秒租约，`im.ack` 需要相同的 `delivery_id`、`lease` 与 `outcome`。`sent` 需要 `remote_id`，`retry` 需要 1–86,400 秒的 `retry_after`，另外支持 `failed` 与 `uncertain`。核销前该话题后来的消息保留在队列，其他话题继续工作。

本次联调使用合成事件与回环 HTTP 服务。源码仓库中运行 `cargo test -p shipforge-core --test im_threads` 核对线程归并和租约恢复，`cargo test -p shipforge-worker slack::tests` 核对实际 HTTP 请求、429 与不确定投递，`python scripts/verify-team.py target/debug` 核对真实团队服务的签名和多空间路由。线上组织的安装、权限审批和联网收发需要由对应组织授权后验证。

### GitHub 与 GitLab 的提交、PR 和 MR

在“连接器”选择 GitHub 或 GitLab，填写仓库路径并勾选“按任务编号关联提交和合并请求”。对应 JSON 是 `{"provider":"github","repository":"example/desktop","link_tasks":true,"enabled":true}`。GitLab 支持子群组，非 gitlab.com 服务器另填 `repository_host`（HTTPS 主机名）。每个接收处理器核对仓库的完整路径与原站地址。在公开 GitHub 仓库，仅关联源 PR 的 `author_association` 为 `OWNER`、`MEMBER` 或 `COLLABORATOR` 的活动。[HiFox GitHub 行为](https://help.hifox.com/docs/integration/github) · [HiFox GitLab 行为](https://help.hifox.com/docs/integration/gitlab)

GitHub 仓库 Webhook 的回调为 `POST /v1/github/{space}/{connector}`，订阅 push 与 pull_request，内容类型选 `application/json`。在接收主机为该连接器签发一个绑定到其空间和 ID 的 hook 令牌，放到环境变量 `REPO_HOOK_TOKEN`。将仓库 Webhook 的 Secret 放入 `REPO_SIGNING_SECRET`。服务用 `--github-config github-routes.json` 或 `SHIPFORGE_GITHUB_WEBHOOKS` 指定下面的配置，使用原始字节与 `X-Hub-Signature-256` 验签。GitLab 使用已有 `POST /v1/integrations/{space}/{connector}` 入口，Secret token 填绑定的 hook 令牌，并启用 Push events 与 Merge request events。[GitHub 验签](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries) · [GitLab 项目事件](https://docs.gitlab.com/user/project/integrations/webhook_events/)

```json
[{"space":"default","connector":"repo","repository":"example/desktop","signing_secret_env":"REPO_SIGNING_SECRET","hook_token_env":"REPO_HOOK_TOKEN"}]
```

GitHub 使用 `X-GitHub-Delivery`，GitLab 使用 `X-Gitlab-Event-UUID`。在 CLI 或已验签的中继中，调用 `provider.preview` 查看 `links`，再用 `provider.ingest` 传入原生事件及稳定 `event_id`。提交消息、PR/MR 标题、描述、来源分支中的完整任务编号如 `SF-12` 会关联到当前空间可共享的任务。合并请求后续变动也沿用已有任务关联。任务详情“评论与活动”的“查看源记录”打开原提交或合并请求。交付号和具体活动均会去重，后续 PR/MR 关闭在本版记为一条活动，任务状态由团队工作流决定。[GitHub 事件载荷](https://docs.github.com/en/webhooks/webhook-events-and-payloads) · [GitLab Merge request events](https://docs.gitlab.com/user/project/integrations/webhook_events/#merge-request-events)

如需收到 PR 自动组织审查，先创建有外部执行者、指令且已启用 Webhook 触发器的自动化。将其 ID 设为连接器的 `fields.review_rule_id`。GitHub PR 新建、重开或转为可审查，GitLab 同项目 MR 新建、重开时，会以原链接和仓库为上下文为该规则建立运行。识别出多个任务时，每个任务产生一个关联运行。没有任务编号时产生一个仓库级运行。规则停用后后续事件继续记录代码活动。同步返回的 `review_runs` 保存运行 ID，复投沿用原结果。[HiFox 的 PR 自动审查示例](https://hifox.com/blog/ai-agents-review-pull-requests/)

对应的源码测试是 `cargo test -p shipforge-core --test vcs_links`。对 GitHub 与 GitLab 的本机 HTTP 回调、凭据撤销和规则运行验收随 `python scripts/verify-team.py target/debug` 执行。其他代码托管平台可由受信中继导入工单，原生 Git 事件解析目前覆盖上表两家。

### 通用接入

| 参考产品中的平台 | 当前可用入口 | 由接入方提供的部分 |
| --- | --- | --- |
| GitHub Issues | 机器工具的 Issues 同步适配器、通用事件入库和回写数据导出 | 已授权的 GitHub 访问、仓库绑定 |
| 腾讯 Bugly | 原生提单地址、按产品映射建任务、去重及签名状态回写 | 产品配置与可选 OpenAPI 凭据。[接入步骤](../../docs/vendor-imports.md) |
| Zoho BugTracker | OAuth 项目发现、增量预览、确认和按批次撤销 | 只读门户令牌与项目绑定。[接入步骤](../../docs/vendor-imports.md) |
| TAPD | 公司或个人身份的项目发现，需求和缺陷分页历史导入、立即检测和按批次回滚 | 公司 API 账号或个人访问令牌。[接入步骤及实时同步边界](../../docs/vendor-imports.md) |
| 禅道、PingCode、Teambition 或兼容 Jira 的自建接口 | 文件字段映射导入，或 `connector.ingest` 按外部工单持续更新 | 事件订阅和载荷转换，各平台账号与回写 |
| CNB、Gitee 及其他代码托管服务 | 由 relay 映射工单到 `connector.ingest`，或使用文件导入 | 提取平台工单并提供对象标识、投递标识和版本 |
| 个人微信、QQ | 由 relay 映射文字消息到 `connector.ingest` | 来源鉴权、事件转换、机器人配置及回复 |

上述通用路径已按统一操作契约验证。十一个平台专用解析器使用本地代表性事件进行验收，覆盖预览、入库、重放、工单冲突和来源限制。[入站验收用例](tests/providers.rs)
