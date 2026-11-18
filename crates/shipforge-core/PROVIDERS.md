# 入站连接器

ShipForge 把工单事件和普通用户文字消息转换成当前空间的任务。下列样例全部使用合成数据。[样例载荷](tests/fixtures/inbound.json)

| 配置中的 provider | 接收内容 | 外部对象标识 | 投递去重 | 可选来源限制 |
| --- | --- | --- | --- | --- |
| `gitlab` | 创建、更新、关闭、重开普通 Issue，支持 UTC 旧日期格式和 ISO 8601 | 项目数字 ID + 项目内 IID | relay 传入上游重试保持一致的 `event_id` | `fields.project_id`、`fields.repository`（命名空间/仓库） |
| `jira` | Jira Cloud 创建或更新 Issue，提取摘要、文本或 ADF 描述、状态类别 | Issue 数字 ID，按连接器区分实例 | relay 从 `X-Atlassian-Webhook-Identifier` 传入 `event_id` | `fields.project_key` |
| `feishu` 或 `lark` | v2 `im.message.receive_v1` 用户文字消息 | 会话 ID + 消息 ID | `header.event_id` | `fields.chat_id` |
| `slack` | Events API 的普通 `message` 回调 | 工作区 ID + 频道 ID + 消息时间戳 | 信封 `event_id` | `fields.workspace_id`、`fields.channel_id` |
| `telegram` | Bot API 的私聊、群组或超级群组普通用户文字消息 | 聊天 ID + 消息编号 | `update_id` | `fields.chat_id` |

这些适配器支持预览、入站创建、重投去重。GitLab 和 Jira 还支持来源工单更新、关闭及重开。聊天适配器将每条独立消息落为一个任务，正文保留完整文本，标题取首个非空行的前 160 个字符。上述适配器的出站回写与上游账号绑定由接入方负责。其他平台可由接入方映射至通用的 `connector.ingest` 契约。GitHub Issues 的单独实现见 CLI 手册。

来源字段遵循供应商公开规范。[GitLab Issue events](https://docs.gitlab.com/user/project/integrations/webhook_events/#issue-events) · [Jira Cloud webhooks](https://developer.atlassian.com/cloud/jira/platform/webhooks/) · [飞书接收消息事件](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive) · [Slack message events](https://docs.slack.dev/reference/events/message/) · [Telegram Bot API](https://core.telegram.org/bots/api#update)

## 使用

1. 在目标空间创建连接器，例如 `{"op":"create","kind":"connector","id":"support","title":"支持群","fields":{"provider":"slack","channel_id":"C3","enabled":true}}`。来源限制字段使用字符串，包括 Telegram 的负数群 ID。
2. 让已验证上游投递的 relay 或本机操作员提交 `{"op":"provider.preview","space":"default","id":"support","provider":"slack","payload":{...}}`。将 `payload` 替换为样例中的实际事件对象。预览返回对象与投递标识、版本、标题、描述和关闭状态。上游载荷含投递 ID 时，显式传入的 `event_id` 必须与其一致。
3. 使用相同参数调用 `provider.ingest`。需要目标空间的管理员权限，服务端可将 relay 的凭据限定到单个空间和连接器。相同投递 ID 与内容重试会返回原结果。新投递按源版本识别延迟更新，通过外部对象标识找到同一个任务。若本地与源同时改动某一字段，结果中的 `action` 为 `conflict`，管理员可重放并指定 `resolve:"source"` 或 `resolve:"local"`。有效更新同时提交任务和同步记录。
4. 受保密控制的 GitLab Issue、设置安全级别的 Jira Issue、机器人、图片及不支持的消息子类型会返回错误。由管理员选择可以复制到目标空间任务列表的来源，核对预览后启用 relay。
