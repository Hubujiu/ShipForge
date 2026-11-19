# ShipForge 命令行与 JSON 协议

### 个人小助手与群头像

`assistant.open` 首次为当前组织成员创建系统助手，返回首选 Runtime 和最近会话。`assistant.history` 以只读方式查看已建立的助手、可用 Runtime 和最多 200 条会话。`assistant.send` 接受 `body`、可选的 `runtime`、`model` 和 `request_id`。省略 `id` 开始新会话，提供 `id` 继续同一会话。结果中的 `space` 是运行和消息所在的空间。使用该空间和返回的 `id` 查询 `chat.history`。

```json
{"op":"assistant.send","space":"default","body":"整理这段会议记录","runtime":"local-tool","request_id":"meeting-01"}
```

`chat.avatar` 读取群头像、摘要和群修订号。群主调用 `chat.avatar.set` 时提供 `id` 和 `expected_rev`，用 `content_base64` 上传图片，或用 `"remove":true` 移除图片。可用 `shipforge --space SPACE group-avatar CHAT ./avatar.png --rev 3` 直接选本地图片。参与者通过 `chat.info` 或 `chat.sidebar` 获得缩略图。[Windows 使用方法](conversations.md)

`machine.storage.preview` 返回工作机存储占用、受保护副本和清理结果。工作机所有者使用 `machine.storage.request` 请求统计或确认清理；工作机用 `machine.storage.take`、`machine.storage.reserve`、`machine.storage.finish` 完成操作。`agent.runtime.options` 返回可见机器公布的模型、思考深度和启动参数，`run.enqueue` 可传本次运行的 `model` 和 `reasoning_effort`。[存储及运行参数示例](storage-and-models.md)

`network.settings` 查看设备代理，`network.test` 测试当前草稿，`network.save` 按 `expected_rev` 保存并应用。`device.about` 读取安装版本及随包手册，`device.update` 查询正式版和 Windows 安装包。五项操作可独立于项目数据库运行。[设备网络与更新](device-network.md)

`status.library` 读取当前组织的状态定义、译名、用量及其在当前空间的 ID。`status.library.save` 创建或按 `expected_rev` 编辑定义。`task_type.workflow` 读取当前空间指定类型的有序状态及有效默认值，`task_type.workflow.save` 按类型的修订号保存有序的 `statuses` 和可选 `default_status`。`status.library.removal` 预览删除与迁入影响，`status.library.remove` 使用同一目标及刚获得的 `preview` 原子迁移数据。[组织状态与类型工作流示例](status-workflows.md)

`machine.runtimes` 读取已发现的工具、版本、在线状态与操作历史。机器所有者通过 `machine.runtime.request` 发起 `rescan`、`install`、`upgrade` 或 `test`。受托工作机通过 `machine.runtime.publish`、`machine.runtime.jobs`、`machine.runtime.finish` 与 `machine.stopped` 报告执行情况。本机 `shipforge-worker service` 管理监督进程、Windows 登录与退出策略。[机器管理命令与配置](machine-management.md)

`organization.ownership` 使用组织当前修订号、成员 ID `target` 和 `action` 管理所有者。可选 `grant`、`transfer` 和 `revoke`。转让时原所有者成为管理员。每个组织至少保留一位所有者。`space.transfer` 将指定子空间的管理权交给已加入该空间的成员，需提交该空间的 `expected_rev`。[组织与空间管理权](navigation.md)

`delivery.history` 列出当前身份仍可查看的邮件动态，传 `invitations:true` 可由组织管理员查邀请邮件。`delivery.open` 接受邮件落地页中的 `shipforge://activity/…` 链接，返回该身份有权打开的目标和关联评论。`delivery.retry` 使用投递 ID 重试邮件服务近期明确拒收的请求。发件服务的 `accepted` 表示已取得供应商回执。[邮件投递与状态](inbox.md)

`organization.settings` 读取当前空间所属组织的语言、优先级档数、修订号和成员数。`organization.configure` 使用 `expected_rev` 保存 `default_language` 或 `priority_levels`。`organization.depart` 使用 `action:"leave"` 或 `"dissolve"`，提交修订号及组织名称 `confirmation`，删除还须提交刚确认的 `members` 人数。[组织设置及数据保留](navigation.md)

`directory.people` 按对象类别查本组织已加入空间的成员和机器。`search` 可指定 `search_scope:"organization"` 跨空间查找。`chat.start` 支持真人私聊、Agent、小队和带初始成员的群聊，`request_id` 使重复请求返回同一会话。[请求示例](navigation.md)

| 平台 | 查找账号 | 授权与轮换凭据 | 发现项目或 List | 预览导入 |
| --- | --- | --- | --- | --- |
| Linear | `linear.accounts` | `linear.authorize`、`linear.credential` | `linear.projects` | `linear.preview` |
| ClickUp | `clickup.accounts` | `clickup.authorize`、`clickup.credential` | `clickup.projects` | `clickup.preview` |
| ONES | `ones.accounts` | `ones.credential` | `ones.projects` | `ones.preview` |

以上操作由集成管理员使用。Linear 和 ClickUp 支持带 state 的浏览器授权，三家平台均可使用个人凭据。`imports.sync_control` 暂停或恢复工作机轮询，`imports.push` 发布已映射的状态或一条本地公开评论。[工作区层级、凭据、字段与回写示例](cloud-trackers.md)

`profiles.verification` 读取本机发起、在十分钟内有效的邮箱验证状态。安装版从邮件网页的「打开 ShipForge 桌面版」链接恢复同一验证表单。

邮箱登录或注册使用 `profiles.login.start` 和 `profiles.login.finish`。读取或修改资料用 `access.account.profile`、`access.account.update`。通过 `access.account.impact` 预览组织及共享机器影响。对于会继续存在的组织，按 `spaces_to_transfer` 列出的子空间先转让管理权。用 `profiles.close.start` 与 `profiles.close.finish` 在验证邮箱后注销账号，服务端操作为 `access.account.verify` 与 `access.account.close`。退出当前会话可用 `profiles.logout`，服务端对应 `access.account.logout`。[邮件配置与数据处理](account-lifecycle.md)

禅道与 PingCode 连接器分别使用 `zentao.credential`、`zentao.projects`、`zentao.preview` 和 `pingcode.credential`、`pingcode.projects`、`pingcode.preview`。用 `imports.commit` 确认预览，`imports.synchronize` 增量拉取并应用，`imports.history` 和 `imports.rollback` 管理批次。启用回写的连接器可通过 `imports.push` 发布已映射状态，或向 PingCode 发布本地评论。`imports.push_history` 查看回执。自建投递端可用 `imports.prepare_push` 和 `imports.finish_push` 操作回写租约。持续同步由工作机 `tracker-sync --watch` 执行。[配置、认证、JSON 示例与故障恢复](tracker-integrations.md)

## Agent 目录、小队与分身

在「市场」可按分类、名称或说明浏览技能包与 Agent 模板。用 `catalog.list`、`catalog.detail` 浏览，`catalog.publish` 发布团队版本，`catalog.install`、`catalog.update`、`catalog.uninstall` 管理空间安装状态，`catalog.instantiate` 创建独立 Agent。空间技能使用 `skill.import` 和 `skill.bind`。从机器依次调用 `skill.discover`、`skill.sources`、`skill.machine_import`。工作机通过 `skill.jobs` 和 `skill.finish` 处理离线积压。小队的 `fields.share_task_directory` 决定后续任务是否在同机复用工作树。 [技能、发布与共享目录示例](skills-market.md)

`agent.directory` 返回当前成员有权使用的 Agent。传 `kind:"crew"` 查看小队，`scope:"organization"` 合并已加入的同组织空间，`query` 搜索名称或描述。`view` 可选 `all`、`mine`、`shared`、`private`、`favorite` 或 `archived`。「我的」按创建人筛选。在 Windows 的「Agents」或「小队」页可切换同样的筛选，并在当前空间和全部可访问空间间切换。设置和执行都使用记录的空间与 ID，可区分同名 Agent。

`agent.organize` 的 `action` 可填 `pin`、`unpin`、`favorite` 或 `unfavorite`。收藏同时置顶，取消收藏保留置顶，其他人的列表独立。`agent.settings` 返回权限、有效指令、逐字段配置和工作机报送的累计 Token 数。缺少用量时桌面显示「—」。`agent.history` 按会话聚合运行，任务运行可回到任务，其他独立运行可打开记录。私有 Agent 的运行历史仅对所有者开放。

`agent.access` 以 `actor` 指定空间成员，`permission` 为 `use`、`manage`、`remove` 或 `transfer`。公开 Agent 默认允许空间成员使用，私有 Agent 由所有者邀请。只有所有者可转让，转让后原 Agent 所有者保留管理权限。`agent.archive` 和 `agent.restore` 接受 `kind:"agent"` 或 `kind:"crew"`。归档 Agent 会取消排队并向正在执行的工作发出取消请求。归档小队会停止接收新指派，已开始的执行及历史会话保留。归档列表中可以恢复。任务评论的 `fields.agent_mentions` 接受 Agent ID 数组，提交时会在该任务的对话中排入请求。

`agent.variant` 在 `target_space` 指定的同组织空间创建分身，返回分身的空间与 ID。创建人必须能使用原 Agent 并能编辑目标空间。原 Agent 的 `variant_permission` 默认 `use`，设为 `manage` 可限制创建者。`allow_variant_environment:false` 使其他人创建的分身跳过原 Agent 的环境变量。分身引用原配置和技能，每项可独立覆盖，用 `agent.resolve` 检查后续运行将采用的配置。`agent.override` 设置 `field` 和 `value`，将值设为 `null` 可恢复继承。技能始终跟随原 Agent。已入队的运行保留当时的配置与技能版本，源权限撤回后也会检查旧快照的访问权。指定源空间机器时，目标空间中需登记同一机器身份，也可以在分身覆盖为目标空间自己的机器。 [HiFox 分身规则](https://help.hifox.com/docs/agent-and-computer/agent-variants)

```json
{"op":"agent.variant","space":"design","id":"review","target_space":"delivery","title":"发布检查","request_id":"review-for-delivery"}
{"op":"agent.override","space":"delivery","id":"替换为返回的分身 ID","field":"model","value":"review-model"}
{"op":"agent.override","space":"delivery","id":"替换为返回的分身 ID","field":"model","value":null}
```

`crew.exclusive` 接受小队 `id`、成员 `agent` 和布尔值 `exclusive`。管理者可在小队详情操作已加入小队的非队长 Agent。专属成员由该小队委派，空间目录与独立接单选择器隐藏该成员。旧私聊与任务可回看，解除专属后恢复使用原有权限。 [HiFox Agent](https://help.hifox.com/docs/agent-and-computer/agents) · [HiFox 小队](https://help.hifox.com/docs/agent-and-computer/crews)

`shipforge.exe` 与原生桌面使用同一个业务核心。本机模式使用当前用户的 ShipForge Workspace 数据目录，安装时指定的数据目录也会被桌面和 CLI 读取。`--data-dir <目录>` 可为本次命令选择独立配置。连接团队服务后，命令以令牌对应的成员身份执行。在程序目录的 PowerShell 中可用 `.\shipforge.exe` 调用下文的 `shipforge` 命令。[机器与服务部署](windows.md) · [完整请求 schema](protocol.schema.json)

## 建单、修改和重新打开

### 收件箱、关注与通知渠道

`comment.reactions` 查看可读评论的表情、数量及本人是否已回应，`comment.react` 发送 `id`、`emoji` 和布尔值 `enabled` 添加或撤销本人的回应。

`inbox.query` 按组织内当前可访问的空间汇总个人提醒，支持分区、搜索、包含与排除空间、任务状态及分页。`inbox.locate` 按当前权限读取提醒目标，`inbox.open` 同时标为已读。`notification.act` 处理单条，`inbox.bulk` 只读标记或清理命中当前分区和筛选的提醒。`inbox.settings` 返回个人渠道、事件和已存选择；`inbox.configure` 更新开关或恢复默认。`inbox.feed` 返回新增桌面通知和游标。`followers.list` 列出可见关注者，`followers.change` 添加或移除单个成员、Agent 或邀请对象。[筛选与投递示例](inbox.md)

TAPD 的 `tapd.credential` 保存当前身份的凭据，`tapd.projects` 发现可见项目，`tapd.preview` 的 `import_mode` 可选 `history`、`changes` 或 `refresh`。使用 `imports.commit` 确认当前预览。[TAPD 权限和模式](vendor-imports.md)

### 对话列表、分叉与任务

`chat.sidebar` 返回当前成员可见的对话，置顶项在前，其余按最近可见消息排序。`chat.organize` 接受 `id` 和 `action`，动作可选 `pin`、`unpin`、`hide` 或 `undo`。撤销隐藏需原操作返回的 `undo_token`。对话产生本人可见的新消息后会重新出现。`chat.history` 按原始发言顺序返回本人仍有权读取的消息。

`chat.fork` 以指定 `message` 为最后一条继承消息，返回新分支记录。普通 Agent 对话和任务 Agent 对话均可分叉。`chat.bind_task` 用现有 `task` 绑定当前对话，`chat.convert` 则让 Agent 根据对话起草新任务。两项绑定操作均由对话所有者发起。`chat.send` 以 `body` 发送消息并将本轮排入该对话的运行队列。转换排队时返回运行 ID。工作机成功调用 `run.report` 时可提交 `task_draft` 对象，包含 `title` 和 `description` 两个字符串。创建和绑定在同一次数据库事务内完成。发送、分叉与转换可设置 `request_id` 安全重试。 [对话操作与示例](conversations.md)

Zoho BugTracker 的连接器支持 `zoho.credential`、`zoho.projects` 和 `zoho.preview`。预览号可用于 `imports.commit` 和 `imports.rollback`，`imports.history` 查看批次。腾讯 Bugly 使用 `bugly.rotate` 创建提单地址，管理员可通过 `bugly.credentials`、`bugly.deliver` 和 `bugly.outbox` 管理可选的状态回写，`bugly.revoke` 断开接入。[字段、授权与可执行示例](vendor-imports.md)

自建导入客户端先用 `imports.binding` 读取项目、绑定修订号和检查点，再用 `imports.stage` 提交该批来源数据并生成预览。Bugly 投递程序通过 `bugly.binding` 读取绑定的组织与产品，用 `bugly.claim` 领取待投递记录，随后用 `bugly.settle` 提交投递结果及租约。[请求字段与重试](vendor-imports.md)

切换任务类型时用 `task.type.preview` 查看旧字段值的去向，再提交目标状态、需要补齐的值和用户确认。管理员解绑前可用 `task_type.bindings.preview` 查看受影响的任务数。保存视图支持 `view.catalog`、`view.organize`、`view.preview`、`view.run`、`view.save_as`、`view.share` 和 `view.open`。任务评论可用 `comment.thread` 定位，用 `comment.resolve` 或 `comment.reopen` 更新整个讨论串，执行中的 Agent 通过 `run.discussion` 提交。完整参数、权限和 JSON 示例见[任务类型、保存视图与讨论串](task-collaboration.md)。

### 空间成员、停用与侧栏

`space.list` 返回当前身份可发现的空间，以及 `joined`、`can_join`、`can_leave` 和管理权限。组织成员使用 `space.join` 自主加入公开空间，`space.leave` 退出。两者都需要明确的 `space`，操作对象始终是当前身份。私有空间和访客通过成员邀请加入。

`space.navigation` 返回自己的模块、启用状态和固定状态。`space.pin` 接收 `space`、`module` 和布尔值 `pinned`。模块 ID 为 `iterations`、`triage`、`views`、`projects`、`customer_requests`。空间管理员通过 `update` 修改空间的 `iterations_enabled`、`triage_enabled`、`customer_requests_enabled`。客户请求记录使用 `customer_request` 类型，支持标题、说明、联系人和 `open`、`in_progress`、`resolved` 三种状态。

先用 `space.deactivate.preview` 获取空间的 `rev`、未完成任务数和 `preview_token`。再调用 `space.deactivate`，传入 `space`、`expected_rev` 及该 `preview_token`。预览后新增或修改未完成任务时，操作会报告冲突。有未完成任务时须提供 `unfinished:"cancel"`，或 `unfinished:"move"` 和组织内可写的目标空间 `target`。转移时沿用单项任务迁移规则，整批成功才生效。停用会保留历史记录并停止待执行和运行中的工作。空间管理员可用 `space.restore` 恢复写入。已取消的任务保留终态。[空间操作与界面](space-lifecycle.md)

在 PowerShell 中可用对象转换 JSON，复杂描述可直接保存成 UTF-8 文件。下面的操作在临时独立目录创建一项工作，并显示更新后的标题。

```powershell
$workspace = Join-Path $env:TEMP ("ShipForge-example-" + [guid]::NewGuid())
$task = (.\shipforge.exe --data-dir $workspace create task '发布前检查' | ConvertFrom-Json).data
.\shipforge.exe --data-dir $workspace update task $task.id --title '完成发布前检查' --rev $task.rev
.\shipforge.exe --data-dir $workspace get task $task.id
.\shipforge.exe --data-dir $workspace list task
```

`shipforge request --file 请求.json` 接受 UTF-8 文件，也可将 UTF-8 JSON 送到 `shipforge request` 的标准输入。`--pretty` 打印缩进。`--space` 设置默认空间，省略时使用 `default`。普通记录写入可携带唯一的 `request_id`，重试时保留原请求及该标识。更新记录时提供 `expected_rev` 可以检测过期修改。运行租约、Webhook 和第三方同步使用各自的去重标识。成功返回 `{"protocol":1,"ok":true,"data":…}`。失败返回 `error.code`、`error.message` 和非零退出码。常见错误有 `invalid`、`conflict`、`forbidden`、`unauthorized`、`not_found` 和 `offline`。

## 业务操作

标签和分组分别使用 `tag`、`tag_group`。标签支持 `fields.color`、`fields.group`、`fields.archived`。组织标签由组织管理员在根空间管理，设置 `fields.scope` 为 `organization`。子空间的列表会同时列出可用的组织标签。空间标签用于当前空间。旧任务的文本标签在升级时补入目录。

`tag.apply` 在一个事务中处理 `tasks` 的全部任务。`action` 可选 `add`、`remove`、`replace` 或 `clear_group`。前三种提供 `labels`，清除分组提供 `group`。同组最后选择的标签生效。归档保留已有任务和历史筛选。多标签筛选要求同时匹配每个标签。`tag.merge` 提供源 `id` 和同范围的目标 `target`，同时迁移任务和保存视图的条件；删除标签会清除其当前引用。任务跨空间移动时保留组织标签。[标签行为与验收](https://help.hifox.com/docs/task/task-labels)

`onboarding.catalog` 返回六个分组及每项的稳定 ID、说明、行动和分值。`onboarding.get` 按登录身份返回当前组织的完成项、积分、名次、首次设置状态和侧栏选项。`onboarding.display` 接受布尔字段 `sidebar_visible` 或 `sidebar_expanded`。`onboarding.setup` 在新库中保存 `organization_name`、`organization_slug`、`work_type` 与非空的 `task_types`。初始化可使用 `request_id` 重试。得分来自真实业务操作。[新手指引与完成条件](onboarding.md)

| 操作 | 用法 |
| --- | --- |
| `invitation.issue`、`invitation.list` | 组织管理员创建或查看邀请。通用链接设置 `channel: "link"`，角色可选 `member`、`guest`。邮箱邀请设置 `channel: "email"` 和 `email`，也可邀请 `admin`。可指定组织下的 `spaces`、毫秒时间戳 `expires_at` 及重试标识 `request_id`。 |
| `invitation.copy`、`invitation.revoke`、`invitation.accept` | 管理员按 ID 复制或停用邀请。复制已停用的通用邀请会开启新的链接，使旧链接继续失效。收件人以已认证身份提交 `link`，邮箱邀请还要求身份与目标邮箱匹配。重复接受不会提升角色或创建第二份成员记录。 |
| `schema`、`whoami`、`organization.list`、`space.list` | 查看接口、当前业务身份、可访问的组织和空间。CLI 快捷命令为 `shipforge schema`。 |
| `jira.read`、`jira.write` | 传组织 ID 到 `space`，选择 `api_version` 2 或 3，`resource` 例如 `issue/OPS-42` 或 `search`。读取用 `params` 传 `jql`、`fields` 或分页。写入提供 `method`（POST、PUT、DELETE）及 Jira 格式的 `payload`。结果含 HTTP 状态 `status` 和文档 `body`。[客户端与服务配置](jira-api.md) |
| `create`、`list`、`get`、`update`、`delete` | `kind` 指定资源。创建提供标题与 `fields`；更新合并提供的字段。返回记录包含空间、ID、标题、字段、修订号与时间。 |
| `search`、`events`、`dashboard` | 搜索允许访问的对象、按游标恢复增量事件及获取任务统计。搜索使用 `query`，事件使用 `after`。 |
| `view.run`、`task.relations`、`project.progress` | 用对象 ID 执行已保存的筛选、读取任务关系或计算专项进度。 |
| `iteration.timeline` | `{"op":"iteration.timeline","owner":"@me"}` 返回本人负责的可见迭代。可省略负责人或传其他成员 ID。可用 `as_of` 指定展示日期，默认取空间迭代时区的今天。返回按开始日期倒序排列，`fields.planning` 包含起止日期、阶段、可见任务数、完成数和容量。开启估算时包括点数。 |
| `iteration.settings`、`iteration.configure` | 读取空间的迭代设置与修订号。空间管理员传 `fields` 局部更新 `mode`、IANA 时区、起始日、周期、冷却、未来期数及三项自动加入规则。支持 `expected_rev`。[配置字段与日期边界](iterations.md) |
| `iteration.start`、`iteration.history` | 前者用迭代 `id` 启动未开始的手动迭代，或由管理员提前启动自动节奏的下一期。后者读结项时冻结的范围，结果按成员的任务访问权限过滤。 |
| `iteration.preview`、`iteration.close`、`triage` | 用迭代 ID 预览及结项。结项的 `target` 为接收未完成任务的开放迭代 ID，省略或传 `null` 退回待规划。分拣用任务 ID 和 `decision`，可取 `accept`、`defer`、`restore`、`reject` 或 `duplicate`。搁置需未来的毫秒时间戳 `until`。重复项的 `target` 接受任务 ID、编号或同组织任务链接。 |
| `follow`、`notification.act` | 指定任务 ID 和布尔值 `enabled` 切换关注。处理本人的提醒时指定通知 ID 和 `action`，可取 `read`、`unread`、`clear`、`restore` 或 `snooze`。稍后提醒的 `until` 为未来的 Unix 毫秒时间戳。 |
| `preferences.get`、`preferences.update` | 读取或更改当前账号的桌面主题、窗口外边框颜色、语言、任务会话偏好、发送方式与三个应用快捷键。更新可带 `expected_rev`，并按字段合并。[值域与示例](personal-settings.md) |
| `task.move`、`task.conversation`、`task.command` | 选择 `id` 指定任务。移动的 `target` 为同组织中有写权限的空间。会话命令打开本人和执行者的任务会话。快捷指令创建关联任务与会话的运行，可传 `instruction` 和幂等 `request_id`。[任务操作](personal-settings.md) |
| `document.history`、`document.restore`、`artifact.save` | 查看文档历史、用指定历史版本生成当前版本、把授权的产物保存到文档库。 |
| `chat.info`、`chat.invitations` | 查看本人对话角色、提醒偏好及可管理的成员，或列出等待本人处理的对话邀请。 |
| `chat.manage`、`chat.preferences` | 管理群人与 Agent、转让群主，邀请或撤回私有会话访问，退出、解散、个人清理或设置提醒。各项均按当前团队身份执行。[成员与群聊操作](conversations.md) |
| `chat.share.create`、`chat.share.read`、`chat.share.revoke` | 对话所有者用 `id`、`message_ids` 和 `scope` 生成只读快照。读取传 `share`，撤销传 `id` 与 `share`，同一选区的所有有效链接一起失效。[范围、内容与示例](conversations.md) |
| `attachment.upload`、`attachment.download` | 上传、读取任务、消息、文档或运行产物中的真实文件。单文件上限 4 MiB，下载核验 SHA-256。文档可按修订版本取文件。 |
| `export`、`import.preview`、`import` | 导出可见业务对象、预检与事务导入，维护外部标识和引用。先预览导入结果再正式提交。 |
| `agent.resolve`、`catalog.list`、`catalog.install`、`tool.authorize` | 查看 Agent 有效配置、浏览与安装内置模板/技能、核对工具授权。Agent、小队、技能及工具配置由通用对象操作维护。 |
| `git.settings`、`git.configure` | 读取组织、空间或 Agent 的 Git 仓库、作者来源与修订号，按该修订号保存所选层设置。Agent 层需要 `id`。`run.enqueue` 可提交 `checkout_branch`。[Git 工作区示例](git-workspaces.md) |
| `machine.register`、`machine.heartbeat`、`machine.disable`、`machine.enable` | 注册机器、更新实时能力、禁用或重新启用机器。机器凭据保存在机器配置旁的状态目录中。 |
| `run.enqueue`、`run.claim`、`run.heartbeat`、`run.report`、`run.cancel`、`run.retry`、`run.reply`、`run.tool` | 创建运行、认领/续租、提交结果、请求取消、重试或回复。`run.tool` 使用机器密钥、当前租约、工具 ID 和调用 ID 复核并记录授权。 |
| `automation.fire`、`automation.tick` | 触发启用的规则，或扫描到期时间计划。用相同事件 ID 重放同一次触发。 |
| `connector.ingest`、`connector.export`、`provider.preview`、`provider.ingest` | 外部任务三方合并、导出回写数据、预览供应商载荷或落单。绑定连接器和稳定事件标识。 |

记录的常用 `kind` 包括 `task`、`task_type`、`status`、`field`、`tag`、`relation`、`view`、`iteration`、`project`、`member`、`document`、`chat`、`message`、`agent`、`crew`、`skill`、`machine`、`automation`、`connector` 与 `tool`。任务的 `fields` 可含 `description`、`status`、`priority`、`type`、`labels` 和 `due_date`，具体类型的必填字段和角色由核心校验。

### 文件和导入

`shipforge upload .\report.pdf` 创建一份私人文档。`shipforge upload .\report.pdf --kind task --id TASK_ID` 将文件附到指定任务。请求形式为 `{"op":"attachment.upload","kind":"task","id":"TASK_ID","filename":"report.pdf","media_type":"application/pdf","content_base64":"…"}`，响应记录中的 `fields.attachments` 给出附件 ID、文件名、字节数与 SHA-256。

`shipforge download task TASK_ID ATTACHMENT_ID .\copy.pdf` 将校验通过的文件保存到一个新路径。按历史版本下载文档附件时可用 `shipforge download document DOC_ID ATTACHMENT_ID .\old.pdf --version 2`。两种操作均需当前身份能访问所属记录。

一次性导入支持 `shipforge.workspace.v1` 导出的对象集合，也支持表格行的字段映射。先将下例提交给 `shipforge request`，检查预览返回的字段映射、样例、计数与警告，确认后把 `op` 改为 `import`。此例中相同连接器名称与外部编号标识同一条导入记录。

```json
{"op":"import.preview","data":{"format":"shipforge.rows.v1","kind":"task","connector":"sheet","mapping":{"Key":"fields.external_id","Summary":"title"},"rows":[{"Key":"PLAN-1","Summary":"验证工作区"}]}}
```

工作区 JSON 导出保留可迁移记录和附件元数据。包含附件原始文件、历史版本与团队令牌摘要的完整恢复，使用关闭进程后的数据目录备份。[Windows 备份说明](windows.md)

### 自动化和第三方回调

创建 `kind:"automation"` 记录时，将规则配置放在 `fields`。先确保 `handler_id` 对应的 Agent 或小队存在。此规则在上海时间每周一至周五 09:30 触发，也允许接收授权的 Webhook。

```json
{"op":"create","kind":"automation","id":"weekday-report","title":"工作日报","fields":{"handler_kind":"agent","handler_id":"inspector","instruction":"整理当日进展","enabled":true,"triggers":[{"type":"schedule","frequency":"weekly","local_time":"09:30","timezone":"Asia/Shanghai","weekdays":[1,2,3,4,5]},{"type":"webhook","enabled":true}]}}
```

用 `frequency:"daily"` 配置每天执行，用 `{"type":"schedule","interval_minutes":60}` 配置固定间隔。状态、评论和 Webhook 触发器可各自启停。机器定期检查到期时间，重连后按持久化游标补发。手动触发提交 `{"op":"automation.fire","id":"weekday-report","event_id":"manual-001"}`。[时间计划与执行条件](../crates/shipforge-worker/README.md)

服务端规则回调路径为 `POST /v1/hooks/{space}/{rule}`。传入 `{"event_id":"build-42","payload":{"build":42}}`，重投同一事件时保持标识和内容一致。规则必须启用 Webhook 触发器。入站连接器使用另一条路径 `POST /v1/integrations/{space}/{connector}`，可以接收供应商的事件对象或 `{"event_id":"…","payload":{…}}`。服务按目标连接器读取供应商名称。GitLab Secret token 走 `X-Gitlab-Token`。受信中继可用 `Authorization: Bearer <hook令牌>`。系统识别 `X-Gitlab-Event-UUID`、`X-Atlassian-Webhook-Identifier`、`X-ShipForge-Event`，也接受供应商载荷内约定的投递标识。

通过 CLI 送入供应商事件时，给 `provider.preview` 和 `provider.ingest` 都传 `space`、连接器 `id`、`provider` 与 `payload`，并按来源提供稳定的 `event_id`。GitLab、Jira、Linear 和 Asana 可接收工单状态变更。飞书、Slack、Telegram、Teams 和 Discord 的独立文字消息建任务，同一话题的回复写入原任务评论。企业微信与钉钉支持独立文字消息建任务。Asana 事件需要补取任务详情，需要解密或 Gateway/Stream 会话的渠道通过受信中继接入。[各供应商载荷、权限与样例](../crates/shipforge-core/PROVIDERS.md)

管理员可用 `im.bind`、`im.unbind` 管理任务与话题的绑定，请求中传连接器 `id`、`task_id`、`provider` 和已预览的 `payload`。查看任务的外发目标用 `im.threads`。向特定 Slack 话题发送评论用 `im.reply`，传入目标连接器与话题对象、任务 ID、正文和稳定的 `request_id`。标准任务评论供内部讨论。原生 Slack 服务支持签名回调、URL 验证和多空间分流。`shipforge-worker slack-deliver --space <空间> --connector <ID>` 领取已授权的消息并回原线程。队列可用 `im.outbox` 按投递状态查询。`im.claim` 和 `im.ack` 领取与确认一次投递。通过 `im.retry` 核对远端已收消息，或在确认需要重发后重新入队。[Slack 配置与恢复](../crates/shipforge-core/PROVIDERS.md)

GitHub Issues 可通过机器端 `github-sync` 拉取，`github-push` 显式回写。要将 GitHub/GitLab 提交与 PR/MR 关联到已有任务，在对应连接器上设置 `repository`、`enabled:true` 与 `link_tasks:true`。原生 GitHub 与 GitLab 回调或 CLI 验证的原始载荷能按提交正文、请求标题、描述及源分支中的任务编号记录活动。结果含 `links` 和 `review_runs`。可选的 `review_rule_id` 将需要审查的请求路由给已有 Webhook 自动化。任务详情提供原页面链接，PR 合并后也能查看活动。[代码仓库接入与自动审查](../crates/shipforge-core/PROVIDERS.md)

#### GitHub App 与 GitLab 项目配对

`forge.status` 返回当前空间的仓库及本人和组织的连接状态。`id` 取 `github`、`gitlab` 或 `gitlab-self`。管理员用 `forge.authorize` 发起 GitHub 或 GitLab.com 的浏览器授权，再提交同一操作的 `callback`。GitHub 组织授权另传 `installation`。自建 GitLab 使用 `forge.credential` 并传 `instance` 和 `token`。授权及 `forge.disconnect` 的 `purpose` 为 `organization` 或 `personal`。请通过标准输入传入令牌与回调。

`forge.repositories` 枚举安装范围内的仓库或具有 Maintainer 权限的 GitLab 项目。`forge.pair` 接收来源提供的 `repository_id`，方向为 `incoming` 或 `both`，可设置 `backlinks`、`pull_requests`、`commits`。私有空间向公开仓库外发时需传 `confirm_public:true`。以 `direction:"off"` 和 `pair` 解除仓库配对。配对后才新建的来源 Issue 会同步为任务。

`forge.thread` 按 `task_id` 读取来源信息和评论。以 `pair`、`task_id`、`body`、稳定的 `request_id` 调用 `forge.reply` 可向双向配对的 Issue 发言，回执含 `identity` 和 `fallback`。`forge.refresh` 从来源读取该配对任务的最新字段。`forge.push` 显式发布本地修改，远端版本冲突时先刷新。`forge.unlink` 解除单个任务的关联并保留任务。三者都传 `pair` 和 `task_id`。`forge.deliver` 让管理员重试一个仓库配对的待发任务回链。`forge.internal` 由已验证的服务适配器调用；团队命令端点会拒绝直接提交此操作。[部署、Webhook 与 Windows 操作](repository-integrations.md)


### Agent 工作时段

Agent 的 `fields.working_hours` 可设每周排班。例如对已有 Agent 提交 `{"op":"update","kind":"agent","id":"inspector","fields":{"working_hours":{"timezone":"Asia/Shanghai","week":{"1":[{"start":"09:00","end":"12:00"},{"start":"13:30","end":"18:00"}]}}}}`，可让它仅在周一两个时段接新运行。周一至周日用 `1`–`7` 的字符串键表示。分身可用 `agent.resolve` 查有效配置；设成 `null` 恢复继承。旧版整点加固定偏移的配置继续有效。[工作时段与夏令时](../crates/shipforge-worker/README.md)

## 团队身份与离线草稿

Windows 左侧的账号按钮打开本机保存的团队身份。每个账号分别保存最近进入的空间、页面、任务或视图。重启和切换会用该账号重新检查访问权。原有任务不可见时返回可访问空间的任务列表，已失效账号会标为需要重新登录。「从本机移除账号」移除该账号保存的登录信息，其他账号和服务端令牌可以继续使用。[HiFox 账号管理](https://help.hifox.com/docs/other/account-management)

组织 Guest 仅可进入显式加入的下属空间。普通组织管理员可用 `space.list` 以及 `get` 或 `update` 空间对象查看和维护私有空间元数据。读取任务、列成员或接收该空间事件需已加入，写入取决于本人空间角色。将私有空间公开需要其内容管理权。单机工作区的安装主体 `owner` 拥有跨空间运维权限；团队服务签发给 `owner` 的令牌在其范围内也代表同一高权限身份。

| 操作 | 用法 |
| --- | --- |
| `access.whoami`、`access.issue`、`access.list`、`access.revoke` | 查询当前登录身份；安装所有者可发放、列出或撤销令牌。发放时提交 `actor` 和 `scope`；明文令牌只在创建响应出现。令牌管理与空间成员权限一起生效。 |
| `access.personal.issue`、`access.personal.list`、`access.personal.revoke` | 成员用团队登录凭据创建自己的个人令牌，提供用途 `label` 和有效期 `lifetime`。可选 `30d`（默认）、`90d`、`1y`（365 天）或 `never`。列表限本人的有效令牌，显示前缀、创建、最近使用、到期和关联机器，时间采用 Unix 秒。吊销传令牌 ID。 |
| `profiles.list`、`profiles.use`、`profiles.forget` | 读取设备的账号列表与当前 ID，或按保存的 `id` 切换及移除。使用 `shipforge connect <服务地址> --token-stdin` 增加账号；附加 `--account <ID>` 可为同一服务与身份重新认证。旧版单账号文件在升级后仍可使用。 |
| `profiles.remember` | 附带当前账号 `id` 以及 `location`，例如 `{"space":"default","page":"tasks","kind":"task","id":"design"}`。另一个账号激活后，旧窗口保存位置的请求会被拒绝。 |
| `sync.status`、`sync.pending` | 查看当前连接、未发送的草稿及冲突。在网络不可达时，客户端可以重开同一身份最近缓存过的查询。 |
| `sync.enqueue`、`sync.flush`、`sync.resolve` | 把原请求放入 `request` 字段排队。恢复连接后发送，冲突保留在队列。确认新版本后传入草稿 `id`、`expected_rev`；或用 `discard:true` 丢弃该笔草稿。 |

写草稿支持任务、评论、文档、视图、专项、迭代和消息。离线更新与删除需要基准修订号。查询可设 `allow_cached:false` 要求访问服务器。写请求可设 `queue_if_offline:true` 允许网络中断时排队。服务端拒绝凭据或权限时会清理该身份缓存，支持离线的查询最长保留一天。对话、消息、通知、运行、产物、跨类型搜索及分享读取均需即时确认访问权限。机器派发、规则和凭据管理也需在线。

```json
{"op":"sync.enqueue","request":{"op":"update","space":"default","kind":"task","id":"替换为任务ID","expected_rev":1,"title":"待同步的新标题"}}
```

团队业务请求可发到 `POST /v1/command` 并附 `Authorization: Bearer <token>`，响应采用 CLI 相同的成功或失败信封。`sync.*` 操作由设备上的客户端处理。终端可同时设置 `SHIPFORGE_SERVER` 和 `SHIPFORGE_TOKEN` 临时连接。日常使用可通过标准输入将令牌交给 `shipforge connect https://team.example.org --token-stdin`，用 `shipforge disconnect` 清除保存的连接。Windows 保存的令牌由当前系统用户的 DPAPI 加密。

令牌范围可选 `read`、`write`、`worker` 和 `hook`。空间成员权限另行校验。安装所有者 `owner` 用团队写凭据管理安装级凭据。签发回调凭据时同时指定 `space`、`resource`（`connector` 或 `automation`）与 `binding`（目标记录 ID）。例如，`{"op":"access.issue","actor":"owner","scope":"hook","space":"default","resource":"automation","binding":"weekday-report"}` 签发只可触发该规则的令牌。

团队登录凭据由安装所有者签发。成员在 Windows 的「设置 → 个人访问令牌」中管理自己用于 CLI、脚本或 Jira 的个人令牌。个人令牌继承签发凭据的身份和权限上限，业务数据仍按当前成员权限校验。服务器只保存摘要和展示前缀。使用相同 `request_id` 重试签发返回原 ID，完整值仅在首次成功响应中提供。

从机器页创建连接令牌，或签发时设置 `source:"machine_setup"`，该凭据首次成功注册的机器会显示在个人列表中。CLI 的 `disconnect` 仅退出本机连接。服务端吊销会使下一个认证请求立即失败。通过个人令牌登录时可以查看个人列表并吊销自身。签发另一枚或管理其他令牌使用团队登录凭据，安装管理员撤销签发凭据也会停止其关联的个人令牌。[HiFox 个人令牌行为](https://help.hifox.com/docs/cli/access-tokens)

## 任务的人员与执行者

Slack 连接器管理员可用 `update` 设置布尔值 `fields.slack_assignment`、`fields.outbound` 和 `fields.assignee_map`。点击者与目标 Slack 用户均需映射到本空间成员。已绑定话题内的任务卡片可用单选框替换整组人员。[卡片配置与回调](../crates/shipforge-core/PROVIDERS.md)

空间设置中的「允许多个处理人」默认关闭。通过 `update` 将空间的 `fields.allow_multiple_assignees` 设为 `true` 可开启。每位处理人共享同一任务状态。「我的任务」和人员筛选会命中集合中的每位成员。按人员分组的看板在每位成员的列显示任务，页面任务总数只计一次。关闭设置会保留原有的多人任务，仍可编辑其人员集合。

`task.assign` 接受任务 `id`、可选的 `expected_rev` 和 `request_id`。`assignees` 是空间成员 ID 数组，默认替换整组，`mode: "append"` 可追加，空数组清空人员。单独提供 `executor` 可指派一个 Agent 或小队，设成 `null` 可撤销执行。省略的项保留原值。

```json
{"op":"task.assign","id":"plan","assignees":["alice","bob"],"mode":"append","executor":{"kind":"agent","id":"inspector"},"expected_rev":3}
```

`create` 和 `update` 支持相同的 `fields.assignees` 和 `fields.executor`。旧版 `assignee_kind`、`assignee` 仍可写入相应一项。可用 `{"op":"list","kind":"task","filter":{"assignees":"@me"}}` 查询本人负责的任务。

导入及外部同步会保留多人成员。GitLab Issue、Jira、Linear、Asana 连接器可设置 `fields.assignee_map`，以外部账号标识为键、空间成员 ID 为值。尚未映射的外部标识保存在 `fields.source_assignees` 并显示在详情中。同步中各方同时更改人员会返回 `conflict`，可用同一事件和 `resolve: "local"` 或 `"source"` 明确处理。[HiFox 任务说明](https://help.hifox.com/docs/space/tasks)


## Teambition 和 CNB

组织管理员用 `teambition.application` 验证和保存组织应用。成员用 `teambition.authorize` 取得授权地址，再用 `teambition.personal` 绑定本人的授权码或访问令牌。`teambition.projects` 列出本人参加的项目。`teambition.preview` 生成待确认的导入批次，提交后 `teambition.synchronize` 读取该项目的变化。`teambition.push` 以操作者的 Teambition 身份写回已关联任务。选择删除还须提供原对象 ID 及当前本地修订号。

用 `cnb.credential` 验证组织的组令牌，`cnb.repositories` 列出授权仓库，`cnb.pair` 配对仓库或轮换流水线密钥。`cnb.ci` 返回仓库专用的配置片段，`cnb.activity` 查看最近事件和回链，`cnb.deliver` 核对并重试待处理的评论。服务在已验证签名的回调内使用 `cnb.internal.receipt`、`cnb.internal.record`、`cnb.internal.claim`、`cnb.internal.finish` 和 `cnb.internal.history` 管理去重及投递。日常操作使用前述公开命令。[接入步骤、权限和 JSON 示例](teambition-cnb.md)

## Asana 工作区和项目

`asana.accounts` 列出个人令牌可见的工作区，`asana.credential` 按组织绑定、轮换或清除指定工作区的共享令牌。`asana.projects` 发现项目，`asana.preview` 预览历史或增量，`asana.synchronize` 读取并提交项目变更。开启写回后，`asana.push` 可发布标题、描述和新评论。一次性发送本地及远端评论时提供 `intent:"comment"`、正文和稳定请求 UUID。也可提供已有本地评论的 ID。完成状态从 Asana 读入。[请求示例与工作机设置](asana-projects.md)

## 专项时间线与分拣投递

`project.timeline` 通过 `basis` 查看任务开始日、截止日或计划区间。`triage.settings` 读取配置，`triage.configure` 设置责任与规则，`triage.submit` 投递任务，`triage.queue` 查询按进入时间排序的审核队列。`triage` 支持接受、拒绝、重复、搁置及恢复，`triage.batch` 原子执行多项接受、拒绝或搁置。`customer_request.convert` 将有权编辑的客户请求转成任务。`task.share` 返回任务编号及链接，`task.open` 按接收者的身份打开。[操作示例与权限](triage-planning.md)

## Agent 排队与机器诊断

运行调度使用 `run.trigger_now` 跳过预约等待，`run.diagnostics` 查询原因与时限，`machine.directories` 查询授权目录，`machine.diagnostics` 查看可见的活动运行。工作机本地使用 `approve-directory` 批准项目目录，使用 `diagnose` 查看进程号和轮换日志。[配置与完整请求](runtime-scheduling.md)

## 对话指令、审阅与文档引用

使用 `chat.slash` 查当前运行环境的命令，`chat.slash.send` 发送。任务快捷指令由 `shortcut.catalog`、`shortcut.save`、`shortcut.organize`、`shortcut.run`、`shortcut.history` 和 `shortcut.delete` 管理。用 `task.review.state` 查看当前待审轮次，`task.review` 发送确认。单独的 `chat.complete` 将会话及绑定任务标为完成。用 `shipforge share-image <分享码或链接> <新文件.png>` 或 `chat.share.image` 保存消息快照。文档的 `document.share`、`document.open`、`document.preview` 和 `document.embeds` 均按当前查看者权限读取。[请求示例、Windows 入口与权限](conversation-commands.md)

## 企业微信智能体

在绑定设备上用 `bot.wecom.connect` 传入 `agent`、`bot_id` 和 `secret`，可选 `dm`、`groups`。`bot.bindings` 设置 `available_for` 为 `wecom` 时返回可绑定的 Agent，以及本空间中本人有权使用、已被其他成员绑定的 Agent 名称与 ID。`bot.policy` 分别开关单聊和群 @。`bot.sync` 收取消息、回传 Agent 文字和文件，`bot.retry` 处理待确认的投递，`bot.disconnect` 解绑。长连接适配器在订阅前用 `bot.wecom.reserve` 占用空闲的机器人和 Agent，验证失败时调用 `bot.wecom.release`，成功后通过 `bot.wecom.event` 提交来源已验证的消息。`device_key` 由绑定设备生成和保存。[Windows 操作、JSON 示例与真实租户验收条件](wecom-agent.md)

## 空间任务助手

用 `task_assistant.catalog` 浏览当前组织内已加入空间的渠道助手。空间管理员用 `task_assistant.settings` 读取配置，用 `task_assistant.bind` 绑定启用的连接器、Runtime 及外部用户映射，用 `task_assistant.configure` 修改运行参数及单一语言的描述、指令覆盖。用 `task_assistant.history` 查看活动，用 `task_assistant.unbind` 解除当前渠道，用 `task_assistant.remove` 移除助手。组织管理员可用 `task_assistant.template` 与 `task_assistant.template.save` 维护中英文模板。每次修改提交读到的 `expected_rev`，首次创建填 0。[渠道能力、管理员配置与群内命令](space-task-assistants.md)

## 飞书和 Lark 智能体

在绑定设备上用 `bot.qr.start` 指定 `provider` 为 `feishu` 或 `lark` 及 Agent，随后用 `bot.qr.check` 等待对应云确认。`bot.lark.policy` 设置单聊、群 @、免 @ 的群 ID、表情和话题继续触发。`bot.lark.event` 由已验证的本机长连接适配器提交。工作机可用 `bot.sync` 或持续接收模式回传当前消息的文字和文件。[双云授权、命令与验收条件](lark-agent.md)

## 个人微信与 QQ 的外部 Agent 会话

`bot.qr.start` 接收 `provider`（`wechat` 或 `qq`）、`agent`、可选 `dm` 与 `groups`，返回本机扫码 `id` 及二维码。`bot.qr.check` 查询状态，微信要求手机验证码时加 `verify_code`。`bot.qq.connect` 用有权限的 QQ `app_id`、`app_secret` 与 `agent` 绑定，可通过标准输入传递凭据。`bot.bindings` 列出自己的绑定，`bot.policy` 设置单聊与群 @ 范围，`bot.sync` 执行一轮收发，`bot.disconnect` 解除绑定。持续收发用 `shipforge-worker bot-sync --space default --binding <ID> --watch`。[Windows 与 CLI 示例](personal-bots.md)

设备适配器在成功授权后用 `bot.attach` 登记用户、空间、Agent、平台账号和设备密钥。`bot.receive` 按上游消息 ID 幂等创建私有对话运行。`bot.claim` 领取该绑定的结果，`bot.ack` 携带对应的租约与送达结果。`bot.retry` 在用户确认后将未确认或失败的投递标为已送达、放弃或重发。`bot.expire` 在上游明确拒绝凭据时关闭绑定。`bot.detach` 用于设备不可用时从工作区中撤销绑定。用户凭据和微信会话回传凭证保存在建立绑定的设备。
