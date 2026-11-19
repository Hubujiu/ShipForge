# 自动化运行与会话

Windows 的「自动化」页面在规则详情中显示状态、最近一次可见运行的健康度，以及按新到旧排列的运行记录。每页八条，可向前或向后翻页。每一次新触发都会创建独立运行和私有对话。对应规则可复用同一 Agent 或小队。触发源任务作为运行上下文，运行不会因此新建任务。[HiFox 自动化说明](https://help.hifox.com/docs/space/automations)

## 负责人和触发来源

状态和评论触发器默认使用实际修改任务或发表评论的成员作为会话负责人。Windows 编辑规则时可以选择「状态或评论触发的会话负责人」。CLI 对应字段为 `fields.run_owner`，空值或 `triggerer` 表示触发者。可在单个状态或评论触发器上单独指定 `run_owner`。可选 `fields.project` 绑定当前空间的专项。负责人须有当前规则、执行者、来源任务及指定专项的访问权限。失权造成的跳过会出现在有权限查看的记录中。

手动运行归执行该操作的成员。时间表使用规则创建者，来自已配置 Webhook 的运行按接收入口的授权身份启动。运行记录区分手动、时间表、状态、评论和 Webhook，保留可用的触发器标识、原始事件、任务或评论以及预定时间。通过私密 Webhook 收到的投递不会虚构人工触发者。

负责人可打开完整对话，读取输出和继续追问。规则创建者及通知成员可看到其有权访问的规则与来源产生的结果摘要。其他成员无法借收件箱或通用运行查询读取负责人的对话、任务快照或 Webhook 请求体。私有会话由能以负责人身份领取任务的工作机执行。退出空间、失去来源任务权限或撤销专项访问后会重新进行权限检查。

## 暂停、重试和归档

- **暂停规则**会停止新的触发，已排队且尚未启动的时间表运行会记为跳过。恢复后从恢复时刻开始安排后续计划。
- **重试此运行**用于已失败、已取消或租约中断的运行。负责人需仍能访问历史上下文及原执行者，规则须处于启用状态。重试复用原运行 ID、已保存的指令、触发上下文和对话，即便规则之后改写过指令。再次发起新工作时请使用「运行一次」。
- **归档规则**保留权限范围内的运行和对话，停止新的计划和重试。详情页仍可打开已有对话。

健康度显示所能看到的最新运行状态。失败、取消、跳过和租约中断显示「需要关注」。已完成显示「健康」，尚无历史显示「尚无运行」。处于队列或执行中的记录会显示相应状态。规则是否暂停或归档单独显示。

## 无人值守工作机

自动化、渠道机器人与被派发的子工作以无人值守方式执行。运行输入的 `fields.interaction` 告诉适配器人工问答、审批均不可用。Runtime 可以通过一行完整的 JSON 向标准输出报告 `{"event":"approval_request"}`、`{"type":"request_user_input"}` 或等待结果 `{"status":"waiting"}`。遇到此类请求时工作机终止该次进程树，核心以 `unattended_input` 记录失败原因。普通有人值守的运行仍支持等待回复。仅静默阻塞、未发送任何结构化信号的外部程序由正常运行超时处理。

## CLI 示例

```json
{"op":"automation.status","space":"default","id":"release-check"}
```

```json
{"op":"automation.history","space":"default","id":"release-check","limit":25}
```

下一页传回上次的 `next_cursor` 作为 `cursor`。范围为 1 至 100 条，默认 25 条。负责人用 `automation.conversation` 带规则 `id` 和 `run` 打开对话，再用 `chat.send` 发送追问。`automation.retry` 接受同一组 ID。网络重试时复用同一个 `request_id`。`automation.lifecycle` 接受 `pause`、`resume` 和 `archive`，提交前使用 `automation.status` 中最新的 `rev` 作为 `expected_rev`。

本地验收使用三种身份、独立服务、实际工作机进程及 Windows 原生控件。测试子进程发出授权请求后等待两分钟，工作机会提前终止它。另一轮重试读取原始指令并继续同一对话。[工作机及 HTTP 用例](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-automation-history.py) · [权限、时间表与分页](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/automation_history.rs) · [Windows 操作](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/automation_run_ui_test.rs)
