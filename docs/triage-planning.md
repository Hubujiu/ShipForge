# 专项排期与空间分拣

## 专项时间线

在 Windows 的「专项」中创建专项，选择成员或 Agent 负责人、阶段、优先级、关联空间及主空间。在任务的创建或编辑表单填写开始日、截止日或计划起止日，选择所属专项。打开专项详情即可按日查看条形时间线。每个任务单独占一行，同期工作可以并排比较。点击任务打开详情，也可以从跨空间专项跳转到有权访问的空间。[HiFox 专项说明](https://help.hifox.com/docs/space/projects)

`project.timeline` 接受专项 `id`、可选 `basis` 及 `as_of`。默认 `schedule` 优先使用任务开始及截止日，缺少这两项时使用计划区间。`start`、`due` 各展示对应的日期。`plan` 展示计划区间。只有单端日期时绘制为当天。缺少该模式日期的任务列在 `undated` 中。逾期按实际截止日、该专项查询空间的日历日和任务终态判断。任务前置关系来自可读的 `blocks`，完成数来自已完成类别。时间线统计与条目受空间成员身份以及逐条任务权限约束。

```json
{"op":"project.timeline","space":"product","id":"launch","basis":"plan","as_of":"2026-11-19"}
```

## 启用与投递

在「空间功能」启用分拣台，打开该模块进入「分拣设置」。可设置收件来源、任务类型、接受前要求优先级，以及单一值班成员的通知或指派方式。空间管理员及组织管理员可以管理模块，设置具体成员或移动规则还需要相应空间内容权限。关闭之前确认将所有待分拣任务移至本类型的待规划状态。任何类型缺少该状态，整个操作回滚，包括已搁置的任务。[HiFox 分拣说明](https://help.hifox.com/docs/space/triage)

| 提交人或入口 | ShipForge 的行为 |
| --- | --- |
| 空间编辑成员 | 普通任务页使用正常工作流状态，队列顶部的「创建待分拣任务」或快捷键 C 主动投入分拣。 |
| 私有空间中受邀的空间访客 | 可提交并查看自己的任务。分拣关闭、来源被禁用或类型没有待分拣状态时拒绝投递。修改状态、指派和处置需要编辑权限。 |
| 未加入公开空间的普通组织成员 | 从空间目录点「提交到分拣台」。专用操作返回收件编号及任务标识。空间任务列表、详情和处置仍要求相应成员身份。 |
| 未受邀的组织 Guest | 不能通过公开空间目录或已知私有空间标识投递。已加入者按其空间角色检查。官方把公开空间访客的细节留给线上权限，ShipForge 对明确授予的空间访客统一执行上述规则。 |
| 客户请求 | 编辑成员在请求详情点「转为任务」。保留标题和描述，联系人留在请求记录。开启分拣且来源与类型匹配时，新任务进入队列。重复转换返回可访问的既有任务。 |
| 外部来源 | 已接入的实时新任务经过同一来源和任务类型检查。保留历史状态的导入继续采用原映射，已关闭事项保留终态。组织所有者或管理员直接创建任务且显式指定状态时按指定状态保存。 |

```json
{"op":"triage.settings","space":"product"}
{"op":"triage.configure","space":"product","expected_rev":3,"enabled":true,"priority_required":true,"sources":["guest","external"],"types":["bug"],"duty":{"mode":"notify","member":"alice"}}
{"op":"triage.submit","space":"product","type":"bug","title":"导出中断","description":"批量导出时窗口关闭","request_id":"intake-1"}
{"op":"customer_request.convert","space":"product","id":"customer-1","type":"bug"}
```

`types:null` 与 `sources:null` 恢复不限类型或来源的默认值。传空数组会排除全部对应项。私有空间仍需邀请。成员使用队列主动投递时也要选择允许的任务类型。自动化规则只在任务新进入队列时运行。

## 分拣与责任

用「筛选」选择类型、优先级、成员或标签，按进入队列的时间切换顺序，可分别显示任务编号、截止日与搁置项。选择任务后，数字键 1、2、3 分别打开接受、重复、拒绝表单。连续按两次 M 也可标记重复。H 打开搁置，J/K 切换任务。输入表单内保留正常的键盘输入。

| 操作 | 状态与数据 |
| --- | --- |
| 接受 | 采用该类型允许的正常默认状态。可附评论、指派人员、设置优先级、标签、专项或日期并订阅更新。要求优先级时先填写。 |
| 拒绝 | 进入该类型的取消状态，可将原因作为评论留在任务中。 |
| 重复 | 支持同组织内可读任务的编号、标识或对应工作区的链接，进入重复类终态。本空间另外生成重复关系。其他成员打开任务时，关联目标按当前权限展示。 |
| 搁置和恢复 | 保存将来某个 Unix 毫秒时间。到期后的下一次队列查询会重新列出任务。可展开「显示稍后处理」并提前恢复。个人收件箱的稍后提醒独立设置。 |
| 批量 | 先逐项选择，再接受、拒绝或搁置。所有任务必须仍可编辑且待分拣。传入逐项版本后，任一项被其他成员更新，整批回滚。 |

```json
{"op":"triage.queue","space":"product","order":"oldest","filter":{"type":"bug"},"include_deferred":false}
{"op":"triage","space":"product","id":"issue-1","expected_rev":2,"decision":"accept","priority":"high","assignees":["alice"],"comment":"已经复现","subscribe":true}
{"op":"triage.batch","space":"product","tasks":["issue-2","issue-3"],"revisions":{"issue-2":1,"issue-3":4},"decision":"reject","reason":"暂时停止维护"}
```

值班模式为 `none`、`notify`、`assign`。通知遵循该成员的收件箱与桌面设置，并要求收件人仍能读取该任务。Windows 队列里的规则按显示顺序执行，编辑器可直接选择匹配类型、创建人、内容、标签及指派、移动、状态等动作。其他条件或动作可用 JSON 对象填写。可调整顺序、启停、编辑或删除规则。可能写入同一字段的规则会显示提示。后匹配规则覆盖先前冲突字段，并能匹配前一规则已设的值。

```json
{"op":"triage.configure","space":"product","expected_rev":4,"rules":[{"id":"payment","title":"支付缺陷","when":{"type":"bug","contains":"支付"},"set":{"priority":"high","assignee":"alice"}},{"id":"security","title":"加急处理安全问题","when":{"priority":"high","contains":"安全"},"set":{"priority":"urgent"}}]}
```

匹配条件包括状态、优先级、人员、创建人、专项、标签、类型、来源、截止日区间、创建时间和标题描述关键词。动作包括移动空间、改状态、指派人员、标签、专项及优先级。移动要求规则配置者在同组织目标空间仍具编辑权限。若目标工作流不允许、当前指派失效或任务已有绑定外部身份、执行历史等空间关联，保留来源队列中的任务并标记需检查。已有正式外部身份的跨空间同步路由、真实供应商账号往返以及原版公开空间访客细节仍待隔离环境联调。

## 任务编号与打开链接

任务详情提供「复制任务链接」和「打开任务链接」，Windows 注册的 `shipforge://task/` 也可通过 `shipforge-desktop --open-link` 打开。任务在创建时获得空间前缀与递增序号，同一空间内可用编号查询。链接携带工作区标识、空间和记录标识。接收者连接相同团队服务后，会以自己的身份重新检查权限。跨设备的系统协议注册和真实供应商编号强制匹配仍待联合验证。[HiFox 任务说明](https://help.hifox.com/docs/space/tasks)

```json
{"op":"task.share","space":"product","id":"issue-1"}
{"op":"task.open","link":"shipforge://task/在此粘贴上一步返回的完整链接"}
```

多身份回归可用 `cargo test -p shipforge-core --locked --test review_planning` 运行。Windows 隐藏原生窗口用 `cargo test -p shipforge-desktop --locked hidden_review_planning_window -- --ignored --test-threads=1` 执行，使用临时本地资料验证分拣、搁置、展示选项、专项时间线和任务链接。[核心验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/review_planning.rs) · [Windows 控件验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/review_ui_test.rs)

使用调试版 CLI 与团队服务运行 `python scripts/verify-planning.py target/debug`，将建立三个临时身份并经本机 HTTP 验证公开投递、私有队列、跨空间时间线和任务链接撤权边界。[团队服务验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-planning.py)
