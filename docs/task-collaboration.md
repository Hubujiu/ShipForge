# 任务类型、保存视图与讨论串

以下示例是交给 `shipforge request` 的 JSON。省略 `space` 时使用 `default`。Windows 工作台和团队服务通过相同的核心操作校验权限。这里的验证使用独立的 ShipForge 数据库和合成成员。

## 切换任务类型

在空间设置的任务类型编辑器中，启用「限制此类型可用字段」，按需要选择绑定字段、必填字段及暂时隐藏的字段。协议中的 `fields.field_bindings` 是有序数组，每项包含字段 ID `field` 和可选布尔值 `enabled`、`required`。默认启用且可选。隐藏绑定保留任务上的值，重新启用即可继续编辑。早期数据库仅配置 `required_fields` 的类型继续接受空间已定义的可选字段。[HiFox 任务类型](https://help.hifox.com/docs/task/task-types)

```json
{"op":"update","kind":"task_type","id":"incident","fields":{"field_bindings":[{"field":"impact","required":true},{"field":"old_report","enabled":false}]}}
```

编辑任务并选择目标类型，提交后会先校验目标类型是否启用、当前状态是否可用，以及目标类型缺少哪些必填值。需要移除已填写字段时，桌面会列出字段名称，确认后一次保存。取消会保留原任务。跨类型共享的字段按字段 ID 保留，切换类型时可以同时补入目标必填字段和选择兼容状态。

```json
{"op":"task.type.preview","id":"issue-17","target":"incident","status":"todo","custom":{"impact":"regional"}}
```

读取结果中的 `remove`、`missing`、`status_allowed`、`expected_rev` 和 `expected_type_rev`。当 `remove` 含字段时，向用户列明损失，经其确认再提交。以下修订号和字段 ID 仅示例，实际使用上一步的结果。

```json
{"op":"update","kind":"task","id":"issue-17","expected_rev":4,"expected_type_rev":2,"confirm_remove":["release_note"],"fields":{"type":"incident","status":"todo","custom":{"impact":"regional"}}}
```

管理员从类型中解绑字段会影响该类型所有任务。先用 `task_type.bindings.preview` 提交 `id` 和完整的 `field_bindings`。结果包含按字段汇总的受影响任务数。批准删除后，以 `update` 修改 `task_type`，同时提交预览返回的 `expected_rev`、`impact_token` 和作为 `confirm_remove` 的 `remove` 数组。任一受影响任务在预览后被更新，服务会返回冲突，可以重新查看影响。

## 保存及分享视图

「全部任务」上方展示已保存视图，较窄的窗口可从「全部保存视图」进入。拖动个人视图调整同一置顶组中的顺序，菜单同时提供前移、后移、置顶和取消置顶。空间设置的任务类型标签保留其配置顺序。个人排序按成员和空间保存。空间共享视图可由创建者或拥有空间编辑权限的组织管理员维护。其他可编辑成员可另存为自己的视图。[HiFox 视图和任务列表](https://help.hifox.com/docs/space/views)

| 操作 | 输入和结果 |
| --- | --- |
| `view.catalog` | 按当前用户的顺序列出可见视图，返回 `fields.personally_pinned` 和 `fields.can_edit_view`。 |
| `view.organize` | 提供个人视图 `id` 和 `pinned`，或用 `before`、`after_view` 提供同组个人视图 ID。拖动不会更改置顶状态。 |
| `view.preview` | 提供草稿 `fields`。用当前成员的权限和正式查询条件取得结果，预览不会创建视图。`with_total: true` 同时返回当前页 `items` 与分页前的 `total`。 |
| `view.run` | 提供保存视图 `id`，可加临时 `changes`。筛选项按键覆盖，临时值设成 `null` 可移除原条件。 |
| `view.save_as` | 提供原视图 `id`、新 `title`、可选 `changes` 与 `visibility`。默认个人可见。复制列表配置，可独立更新，任务和个人置顶状态各自保留。 |
| `view.share`、`view.open` | 前者按 `id` 返回视图 `link`。后者接收完整链接并按打开者当前权限返回 `view` 和 `tasks`。 |

```json
{"op":"view.save_as","id":"team-release","title":"我负责的发布","visibility":"private","changes":{"filter":{"assignee":"@me"},"layout":"list"}}
```

Windows 可在视图旁复制链接，从「全部保存视图 → 打开视图链接」粘贴打开。已安装的工作台也能接收注册到本机的 `shipforge://view/` 链接。链接包含工作区标识。接收者须连接签发链接的同一个工作区，个人视图仍限本人。打开时重新计算任务，撤销成员资格后再次打开会被拒绝。原版帮助明确另存当前配置和复制视图链接，未规定复制视图必须克隆收藏、操作历史或个人顺序。

## 定位、解决和重新打开讨论

从任务详情添加顶层评论，在任意评论下点「回复」可继续同一讨论。也可在全局搜索命中评论后打开对应任务，展开并标出该评论。点「用此评论解决」会解决整串并记录是哪条评论给出结论。已解决讨论默认收起，展开后可重开，或将标记移到另一条评论。评论正文和回复层级均保留。[HiFox 评论与回复](https://help.hifox.com/docs/agent-and-computer/comments-and-replies)

```json
{"op":"comment.thread","task_id":"issue-17","id":"nested-reply"}
{"op":"comment.resolve","task_id":"issue-17","id":"nested-reply","expected_rev":3}
{"op":"comment.reopen","task_id":"issue-17","id":"another-reply","expected_rev":4}
```

每行是独立请求。`comment.thread` 返回任务、被定位的评论 ID、根评论和调用者可见的回复。变更使用根评论的 `expected_rev`。根的 `fields.resolved` 是整串状态，`resolution_comment` 指向被选中的评论。任务内有编辑权的成员可以解决或重开讨论。不可见、跨任务、循环和缺失父评论的引用不能变更状态。ShipForge 也接受本地工作区已有的自定义记录 ID。删除带回复的评论前须先处理回复，以保留有效层级。[HiFox CLI 讨论命令](https://help.hifox.com/docs/cli/cli)

工作机通过 `run.discussion` 提交 `id`（运行 ID）、`machine_id`、`machine_key`、`lease` 及 `action`。`comment` 和 `reply` 需要 `body` 和一次发言稳定复用的 `call_id`。`reply`、`resolve`、`reopen` 还需 `comment_id`。服务根据运行的实际任务、执行 Agent 和请求成员检查权限。Agent 可以回复可见讨论，只能解决或重开由同一 Agent 创建根评论的讨论。运行结束或租约失效后写入会被拒绝。登录成员创建普通评论时无法指定 Agent 作者。
