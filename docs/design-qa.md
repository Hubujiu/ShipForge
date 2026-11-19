# Windows 原生界面验收

## 对话工作目录与分支

Windows 原生窗口从已有 Agent 消息打开「使用工作目录分叉」，显示独立工作树选项和原生基线分支文本框。保存后用独立 CLI 读回新对话、分支和模式。隔离测试启动两个独立工作机进程和真实 Git 服务，分别验证同目录共享未提交文件、独立 worktree、同一分支对话的后续指令，以及另一工作机只检出远端可访问分支。[原生窗口](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/chat_ui_test.rs) · [工作机验证](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-conversation-workspaces.py)

## 个人小助手和群头像

Windows x64 隐藏 GPUI 窗口使用独立数据库。首次进入「小助手」后，输入区为空，数据库中的会话数为零。原生文本框发送第一条消息后打开刚创建的会话，再次进入时得到空白编辑器。点击「最近对话」重新打开原会话。群详情的文件选择入口读取合成 PNG，群主窗口显示可访问性角色为 Image 的头像，移除后 CLI 返回空图片。这个用例运行真实窗口、后台命令和文件读取。[Windows 窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/assistant_ui_test.rs)

本地 HTTP 服务另以五个独立 CLI 配置验证读写令牌、同组织跨空间助手、他人不可访问会话和运行、群主转让、移出及解散。工作机启动合成外部进程，第二轮收到首轮用户问题和机器回答。[团队服务和工作机验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-assistant-groups.py)

## 网络代理、关于与独立桌面通知

Windows GPUI 窗口通过辅助功能树打开代理设置，切换模式并检测回环 HTTP 服务，保存后再读设置，最后从「关于」返回新手指引。CLI 在故意损坏的项目库旁验证 HTTP CONNECT、HTTPS 握手、SOCKS5 域名代理解析、密码作用域、修订冲突和回环直连。[原生窗口](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/live_navigation_test.rs) · [网络验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-device-network.py)

原生收件箱用例通过渠道设置关闭收件箱，从一次真实任务指派生成桌面链接，打开对应任务并检查未读数。多身份测试覆盖他人链接和撤销评论访问后的状态。[原生收件箱](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/inbox_ui_test.rs) · [团队服务](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-inbox.py)

## 机器存储与模型配置

在真实 Windows GPUI 窗口打开「存储配额与清理策略」，将最低可用空间设为 1 GiB 并启用定时清理。独立客户端读回保存值，辅助功能树中出现「磁盘可用空间过低」。固定测试工作区后，清理按钮消失；取消固定后重新出现。隔离的工作机测试通过本机 Git HTTP 服务产生真实工作树和镜像，并用高于测试盘实际余量的阈值触发提醒。保留工作树时镜像受保护，确认回收工作树后固定状态继续保护镜像。解除固定并启用定时策略后，工作机回收镜像，远程和绑定仓库的引用及文件保持完整。[Windows 原生界面用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/storage_ui_test.rs) · [实际工作机与磁盘用例](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-storage-policy.py)

隐藏的原生 Windows D3D 窗口从机器详情进入存储面板。取消清理后保留原有操作历史，选择确认后独立客户端读到绑定同一指纹的清理请求。随后在 Agent 详情读取机器提供的模型和思考深度，配置后通过独立客户端读回。工作机进程在临时 Git 仓库中验证内容改变后的冲突、绑定和恢复保护、只删除选中 worktree，以及子进程运行期间继续统计并拒绝清理。第二组运行用 Python 读取实际 argv 与环境，验证单次覆盖和整段含空格参数。[原生窗口验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/storage_ui_test.rs) · [工作机验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/tests/storage_and_launch.rs)

## 三层 Git 设置与仓库继承

隐藏 Windows GPUI 窗口通过原生辅助功能树确认组织页、已加入空间和 Agent 详情的 Git 入口。测试从空配置开始，在组织填写提交者姓名，在空间填写邮箱，在 Agent 单独改变共同作者设置，逐项核对实际生效值与来源。所有设置经桌面表单提交，再由独立客户端读取同一临时数据库。

空间用 SSH 简写绑定仓库，并写入说明和检出分支。Agent 依次选用指定空间仓库、全部和无。跨空间分身添加仓库后，测试核对本体原有清单、分身移除绑定前的确认以及恢复继承后的结果。另一客户端抢先修改同一条配置时，旧表单显示冲突，重新读取后呈现新值。测试也用只读身份取得真实响应，验证其原生面板只提供查看操作，核心拒绝该身份写入。[Windows 原生 Git 专项测试](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/git_workspace_ui/native_test.rs)

## 工作机服务、托盘与运行时能力

隔离的 Windows D3D 窗口通过辅助功能树找到「本机服务设置」。读取临时配置后，切换为打开桌面时启动、关闭收起到托盘，再次读取验证设置已保存。关窗会打开退出确认。在同一个隐藏窗口中，Windows 通知区域接受了真实图标，验收检查隐藏状态并移除了该图标。随后从机器详情打开「运行时能力」，验证重扫、升级和模型测试按钮，确认模型测试后从独立客户端读回已排队的请求。[Windows 原生验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/machine_management_ui_test.rs)

工作机进程验收启动独立监督程序，检查重复启动返回同一 PID、后台保留、重启更换 PID、取消后台后停止。另一次重启中断了已经启动的 Python 子进程，核心记录为已取消。本机合成安装器依次安装和升级版本，连接测试验证随机回包、错误回包及登录错误。Windows Shell 快捷方式在临时目录创建，读取回显的配置与数据路径后删除。实际账户开机目录和供应商账号留给带账号的部署环境点验。[真实进程验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/tests/machine_management.rs) · [快捷方式验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/src/service/startup.rs)

## 组织状态与空间类型工作流

隐藏的 Windows 原生窗口通过表单新建“质量复核”，填写英文译名，打开“列表默认折叠”，再将其设为 Bug 工作流第一项和默认值。任务表单依序列出该类型的三个状态，切换英文后，筛选器与看板列显示“Quality check”。列表原先收起任务，点分组后可以展开。窗口预览迁入“待处理”并确认删除，独立客户端读到任务状态和默认值已更新。[原生窗口回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/status_ui_test.rs) · [多身份团队服务回归](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-status-workflows.py)

## 两个成员窗口与组织默认设置

两个隐藏的 Windows 原生窗口使用独立身份连接同一团队服务。两名成员通过目录和搜索打开同一私聊，一端发出的消息自动出现在另一端。切断其中一端的连接后，该端清空会话正文；在另一端发送一条新消息，恢复连接后原窗口补齐消息且只出现一次。撤销成员的空间权限后，其旧窗口自动退出该空间，后退和搜索也无法重新打开这段对话。[双窗口验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/live_navigation_test.rs)

另一个 Windows 用例通过组织表单保存默认语言和优先级档数，在新建任务的选择器分别读取最低与无优先级。核心和 Jira HTTP 验收还验证了导入预览可回滚、已用的最低优先级在降为四档后保留、Jira 清除优先级可往返。[组织设置](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/organization_settings.rs) · [Jira API](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-jira.py)

## 技能市场和同机任务目录

隐藏的 Windows GPUI 窗口在临时工作区中打开市场，检查真实 UI Automation 节点，并走桌面动作入口打开条目详情、安装技能、选择要绑定的 Agent。切换到没有条目的分类后，原有详情按钮消失。空间技能页能打开目录和机器导入，实际提交文件后从第二个客户端读取到了新技能。小队详情中开启共享后，按钮变为「关闭同机共享任务目录」。[原生验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/market_ui_test.rs)

正式工作机可执行文件启动两个并行子进程。各自写入的 PID 和队长写下的任务标识在同一任务的工作树中相互可见，汇总进程读取两份结果。另一任务使用新的工作树。隔离夹具里绑定源仓库的 Git 状态保持干净。独立的服务端和两个 CLI 配置同时检验组织发布、共享安装、升级、卸载和版本快照。 [工作机验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/tests/native_runtime.rs) · [HTTP 验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-skill-market.py)

## 邮箱登录和注销

隐藏的 Windows 原生窗口连接临时团队服务，在账号列表填写邮箱，读取设备配对码，使用隔离邮件网关返回的浏览器链接完成验证。关闭验证表单后，邮件网页返回的应用链接可重新打开原表单。登录后查看与修改姓名，预览将解散的组织，重新验证并输入「注销」，界面回到本机工作区。另用两个独立设备配置和另一位成员检验撤权、新手积分和组织数据保留。[Windows 窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/identity_ui_test.rs) · [账号验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-identity.py)

## 禅道和 PingCode

Windows x64 隐藏 D3D 窗口打开两个连接器详情，通过 UI Automation 核对不同平台的凭据、发现、预览、同步、回写和批次入口。分别打开密码及用户授权码表单，验证敏感输入控件，再将预览切换到「检查增量变更」。在 PingCode 原生窗口输入合成预览批次号并提交，独立数据库连接读到一条新任务。随后在窗口撤回该批，任务数恢复为零。单独的 HTTP 上游夹具覆盖三种 PingCode 身份与禅道认证、权限和真实发布请求。[原生窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/tracker_ui_test.rs) · [HTTP 验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-trackers.py)

## 收件箱与通知设置

Windows x64 隐藏 D3D 窗口使用独立工作区，另一个合成身份发送指派和评论提及。通过 UI Automation 读取通知标题，打开对应任务及评论，再关闭详情，核对筛选仍停在「重要」。从同一窗口将提醒稍后处理并恢复，随后在桌面渠道关闭分配组、用第二个客户端读取该配置，恢复默认后确认覆盖项消失。另用真实团队服务和三个 CLI 账号验证到时自动返回、按空间清理以及移除成员后的即时权限。[原生窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/inbox_ui_test.rs) · [服务端验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-inbox.py)

在 Windows x64 上启动 ShipForge 的 GPUI 桌面程序，使用独立 SQLite 工作区核对了空间看板、任务详情和新建自动化。下表的三张图取自实际窗口，任务、文件和执行者均为验收用的虚构数据。参考客户端为 HiFox 0.4.538。[参考界面编号与原图说明](interfaces.md)

## 账号切换与个人令牌

Windows x64 上的隐藏 GPUI 窗口连接到临时启动的团队服务。两名成员分别使用读、写登录凭据。从新开的窗口恢复只读成员上次访问的文档页，在账号列表切换为另一成员。在令牌用途名称输入框和有效期选择器中完成设置，签发 90 天个人令牌，使用该令牌调用团队 API 确认身份，再通过列表中的确认弹窗吊销。随后撤销当前登录凭据，窗口自动回到本机账号列表，用同一账号的新凭据重新认证成功。[原生窗口验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/account_ui_test.rs)

## 从初次对照到复验

| 位置与影响 | 初次观察 | 修改与同状态复验 |
| --- | --- | --- |
| P1 · UI02 看板第一列 | 打开空间任务时，第一列左边界超出视口，首张卡片的编号和标题开头被截断。 | 看板按可用宽度排列，滚动从首列开始。复验时可同时看到「待开始」、两张完整卡片及后续状态列。 [空间任务看板](images/windows-board.jpg) |
| P1 · UI03 任务详情 | 宽窗中属性占据正文整行，查看活动或附件要向下滚动。旧界面同时显示关注和取消关注，用户难以判断当前状态。 | 详情使用 920 px 宽度上限，正文和活动在左侧，属性在右侧。详情可用宽度低于 680 px 时回到单列。复验时首屏同时显示正文、附件和九项属性，关注按钮随操作切换。 [任务详情](images/windows-task-detail.jpg) |
| P1 · UI26 新建自动化 | 原表单把指令和配置排成一列，复杂条件需要编辑 JSON，难以确认执行时间和启用状态。 | 宽窗将执行指令和执行者放在左侧，触发器与收件人放在右侧。选「定时运行」和「每天」后显示当地时间与时区。增删第二张触发器后，第一张仍保留每天 09:00、上海和启用状态。 [自动化编辑器](images/windows-automation.jpg) |

## 画面对照

参考原图位于本地调研图库 `HiFox-0.4.538/ui/`，对应 `02-space-tasks.png`、`03-task-detail.png` 和 `26-new-automation.png`。前两张为 1442 × 901 px，第三张为 1440 × 900 px。ShipForge 原生视口设为 1440 × 900，三张窗口截图均为 1442 × 901 px。对照时将完整图等比放入同一尺寸的左右两栏，再分别放大首列卡片、属性区与触发器。参考资料中的账号和机器信息仅保存在本地。

- **看板。** 从左到右依次为应用导航、空间导航和状态列。参考空间只有一张示例卡，ShipForge 截图有四张任务，分布在三个有内容的状态列。局部核对了列名、数量、卡片编号与换行起点。
- **详情。** 两张图都在看板上打开右侧详情，保留左侧导航和一部分原列表。参考任务有多段说明，验收任务有两行正文和一个附件。检查了标题、段间距以及属性名和值的对齐。
- **自动化。** 两张图均为浅色遮罩上的居中对话框。参考图保留空名称，ShipForge 已填入规则与执行者。局部对照了每日执行时间、时区和创建按钮。ShipForge 为时间与时区各用一行，以容纳时区名称。

标题采用较重字形，Windows 正文使用 Microsoft YaHei UI。任务标题和正文分别为 22 px 和 14 px，两行描述保留换行。三个页面均使用浅底、细边框和小圆角区分区域。状态列同时显示圆点、文字和数量，当前导航与输入焦点分别使用背景色与外框。ShipForge 使用自己的品牌标记和线性操作图标，公开截图采用原始窗口捕获。小字号与灰色说明以实际窗口目视核对，此轮没有进行对比度测量或屏幕阅读器全流程验收。

## 实际操作记录

| 操作 | 复验结果 |
| --- | --- |
| 从看板新建任务，关闭并重新启动桌面程序 | 同一工作区再次显示 SF-5，打开后标题、两行说明及先后顺序正确。点击「关注」变为「取消关注」，再次点击回到「关注」。 |
| 上传并下载任务附件 | 将 31 字节的「验证文本.txt」上传至 SF-5，点击附件保存为新文件。两份文件的 SHA-256 均为 `74b9ab61c08e2fa5c5540daef6906ff688f98414a31a8a424b15d286a5160b8b`。 |
| 保存自动化规则 | 从界面创建「每日验收摘要」，选择「验收助手」。另一 CLI 进程读到一条已启用的每日触发器，时间 09:00，时区 `Asia/Shanghai`。 |
| 修改已有文档 | 在「工作空间使用指南」追加一行，点击「保存修改」。桌面历史列出版本 2 和版本 1，另一 CLI 进程读回带换行的新版正文，修订号为 2。 |

本次实机检查范围为上述浅色宽窗和本地工作区。其他显示缩放、窄窗的键盘遍历、定时器实际触发及远端离线冲突均未计入这次目视结果。各模块另有独立的功能验证记录。[Windows 功能覆盖](windows-coverage.md)

final result: passed

## 后续 Windows 空间导航与停用复验

在 Windows x64 上启动隐藏的 GPUI 原生窗口，使用独立 SQLite 工作区、软件研发空间和一条未完成任务。用例通过窗口的 UI Automation 树核对按钮，通过桌面操作入口提交变更，再从工作区读取任务终态。[可复现的原生窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/space_ui_test.rs)

| 操作 | 实际结果 |
| --- | --- |
| 从「更多」打开未固定的专项，返回任务页，然后固定专项 | 首次离开专项时侧栏入口消失。固定后返回任务页，专项仍在侧栏。 |
| 打开尚未启用的客户请求，启用后重开 | 首次进入空间功能页，并显示「启用模块」。重开后显示「新建客户请求」。 |
| 查看停用预览，不选处理方式提交，再选择取消未完成任务 | 预览为一项。空选项保留弹窗并显示错误，确认取消后转到「已停用」目录。 |
| 查看历史并恢复 | 历史任务仍能打开，侧栏快捷创建按钮消失。确认恢复后空间可写，原任务保持已取消。 |

## Teambition 和 CNB 的 Windows 连接器

隐藏 D3D 窗口打开两个连接器，用原生辅助功能树检查组织应用、个人授权、项目发现、回写、仓库配对和投递记录入口。测试读取密码字段的遮罩类型、逐项检查删除确认字段，在窗口提交暂停同步并查询 CNB 活动。与团队及供应商交互的结果由独立 HTTP 用例核对。[Windows 窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/enterprise_integration_ui_test.rs) · [CLI 与工作机验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-teambition-cnb.py)

Asana 同一窗口验证工作区发现、遮蔽的 PAT、项目预览、立即同步和字段或评论写回表单。同时在 CLI 和工作机回归核对真实服务返回的 Asana 处理人、状态、受限字段写入和评论回执。[Windows 窗口](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/enterprise_integration_ui_test.rs) · [Asana 隔离服务](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-asana-projects.py)

## Windows 对话列表、分叉和任务关联

在隐藏的 Windows x64 GPUI 窗口打开独立 SQLite 工作区。窗口使用真实桌面控件、后台命令和搜索输入框，测试从 UI Automation 树读取按钮标签。对话、Agent、任务和两条消息均为合成数据。 [原生窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/chat_ui_test.rs)

| 操作 | 窗口与数据结果 |
| --- | --- |
| 将对话置顶，再隐藏并撤销 | 隐藏期间对话标题消失，提示显示「撤销」。撤销后标题和「取消置顶」按钮再次出现。 |
| 隐藏后用另一个本地客户端发送同一对话的新消息 | 刷新后对话返回最近列表，按钮变为「置顶」。 |
| 在第一条消息分叉 | 打开新会话，历史仅含第一条发言。 |
| 从分支搜索并绑定已有任务，再打开任务 | 可以从分支打开任务，再从任务详情回到分支。源对话维持独立。 |

另以实际 CLI 和工作机启动外部 Python 合成运行时。进程分别读取源与分支的真实上下文，成功回传任务草稿并绑定原对话。重发相同请求只得到原运行和一个任务。另有核心用例占用运行租约，在同一对话提交转换，核对后一项等待先前工作完成。 [工作机验收脚本](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-chat-workflows.py) · [两个身份、租约与继承权限用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/chat_workflows.rs)

## Windows Agent 目录、分身和小队

在独立的 Windows x64 GPUI 窗口，UI Automation 读到了空间目录、设置页和历史按钮。合成工作区包含两个空间、两名 Agent、一支小队和一项已入队的工作。 [可重跑的原生窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/agent_directory_ui_test.rs)

| 操作 | 实际结果 |
| --- | --- |
| 从目录打开 Agent，再进入运行历史 | 目录可搜索、筛选，历史页有一项工作并能返回设置。 |
| 归档后关闭详情，打开「已归档」，再恢复 | 原 Agent 从常用目录消失，在归档视图能打开并恢复使用。 |
| 在设置中创建分身，从空间选择器选择第二个空间 | 窗口切到第二个空间，并打开新分身，可看到逐字段覆盖和恢复继承入口。 |
| 将小队成员设为专属，再回 Agent 目录 | 小队详情出现「取消小队专属」，独立目录不再显示该成员。 |

团队服务和三个独立 CLI 身份另验证同名跨空间结果、按人收藏、撤销源 Agent 权限及断网后的取数结果。核心用例验证领取运行后归档的取消状态、公开 Agent 私人历史的匿名行和管理者的只读范围。 [HTTP 验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-agent-directory.py) · [核心验收](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/agent_directory.rs)


### Linear、ClickUp 与 ONES 的集成表单

Windows 隐藏 GPUI 窗口在临时资料目录打开三个连接器的详情。原生无障碍树可读到账户发现、项目选择、凭据、预览和回写入口。测试检查个人令牌输入遮蔽、OAuth 回调字段、增量模式选择，并实际提交「暂停」后从另一客户端读回连接器状态。网络鉴权、工作机同步和回写另由模拟供应商及真实团队服务逐项验证。[原生窗口用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/cloud_tracker_ui_test.rs) · [隔离服务验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-cloud-trackers.py)

### 专项时间线和分拣

Windows 隐藏 D3D 窗口使用独立临时资料，打开分拣模块与待分拣任务，读取原生无障碍树中的接受、标记重复、拒绝、搁置和分享控件。通过原生表单提交搁置并检查数据库队列，再打开「显示稍后处理」找回任务。在专项详情验证日期模式和已排期条目，再用工作区限定的任务链接打开正确的详情。此次运行验证 ShipForge 本机窗口及核心数据，跨成员授权另由合成角色回归覆盖。真实 HiFox 组织仅用于只读参考。[Windows 回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/review_ui_test.rs) · [多身份回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/review_planning.rs) · [操作说明](triage-planning.md)

## Windows 对话指令和文档引用

独立本机测试工作区打开隐藏的原生 D3D 窗口，经 Windows 辅助功能树定位任务指令、管理、文档引用和 PNG 保存入口。测试向编辑器派发 Tab 后确认仅插入命令，派发 Esc 后确认输入框仍在，再发送并核对后端消息。工作机报告结果后，从详情发送审阅确认，任务保持原状态。PNG 在测试目录写出并核对文件签名。测试未接外部商业 Runtime。[原生测试](https://github.com/Hubujiu/ShipForge/blob/v0.11.0/crates/shipforge-desktop/src/conversation_ui_test.rs)

## 企业微信智能体

在真实 Windows 隐藏 D3D 窗口中，用临时工作区打开「外部应用集成 → 企业微信智能体」，从可用 Agent 列表选择 Agent，检查 BotID 文本框和掩码 Secret 字段，再向本机隔离的企业微信服务验证绑定。桌面显示接收范围和解绑入口。通过 Agent 详情的入口关闭群 @ 并提交，独立客户端重读到单聊启用、群 @ 关闭。解绑后，Agent 重新出现在候选中。企微企业账号、真实外部群及其消息不在这项原生验收中。[原生控件与数据读回](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/wecom_ui_test.rs) · [独立网络与多身份测试](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-wecom-bots.py)

## Windows 空间任务助手

使用临时工作区和隐藏的原生 D3D 窗口打开「外部应用集成 → 空间任务助手」，选择启用的 Slack 渠道并绑定。窗口测试保存英文组织模板的描述，覆盖并恢复继承，通过解除绑定确认相应状态。独立的多身份验收用实际工作机执行签名群消息，写入该空间的任务。原话题的回执由出站工作机向本机模拟的 Slack 服务提交。企业微信空间助手提供配置入口，普通 Agent 的长连接另列。[Windows 控件与状态](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/task_assistant_ui_test.rs) · [服务器与工作机](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-space-task-assistants.py) · [Slack 出站](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/src/slack/tests.rs) · [配置和渠道范围](space-task-assistants.md)

## 邮件通知与管理权转让

Windows 原生窗口用已验证的临时成员登录，在其独立团队服务产生一条邮件动态。从邮件里的桌面链接打开后，能在窗口中读取对应的任务标题，并从邮件设置进入投递记录。另一项原生窗口测试通过组织设置向第二位成员授予所有者权限。核心回归还验证了子空间仅可转给已加入的成员，使用过期版本会被拒绝，原所有者离开后任务仍可访问。[原生窗口](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/live_navigation_test.rs) · [管理权回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/organization_settings.rs)

## Windows 运行调度、目录与诊断

独立测试工作区在隐藏的原生 D3D 窗口打开「机器」，通过 Windows 辅助功能树识别授权目录和本机诊断入口。测试从 Agent 编辑器选择同一机器上的两个目录、切换到共用任务额度，提交后重读数据库验证保存。随后打开已预约任务，在详情触发提前排队，确认横幅消失，再从运行记录读取该次诊断。独立工作机验收用两个实际目录各启动 Python 进程，第三个任务等待目录释放，合成服务返回 429 后得到额度错误和本机 PID 日志。[原生窗口测试](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/runtime_ui_test.rs) · [工作机进程测试](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/tests/native_runtime.rs) · [用户指南](runtime-scheduling.md)


## 自动化 Webhook 安全设置

隐藏的 Windows D3D 窗口在已有规则中新增 Webhook，保存后通过辅助功能树定位「复制 URL」。从保存后的触发器进入安全设置，在原生文本框填入 IPv4 网段和 IPv6 地址，勾选两项校验后，独立客户端读回规范化名单。查看签名密钥后返回原编辑器，投递记录显示实际记录的来源地址。确认轮换后收到新 URL，旧入口立即返回 404。[Windows 原生测试](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/webhook_ui_test.rs) · [HTTP 和三身份验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-automation-webhooks.py)

## Windows 自动化运行详情

在临时工作区建立九次独立运行，从真实 Windows 隐藏 D3D 窗口打开规则。辅助功能树能定位暂停和下一页，末页的历史可打开原运行的私有对话并显示发送消息控件。回到规则依次暂停、恢复和归档，第二个客户端从同一数据库读回每次状态。服务端专项另用三个独立身份与本机运行时验证收件箱摘要、原会话续聊和人工审批失败。[原生窗口测试](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/automation_run_ui_test.rs) · [工作机和 CLI 验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-automation-history.py)

## 仓库授权与配对

在隔离 Windows 隐藏 D3D 窗口打开「集成」，辅助功能树中出现 GitHub、GitLab.com 和自建 GitLab。选择自建 GitLab，在真实文本控件输入模拟 HTTPS 实例和 PAT，提交后从项目列表选择测试仓库。将方向改为双向，提交后出现复制项目 Webhook 密钥的按钮。独立客户端读回双向配对。打开合成来源任务后，从任务详情的自建 GitLab 来源进入专用回复框，在原生多行文本控件输入正文并提交，回环项目服务收到发往该 Issue 的 HTTP POST。窗口提示已使用组织身份发送，测试从回执读回同一来源评论 ID。解除组织授权后，仓库列表为空。GitHub 与 GitLab.com 的 OAuth、浏览器回调及三身份收发通过独立 HTTP 夹具验证。真实租户仍待联调。[Windows 仓库控件](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/repository_ui_test.rs) · [来源协议回归](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-forge.py)
