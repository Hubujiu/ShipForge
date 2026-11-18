# ShipForge 命令行与 JSON 协议

`shipforge.exe` 与原生桌面使用同一个业务核心。本机模式使用当前用户的 ShipForge Workspace 数据目录，安装时指定的数据目录也会被桌面和 CLI 读取。`--data-dir <目录>` 可为本次命令选择独立配置。连接团队服务后，命令以令牌对应的成员身份执行。在程序目录的 PowerShell 中可用 `.\shipforge.exe` 调用下文的 `shipforge` 命令。[机器与服务部署](windows.md) · [完整请求 schema](protocol.schema.json)

## 建单、修改和重新打开

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

| 操作 | 用法 |
| --- | --- |
| `schema`、`whoami`、`organization.list`、`space.list` | 查看接口、当前业务身份、可访问的组织和空间。CLI 快捷命令为 `shipforge schema`。 |
| `create`、`list`、`get`、`update`、`delete` | `kind` 指定资源。创建提供标题与 `fields`；更新合并提供的字段。返回记录包含空间、ID、标题、字段、修订号与时间。 |
| `search`、`events`、`dashboard` | 搜索允许访问的对象、按游标恢复增量事件及获取任务统计。搜索使用 `query`，事件使用 `after`。 |
| `view.run`、`task.relations`、`project.progress` | 用对象 ID 执行已保存的筛选、读取任务关系或计算专项进度。 |
| `iteration.timeline` | `{"op":"iteration.timeline","as_of":"2026-11-18"}` 返回可见的迭代数组。不传 `as_of` 时取当前 UTC 日期。按开始日期倒序，无开始日期的迭代排在最后。`fields.planning` 包含起止日期 `start`、`end`，阶段 `phase`（`active`、`upcoming`、`overdue`、`closed` 或 `unscheduled`），可见任务数 `total`、完成数 `completed` 及可空的任务容量 `capacity`。 |
| `iteration.preview`、`iteration.close`、`triage` | 用迭代 ID 预览及结项。结项的 `target` 为接收未完成任务的开放迭代 ID，省略或传 `null` 退回待规划。分拣用任务 ID 和 `decision`，可取 `accept`、`defer`、`reject` 或 `duplicate`。重复项的 `target` 为原任务 ID。 |
| `follow`、`notification.act` | 指定任务 ID 和布尔值 `enabled` 切换关注。处理本人的提醒时指定通知 ID 和 `action`，可取 `read`、`unread`、`clear`、`restore` 或 `snooze`。稍后提醒的 `until` 为未来的 Unix 毫秒时间戳。 |
| `document.history`、`document.restore`、`artifact.save` | 查看文档历史、用指定历史版本生成当前版本、把授权的产物保存到文档库。 |
| `attachment.upload`、`attachment.download` | 上传、读取任务、消息、文档或运行产物中的真实文件。单文件上限 4 MiB，下载核验 SHA-256。文档可按修订版本取文件。 |
| `export`、`import.preview`、`import` | 导出可见业务对象、预检与事务导入，维护外部标识和引用。先预览导入结果再正式提交。 |
| `agent.resolve`、`catalog.list`、`catalog.install`、`tool.authorize` | 查看 Agent 有效配置、浏览与安装内置模板/技能、核对工具授权。Agent、小队、技能及工具配置由通用对象操作维护。 |
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

通过 CLI 送入供应商事件时，给 `provider.preview` 和 `provider.ingest` 都传 `space`、连接器 `id`、`provider` 与 `payload`，并按来源提供 `event_id`。GitLab、Jira、Linear 和 Asana 可接收工单状态变更。飞书、Slack、Telegram、Teams、Discord、企业微信和钉钉可将支持的文字消息生成任务。Asana 事件需要补取任务详情，需签名验证、解密或 Gateway/Stream 会话的渠道由受信中继接入。GitHub Issues 由机器端 `github-sync` 拉取和 `github-push` 显式回写。[各供应商载荷、权限与样例](../crates/shipforge-core/PROVIDERS.md)

## 团队身份与离线草稿

| 操作 | 用法 |
| --- | --- |
| `access.whoami`、`access.issue`、`access.list`、`access.revoke` | 查询当前登录身份；安装所有者可发放、列出或撤销令牌。发放时提交 `actor` 和 `scope`；明文令牌只在创建响应出现。令牌管理与空间成员权限一起生效。 |
| `sync.status`、`sync.pending` | 查看当前连接、未发送的草稿及冲突。在网络不可达时，客户端可以重开同一身份最近缓存过的查询。 |
| `sync.enqueue`、`sync.flush`、`sync.resolve` | 把原请求放入 `request` 字段排队。恢复连接后发送，冲突保留在队列。确认新版本后传入草稿 `id`、`expected_rev`；或用 `discard:true` 丢弃该笔草稿。 |

写草稿支持任务、评论、文档、视图、专项、迭代和消息；离线更新与删除需要基准修订号。查询可设 `allow_cached:false` 要求访问服务器。写请求可设 `queue_if_offline:true` 允许网络中断时排队。服务端拒绝凭据或权限时会清理该身份缓存；离线缓存最长保留一天。机器派发、规则和凭据管理都需要在线确认。

```json
{"op":"sync.enqueue","request":{"op":"update","space":"default","kind":"task","id":"替换为任务ID","expected_rev":1,"title":"待同步的新标题"}}
```

团队业务请求可发到 `POST /v1/command` 并附 `Authorization: Bearer <token>`，响应采用 CLI 相同的成功或失败信封。`sync.*` 操作由设备上的客户端处理。终端可同时设置 `SHIPFORGE_SERVER` 和 `SHIPFORGE_TOKEN` 临时连接。日常使用可通过标准输入将令牌交给 `shipforge connect https://team.example.org --token-stdin`，用 `shipforge disconnect` 清除保存的连接。Windows 保存的令牌由当前系统用户的 DPAPI 加密。

令牌范围可选 `read`、`write`、`worker` 和 `hook`。空间成员权限另行校验。只有持有写令牌的安装所有者 `owner` 能通过 API 管理令牌。签发回调凭据时同时指定 `space`、`resource`（`connector` 或 `automation`）与 `binding`（目标记录 ID）。例如，`{"op":"access.issue","actor":"owner","scope":"hook","space":"default","resource":"automation","binding":"weekday-report"}` 签发只可触发该规则的令牌。
