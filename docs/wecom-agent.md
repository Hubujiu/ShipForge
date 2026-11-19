# 企业微信智能体

把空间里的 Agent 绑定到企业微信智能机器人后，成员可以私聊它，也可以把它加入群聊并 @ 它。每个私聊和每个群单独保存对话。企业微信里的输入进入 Agent 对话，后续分派、跟踪和验收可以另建空间任务。[HiFox 企业微信集成](https://help.hifox.com/docs/integration/wecom)

## 在 Windows 连接

1. 在企业微信管理后台进入「管理工具 → 智能机器人」，创建机器人，选择 API 模式和长连接，复制 BotID 与 Secret。需要创建机器人的企业权限。[企业微信管理后台](https://work.weixin.qq.com/wework_admin/frame)
2. 在 ShipForge 打开「外部应用集成 → 企业微信智能体 → 绑定 Agent」，或从目标 Agent 的详情进入「企业微信智能体」。选择具有使用权的 Agent，填写 BotID 和长连接 Secret，可分别设置「响应私聊」「响应群内 @」。点「验证并绑定」后，客户端会向企业微信建立经过 TLS 验证的订阅。收到授权成功回执后才显示「已连接」。[官方长连接 SDK](https://github.com/WecomTeam/aibot-node-sdk)
3. 保持绑定设备在线，并为 Agent 的 Runtime 配备可领取运行的工作机。桌面在绑定成功时启动同目录的后台接收程序。重启后可在绑定上点「启动后台接收」。在绑定上点「同步一次」可读取一轮结果。若后台接收程序已持有长连接，客户端直接等待该进程的一轮收发，避免重新登录机器人。
4. 机器人可见范围内的成员可以私聊它。在允许加入机器人的群里邀请它，再 @ 它提问。企业微信把指向机器人的群消息交给订阅。ShipForge 的「接收范围」提供私聊和群 @ 两个单独开关。[官方回调结构](https://github.com/WecomTeam/aibot-node-sdk/blob/main/src/types/message.ts)

每个 Agent 在本空间可绑定一个企业微信机器人。同一个 BotID 只允许一个活动绑定。已绑定的 Agent 从候选列表中移出。空间编辑者能看见自己可使用的 Agent 中哪些已经由同事绑定，自己的绑定可调整或断开。同事的 BotID、投递记录和私有聊天按所有者隔离。

## 消息、文件和回执

| 操作或状态 | 可观察的结果 |
| --- | --- |
| 私聊或在群内 @ 机器人 | 消息写入对应的私有外部对话，创建 Agent 运行。重发同一上游消息 ID 只处理一次。 |
| 在同一会话发送 `/new` 或 `/new 首条提示` | 该会话开始新的上下文。单独使用命令会收到确认，附带的文字作为新会话第一条输入。其他群和私聊可继续原会话。 |
| 发文字、语音转写、图文、图片、文件或视频 | 当次消息的文字及可读取的附件进入工作机。设备从受信任的 HTTPS 下载地址取文件，使用本条消息的 AES 密钥解密，每条最多六项、合计 4 MiB。工作机在运行期间提供本地文件路径。只读当前消息的正文与附件。 |
| Agent 返回文字和文件产物 | 首条较快完成的回复使用来源消息的回执，延迟完成或后续分段按原会话 ID 发送。带有实际文件内容且绑定人仍有读取权的产物会先分片上传，再发回原来的私聊或群。 |
| 网络暂断或收发限流 | 绑定设备会重新订阅，保留已接收的待处理消息和投递记录。企业微信明确限流时会延迟再试。发送后未收到回执时，可在「处理未确认回传」核对原会话，再确认已送达、放弃或要求重发。 |
| 在后台重置凭据、收到长连接被取代通知或主动解绑 | 上游拒绝授权或通知连接被取代后，接收程序停止并将绑定标为失效。主动解绑停止后续接收和投递，之前的对话仍可按原权限查看。重新接入需要有效凭据。 |

设备通过官方公共云长连接 `wss://openws.work.weixin.qq.com` 验证 BotID 和 Secret。Windows 用当前操作系统用户的 DPAPI 保护本地凭据、回执和缓冲的消息正文。团队服务保存绑定、私有对话、运行与投递状态。附件在临时下载窗口内重试，密钥、媒体地址和明文不会写入诊断计数。[长连接和媒体接口](https://github.com/WecomTeam/aibot-node-sdk/blob/main/src/client.ts) · [官方附件解密规则](https://github.com/WecomTeam/aibot-node-sdk/blob/main/src/crypto.ts)

## CLI

用 `shipforge request` 的标准输入提交 JSON。连接请求里的 `helper` 是空间内的 Agent ID。返回的 `id` 用于管理该绑定。

```json
{"op":"bot.bindings","space":"default","available_for":"wecom"}
```

```json
{"op":"bot.wecom.connect","space":"default","agent":"helper","bot_id":"<BotID>","secret":"<长连接 Secret>","dm":true,"groups":true}
```

```json
{"op":"bot.policy","space":"default","id":"<绑定 ID>","dm":false,"groups":true}
```

```json
{"op":"bot.sync","space":"default","id":"<绑定 ID>"}
```

每个完成的收发轮次返回 `received` 和 `outgoing`。后台仍在接收或上传大文件时可返回 `state: "receiving"` 和 `background: true`。`rejected` 统计该轮因无效数据或附件解密失败而跳过的消息，`waiting_media` 统计等待在下载窗口内重试的消息。长期接收用 `shipforge-worker bot-sync --space default --binding <绑定 ID> --watch`。查看待处理投递用 `bot.bindings`，复核后可用 `bot.retry` 提交 `confirm`、`discard` 或 `resend`。断开用 `bot.disconnect`，传 `space` 与 `id`。

## 已验证的条件

隔离验收使用三个 ShipForge 身份、真实 CLI、工作机和本机的独立企业微信 WebSocket 模拟服务。覆盖错误凭据、订阅时已经在途的消息、群与私聊的开关和上下文、去重、加密附件读取、跨成员读权、分片回传、缺失回执、撤权、连接被替代和解绑。Windows 原生窗口的绑定与管理验收使用同一隔离服务。[服务及工作机验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-wecom-bots.py) · [原生窗口验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/wecom_ui_test.rs)

真实公有云仍需由测试企业管理员创建 API 模式智能机器人，验证成员可见范围、拉群、官方限流与真实文件送达。企业私有化部署使用管理员提供的专属网关地址，当前客户端连接公共云。空间任务助手还有独立的身份、运行配置和企业微信入口，其绑定及任务回注需单独验收。[产品中的两种企微入口](https://help.hifox.com/docs/integration/wecom)
