# 飞书与 Lark 智能体

Windows 的「外部应用集成」分别显示「飞书智能体」与「Lark 智能体」。两边可同时连接同一个 ShipForge Agent。国内飞书账号选飞书，国际账号选 Lark。每个云端中，一个 Agent 同时接一个机器人，一个已占用的应用需在原绑定中解绑后才能重用。[HiFox 飞书与 Lark 集成](https://help.hifox.com/docs/integration/feishu)

## 绑定与运行

1. 进入对应云端，点「绑定 Agent」并选择有使用权的 Agent，Windows 会立即显示授权二维码。在 Agent 详情直接点「飞书智能体」或「Lark 智能体」也能开始绑定。已绑定的入口会打开其接收设置。
2. 在对应平台扫码或点「打开授权链接」，在官方页面确认创建及授权。ShipForge 定期检查状态，链接过期或选错云端后可以重新生成。绑定用户需有空间编辑权限和可用的 Agent。
3. 同目录的工作机进程在绑定设备上启动长连接接收。Agent 还需有能运行其 Runtime 的工作机。重启后可选「启动后台接收」恢复，也可以「同步一次」检查。
4. 在「接收范围」调整单聊、群 @、免 @ 群、话题继续触发和表情。想停止接收可选「解除绑定」，历史对话保留。飞书和 Lark 分别撤销。

设备依照上游授权流程取得一次性设备码，通过扫码账号确认所属云端，再向所选云验证机器人和应用。Windows 用当前用户的 DPAPI 保存设备码、应用密钥与短期令牌。团队服务按成员与空间存绑定和投递记录。消息由绑定设备主动连接官方 WebSocket 接收，可通过普通出站 HTTPS 上传和回复。[官方扫码注册 SDK](https://github.com/larksuite/oapi-sdk-go/tree/v3_main/scene/registration) · [官方长连接协议](https://github.com/larksuite/oapi-sdk-go/tree/v3_main/ws)

## 收发效果

| 飞书或 Lark 中的操作 | ShipForge 的行为 |
| --- | --- |
| 私聊中发消息，或在群中 @ 机器人 | 为对应 Agent 提交私有对话运行。不同云、绑定、群与话题拥有独立上下文。普通消息不创建任务。 |
| 在指定群中发送普通文字 | 在范围中填写具体的 `oc_` 群 ID，可填多个。仅提及真人时跳过。群消息内容的上游读取需要管理员批准并发布相应权限，飞书客户端里还要能把机器人拉进该群。 |
| 在话题群开新话题并继续回复 | 开启该群免 @ 后可响应首条。后续免 @ 回复还需要开启话题继续触发。话题内 @ 会回复原话题；普通对话群顶层 @ 会回复群消息。 |
| 给对话群消息加触发表情 | 启用表情触发后，默认 `THUMBSUP` 可启动运行并在话题内回复。任何能在同一消息加表情的成员可用终止表情停止该条消息触发的运行。默认终止代号为 `CrossMark`。完成或停止时会尝试移除工作标记。 |
| 发送 `/new` 或 `/new 首条提示` | 在当前私聊、群或话题内开启新会话。只有命令时返回确认；有提示时作为新会话第一条输入。 |
| 发送文字、富文本、图片、文件或视频 | 支持当前消息的段落、链接、代码块、常见卡片文字与可访问媒体。附件随入站消息保存在私有会话，工作机把当次输入文件放入运行期间的临时目录。每条消息最多读取六项，合计 4 MiB。媒体不可读取时保留文件提示。 |
| Agent 回复文字及附带文件的产物 | 按来源消息回复，保留群话题。纯文字使用文本消息，Markdown 可使用富文本，代码块或表格可使用卡片。返回的文件上传后作为文件消息发送。平台限流可延迟重试；送达状态不明时可核对原消息再处理。 |

上游租户需允许机器人收取单聊、群 @ 和相关事件，并授予以机器人身份发消息、读取会话资料、访问媒体及操作表情的权限。要监听某些群中的普通消息，可能还需额外申请并发布读取群消息的 `im:message.group_msg`。应用可用范围和群内策略仍由租户决定。[接收消息事件](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive) · [消息正文格式](https://open.larksuite.com/document/uAjLw4CM/ukTMukTMukTM/im-v1/message/create_json) · [回复原消息](https://open.larksuite.com/document/server-docs/im-v1/message/reply)

## CLI 请求

在绑定设备上把以下 JSON 分别交给 `shipforge request`。示例里的 `helper` 是当前空间的 Agent ID。国际版把第一条请求的 `provider` 设为 `lark`。二维码及授权链接应由发起绑定的人查看。

```json
{"op":"bot.qr.start","space":"default","provider":"feishu","agent":"helper"}
```

```json
{"op":"bot.qr.check","space":"default","id":"<返回的 ID>"}
```

```json
{"op":"bot.lark.policy","space":"default","id":"<绑定 ID>","dm":true,"groups":true,"plain_chats":["oc_example"],"reactions":true,"trigger_emoji":"THUMBSUP","stop_emoji":"CrossMark","thread_followup":false}
```

```powershell
shipforge-worker bot-sync --space default --binding <绑定ID> --watch
```

`bot.bindings` 返回本人的绑定和接收设置，传入 `available_for:"feishu"` 或 `available_for:"lark"` 可取自己能用且该云尚可绑定的 Agent。`bot.sync` 执行一轮收发，`bot.disconnect` 释放绑定并清理本机凭据。`bot.lark.event` 是已验证的平台适配器调用，必须携带绑定设备密钥、应用、租户、来源消息、事件、发送者与会话标识。事件重发沿用同一回执，拆分的 WebSocket 帧在完整组装并提交后才确认。`bot.claim` 为结果给出原消息和话题目的地，`bot.ack` 记录送达状态。

## 验收条件

本地黑盒启动正式 CLI、工作机与团队服务，另在两个回环地址运行独立的模拟云。三个合成成员验证重复事件、同名跨云标识、私有消息和附件权限、话题隔离、表情终止、结果回传和分别解绑。Windows 控件用例打开真实无焦点 D3D 窗口，验证两个入口、候选过滤、扫码链接、过期、范围修改及解绑。[双云三身份验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-lark-bots.py) · [Windows 控件验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/lark_ui_test.rs)

真机扫码、租户审核、应用发布、对方客户端的表情及卡片显示、平台限流和正式网络连通性需要各云获授权的测试租户。私有任务的外部成员身份映射、多任务话题绑定和补历史、跨云组织级通知、空间任务助手模板及云文档工具各有独立配置及验收条件。[HiFox 各类连接及话题规则](https://help.hifox.com/docs/integration/feishu)
