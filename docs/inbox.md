# 收件箱与通知

打开左栏「收件箱」，可以查看当前组织中自己有权访问的各个空间。任务的指派、提及、评论及属性更新按接收者和任务聚合。进入「重要」后，普通更新会继续追加在同一行。读状态和重要性分别保存。「重要」会在下次进入时恢复，「其他」下次回到「全部」。[HiFox 收件箱](https://help.hifox.com/docs/agent-and-computer/inbox)

| 要完成的操作 | Windows 中的入口与结果 |
| --- | --- |
| 找到提醒 | 「筛选收件箱」可查任务标题、已收到且仍有权读取的评论或对话正文，支持包含空间、排除空间和仅未读。分段显示各自的未读数。 |
| 从提醒回到工作 | 点击一行后原地打开任务、会话或自动化详情。与评论关联时展开相关讨论，失去评论权限时回到可读任务。具有只读令牌的成员可查看详情。 |
| 稍后处理 | 可选 20 分钟、两小时、明早、下周一或自定义本地时间。到期后返回原分区，新的直接提及可让任务提前回来。 |
| 批量清理 | 「全部标为已读」与「清理当前分区」遵循当前搜索、分区和空间条件。清理可选全部或指定状态，并记住选择。可从「已清理」恢复。 |
| 管理关注者 | 任务详情显示关注操作和「更改关注者」。可搜索并添加成员、Agent 或邀请中的成员。创建者、处理人、评论者、任务提及对象和成员字段选中的人自动关注。成员可取消关注，编辑其他字段会保留选择。仅已加入且仍能访问任务的成员接收提醒。 |
| 设置通知 | 打开「通知设置」，选择渠道后切换总开关、事件组或单项，并可恢复该渠道默认值。每个成员在同组织的设置由服务端保存。关闭桌面事件仍保留收件箱记录。 |

Windows 工作台打开时约每五秒读取新提醒。前台窗口有可直接打开事项的横幅；窗口处于后台时还会向 Windows 通知中心提交原生横幅。系统横幅使用安装程序注册的开始菜单应用标识和应用链接。系统设置、专注模式与通知权限会影响展示。便携版在窗口内提供同一入口。

九个渠道均可保存事件选择。收件箱、Windows 和邮件已接入事件源。移动推送、浏览器推送以及飞书、钉钉、Slack、Lark 的个人通知需要相应订阅及授权。在已绑定的 Slack 任务话题中，可以直接向原线程回传消息。[Slack 任务话题](../crates/shipforge-core/PROVIDERS.md)

## 邮件邀请与工作提醒

团队服务使用和邮箱登录相同的发件人、服务凭据和公开 HTTPS 地址。管理员在成员页发出邮箱邀请后可查看邮件状态。收件人从邮件打开浏览器中的「在 ShipForge 中查看邀请」，再用受邀邮箱登录桌面并确认。管理员停用邀请后，链接随即失效，等待重试的发件记录会取消。[发件配置](account-lifecycle.md)

成员可以单独开启邮件，关闭其他渠道。工作提醒只发往该账号已经验证的邮箱。邮件使用通用标题，打开链接需要当前身份仍有权访问原来的任务、会话或运行。桌面的「通知设置 → 邮件 → 邮件投递记录」可查看状态。发送前会再核对权限、偏好和邀请有效期。

服务将待发信件和邀请或业务变更一同写入本地数据库。每封使用独立的幂等键，网络超时、429 或服务故障会在保留期内用同一个键重试。收到成功回执显示「邮件服务已接收」。明确被拒收的邀请可重试，超过幂等保留期且发送结果未知的信件进入待确认状态。邮箱供应商的最终投递、垃圾邮件过滤和收件人打开情况以该服务的记录为准。[Resend 幂等规则](https://resend.com/docs/dashboard/emails/idempotency-keys)

## CLI

```json
{"op":"inbox.query","space":"default","section":"important","query":"验收","exclude_spaces":["archive"]}
{"op":"inbox.bulk","space":"default","section":"other","action":"clear","statuses":["done"]}
{"op":"inbox.configure","space":"default","channel":"desktop","key":"assignment","enabled":false}
{"op":"inbox.configure","space":"default","channel":"desktop","reset":true}
{"op":"followers.change","space":"default","id":"task-id","target_kind":"member","target":"member-id","enabled":true}
```

桌面订阅端先调用 `inbox.feed` 获取当前游标，把响应的 `cursor` 作为下一次请求的 `after`。切换身份或组织后重新开始订阅。系统链接只定位到签发它的工作区，每次打开都会复查接收者和目标权限。[HiFox 关注规则](https://help.hifox.com/docs/task/follow-tasks) · [Windows 通知激活格式](https://learn.microsoft.com/en-us/uwp/schemas/tiles/toastschema/element-toast)
