# 空间迭代

在 Windows 的空间「迭代」页，管理员可以打开「迭代设置」选择自动节奏或手动模式。在空间设置关闭迭代后，可以从同一处重新开启。团队服务自行检查每个空间的当地日期。独立桌面、本机 CLI 和团队请求也会在访问时补齐停机期间经过的边界。这个流程可以独立于执行 Agent 的机器运行。[参考产品的迭代规则](https://help.hifox.com/docs/space/iterations)

## 配置自动节奏

读取 `{"op":"iteration.settings","space":"default"}` 返回该空间配置、默认值与修订号。首次配置示例使用两周工作周期，两天冷却，提前创建两期。日期按当地日历计算，结束日当天仍在本期内。

```json
{"op":"iteration.configure","space":"default","expected_rev":1,"fields":{"mode":"automatic","timezone":"Asia/Shanghai","anchor_date":"2026-11-16","weeks":2,"cooldown_days":2,"future_count":2,"auto_join_active":false,"auto_join_in_progress":true,"auto_join_completed":true}}
```

| 字段 | 配置与结果 |
| --- | --- |
| `mode` | `automatic` 维护当前和未来迭代。`manual` 由成员启动及结项。`disabled` 关闭空间迭代。 |
| `timezone`、`anchor_date` | 使用 IANA 时区与 `YYYY-MM-DD` 节奏起始日。未设置时区时取空间时区，缺省用 UTC。首次选择过去的起始日，从当前适用周期开始。 |
| `weeks`、`cooldown_days`、`future_count` | 分别取 1–8 周、0–56 天、0–26 期。冷却期间没有当前期。有下一期时未完成任务顺延，无下一期时移出原迭代。 |
| `auto_join_active` | 新任务或状态变化进入待执行、进行中，或未完成任务新增、修改截止日时，可自动归入本期。分拣中及已取消任务不参与。 |
| `auto_join_in_progress`、`auto_join_completed` | 在适用的日期窗口内，未安排迭代的任务进入进行中或完成状态时加入。冷却期的匹配变化归入下一期。 |

开启规则时只回看当前工作窗口或冷却窗口内确实发生的匹配变化。修改旧任务的标题、描述，或保留一个窗口之前已存在的状态，都不会触发这项回填。任务已有迭代时保留其归属。CLI 明确传 `fields.iteration:null` 可在同一次更新中将任务移出。手动移出和元数据导入会记录各自的事件边界，之后发生新的匹配状态变化时再自动归入。

在结束日后的第一个当地日期，系统记录结项范围，仅将未完成且未取消的任务转给下一期。完成与取消的任务仍在原期。跨过多个边界时按时间顺序处理，重复检查同一天不会再次顺延。时间轴区分进行中、即将开始、更远的计划期和已结束。配置节奏发生变化时，已开始的迭代保留原有结束日期，未开始的期按新日历重新排期。减少未来期数时，被移除期中的工作回到待规划。自定义工作流中没有待规划状态的任务保留原状态并清除迭代字段。

管理员在下一期选择「今天开始」，会结束旧的当前期并将未完工作转到所选期，从当天继续计算周期。关闭模块会结束已开始的期、移除未开始的期，保留结项范围及历史任务。再次开启从手动模式开始。

## 手动规划和历史

在手动模式创建迭代可填写开始日、结束日和负责人。已配置的手动空间在留空时使用当地今天及配置的迭代时长，负责人默认创建人，也可以留空。未配置的新空间和旧数据继续支持无日期的待排期记录。打开详情选择「今天开始」；手动模式允许多期同时进行。「我负责的」只筛选本人负责的迭代，实际权限仍取决于空间和任务访问权。

```json
{"op":"iteration.start","id":"sprint-a"}
{"op":"iteration.timeline","owner":"@me"}
{"op":"iteration.preview","id":"sprint-a"}
{"op":"iteration.close","id":"sprint-a","target":"sprint-b"}
{"op":"iteration.history","id":"sprint-a"}
```

手动结项时可将剩余任务转入另一开放迭代，也可以省略 `target` 移出。`iteration.history` 省略 `id` 时列出空间可见的结项记录，禁用模块后仍可从当前空间详情查看。传迭代 ID 返回 `captured`、结项时间和当时的任务状态。历史记录同时检查当时和当前的任务可见性。已完成任务可以补记进历史，未完成任务不能新加入已结项的迭代。没有结项快照的旧记录返回 `captured:false`。手动删除迭代会解除任务归属并移除其统计。

## 回归场景

核心使用固定时钟验证洛杉矶春季夏令时、本地午夜、两日冷却、补跑多个周期、重复调用、不同成员的快照范围、历史补记及重启。Windows 双进程测试在隔离数据库中模拟停机一周，重新启动团队服务后，先从数据库确认其已自行结项，再用 CLI 读取移动结果。[核心回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/iterations/tests.rs) · [团队服务实际启动检查](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-iterations.py)
