# 入站连接器

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

这些适配器支持预览、入站创建、重投去重。GitLab、Jira、Linear 和 Asana 还支持来源工单更新、关闭及重开。聊天适配器将每条独立消息落为一个任务，正文保留完整文本，标题取首个非空行的前 160 个字符。上述适配器的出站回写与上游账号绑定由接入方负责。其他平台可由接入方映射至通用的 `connector.ingest` 契约。GitHub Issues 的单独实现见 CLI 手册。

来源字段遵循供应商公开规范。[GitLab Issue events](https://docs.gitlab.com/user/project/integrations/webhook_events/#issue-events) · [Jira Cloud webhooks](https://developer.atlassian.com/cloud/jira/platform/webhooks/) · [飞书接收消息事件](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive) · [Slack message events](https://docs.slack.dev/reference/events/message/) · [Telegram Bot API](https://core.telegram.org/bots/api#update)

Linear 使用 webhook 的 Issue 数据。Asana 的 webhook 只含变更提示；中继将批量事件拆成单条，获取同一 GID 的任务后发送 `payload: {"events":[event],"task":{"data":task}}`。轮询时可直接发送 `payload: {"data":task}`。读取任务时请取 `gid,resource_type,name,notes,completed,modified_at`，设置来源限制时再取 `workspace.gid,memberships.project.gid`。[Linear Webhooks](https://linear.app/developers/webhooks) · [Asana Webhooks](https://developers.asana.com/docs/webhooks-guide) · [Asana Get a task](https://developers.asana.com/reference/gettask)

Teams 使用 relay 验证过的 Bot Framework Activity，发送方需要有用户角色或 Entra 用户标识。Discord 使用经授权的 Gateway 会话，获取正文需要应用具备相应的 Message Content 权限。企业微信的 relay 验签并解密 XML 后，将原消息字段作为 JSON 传入。钉钉使用 SDK 认证后的回调数据或已验证的 HTTP 机器人文本。[Microsoft Teams 会话](https://learn.microsoft.com/en-us/microsoftteams/platform/bots/build-conversational-capability) · [Discord Gateway Events](https://docs.discord.com/developers/events/gateway-events#message-create) · [企业微信消息格式](https://developer.work.weixin.qq.com/document/path/90239) · [钉钉机器人回调模型](https://github.com/open-dingtalk/dingtalk-stream-sdk-go/blob/main/chatbot/model.go)

## 使用

1. 在目标空间创建连接器，例如 `{"op":"create","kind":"connector","id":"support","title":"支持群","fields":{"provider":"slack","channel_id":"C3","enabled":true}}`。来源限制字段使用字符串，包括 Telegram 的负数群 ID。
2. 让已验证上游投递的 relay 或本机操作员提交 `{"op":"provider.preview","space":"default","id":"support","provider":"slack","payload":{...}}`。将 `payload` 替换为样例中的实际事件对象。预览返回对象与投递标识、版本、标题、描述和关闭状态。上游载荷含投递 ID 时，显式传入的 `event_id` 必须与其一致。
3. 使用相同参数调用 `provider.ingest`。需要目标空间的管理员权限，服务端可将 relay 的凭据限定到单个空间和连接器。相同投递 ID 与内容重试会返回原结果。新投递按源版本识别延迟更新，通过外部对象标识找到同一个任务。若本地与源同时改动某一字段，结果中的 `action` 为 `conflict`，管理员可重放并指定 `resolve:"source"` 或 `resolve:"local"`。有效更新同时提交任务和同步记录。
4. 受保密控制的 GitLab Issue、设置安全级别的 Jira Issue、机器人、图片及不支持的消息子类型会返回错误。由管理员选择可以复制到目标空间任务列表的来源，核对预览后启用 relay。

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

## 其他供应商的接入范围

| 参考产品中的平台 | 当前可用入口 | 由接入方提供的部分 |
| --- | --- | --- |
| GitHub Issues | 机器工具的 Issues 同步适配器、通用事件入库和回写数据导出 | 已授权的 GitHub 访问、仓库绑定 |
| 禅道、TAPD、Bugly、PingCode、Teambition、Zoho BugTracker 或兼容 Jira 的自建接口 | 文件字段映射导入，或 `connector.ingest` 按外部工单持续更新 | 事件订阅和载荷转换，各平台账号与回写 |
| CNB、Gitee 及其他代码托管服务 | 由 relay 映射工单到 `connector.ingest`，或使用文件导入 | 提取平台工单并提供对象标识、投递标识和版本 |
| 个人微信、QQ | 由 relay 映射文字消息到 `connector.ingest` | 来源鉴权、事件转换、机器人配置及回复 |

上述通用路径已按统一操作契约验证。十一个平台专用解析器使用本地代表性事件进行验收，覆盖预览、入库、重放、工单冲突和来源限制。[入站验收用例](tests/providers.rs)
