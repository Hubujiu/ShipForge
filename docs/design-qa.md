# Windows 原生界面验收

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


### 专项时间线和分拣

Windows 隐藏 D3D 窗口使用独立临时资料，打开分拣模块与待分拣任务，读取原生无障碍树中的接受、标记重复、拒绝、搁置和分享控件。通过原生表单提交搁置并检查数据库队列，再打开「显示稍后处理」找回任务。在专项详情验证日期模式和已排期条目，再用工作区限定的任务链接打开正确的详情。此次运行验证 ShipForge 本机窗口及核心数据，跨成员授权另由合成角色回归覆盖。真实 HiFox 组织仅用于只读参考。[Windows 回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/review_ui_test.rs) · [多身份回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/review_planning.rs) · [操作说明](triage-planning.md)
