# 对话成员、提醒与分享

在 Windows 的「对话」页新建普通对话或群聊。群主可从详情里的「管理成员」和「管理 Agent」添加或移出成员。修改名称、描述使用编辑对话。群成员可退出，群主可转让或解散。解散后成员无法继续查看该群的消息、产物和链接。有关联运行仍在执行时，先处理运行。

## 私有会话与群聊

所有请求可指定 `space`，默认是 `default`。创建私有群聊的示例为 `{"op":"create","kind":"chat","id":"review","title":"发布评审","fields":{"chat_type":"group","visibility":"private"}}`。普通对话使用 `chat_type:"conversation"`。之前创建的普通对话继续使用已有记录。

| 当前行为 | JSON 操作与所需身份 |
| --- | --- |
| 查看本人可见的对话管理信息 | `{"op":"chat.info","id":"review"}` 返回所有者、本人是否为所有者及提醒偏好。群成员可见当前成员和有权查看的 Agent。私有会话的授权详情和待接受名单由所有者查看。 |
| 管理群成员或 Agent | 群主发送 `{"op":"chat.manage","id":"review","action":"add","target":"bob"}`。移出用 `remove`。Agent 要增加 `"target_kind":"agent"`。 |
| 转让或离群 | `transfer` 的 `target` 为有编辑权限的空间成员。新群主须已经入群，普通私有会话可直接转让。旧所有者保留参与资格，直到退出或新所有者移出。普通成员用 `leave`，群主可选 `dissolve`。 |
| 邀请参与私有会话 | 能参与对话的成员发送 `invite` 和目标成员 ID。收件人读取 `{"op":"chat.invitations"}`，用 `{"op":"chat.manage","id":"review","action":"accept"}` 接受，或用 `decline` 拒绝。邀请接受前只显示邀请人和标题。 |
| 撤回访问 | 私有会话所有者发送 `revoke` 和成员 ID。私有任务会话同时检查任务可见性。消息、附件、产物、通知及其搜索结果随当前权限过滤。团队客户端断线时也要求重新连接以打开这些对象。 |
| 只清理本人历史 | `{"op":"chat.manage","id":"review","action":"clear","request_id":"clear-review-once"}`。当前历史从该成员的视图移除，其他成员仍可读取。此后收到的新消息正常显示。 |
| 提醒和免打扰 | `{"op":"chat.preferences","id":"review","muted":true,"ignore_all":true}` 改变本人设置。发送消息时在 `fields.mentions` 填成员 ID 数组，群聊可加 `mention_all:true`。直接提及本人会穿透免打扰与群体提及过滤。 |

## 选择内容与撤销分享

所有者在对话详情选择「分享已选消息」，逐条选择内容和范围。内容按原始消息顺序保存。关联工作机的回复只有在运行完成后才能分享。每次可选择 1 至 100 条消息，快照上限为 1 MiB，包括发言人显示名、Markdown 正文及附件文件名。本版本提供链接和纯文本，图片式分享尚待实现。

```json
{"op":"chat.share.create","id":"review","message_ids":["first","answer"],"scope":"organization","request_id":"share-review"}
```

默认范围 `organization` 要求读取时仍有该组织的成员身份。显式选择 `link` 后，拿到链接的人可访问快照。后续讨论和编辑不会改变快照内容。团队 CLI 与桌面返回配置的团队服务地址加 `/share/<分享码>`。在浏览器打开持链可读的分享可直接读取纯文本。组织内分享请在已连接的桌面「对话 → 打开分享链接」粘贴链接，或使用认证 CLI。HTTP 客户端可向同一端点发送成员的 Bearer 令牌。本机模式用户可在同一工作区粘贴分享码。

```json
{"op":"chat.share.read","share":"替换为64位分享码"}
{"op":"chat.share.revoke","id":"review","share":"替换为64位分享码"}
```

「撤销此选区的分享」会使指向同一批消息的所有有效快照链接同时失效。另一批消息的快照单独管理。撤回会话成员的实时访问、撤销冻结的快照分别有独立的操作入口。

## 隔离环境复验

1. 用安装所有者在临时团队服务创建 A、B、C 三个普通成员，分别用个人写令牌连接独立的客户端配置。
2. A 创建私有会话并上传消息附件，邀请 B。B 在接受前尝试读取附件，接受后再读取。A 撤回 B 的访问后，再检查直接读取、附件下载、搜索与事件记录。
3. A 创建群组，加入 B 和 C。B 开启免打扰及忽略群体提及。A 依次发送普通消息、群体提及、直接提及 B。B 应仅收到最后一条通知。B 清理个人历史后，让 C 打开相同的历史消息，然后再发送一条新消息。
4. A 为同一批消息分别创建组织范围和持链可读的分享。无凭据打开前者应要求身份验证，组织外身份会被拒绝。A 编辑消息，再次打开已生成的链接应保留原文。撤销后两个入口都应返回不存在。

本仓库的核心回归和 Windows 本地团队服务分别执行了上述身份与快照用例。第三方生产租户的多成员联调单独记录。[核心测试](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/chat.rs) · [双客户端与 HTTP 检查](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-chat.py) · [参考产品对话说明](https://help.hifox.com/docs/agent-and-computer/chat-with-agent)
