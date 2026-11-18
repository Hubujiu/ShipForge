# HiFox 功能基线

这份清单用于确定 ShipForge 的产品范围和验收行为。参考日期为 2026-11-18，实机为 Windows 版 HiFox 0.4.538。网页帮助、桌面壳和账号已开放的模块可以分别更新。下文的“实机”表示打开过该界面，“文档”表示官方描述了行为，“预告”表示官网仍标记为未来能力。右栏都是 ShipForge 的交付条件。[产品首页](https://hifox.com/) · [官方帮助目录](https://help.hifox.com/docs/welcome) · [实机界面索引](interfaces.md)

## 空间、任务与研发计划

| 编号与能力 | HiFox 中可以做什么 | ShipForge 应能通过的验收 |
| --- | --- | --- |
| F01 组织、成员与空间 | 成员先加入组织，再依空间角色获得权限。组织角色与空间角色分别生效。任务、迭代和专项有各自的空间归属。实机有组织切换、成员目录、邀请入口、空间导航和设置。[成员](https://help.hifox.com/docs/space/members) · [空间](https://help.hifox.com/docs/space/spaces) | 两名成员以不同角色打开同一空间，管理入口、可见内容及服务端写入权限一致。移出私有空间后，搜索也应停止显示其内容。 |
| F02 任务、负责人和处理人 | 空间任务共有稳定编号、描述、类型、状态、优先级，以及可选日期、标签、迭代和专项。负责人承担验收，当前处理人可以是同事、Agent 或小队。看板卡片与详情都可指派。实机可见新建入口和右侧详情。[任务系统](https://hifox.com/features/tasks/) · [任务帮助](https://help.hifox.com/docs/space/tasks) | 创建任务、换处理人、更新描述后，从编号、列表、看板和 CLI 打开同一条记录，负责人保留。 |
| F03 类型、字段和状态 | 需求、缺陷等类型可有各自的字段与工作流；具体状态归属稳定的状态类别。实机的状态设置显示排序、默认项及用量，另有自定义字段管理页。标签可跨任务复用，并参与检索。[任务类型](https://help.hifox.com/docs/task/task-types) · [任务状态](https://help.hifox.com/docs/task/task-statuses) · [标签](https://help.hifox.com/docs/task/task-labels) | 同空间两种任务可有不同必填字段和状态。重命名某个状态后，既有任务和按类别筛选的视图仍可定位。 |
| F04 子任务、依赖与重复项 | 父子、阻塞、相关、重复表达不同关系；在详情内可以追踪另一侧的任务。专项用于对工作按目标归组。[任务关联](https://help.hifox.com/docs/task/task-relations) · [任务帮助](https://help.hifox.com/docs/space/tasks) | 从父任务找到子任务，从受阻任务找到阻塞方；重复关系显示归并目标，删除关系后两个详情同时更新。 |
| F05 我的任务、布局、检索与仪表盘 | 实机的个人任务页有“指派给我的”和“我创建的”，支持页内查找和筛选。个人仪表盘显示完成情况、状态和优先级分布、逾期及近期到期风险。空间页有任务类型切换、看板分列和显示选项。官方还列出列表、表格及可配置分组。[视图](https://help.hifox.com/docs/space/views) · [项目管理](https://hifox.com/features/project-management/) | 同一筛选在看板、列表和表格得到同一批任务；改处理人后个人清单更新，打开后退保留原筛选和位置。 |
| F06 保存视图 | 命名视图保存条件、分组、排序与布局，可供个人或空间使用。实机无已有视图时提供创建入口。[视图](https://help.hifox.com/docs/space/views) | 保存“高优先级且未完成”的共享视图，另一成员可打开；任务完成后下次查询立即移出。 |
| F07 迭代与待规划 | 迭代是空间内的时间段，管理范围、进度和剩余工作。实机时间轴展示日期、未来周期和容量信息。手动结束时要处理未完成项。[迭代](https://help.hifox.com/docs/space/iterations) | 结项预览列出剩余任务；分别测试转入下一期与退回待规划，保留任务编号、关联和历史。 |
| F08 专项 | 专项围绕目标聚合任务，可跨多个迭代；每个任务至多关联一个专项。帮助页说明可以管理参与空间、进度与负责人。[专项](https://help.hifox.com/docs/space/projects) · [任务系统](https://hifox.com/features/tasks/) | 同一专项展示不同迭代的任务，移动迭代仍计入专项进展；移除专项归属后更新汇总。 |
| F09 分拣台 | 空间可开启访客和外部系统的进线审核。启用、来源、任务类型及优先级要求共同决定入口和下一步；可接受、延后、拒绝或标记重复。[分拣台](https://help.hifox.com/docs/space/triage) | 同空间的成员建单和访客投递分别走各自流程；启用优先级要求后，缺优先级的待分拣项不能进入迭代。 |

## 协作、知识与个人工作

| 编号与能力 | HiFox 中可以做什么 | ShipForge 应能通过的验收 |
| --- | --- | --- |
| F10 评论、回复与关注 | 讨论挂在任务上，支持回复、提及和关注。新信息可以回到既有执行上下文；关注者按个人设置接收动态。[评论和回复](https://help.hifox.com/docs/agent-and-computer/comments-and-replies) · [关注任务](https://help.hifox.com/docs/task/follow-tasks) | 被提及成员能从提醒回到原任务与评论。取消关注后保留已有讨论，直接提及仍按通知规则处理。 |
| F11 收件箱 | 通知按事项聚合，可筛重要程度、稍后提醒、标记已读和清理。实机有三个时间分区及重要性筛选；规则运行也可通知所选接收人。[收件箱](https://help.hifox.com/docs/agent-and-computer/inbox) · [自动化](https://help.hifox.com/docs/space/automations) | 同一任务的两次变更聚为一行，打开定位到原对象。一名用户延后提醒后，另一人的提醒和任务状态保持原值。 |
| F12 对话与产物 | 可在任务外与 Agent 对话，查看消息、运行进展和产物，可把产物保存到文档库。实机有独立对话导航、会话空态和个人小助手输入区。[与 Agent 对话](https://help.hifox.com/docs/agent-and-computer/chat-with-agent) · [文档库](https://help.hifox.com/docs/space/docs) | 新会话可再次打开；从一条消息追到其运行和产物，再从个人文档库打开已保存版本。 |
| F13 文档库 | 安装版实际打开了个人与团队目录、上传和新建入口。帮助页还描述正文搜索、历史版本、恢复和应用内引用。部分产品页页脚仍将文档列为预告，版本恢复本次仅有文档证据。[文档库](https://help.hifox.com/docs/space/docs) · [产品页](https://hifox.com/features/tasks/) | 个人草稿仅本人可见，团队文档按权限检索；恢复历史版本后仍可查看恢复前的版本，并能从任务链接定位文档。 |
| F14 桌面导航、搜索与跨端 | 实机有组织切换、后退/前进、全局搜索、首页、对话、成员和设置。官网还描述用手机查看进展、回复与派发任务；本次实机覆盖 Windows。[产品首页](https://hifox.com/) | 桌面后退回到原空间及筛选；同一对象用权限范围内的标识可在另一客户端定位。移动端列为后续独立交付。 |

## Agent、机器与运行

| 编号与能力 | HiFox 中可以做什么 | ShipForge 应能通过的验收 |
| --- | --- | --- |
| F15 Agent 与分身 | Agent 保存指令、技能、机器、Runtime 与配置，可指派并复用。分身各属一个空间，逐字段继承或覆盖原配置。实机能看见空间 Agent、共享与私有分区。[Agent](https://help.hifox.com/docs/agent-and-computer/agents) · [分身](https://help.hifox.com/docs/agent-and-computer/agent-variants) | 在分身覆盖机器后，父 Agent 改指令仍传到分身；恢复机器继承后采用父值。已开始运行仍有原配置版本可追溯。 |
| F16 小队 | 小队有队长、成员、说明、所属空间及可见性，能作为单一处理人接单，由队长协调分工和汇总。实机看到小队的共享与私有列表。[小队](https://help.hifox.com/docs/agent-and-computer/crews) | 任务指派给小队后，用户在原任务能看到队长、分工进展和最后的汇总；重试一个子步骤可保留先前记录。 |
| F17 技能、市场与连接器 | 技能可绑定给 Agent，沉淀团队的执行方法。实机市场提供技能与 Agent 模板两类内容、搜索和分类；空间技能页另有连接器标签。[技能](https://help.hifox.com/docs/agent-and-computer/skills) · [Agent 配置](https://hifox.com/features/agents/) | 从模板建立的 Agent 可独立配置；能追溯某次执行采用的技能版本，卸载的技能从后续指派的选择器移除。 |
| F18 机器、Runtime 与工作时间 | 可接入自有机器、已有编程工具与订阅额度。用户机器或共享机器接收工作，管理可见范围和运行环境。实机的机器详情分别展示本地服务和已发现 Runtime 的状态。官方还列出可执行工作时段及机器不可用时排队。[自带工具与机器](https://hifox.com/) · [机器](https://help.hifox.com/docs/agent-and-computer/computers) · [工作时间](https://help.hifox.com/docs/agent-and-computer/working-hours) | 离线、未发现所需 Runtime、时段外或容量已满时展示具体等待原因；机器重连后，仅符合条件的任务继续。 |
| F19 仓库与工作目录 | 配置可以绑定仓库、临时目录或指定目录。Git worktree 可隔离并发任务，直接使用现有目录时串行，多目录与隔离模式的限制不同。[工作目录](https://help.hifox.com/docs/agent-and-computer/working-directory) · [连接代码库](https://help.hifox.com/docs/agent-and-computer/repositories) | 同一仓库的两个隔离运行拥有独立目录；对同一指定目录的两个直接运行按序开始，结束后可找到对应交付引用。 |
| F20 指派、执行与回传 | 待规划任务提前指派会等待，移入可执行状态后才派发。机器启动已配置的外部工具，讨论、进度、人工问题、结果和产物回到任务。官方列有按次覆盖模型，实机另有每个 Agent 的运行历史标签。[任务执行](https://help.hifox.com/docs/agent-and-computer/agent-task-execution) | 先在待规划指派，再移入待办，仅出现一次有效运行；人工回复后能接续上下文，取消与完成在历史里可区分。 |
| F26 Agent 工具、知识库与 MCP | 实机 Agent 设置有工具、知识库与 MCP 分区，展开后可添加连接。空间技能页另有连接器标签。具体提供商动作和数据范围需逐一验证。[Agent 配置](https://help.hifox.com/docs/agent-and-computer/agents) | 两个 Agent 只看到分别获准装配的工具；运行记录能说明实际使用的连接，撤销后不能再发起新调用。 |

## 自动化、集成与部署

| 编号与能力 | HiFox 中可以做什么 | ShipForge 应能通过的验收 |
| --- | --- | --- |
| F21 自动化规则与运行历史 | 空间规则指定执行者、指令、触发器与结果通知。官方列出定时、任务状态、评论和 Webhook 触发，可单独启停触发器并追看历史。实机可进入自动化页。[自动化帮助](https://help.hifox.com/docs/space/automations) · [功能页](https://hifox.com/features/automations/) | 定时与事件同时存在时可单独停用；同一事件重送一次只产生一次运行，终态回链到规则和任务。 |
| F22 项目管理、代码托管与 IM 集成 | 帮助目录涵盖 Jira（另有兼容 API）、禅道、TAPD、Bugly、PingCode、Teambition、Zoho BugTracker、Asana，代码托管集成列有 GitHub、CNB、GitLab，仓库绑定另见 Gitee 与自托管等官方说明，消息渠道列有飞书、企业微信、钉钉、微信、QQ、Slack、Discord、Telegram、Teams。具体映射、支持方向和前提因连接器而异；实机查看了设置目录。[集成目录](https://help.hifox.com/docs/welcome) · [概念对照](https://help.hifox.com/docs/integration/concept-mapping) · [Jira](https://help.hifox.com/docs/integration/jira) · [GitHub](https://help.hifox.com/docs/integration/github) · [飞书](https://help.hifox.com/docs/integration/feishu) | 为每个适配器记录导入、持续同步、回写及身份映射支持情况。先用一个已授权测试库验证重复事件、远端修改和冲突，再按这个契约接其他供应商。 |
| F23 一次性导入与导出 | 设置里有独立的导入/导出页。本次账号显示 GitHub、Linear 等一次性导入和任务数据导出仍待开放；GitHub Issues 持续集成另有入口和官方说明。[GitHub 集成](https://help.hifox.com/docs/integration/github) · [概念对照](https://help.hifox.com/docs/integration/concept-mapping) | 导入先显示字段映射和预检，再以外部标识去重；导出展示对象、字段和格式。按操作分别呈现可用状态。 |
| F24 CLI、账号与部署 | 官方提供 CLI、个人访问令牌、账号管理和私有部署说明。Windows 安装目录带有 CLI 可执行文件，界面呈现本机常驻服务；服务端部署细节采用独立设计。[帮助目录的 CLI 章节](https://help.hifox.com/docs/welcome) · [账号管理](https://help.hifox.com/docs/other/account-management) · [私有部署](https://hifox.com/solutions/private-deployment/) | 在自托管测试环境用限定权限的身份调用 CLI，执行与 GPUI 等价的业务操作。撤销权限后请求被拒，更新客户端时项目数据可正常重新打开。 |
| F25 可视化 Workflow | 首页将可视化工作流标为 Coming Soon。当前规则式自动化已有帮助和界面；可视化节点、连线和发布语义仍待产品确认。[官网预告](https://hifox.com/) · [现行自动化](https://hifox.com/features/automations/) | 进入后续探索清单。首版规则配置以可运行、可暂停、可追踪为验收条件。 |

## 首次使用

| 编号与能力 | HiFox 中可以做什么 | ShipForge 应能通过的验收 |
| --- | --- | --- |
| F27 首次进入与新手指引 | 设置组织和默认空间，展开侧栏四个快捷步骤，打开六组共 36 项的完整指引，查看当前积分和名次。[快速上手](https://help.hifox.com/docs/quickstart) | 首次初始化可继续，旧库升级保留数据。业务操作成功后计分，重复提交和阅读说明时分数保持稳定。两个成员的进度各自保存。[逐项条件](onboarding.md) |

各能力的桌面入口、界面状态和取证范围见[实机界面索引](interfaces.md)。页面数量仅对应那一批截图。独立功能、前置条件和联调范围见[行为覆盖清单](parity-inventory.md)。服务端数据一致性、机器调度和落地顺序见[Rust + GPUI 架构](architecture.md)。
