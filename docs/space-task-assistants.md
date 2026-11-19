# 空间任务助手

在 Windows 的「外部应用集成 → 空间任务助手」中，左侧所选空间决定任务归属。每个空间在每种渠道各有一个助手。成员能浏览自己已加入空间的助手，空间管理员能绑定、查看运行活动、修改设置或解除绑定。任务、评论和活动记录会保留助手与发起成员的身份。普通 Agent 仍从「Agent」或「智能体机器人」发起会话。

## 绑定和设置

1. 在当前空间建立并启用已获授权的渠道连接器。Slack 要固定工作区、频道、应用与机器人标识。飞书与 Lark 分别固定租户、群和机器人的 open ID。钉钉固定企业及群。
2. 打开对应渠道的「绑定任务助手」，选择连接器并填写 Runtime。映射表的键是渠道用户 ID，值是本空间中有编辑权限的成员 ID。可按需选择运行机器、默认模型及空间技能。指定的机器须支持所选 Runtime，并能领取绑定管理员的私有工作。
3. 设置中的中文与英文分别继承组织模板。组织管理员可修改两种语言的描述及运行指令。可用变量为 `{{productName}}`、`{{imName}}`、`{{cliName}}` 和 `{{spaceName}}`。空间管理员可对单种语言的单个字段启用覆盖，取消勾选即恢复该字段的继承。未指定语言时采用组织默认语言。
4. 解除绑定后可用原配置重新绑定。移除后再次绑定会建立新助手。两种操作都会停用旧绑定的运行及待发回执，历史任务保留原始助手标识。

## 通过 CLI 配置

以下是三个独立的 JSON 请求。请先将样例中的空间、外部用户、本地成员、Runtime 及连接器替换成已有的值。首次绑定传 `expected_rev: 0`。修改或解绑前重新读取设置，提交返回的最新版本。

```json
{"op":"task_assistant.catalog","space":"default","language":"zh-CN"}
```

```json
{"op":"task_assistant.bind","space":"default","provider":"slack","connector":"slack-team","runtime":"local-tool","sender_map":{"U_ALEX":"alex"},"expected_rev":0}
```

```json
{"op":"task_assistant.settings","space":"default","provider":"slack","language":"en"}
```

使用 `task_assistant.configure` 提交运行设置或语言覆盖，传 `"instructions":null` 或 `"description":null` 恢复相应字段的继承。使用 `task_assistant.template` 和 `task_assistant.template.save` 读写组织模板，使用 `task_assistant.history` 查看当前助手的最近 100 条活动。用 `task_assistant.unbind` 解除当前渠道，用 `task_assistant.remove` 移除该助手。

## 群内发起任务

在所绑定的群中 @ 助手并发出明确请求，例如「创建任务 处理解析失败」或「查看 SF-12」。首次请求完成后，在同一 Slack 或飞书话题中可继续输入「修改这个任务……」或「评论……」。也可按当前空间的任务编号明确指定目标。运行会接收本话题中最近 12 次助手交流及代码块。后台只按该渠道用户映射的成员权限受理请求，群聊无法查看私有任务。运行期间撤权会阻止写入，目标任务出现并发修改时需重新发出修改请求。

| 渠道 | 已验证的行为 | 实际部署所需条件及待验项目 |
| --- | --- | --- |
| Slack | 签名群 @ 与已关联话题内的命令通过服务入站，经工作机建单、查询、修改或评论。出站工作机向原频道的原话题提交回执并记录远端消息标识。重复事件不会再次建单。 | 预先获准的应用、事件签名密钥、连接器钩子和有发帖权限的机器令牌。真实租户安装与消息展示待联调。 |
| 飞书、Lark | 已认证中继提交指定租户、群和机器人的群 @。两朵云的助手和凭据独立，富文本中的代码块随请求进入 Runtime。 | 需自行提供已验证的入站中继。空间助手的原生扫码、话题回执与文件回传待接入。 |
| 钉钉 | 已认证中继可提交订阅主题的群 @ 回调，在绑定空间发起任务。 | 需具有授权的 Stream 中继。空间助手的直连、回复和文件回传待接入。 |
| 企业微信 | 可管理空间助手身份、模板和运行设置。 | 空间助手仍待与原生收发通道连接。普通 Agent 的企业微信长连接可单独使用。[企业微信 Agent](wecom-agent.md) |

功能语义参照 HiFox 的三份集成文档。[飞书集成](https://help.hifox.com/docs/integration/feishu) · [钉钉集成](https://help.hifox.com/docs/integration/dingtalk) · [企业微信集成](https://help.hifox.com/docs/integration/wecom)

ShipForge 会忽略机器人自己发送的 Slack 消息，签名事件和出站均按配置的工作区和频道校验。[Slack 群 @ 事件](https://docs.slack.dev/reference/events/app_mention/)

本地测试使用独立团队、合成用户和回环服务。可从源码复验签名事件、读写权限、来源标记、工作机与出站回执，并查看 Windows 原生操作。[命令行与服务器验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-space-task-assistants.py) · [工作机出站验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/src/slack/tests.rs) · [权限与模板用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/space_task_assistants.rs) · [原生窗口操作](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/task_assistant_ui_test.rs)
