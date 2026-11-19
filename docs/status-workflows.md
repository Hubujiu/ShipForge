# 组织状态库与类型工作流

在 Windows 工作台的设置中打开“任务状态”。组织管理员可以按类别维护可复用状态、默认名称、中英文译名、颜色、顺序及分组列表的初始折叠设置。用量显示使用该状态的任务数、显式引用它的类型工作流数和涉及的空间数。组织管理员可见整个组织的汇总。其他成员只看到自己能访问的数据汇总。

在空间的“任务类型”中打开一个类型，选择“配置类型工作流”，依次点选允许的状态，再选新任务默认状态。取消勾选并重新选择可调整顺序，触发器按实际保存顺序显示。留空状态列表会使用当前空间的可用状态。任务创建、编辑、过滤、按类型查看的看板和按状态分组的列表会读取这些配置。点击已折叠分组的标题可以展开任务。

七个类别是 `triage`、`backlog`、`todo`、`in_progress`、`done`、`cancelled`、`duplicate`。同一类别可有多个状态。新任务优先使用类型默认值，然后使用工作流允许的空间默认值，再选择可用的非终态。最后三个类别都不能作为默认值。待分拣默认值需要空间启用分拣。已被任务使用的状态须先迁出，才能从该类型的可用状态中移除。参考产品以状态类别驱动执行、停止与视图分组。[HiFox 任务状态](https://help.hifox.com/docs/task/task-statuses) · [HiFox 任务类型](https://help.hifox.com/docs/task/task-types)

## 读取与配置

以下 JSON 可以保存为 UTF-8 文件并通过 `shipforge request --file 文件名` 运行。示例使用空间 `engineering` 和已启用的 `bug` 类型。先读取状态库及类型当前的 `rev`。

```json
{"op":"status.library","space":"engineering","language":"en"}
```

```json
{"op":"task_type.workflow","space":"engineering","id":"bug","language":"en"}
```

组织管理员可以从自己有访问权的空间新建状态。既有记录的修改须给出最近读到的 `expected_rev`。仅在对应界面语言已有译名时显示译名，其他语言显示 `title`。CLI 可提交其他语言标签及相应译名。Windows 提供简体中文和英文。

```json
{"op":"status.library.save","space":"engineering","id":"qa-review","title":"质量复核","fields":{"category":"in_progress","names":{"en":"Quality review"},"order":20,"color":"#398A72","collapsed":true},"request_id":"create-review"}
```

空间管理员用 `{"library":"qa-review"}` 选择组织定义。每次请求的 `statuses` 顺序即实际工作流顺序。下面的 `expected_rev` 应替换为刚读取的类型修订号。

```json
{"op":"task_type.workflow.save","space":"engineering","id":"bug","expected_rev":2,"statuses":["todo",{"library":"qa-review"},"done"],"default_status":{"library":"qa-review"}}
```

响应中的状态 ID 是任务在该空间实际使用的 ID。既有空间如果已经有同名 ID 的独立定义，组织状态会获得另一个本地 ID。已有任务和旧状态的名称、类别均保留。字符串或 `{"local":"…"}` 始终引用空间内状态。新空间沿用组织状态，已单独改名的空间副本继续独立维护。

## 删除与迁入

在组织状态旁选择“预览状态迁移”。选择目标后，窗口显示将受影响的任务和类型工作流数量。确认后，系统会把任务、已配置的状态顺序及其默认值迁至目标，移除已关联的空间副本。停用空间的历史记录也会更新并保留审计记录。其他空间的独立同 ID 状态保持原有引用。

```json
{"op":"status.library.removal","space":"engineering","id":"qa-review","target":"in_progress"}
```

`can_remove` 表示当前可执行，`reason` 返回阻止删除的原因。组织默认状态需先改指其他状态，每个类别至少保留一项。已被任务或工作流引用时必须提供迁入目标。迁移既有默认值时目标也必须允许作为默认状态。

```json
{"op":"status.library.remove","space":"engineering","id":"qa-review","target":"in_progress","preview":"替换为刚收到的预览指纹","request_id":"remove-review"}
```

返回 `conflict` 时重新预览并核对用量。相同的 `request_id` 和请求内容可以重试已完成的写入，每次都按调用者当前的管理权限检查。专项回归使用临时 SQLite 工作区、四个独立凭据、真实团队 HTTP 服务和 Windows 原生窗口。[核心迁移回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/status_workflows.rs) · [CLI 与 HTTP 回归](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-status-workflows.py) · [Windows 原生窗口](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/status_ui_test.rs)
