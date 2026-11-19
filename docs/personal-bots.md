# 个人微信与 QQ 机器人

Windows 工作台的「外部应用集成 → 微信与 QQ」可把一个 Agent 接入手机会话。绑定后，外部文本进入该 Agent 的私有对话。用户可在不同的 QQ 单聊、群里同时使用一个机器人，每个会话单独保存上下文。Agent 需要有可用的运行环境和工作机。[HiFox 微信说明](https://help.hifox.com/docs/integration/wechat) · [HiFox QQ 说明](https://help.hifox.com/docs/integration/qq)

| 操作 | 个人微信 | QQ 机器人 |
| --- | --- | --- |
| 连接 | 选择「微信扫码」及 Agent，用本人微信扫描二维码，并在手机确认。出现验证码时在 Windows 输入手机上的数字。二维码失效后选「重新生成二维码」。 | 选择「QQ 扫码」及 Agent，用手机 QQ 扫码授权。持有官方机器人 App ID 与 App Secret 的管理者也可以选择「使用 QQ 凭据」。 |
| 收取 | 接收完成授权的微信用户与机器人的文字消息，也可读上游已提供的语音转写。发送 `/new` 后得到确认，之后的消息进入新会话。 | 接收官方网关派发的单聊和群 @ 事件。机器人必须对相应会话可用。在接收范围里可分别关闭单聊或群 @。 |
| 回复 | 完成的运行会返回文字和已附带文件内容的产物。每个文件与对应文字使用原来消息的回传凭证。单文件上限为工作区附件的 4 MiB。 | 通过官方单聊或群消息接口向原会话发送文字，携带原消息 ID 和分段序号。实际可发送次数、时限与内容类型由 QQ 开放平台决定。 |
| 管理 | 同一工作区的一个微信账号同时绑定一个 Agent。换 Agent 时先解除旧绑定。 | 同一绑定用户在同一空间用原机器人绑定其他 Agent 会替换旧绑定。跨成员或跨空间转移需原绑定所有者先解绑。 |

「启动后台接收」可在设备重启后恢复接收。桌面完成扫码或凭据绑定后会启动同目录的 `shipforge-worker`。保持设备联网，工作机要能领取所选 Agent 的运行。断线后，后台进程重试。微信从已确认的游标继续，QQ 使用已提交的网关会话和序列号恢复。上游宣告会话无效时重新识别。微信登录过期或 QQ 身份凭据被拒绝时，需重新授权。可在 Agent 详情中打开同一管理入口。

相同上游消息 ID 在同一账号和会话中只创建一个运行。一条结果有多个部分时依次投递。限流后按延迟重试。设备在投递过程中断线时，服务端可能还无法确认对方是否接收。此时在「处理未确认回传」中，根据手机上的实际结果选择「已送达」「放弃」或「重新发送」。解除绑定后停止接收和后续投递，历史记录保存在原工作区；已交付给平台的请求由平台处理。

团队服务记录绑定用户、Agent、渠道账号标识、事件及投递状态。绑定明细只可由所属空间的绑定用户读取，操作需要其空间编辑权。Windows 设备使用当前用户的 DPAPI 保护扫码密钥、机器人凭据、微信回传凭证和检查点。后台进程需要使用建立绑定时的操作系统用户及工作区配置。其他系统依赖用户专属设备配置目录的文件权限。

### CLI

在现有工作区中准备好 Agent 后，通过 `shipforge request` 的标准输入传入 JSON。下面的 `helper` 为该空间的 Agent ID。返回的 `id` 用于查询扫码、同步或解绑。

```json
{"op":"bot.qr.start","space":"default","provider":"wechat","agent":"helper"}
```

```json
{"op":"bot.qr.check","space":"default","id":"<扫码操作 ID>"}
```

若微信返回 `need_verifycode`，可在下一次查询中加入手机显示的 `verify_code`。QQ 的 `provider` 为 `qq`。返回的 `qr_matrix` 是由 0 和 1 组成的方阵；Windows 会直接显示二维码。应将 `qr_content` 和扫码界面交给发起绑定的用户。

```json
{"op":"bot.policy","space":"default","id":"<QQ 绑定 ID>","dm":true,"groups":false}
```

```powershell
shipforge-worker bot-sync --space default --binding <绑定ID> --watch
```

`bot.bindings` 可取当前绑定与待处理投递，`bot.sync` 执行一轮收发，`bot.disconnect` 停止绑定并清除当前设备保存的相关凭据。QQ 官方网关自行建立出站 WebSocket，微信使用 iLink 长轮询。两项均可在能访问腾讯服务和工作区的 Windows 设备上运行。[腾讯 iLink 协议](https://github.com/Tencent/openclaw-weixin/blob/main/docs/protocol.md) · [QQ Bot 授权 SDK](https://github.com/tencent-connect/qqbot-agent-sdk) · [QQ 消息发送接口](https://github.com/tencent-connect/bot-docs/blob/main/docs/develop/api-v2/server-inter/message/send-receive/send.md)

### 验收范围

本地测试启动正式的 ShipForge CLI、团队服务、仿真 iLink HTTP 与 QQ WebSocket。合成账号验证扫码失效与验证码、QQ 凭据解封、断线恢复、权限隔离、向原会话回复以及微信附件解密。Windows 用例通过真实 GPUI 控件和无焦点 D3D 窗口检查二维码、凭据表单、范围设置和解绑。真实手机授权、上游帐号灰度、网络可达性、平台审核与反滥用限额需在经授权的独立微信和 QQ 账号上联调。当前微信入口采用本人扫码流程；其他所有者代绑和入站图片、文件解读属于后续能力。[三身份传输验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-personal-bots.py) · [Windows 控件验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/personal_bots_ui_test.rs)
