# Agent 排队、目录与诊断

## 设置运行额度

在 Windows 端打开「Agent」，选择成员并编辑设置。「任务并发数」和「对话并发数」分别限制任务和对话，默认各为 1，最多可设 64。选择「共用任务额度」后，两类运行合计使用任务额度。所有 Agent 还受机器总容量约束。

同一对话逐条执行。同一任务的不同对话可以并行。不属于对话的执行与同一 Agent 在该任务内的其他执行互斥。开启共享工作目录的小队在同一台机器上按顺序执行同一任务。

开始执行时才计算运行时限。默认 180 分钟，可设 15 至 43,200 分钟。工作机可再限制单个外部程序的运行时间，先到期的限制生效。排队默认允许 24 小时，可设 1 至 168 小时。超时产生失败记录，重新尝试会领取新租约，重新计算时限。

```json
{"op":"update","kind":"agent","id":"review","fields":{"max_parallel":3,"chat_parallel":2,"chat_capacity_mode":"independent","run_timeout_minutes":180,"queue_timeout_hours":24}}
```

## 排班与立即触发

编辑 Agent 时选择「排班方案」。选择「工作日夜间及周末」后，工作日为当地时间 00:00–08:00 和 20:00–24:00，周末全天可执行。自定义排班每天最多两段。IANA 时区按当地夏令时计算。任务指派、评论提及默认遵守排班，直接聊天可单独勾选。

```json
{"op":"update","kind":"agent","id":"review","fields":{"working_hours":{"preset":"off_peak","timezone":"Asia/Shanghai","triggers":{"assignment":true,"comment":true,"chat":true}}}}
```

在任务的「预约执行」输入带时区的日期时间，例如 `2027-06-02T20:30:00+08:00`。通过 CLI 执行 `run.enqueue` 时也可设 `scheduled_at`。指定时刻如尚未开班，系统顺延到下一次开班。任务、对话和运行详情会显示等待横幅。首次开班前保留全部排队额度。已启动的任务可跨过下班时间继续运行。

紧急时点击「立即触发」，或执行下方请求。机器、权限、并发和目录条件照常检查。

```json
{"op":"run.trigger_now","id":"RUN_ID"}
```

## 为机器授权目录

打开「机器」→「授权本机工作目录」，选择此机器的配置文件，给项目目录命名，点击「浏览…」选择文件夹。Git 项目可用隔离工作树。需要直接修改文件时勾选「允许直接写入此目录」。也可在目标机器运行命令。

```powershell
shipforge-worker approve-directory --config D:\ShipForge\machine.json --name app-a --directory D:\Projects\AppA --direct
shipforge-worker approve-directory --config D:\ShipForge\machine.json --name app-b --directory D:\Projects\AppB --direct
```

工作机会在后续心跳上报变更。编辑 Agent 时选择「指定机器」、目录策略和「获准工作目录」。远程选择器展示目标机器公布的目录名和可用模式。直接模式可顺序选多个目录。首条空闲路径接单，全部占用时继续排队。同一路径的不同别名也互斥。隔离运行可指定一个 Git 项目，也可使用机器默认的隔离环境。

```json
{"op":"machine.directories","id":"build-box"}
{"op":"update","kind":"agent","id":"review","fields":{"machine_id":"build-box","directory_mode":"direct","profiles":["app-a","app-b"]}}
```

同一任务或对话沿用原先的目录和模式。从 Agent 默认列表中移除某个目录后，原绑定继续生效。若机器撤回授权，任务可重新选择有效目录。已绑定的对话会在启动前报告原目录不可用。恢复该授权或新建对话可继续工作。隔离工作树按任务或对话保存。不同私有上下文有独立的本机空间。

## 查看诊断

运行详情的「运行诊断」显示状态、尝试次数、机器、运行时、等待原因、时限和处理建议。上游返回 429 时，工作机使用服务错误代码区分限流与账单额度，缺少代码则保留「服务返回 429」。403 表示拒绝访问，408 与 504 表示请求超时，413 表示请求体过大。模型、上下文、图片尺寸、格式和功能不匹配也有独立类别。

```json
{"op":"run.diagnostics","id":"RUN_ID"}
{"op":"machine.diagnostics","id":"build-box"}
```

机器详情显示在线状态、容量、已公布运行时及当前用户有权查看的活动运行。在机器所在的桌面打开「机器」→「本机进程与日志」，或执行 `shipforge-worker diagnose --config D:\ShipForge\machine.json`，可查工作机管理的活动 PID、尝试次数和最近 100 条状态事件。日志达到 512 KiB 后轮换，保留上一份。诊断记录只保存运行标识、时间、进程号、状态和标准错误类别。

这些规则对应 HiFox 的运行、排班、工作目录与排障流程。[Agent 运行设置](https://help.hifox.com/docs/agent-and-computer/agents) · [工作时间](https://help.hifox.com/docs/agent-and-computer/working-hours) · [工作目录](https://help.hifox.com/docs/agent-and-computer/working-directory) · [执行排障](https://help.hifox.com/docs/agent-and-computer/agent-task-execution)
