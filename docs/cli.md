# ShipForge 命令行与 JSON 协议

`shipforge.exe` 与原生桌面使用同一个业务核心。本机模式默认使用当前用户的 ShipForge Workspace 数据目录，`--data-dir <目录>` 选择独立的工作区。连接到团队服务后，命令以登录令牌对应的成员身份执行。服务端和机器端均可通过 `--help` 查看命令参数。[机器与服务部署](windows.md) · [完整请求 schema](protocol.schema.json)

## 建单、修改和重新打开

在 PowerShell 中可用对象转换 JSON，复杂描述可直接保存成 UTF-8 文件。下面的操作在临时独立目录创建一项工作，并显示更新后的标题。

```powershell
$workspace = Join-Path $env:TEMP 'ShipForge-example'
$task = (shipforge --data-dir $workspace create task '发布前检查' | ConvertFrom-Json).data
shipforge --data-dir $workspace update task $task.id --title '完成发布前检查' --rev $task.rev
shipforge --data-dir $workspace get task $task.id
shipforge --data-dir $workspace list task
```

`shipforge request --file 请求.json` 或将 JSON 送到 `shipforge request` 的标准输入可执行下表所有操作。`--pretty` 打印缩进。`--space` 设置默认空间，省略时使用 `default`。写入可以携带唯一的 `request_id` 来安全重试，修改既有记录时提供 `expected_rev` 以检查过期写入。成功返回 `{"protocol":1,"ok":true,"data":…}`；失败返回 `error.code`、`error.message` 和非零退出码。常见错误有 `invalid`、`conflict`、`forbidden`、`unauthorized`、`not_found` 和 `offline`。

## 业务操作

| 操作 | 用法 |
| --- | --- |
| `schema`、`whoami`、`organization.list`、`space.list` | 查看接口、当前业务身份、可访问的组织和空间。CLI 快捷命令为 `shipforge schema`。 |
| `create`、`list`、`get`、`update`、`delete` | `kind` 指定资源。创建提供标题与 `fields`；更新合并提供的字段。返回记录包含空间、ID、标题、字段、修订号与时间。 |
| `search`、`events`、`dashboard` | 搜索允许访问的对象、按游标恢复增量事件及获取任务统计。搜索使用 `query`，事件使用 `after`。 |
| `view.run`、`task.relations`、`project.progress` | 用对象 ID 执行已保存的筛选、读取任务关系或计算专项进度。 |
| `iteration.preview`、`iteration.close`、`triage` | 预览迭代剩余工作，然后结项并处理剩余任务；分拣动作作用于对应任务。 |
| `follow`、`notification.act` | 关注任务或处理调用者自己的聚合提醒。评论、对话、消息分别使用通用对象操作。 |
| `document.history`、`document.restore`、`artifact.save` | 查看文档历史、用指定历史版本生成当前版本、把授权的产物保存到文档库。 |
| `export`、`import.preview`、`import` | 导出可见业务对象、预检与事务导入，维护外部标识和引用。先预览导入结果再正式提交。 |
| `agent.resolve`、`catalog.list`、`catalog.install`、`tool.authorize` | 查看 Agent 有效配置、浏览与安装内置模板/技能、核对工具授权。Agent、小队、技能及工具配置由通用对象操作维护。 |
| `machine.register`、`machine.heartbeat`、`machine.disable`、`machine.enable` | 注册机器、更新实时能力、禁用或重新启用机器。机器凭据保存在机器配置旁的状态目录中。 |
| `run.enqueue`、`run.claim`、`run.heartbeat`、`run.report`、`run.cancel`、`run.retry`、`run.reply`、`run.tool` | 创建运行、认领/续租、提交结果、请求取消、重试或回复。`run.tool` 使用机器密钥、当前租约、工具 ID 和调用 ID 复核并记录授权。 |
| `automation.fire`、`automation.tick` | 触发启用的规则，或扫描到期时间计划。用相同事件 ID 重放同一次触发。 |
| `connector.ingest`、`connector.export`、`provider.preview`、`provider.ingest` | 外部任务三方合并、导出回写数据、预览供应商载荷或落单。绑定连接器和稳定事件标识。 |

记录的常用 `kind` 包括 `task`、`task_type`、`status`、`field`、`tag`、`relation`、`view`、`iteration`、`project`、`member`、`document`、`chat`、`message`、`agent`、`crew`、`skill`、`machine`、`automation`、`connector` 与 `tool`。只读及派生对象的写入由相应业务操作负责。任务可在 `fields` 中提供 `description`、`status`、`priority`、`type`、`labels`、`due_date` 等；具体类型的必填字段和角色由核心校验。

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

向 `POST /v1/command` 发送同样的请求并附 `Authorization: Bearer <token>`，得到与 CLI 相同的成功或失败信封。终端应用可以使用 `SHIPFORGE_SERVER` 和 `SHIPFORGE_TOKEN` 两个环境变量临时连接。日常使用可将令牌通过标准输入交给 `shipforge connect https://团队地址 --token-stdin`，用 `shipforge disconnect` 清除保存的连接。Windows 保存的令牌由当前系统用户的 DPAPI 加密。
