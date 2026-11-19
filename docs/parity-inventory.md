# Windows 产品入口与行为盘点

参考样本为 2026-11-18 核对的 HiFox Windows 0.4.538。Windows 对照以 ShipForge 0.6.0 的验收记录为起点。本表的单位是可单独安排验证的用户操作或条件分支。一个页面可产生多行，一条跨端行为可涉及多个页面。表的行数、帮助篇数和截图张数都不能作为产品的页面总数或完成率。

## 取证范围

各模块的验收目的见[功能基线][features]，通用记录的创建、更新和校验入口见[业务核心][domain]。

| 来源 | 本次实际核对的范围 | 能支持的结论 |
| --- | --- | --- |
| 官网 | 从 robots 指向的 sitemap 取得 26 个 URL，中英文各有 13 个对应页面。主页、4 个功能页和 8 个方案页均计入来源清单。 | 产品承诺、平台清单、预告和应用场景。解决方案中的工作流程不能计作额外客户端入口。[官网][home] · [站点地图][sitemap] |
| 官方帮助 | 11 月 18 日首轮导航有 62 篇。11 月 19 日再次读取欢迎页，共 65 篇，新增 Linear、ClickUp 和 ONES。九篇最佳实践列在文末。 | 操作前提、平台方向、角色和限制。更新日志和本机安装版本分别记录。[帮助导航][welcome] · [更新日志][changes] |
| 安装与 REA | 在安装目录重新核对两份 ASAR 的 SHA-256，与既有分析一致。对派生渲染模块的可见路由常量及桌面桥接能力作只读复核。 | 壳处理登录、注册、登出、组织、认证、邀请、引导、配对等路径前缀。静态线索无法证明该路径可达、业务授权或服务端行为。[安装样本与边界][rea] |
| 参考窗口记录 | 首批 UI01–UI36、补01–补07 的图像文件逐个匹配其本地清单的 SHA-256。另核对新手指引专门批次的侧栏、全页及展开项记录。 | “图”表示既有实机画面里观察到的状态。“菜单”仅指另一个已打开页面中出现了该名称。单个空态、已保存的截图和成功点击后的结果分别记载。[首批索引][ui] |
| ShipForge Windows | 既有公开验收记录分列看板、任务详情、附件、文档正文和自动化表单。核心测试、客户端协议、机器子进程和隔离 HTTP 服务分别给出定位。 | 只有具名实机操作记“实机”。“测试”指列出的特定协议或进程场景，“实现”仅表示能定位源码，“未证”表示该项还没有正向证据。三种标记均不能推断跨供应商端到端一致。[Windows 操作记录][native] · [协议][cli] |

### 怎样读表

“有对应”表示该行所述的具体操作已经有相应实现。“部分”说明实现覆盖了其中一段。“缺口”表示在本次核查范围内缺少该专用流程。“待核”用于前提、行为细节或版本对应仍待确认的项目。“预告”和“跨端”分别记录官方未来方向和 Windows 无法独立核证的场景。各行“验收维度”标出可观察行为、数据或权限。末栏写出能重复检验的输入、角色、触发及结果。功能一致性可以通过官方帮助的行为契约与自有实现的可重复验证建立；截图用于厘清含糊的入口或状态。当前 HiFox 组织只读，多成员写入、向外发信、绑定真实账号或执行真实任务均标为待联调。

“图 UIxx”沿用原图库编号。“图 补01–补07”依次对应组织设置、任务标签、Git 设置、偏好、键盘、通知渠道和桌面通知事件。新手指引另用“图 指引”。文中的本机标记均指参考方，ShipForge 的实操证据单列在 Windows 一栏。公开表仅记用户能观察的行为和证据定位。

## 工作台、身份与新手状态

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| A01 | 启动、登录、注册、重新认证、登出 | 壳 · [账号][account] | 有对应 | 测试 · [邮箱和设备登录][verified-identity] · [自托管配置][identity-guide] | 行为、数据、权限、待联调 | 新邮箱的双端验证创建独立身份与组织。已有邮箱使用原身份，客户端支持重登、退出及位置恢复。邮件网页可返回安装版；正式邮箱的送达时间另验。 |
| A02 | 新账号首次进入组织、选择或创建组织 | 壳 · [快速上手][quick] | 部分 | 测试 · [全新 Windows 工作区][guide-ui] · [注册与组织隔离][verified-identity] | 行为、数据、权限、待联调 | 邮箱验证新账号得到专属组织，可设置组织、默认空间及起始类型，或加入邀请。微信注册及原版云端入驻尚待联调。 |
| A03 | 多账号与多个组织间切换，重启恢复最后位置 | [账号][account] | 有对应 | 测试 · [两个账号与旧版升级][account-tests] | 行为、数据、权限 | 两个独立成员可在同一设备切换并分别找回页面、空间和任务。测试覆盖撤销空间权限后回退、过期账号重新认证、异账号拒绝替换和本机退出。 |
| A04 | 邀请链接、授权返回与机器配对入口 | 壳 · [成员][members] · [机器][computers] | 部分 | 测试 · [邀请协议][invites-tests] · [机器配对][execution] | 行为、数据、权限、待联调 | 邮箱和通用邀请覆盖身份、空间范围、过期与撤销，安装版通过系统链接打开接受窗口。真实认证回跳、邮件投递和设备配对另行核对。 |
| A05 | 永久注销账号并退出各组织 | [账号][account] | 有对应 | 测试 · [隔离邮箱、两个设备与其他成员][verified-identity] · [影响说明][identity-guide] | 行为、数据、权限、待联调 | 核验邮箱及文字确认后关闭账号，保留他人组织的业务。共享机器权限阻断注销，多人组织解散须另行确认。另端会话与个人令牌立即失效，同邮箱重注册得到新身份。原版现用组织保持只读。 |
| A06 | 标题栏后退、前进、刷新，恢复空间和对象 | 图 UI02、UI03、UI25 | 有对应 | 实现 · [导航][shell] · [桌面状态][app] | 行为 | 分别从抽屉、搜索结果和跨空间位置返回，检查筛选、焦点及滚动位置。 |
| A07 | 全局搜索及命令菜单，按对象、操作、权限定位 | 图 UI25 · [空间][spaces] | 有对应 | 测试 · [双窗口与撤权][live-windows] · [搜索][navigation-flow] | 行为、权限 | 按名称筛选新建、操作、导航及跨空间对象。两个空间中的同名任务显示独立位置，打开时重验权限，撤权后的搜索与后退均无法重现旧内容。 |
| A08 | 首页的快捷新建、双层导航、自定义导航、折叠更多模块 | 图 UI01、UI21 · 菜单 UI01 | 有对应 | 测试 · [快捷新建][live-windows] · [空间导航][space-lifecycle-tests] | 行为 | 首页可选择任务或四种对话，空间功能可固定、取消、折叠到更多。按成员读取持久化导航，测试涵盖重开和不同账号。 |
| A09 | 侧栏新手清单，展开四个快捷行动、更多、隐藏 | 图 指引 · 图 UI01 | 有对应 | 测试 · [原生侧栏操作][guide-ui] · [成员设置持久化][guide-tests] | 行为、数据 | 自建 Windows 隔离库实测四项行动、展开、隐藏、恢复和重开。 |
| A10 | 新手全页，六组 36 项、进度、积分和名次 | 图 指引 · [快速上手][quick] | 有对应 | 测试 · [原生完整向导][guide-ui] · [36 项计分][guide-tests] · [用户手册][guide-manual] | 行为、数据 | Windows 原生窗口展开六组。公开业务操作可完成 36 项并达到 560 分。旧档恢复、同分名次及成员隔离分别核对。 |
| A11 | 从新手行动进入工作流并返回，跨设备完成相同步骤 | 图 指引 · [快速上手][quick] | 有对应 | 测试 · [真实业务计分][guide-tests] · [Windows 工作流][guide-ui] · [两份独立设备资料][verified-identity] | 行为、数据、权限、待联调 | 36 个指引行动通过实际操作计分。同一账号从另一设备能读到首次设置和已获积分，重复完成不重复计分，其他成员有独立进度。原版多人或第三方事件归属待独立组织联调。 |
| A12 | 本地机器入口、帮助与交流群、更新/关于 | 菜单 UI01、UI33 · 壳 · [CLI][h-cli] | 部分 | 实现 · [设备设置][device-guide] · [执行端控制][native-worker] · [桌面入口][main] | 行为 | 机器入口与关于可用。版本、本机手册和新手指引可离线打开，更新读取正式发布。社区入口与地区差异仍待核对。 |
| A13 | 在浏览器或手机查看、回复阻塞、派发到桌面机器 | [任务产品页][p-tasks] · [官网][home] | 跨端 | 测试 · [团队 API][team] | 行为、数据、权限、待联调 | 两客户端 HTTP 验证限于服务。移动推送、手机应用内打开与跨端接续需真实设备。 |
| A14 | 专用网络环境、代理、登录失败和重新进入工作区 | UI33 · Windows 原版代理设置 · [私有化方案][s-private] | 部分 | 测试 · [设备网络验收][device-tests] · [客户端][client] | 行为、数据、权限、待联调 | 原版已点开系统、直连、手动模式。ShipForge 支持 HTTP、HTTPS、SOCKS5，测试草稿、保存应用及回环绕行。真实企业代理、PAC 与证书条件待联调。 |

## 设置、组织与成员

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| S01 | 组织设置中的默认语言、任务优先级档数与危险操作区 | 图 补01 · [成员][members] | 有对应 | 测试 · [Windows 表单][live-windows] · [组织与导入][org-default-tests] | 行为、数据、权限 | 新空间按组织语言初始化。四档、五档与无优先级分别存储，降低档数保留历史；导入最低级时自动升级。离开与解散校验身份、组织名和成员影响。 |
| S02 | 组织空间目录、公开加入和退出、可选模块、个人固定、停用与恢复 | 菜单 UI33 · 图 UI21 · [空间][spaces] | 有对应 | 实机 · [Windows 空间验收][native] · [原生窗口用例][space-window] · 测试 · [成员与状态][space-lifecycle-tests] | 行为、数据、权限 | Windows 实测临时入口、固定、启用客户请求、预览未完成任务、停用后历史只读与恢复。隔离数据库覆盖公开加入、私有权限、迁移回滚、运行租约失效及个人固定跨重启。 |
| S03 | 组织成员清单、角色、邀请、邀请链接及外部账号 | 图 UI33 · [成员][members] | 部分 | 测试 · [邀请权限][invites-tests] · [两身份加入][invites-http] · [邮件回执][mail-tests] | 行为、数据、权限、待联调 | 可指定邮箱、角色和空间，或使用共享链接。支持多所有者、转让和最后一位所有者保护。发信、撤销与同一回执重试已在独立服务验证，生产发件域和第三方身份合并待联调。 |
| S04 | 空间成员与组织角色并存，访客仅能访问获邀空间 | [成员][members] · [分拣][triage] | 有对应 | 测试 · [成员与空间][workspace-tests] · [权限实现][access] | 行为、数据、权限、待联调 | 合成 Guest 获取显式成员资格前后、撤销后分别验证列表、搜索和直接读取。未加入的组织管理员可管理私有空间元数据，成员及任务仍需空间权限。 |
| S05 | 成员 rail 区分 Agent、小队、真人、机器，检索并打开成员抽屉 | 图 UI17 · [成员改版][nov13] | 有对应 | 测试 · [成员目录与真人对话][live-windows] · [使用说明][navigation-flow] | 行为、数据、权限 | 分类查询由服务端逐空间、逐对象鉴权。两名真人在两个空间同名时仍可按空间与 ID 区分，点击真人详情会打开选定空间的双方私聊。 |
| S06 | 管理范围的 Agent、小队、机器与空间内的同名入口 | 图 UI06、UI11、UI13 · 菜单 UI33 | 部分 | 实现 · [目录][pages] | 行为、数据、权限 | 检查共享/私有及本人/空间过滤，管理动作对不同身份可见范围。 |
| S07 | 全局与空间任务标签、名称筛选、分组、互斥、合并及归档 | 图 补02 · [标签][labels] | 有对应 | 测试 · [标签范围与批量事务][tag-tests] · [Windows 标签表单][tag-ui] | 行为、数据、权限 | 组织与空间标签可并用，同组后选替换先选。回归覆盖归档保留历史、合并更新任务和视图、多标签交集筛选、跨空间保留组织标签及批量失败回滚。 |
| S08 | 组织状态库的类别、用量、顺序、默认值及多语言名 | 图 UI14 · [状态][statuses] · [11 月更新][changes] | 有对应 | 实机 · [Windows 验收][native] · [窗口用例][status-window] · [独立身份 HTTP][status-team] · [迁移回归][status-tests] | 行为、数据、权限 | 已验中英文及缺译回退、状态分组折叠与展开、空间同 ID 冲突保留、跨空间用量、默认值、过期预览以及停用空间迁入。 |
| S09 | 组织默认的字段、关联类型及空间任务类型管理 | 图 UI34、UI35 · [类型][types] · [关联][relations] | 有对应 | 测试 · [工作区][workspace-tests] · [类型工作流][status-tests] | 行为、数据、权限 | 已验空间管理员可配置各类型默认状态及顺序，移除在用状态会被拒绝；字段与关联另有工作区回归。 |
| S10 | 组织、空间、Agent 三级代码库与提交作者设置 | 图 补03 · 菜单 UI29 · [代码库][repos] | 部分 | 测试 · [工作树][worker-tests] | 行为、数据、权限 | 本地已批准仓库能执行。核验空间继承、Agent 覆盖、作者邮箱、共同署名和实际 Git 提交作者。 |
| S11 | 个人资料、第三方账号绑定、重新绑定与外部账号归属 | 菜单 UI33 · [账号][account] · [GitHub][i-github] | 部分 | 测试 · [已验证账号的个人资料][verified-identity] · [身份][auth] | 行为、数据、权限、待联调 | 姓名、头像地址及简介可编辑，两个设备共享，覆盖并发版本冲突。第三方身份绑定、换绑和外部评论归属仍待实现与联调。 |
| S12 | 偏好设置含五种主题、界面语言、任务会话自动打开开关 | 图 补04 · [更新日志][changes] | 有对应 | 测试 · [原生偏好和输入][personal-ui] · [个人设置手册][personal-doc] | 行为、数据 | 已在 Windows 原生窗口切换全部主题与 English、验证两种发送设置。设置按成员持久化。操作系统主题切换仍受 Windows 配色控制。 |
| S13 | 快捷键，发送消息组合、录制、冲突与恢复默认 | 图 补05 | 有对应 | 测试 · [原生键盘录制与输入法][personal-ui] · [设置持久化][personal-regression] | 行为 | 已验证组合冲突、恢复、重开及实际消息文本框的 Enter/Shift+Enter 输入与提交。 |
| S14 | 通知设置，桌面、移动、浏览器、邮件、收件箱与四个 IM 渠道 | 图 补06 · [收件箱][inbox] | 部分 | 测试 · [多身份隔离验证][inbox-http] · [邮件独立投递][mail-tests] · [使用说明][inbox-manual] | 行为、数据、权限、待联调 | 九个渠道分别保存总开关、组与事件。关闭收件箱后，Windows 通知和邮箱仍可独立投递，且收件箱未读数保持原值。移动、浏览器及四种 IM 的个人通知路由待联调。 |
| S15 | 桌面通知详情总开关、事件组、恢复默认 | 图 补07 · 壳 · [收件箱][inbox] | 部分 | 实机 · [Windows 通知设置][native] · [原生用例][inbox-window] · [多身份隔离验证][inbox-http] | 行为、数据、权限、待联调 | 已通过 Windows 原生设置停用分配提醒并恢复默认，关闭收件箱后用仅桌面投递的链接打开任务。访问权限随成员和来源评论重新检查。系统横幅及通知中心鼠标激活待安装环境验收。 |
| S16 | 个人访问令牌，创建、到期、仅一次展示明文、吊销 | 菜单 UI33 · [个人令牌][pat] | 有对应 | 测试 · [两个身份和真实 CLI][personal-token-tests] | 行为、数据、权限 | 团队成员可选 30 天、90 天、一年或永久。验证首次披露、近期使用、可见机器与吊销后立即拒绝认证。安装签发的凭据限制派生权限，详情见手册。 |
| S17 | 设置中的开始使用、网络代理、关于、地区/账号条件 | 菜单 UI33 · Windows 原版代理设置 · [账号][account] | 部分 | 实现 · [设备设置][device-guide] · [目录][pages] | 行为、权限 | 设置可打开代理、关于和按成员保存的新手指引；独立设备资料分别存代理。原版关于与地区条件仍待核对。 |
| S18 | 组织全局默认与空间、用户、设备的配置归属 | 图 补01、补03、补04、补07 · [代码库][repos] | 部分 | 实现 · [客户端][client] · [权限][access] | 行为、数据、权限、待联调 | 用两个成员两台机器测跨空间继承、按账号同步与设备独享。偏好持久化本身无法证明其生效。 |

## 任务、迭代、专项与分拣

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| T01 | 空间内新建任务，填写标题、描述、属性、处理人及负责人 | 图 UI02、UI03、UI24 · [任务][tasks] | 有对应 | 实机 · [建单及重开][native] · [多成员指派][assign-ui] | 行为、数据、权限 | 已在原生空间设置开启多人，从表单同时选中成员与独立执行者并验证保存。两客户端跨角色创建、同一编号重开仍须按配对数据比对。 |
| T02 | 空间看板、类型切换、折叠空列、列表、表格和分组 | 图 UI02 · [视图][hviews] | 有对应 | 实机 · [看板][native] · [多人分组][assign-ui] · 实现 · [列表][views] | 行为 | 已验同一任务显示在两个人员列，页面总数仍是一项。校验拖放、键盘操作、多列滚动、两种布局下相同筛选结果。 |
| T03 | 我的任务中指派给我/我创建的，查找与仪表盘 | 图 UI18、UI19 · [视图][hviews] | 有对应 | 实现 · [个人页与统计][views] · [查询][query] · [按多人集合查询][assign-core] | 行为、权限 | 个人归属、逾期边界、近期到期及按状态/优先级统计需用多成员数据比对。 |
| T04 | 任务详情、编辑、关注切换、上传下载附件 | 图 UI03 · [关注][follow] · [评论][comments] | 有对应 | 实机 · [关注及附件往返][native] | 行为、数据、权限 | 已实测单份附件字节与重开。仍需比对大文件、长评论、富文本、图片粘贴及多附件错误反馈。 |
| T05 | 任务类型绑定字段和状态，切换类型时处理值 | 图 UI14、UI34 · [类型][types] | 有对应 | 测试 · [多身份及并发][work-tests] · [原生窗口][work-window] · [状态工作流界面][status-window] | 行为、数据、权限 | 已验字段损失确认、共享与隐藏值保留、必填和工作流校验、解绑期间并发修改后须重新预览；类型选择器展示自己的状态顺序和默认值。 |
| T06 | 子任务、阻塞、相关、重复与自定义方向，从两端追踪 | 图 UI35 · [任务关联][relations] | 有对应 | 测试 · [关系约束][workspace-tests] | 行为、数据、权限 | 以跨空间可见/不可见的另一端验证关系、删除、去重和重新打开。 |
| T07 | 保存个人或空间视图，预览、另存、链接、个人置顶和排序 | 图 UI05、UI28 · [视图][hviews] | 有对应 | 测试 · [多身份及顺序][work-tests] · [Windows 窗口][work-window] · [团队 CLI][work-service] | 行为、数据、权限 | 已验配置另存、临时筛选、置顶和组内排序、重开库、相同链接按当前成员权限查询以及撤权。复制配置以官网说明的另存行为验收。 |
| T08 | 迭代时间轴、待规划、日期、容量、完成数、当前和未来 | 图 UI04 · [迭代][iterations] | 有对应 | 测试 · [时间轴][collab] · 实现 · [原生迭代][planning] | 行为、数据、权限 | 已核验日期和任务计数。比对有数据/无日期、关闭、未启动和不同角色的实际窗口。 |
| T09 | 手动结项前预览，将未完成项移入下期或待规划 | [迭代][iterations] | 有对应 | 测试 · [原子转移][workspace-tests] | 行为、数据 | 跨端并发结项、存在取消状态与目标关闭时核对失败提示、剩余任务去向和编号。 |
| T10 | 启用自动迭代、按周期建未来迭代、自动加入和顺延 | [迭代][iterations] | 有对应 | 测试 · [固定时钟与成员隔离][iteration-tests] · [Windows 服务补跑][iteration-service] · [使用手册][iteration-manual] | 行为、数据、权限 | 固定时钟跨过夏令时和当地午夜，验证冷却、仅顺延未完成任务、只回填窗口内的匹配变化。临时服务离线一周后在首次客户端请求前完成结项。停用后检查历史及未来任务。 |
| T11 | 专项空态、新建、多空间范围、负责人及汇总进度 | 图 UI22 · [专项][projects] | 有对应 | 测试 · [跨空间进度][collab] | 行为、数据、权限 | 参考图为空态。使用多空间成员与管理员打开有数据的专项，核对分母、失权和隐藏任务。 |
| T12 | 专项时间线，按开始、截止或计划区间查看排期 | [专项][projects] · [项目产品页][p-planning] | 有对应 | 测试 · [排期与权限][planning-tests] · [原生控件][planning-native] | 行为、数据、权限 | 图形条带显示重叠日期与前置关系，可按日期依据切换并打开任务。无日期工作独立列出。不同成员的跨空间范围、任务与总数独立计算。 |
| T13 | 分拣开关、进入设置、来源与类型、未加入空间的投递 | 图 UI23 · [分拣][triage] | 部分 | 测试 · [投递与关闭][planning-tests] · [使用说明][planning-flow] | 行为、数据、权限、待联调 | 私有空间受邀访客与未加入公开空间的普通组织成员有独立入口。关闭后不可投递，待分拣迁移失败时整批回滚。已受邀公开空间访客的原版限制与真实供应商联调待验证。 |
| T14 | 接受、搁置、拒绝、重复、分拣责任、多规则和批处理 | [分拣][triage] | 部分 | 测试 · [队列事务与角色][planning-tests] · [Windows 表单][planning-native] | 行为、数据、权限、待联调 | 支持按时间返回、提前恢复、批量处置、通知或指派指定成员、按序覆盖的规则及冲突提示。保留已有外部标识或执行历史的跨空间路由待联调。 |
| T15 | 任务编号、链接分享、外部 Issue 编号及历史来源 | [任务][tasks] · [Jira][i-jira] | 部分 | 测试 · [链接权限][planning-tests] · [导入][metadata] | 行为、数据、权限、待联调 | 空间内编号、复制链接、工作区校验、接收者任务权限与跨组织重复目标已覆盖。真实厂商编号强制匹配及另一台设备上的协议关联待联调。 |
| T16 | 任务活动、讨论线程、@、回复、快捷审阅与解决/重开 | 图 UI03 · [评论][comments] | 有对应 | 实机 · [Windows 记录][native] · [线程与执行租约][work-tests] · [审阅事务][commands-tests] · [Windows 审阅操作][commands-native] | 行为、数据、权限、待联调 | 已验任意层回复与重开、原生发送审阅确认仅清待审、失败或权限不足保留待审、重复请求仅一条评论。真实外部 Agent 或 IM 提醒仍待隔离环境联调。 |
| T17 | 关注者、自动关注与只通知相关人 | [关注][follow] · [收件箱][inbox] | 有对应 | 测试 · [自动与手动关注][inbox-tests] · [多身份收件箱][inbox-http] | 行为、数据、权限、待联调 | 支持按人更改、描述与评论提及、任务处理人及单/多选成员字段自动关注。取消关注后改无关属性不重新订阅，自动关注不会产生新增关注者提醒。Jira 生产 watcher 映射另行联调。 |
| T18 | 任务指派 Agent/小队，待规划后进入可执行状态、回收结果 | 图 UI03 · [任务执行][run-help] | 有对应 | 测试 · [工作流与租约][execution] · [真实进程][worker-tests] | 行为、数据、待联调 | 用已授权的真实外部 Runtime 验证请求人工判断与验收状态；示例程序不能代表所有模型。 |
| T19 | 收件箱重要/普通、未读、稍后唤醒、清理、空间搜索 | 图 UI09 · [收件箱][inbox] | 有对应 | 实机 · [打开任务并回到原分区][native] · [真实 HTTP 验收][inbox-http] | 行为、数据、权限 | 从两个空间收取同名任务，验证搜索正文、筛选空间、单分区批量已读/清理、稍后到期返回及撤权后消失。读过的重要活动保留分区。 |
| T20 | 任务与对话用量、运行中/阻塞状态与侧栏增量更新 | 图 UI36 · [任务执行][run-help] · [11 月日志][changes] | 部分 | 测试 · [进度/取消][worker-tests] | 行为、数据、待联调 | 参考 Agent 详情仅见零条历史。“运行中”持续时间、其他成员收到更新和空态退场需实际运行。 |

## 对话、文档、市场与 Agent

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| C01 | 独立对话、个人小助手、任务内对话与真人私聊 | 图 UI16、UI32 · [对话][chat] · [系统助手][agents] | 有对应 | 实机 · [Windows 记录][native] · [跨身份助手和任务上下文][assistant-tests] · [HTTP 服务及真实工作机][assistant-team] · [成员对话][chat-tests] | 行为、数据、权限、待联调 | 首次进入创建本人组织助手，发送后保存会话，重入可打开最近记录。本人工作机已完成两轮外部进程对话。实际商业 Runtime 的账号和模型另行联调。 |
| C02 | 新建 Agent/真人对话、群聊和新任务的选择器 | 菜单 UI01 · [成员与群聊][nov13] | 有对应 | 测试 · [原生创建入口][live-windows] · [群聊与私聊][nav-tests] | 行为、数据、权限 | 当前空间内选人、Agent 或小队，群组可一次加入多人和 Agent。已测首条群消息、无效成员整批回滚、真人双向重开同一私聊以及无关成员拒读。 |
| C03 | 群聊添加/移除人或 Agent、头像名称描述、退出与群主解散 | [群聊][chat] · [群组更新][nov13] | 有对应 | 实机 · [Windows 记录][native] · [文件选择及图片显示][assistant-native] · [HTTP 群主转让][assistant-team] · [群成员与所有者][chat-tests] · [群头像使用手册][conversation-help] | 行为、数据、权限 | 窗口上传、显示和移除群头像。多身份验证图片归一化、修订冲突、群主转让前后权限、成员移出和群解散后的读取限制。 |
| C04 | 群聊免打扰、@所有人、@我穿透和仅清自己历史 | [群聊偏好][nov13] | 有对应 | 测试 · [三人提醒与历史][chat-tests] | 行为、数据、权限、待联调 | A 发普通消息、群体提及、单独提及 B。B 开免打扰且忽略群体提及，仅收到单独提及。B 清空后 A 和 C 历史仍可读。真实客户端系统推送另行联调。 |
| C05 | 私有会话授权管理、邀请/撤回、任务内私有会话 | [会话授权][nov13] · [对话][chat] | 有对应 | 测试 · [两个独立登录的本地团队客户端][chat-http] · [任务权限][chat-tests] · [隐私继承][execution] | 行为、数据、权限、待联调 | 接受前、撤回后，直接读取、附件下载、搜索、事件和产物各按当前身份验证。任务可见性收紧也收回会话访问。离线不能重开此前取过的消息，参考生产租户的跨账号步骤待联调。 |
| C06 | 对话列表单人置顶、不显示、撤销、新消息恢复 | [侧栏更新][changes] · [对话][chat] | 有对应 | 实机 · [Windows 记录][native] · [窗口用例][chat-flow-ui] · [个人状态][chat-flow-tests] | 行为、数据、权限、待联调 | 独立成员的置顶和隐藏可跨重开保留，撤销恢复原位置。产生本人可见的新消息后重新进入最近对话。 |
| C07 | 从指定消息 Fork、分享已完成片段、转任务、Slash 与快捷指令 | [对话][chat] | 有对应 | 实机 · [Windows 记录][native] · [Windows 操作][commands-native] · [外部进程][chat-flow-worker] · [三身份核心][commands-tests] · [HTTP 图片分享][commands-team] · [已有 Fork、转任务][chat-flow-tests] | 行为、数据、权限、待联调 | 已验 Slash 的 Tab 插入、Esc 关闭、运行时下线、忙时排队、独立会话和任务快照，以及图片导出、组织限制和撤销。外部 Runtime 声明的原生命令逐家执行仍待联调。 |
| C08 | 对话工具调用、流式进度、人类回复、同目录继续 | [对话][chat] · [任务执行][run-help] | 部分 | 测试 · [MCP 子进程与进度][worker-tests] | 行为、数据、待联调 | 工具回报与续跑有测试。实际长对话工具卡、流式输出和中途停单在双端的展示待验证。 |
| C09 | 文档库个人/团队区、新建、检索、分享链接与实时权限 | 图 UI08 · [文档][docs-help] | 有对应 | 实机 · [Windows 记录][native] · [Windows 文档入口][commands-native] · [多身份 HTTP][commands-team] · [正文及历史][workspace-tests] | 行为、数据、权限 | 任务、评论和文档里的引用按当前读者打开。已验跨空间身份、正文变更与移出空间后，链接及嵌入同步反映新权限。组织权限检查在本地团队服务验证。 |
| C10 | 文档历史、恢复、对话产物存入库与附件下载 | 图 UI08 · [文档][docs-help] | 有对应 | 实机 · [文档版本][native] · 测试 · [文件及产物][collab] | 行为、数据、权限 | 实机编辑形成第二版。历史恢复、保存产物后删除源、另一成员下载是独立的测试场景。 |
| C11 | 市场的技能/Agent 模板两个入口、分类、搜索、详情和安装 | 图 UI07 · [技能][skills] · [官网][home] | 有对应 | 实机 · [原生市场筛选、详情及安装][native] · [HTTP 跨空间目录][market-http] | 行为、数据、权限 | 可按关键词及分类浏览内置和团队条目。空间成员看到共享安装状态，模板可创建独立实例。 |
| C12 | 技能包合集、团队发布、更新/卸载、从模板创建独立 Agent | 图 UI07 · [市场][home] · [技能][skills] | 有对应 | 测试 · [双身份团队发布、版本和绑定][market-http] · [独立实例与本地修改冲突][market-tests] | 行为、数据、权限 | 发布范围可选空间或组织；技能合集升级与卸载会更新空间副本，已排队的运行保留原版本。Agent 模板实例与其技能副本可独立维护。 |
| C13 | 空间技能与连接器切换，URL、文件/文件夹或机器技能导入 | 图 UI20 · [技能][skills] · [项目技能目录][nov13] | 有对应 | 实机 · [原生文件导入][native] · [项目技能发现与队列][worker-tests] · [URL 与文件树读取][skill-files] | 行为、数据、权限、待联调 | 已验证文件、目录、ZIP、同名关联及机器重连处理积压。Windows CLI 实际从 GitHub 固定版本和公开 Gist 读取技能，并在临时工作区复查名称和已存指令。[实网验收][live-skills] |
| C14 | 空间 Agent、全部/我的/共享/私有、设置与运行历史 | 图 UI06、UI29、UI36 · [Agent][agents] | 有对应 | 实机 · [Windows 目录和非空历史][native] · [跨空间同名与个人收藏][directory-http] · [权限和用量][directory-tests] | 行为、数据、权限、待联调 | 目录支持空间、归属、可见性、归档和搜索。私有 Agent 的运行记录仅所有者可见。普通成员在公开 Agent 的历史里看到匿名私聊行，管理者可读有对应运行的历史正文。累计用量来自真实回报，未获数据时显示空值。 |
| C15 | Agent 指令、归档与恢复，归档后停止新指派/提及 | 图 UI29 · [Agent][agents] | 有对应 | 实机 · [原生归档与恢复][native] · [运行取消和评论提及][directory-tests] | 行为、数据、权限 | 队列中的工作取消，正在工作的机器接到取消信号。历史任务、评论、对话保留。归档项退出独立指派和提及范围，恢复后可重新使用。 |
| C16 | 分身跨空间复用、逐字段覆盖、恢复继承及父配置更新 | [分身][variants] · [Agent 产品页][p-agents] | 有对应 | 实机 · [选择空间创建分身][native] · [两个独立身份与服务][directory-http] · [技能来源和快照][directory-tests] | 行为、数据、权限、待联调 | 新运行继承当前配置，旧运行保留既有快照。模型、参数等可分别恢复继承。私有源撤权会收回分身与其运行快照的访问。各机器的真实商业 Runtime 仍需按接入环境验证。 |
| C17 | 小队分组、队长分派、成员结果汇总、真人协作 | 图 UI13 · [小队][crews] | 部分 | 测试 · [委派及汇总][execution] | 行为、数据、权限、待联调 | Agent 子运行与汇总可追踪。验证人机混编、在同一任务中 @ 真人及中途替换成员。 |
| C18 | 小队聊天、收藏、专属 Agent、共享成员工作目录及归档 | [小队][crews] | 有对应 | 实机 · [小队原生设置][native] · [同机双进程与隔离任务][worker-tests] · [专属成员接单与私聊边界][directory-tests] | 行为、数据、权限 | 小队管理者决定后续任务是否共享。队长、成员与汇总在同机保留本地修改。不同任务、小队、机器与受限会话分开。 |
| C19 | 工具、知识库、MCP 连接、授权与真正调用外部服务 | 图 UI31 · [Agent][agents] | 部分 | 测试 · [受限 MCP 子进程][worker-tests] · [文档权限][execution] | 行为、数据、权限、待联调 | 参考画面仅展开工具分组。需有凭据的第三方工具验证授权、撤销、错误回传与轮换。 |
| C20 | Agent 设置的触发器与消息、任务列表、关联仓库及自动化 | 菜单 UI29 · [Agent][agents] | 部分 | 实现 · [设置表单][forms] | 行为、数据、权限、待联调 | 逐项打开设置后验证变更是否生效；面板中显示名称不代表各平台授权成功。 |

## 执行机器、日程与自动化

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| W01 | 安装/连接本机或远程机器，个人、空间、共享范围及心跳 | 图 UI11、UI12 · [机器][computers] | 部分 | 测试 · [真实执行端][worker-tests] | 行为、数据、权限、待联调 | 需在两台主机验证配对、所有者与共享访问、机器离线和重新绑定。 |
| W02 | 单个 Runtime 探测、重扫、错误、安装/升级及模型登录 | 图 UI12 · 壳 · [机器][computers] · [官网][home] | 部分 | 实机 · [原生验收][native] · [工作机隔离验收][machine-life-test] · [机器管理][machine-management] | 行为、数据、权限、待联调 | 单工具版本、安装、升级和模型回包经过本机合成程序覆盖。Claude Code 的公共分发、真实付费模型和各供应商交互登录仍待联调。 |
| W03 | 选 Runtime、模型/推理深度、环境、批准模式与额外参数 | 图 UI30 · [Agent][agents] | 部分 | 实机 · [原生验收][native] · [执行端实测][storage-model-test] · [机器配置][storage-models] | 行为、数据、权限、待联调 | 已验证模型与思考深度、逐次覆盖、固定参数及环境作用域。各商业 Runtime 的批准模式、完整模型元数据和登录环境仍待联调。 |
| W04 | 按任务/聊天分别限并发，最大运行时长、排队超时和取消 | 图 UI30 · [运行][run-help] · [更新日志][changes] | 有对应 | 测试 · [独立或共用额度与双时限][runtime-tests] · [进程取消][worker-tests] | 行为、数据、权限 | 任务和对话支持单独或共用额度，机器容量仍生效。排队和执行各有计时器。重试换发租约，旧结果不能覆盖新尝试。 |
| W05 | Agent 工作时段按星期、每日一至两段、IANA 时区 | 菜单 UI30 · [工作时间][hours] | 有对应 | 测试 · [逐日排班与夏令时][weekly] · [真实领取][execution] | 行为、数据、待联调 | 本轮按 IANA 时区与星期校验 0–2 段、上下界、两次秋季重复时刻及分身恢复继承；旧单段固定偏移兼容。参考实际运行待联调。 |
| W06 | 空闲时段外的等待原因、立即触发、已运行任务跨下班时刻 | [工作时间][hours] | 有对应 | 实机 · [Windows 预约横幅][native] · [调度与触发][runtime-tests] · [执行队列][worker-core] | 行为、数据、权限 | 指派、评论提及、直接聊天各有开关。计划时间可推迟至下一窗口；首次开班前不消耗等待额度。手动触发保留权限和并发约束。 |
| W07 | 独立 Git 工作树或空目录，多任务并行与清理 | 图 UI30 · [目录][directory] | 有对应 | 测试 · [两个真正工作树][worker-tests] | 行为、数据、权限、待联调 | 已验证隔离仓库。还需在桌面展示 Git 改动、review、合并/丢弃及清理状态。 |
| W08 | 选择目标机器目录、多路径择空闲、直接目录互斥 | 图 UI30 · [目录][directory] | 有对应 | 实机 · [Windows 目标机器目录选择器][native] · [真实进程与路径互斥][worker-tests] · [同一任务沿用原目录][runtime-tests] | 行为、数据、权限 | 本机用户授权目录后可按机器选择。直接模式按优先顺序选空闲路径，已占满的任务排队。授权仍有效时，既有任务或对话保留路径与模式。远程列表展示获准项目目录。 |
| W09 | 组织/空间/Agent 绑定仓库、平台凭据、默认分支及 PR/MR | 图 补03 · [代码库][repos] · [官网][home] | 部分 | 实现 · [工作机][worker-manual] | 行为、数据、权限、待联调 | 本地 Git 工作树与托管平台自动拉取/推送、生成 PR、独立作者设置分别核验。 |
| W10 | 运行状态、租约、心跳、人工回复、取消、产物和重试 | 图 UI36 · [执行][run-help] | 部分 | 测试 · [租约][execution] · [实际进程][worker-tests] | 行为、数据、权限、待联调 | 核心/进程路径已测。参考只有空历史截图；需非空、运行中、等待、失败、离线重连同场比对。 |
| W11 | 模型错误、限流、额度、凭据失效、机器诊断和本机日志 | [执行排障][run-help] · [CLI][h-cli] | 部分 | 实机 · [原生诊断入口][native] · [运行隔离与分类][runtime-tests] · [本机进程日志][worker-tests] | 行为、数据、权限、待联调 | 结构化错误码覆盖认证、权限、账单、限流、模型、上下文和图片。真实子进程验证 429、PID 和日志脱敏。尚待使用真实供应商账户验证各适配器错误语料。 |
| W12 | 本地存储占用、清理已结束的工作目录、迁移和升级 | [CLI][h-cli] · [变更记录][changes] | 部分 | 实机 · [原生验收][native] · [执行端实测][storage-model-test] · [机器配置][storage-models] | 行为、数据、权限、待联调 | 已验证容量预览、确认、内容冲突、活动租约和恢复保护及单个 Git worktree 回收。跨磁盘迁移与操作系统低空间告警仍待验。 |
| W13 | 空间自动化的空态、创建、执行者及多触发器启停 | 图 UI01、UI26、UI27 · [规则][automations] | 有对应 | 实机 · [原生表单][native] · 测试 · [规则][execution] | 行为、数据 | 实际保存过一条每日规则。分别触发状态变化、评论关键词与手动执行，观察同一运行回链。 |
| W14 | 间隔、每日/每周，IANA 时间、规则暂停后重启与补发 | 图 UI26、UI27 · [规则][automations] | 有对应 | 实机 · [保存时间][native] · 测试 · [时区计划][schedule] | 行为、数据 | 表单保存与 Worker 按时执行需分别举证；测试夏令时与持续在线的实际规则。 |
| W15 | 规则 Webhook 的地址、绑定凭据、去重、结果通知 | 图 UI27 · [自动化产品页][p-auto] | 部分 | 测试 · [团队回调][team] | 行为、数据、权限、待联调 | 绑定服务回调与重复投递可测。外部系统真实调用、目标收件人与原生通知另验。 |
| W16 | 规则历史、成功率/健康度、无人值守会话、暂停恢复归档 | [规则][automations] · [产品页][p-auto] | 部分 | 实现 · [规则与运行][automation-code] | 行为、数据、权限、待联调 | 运行记录存在。核验暂停保留历史、手动补跑、失败重试策略与无人值守会话访问。 |

## 外部集成、导入与实际供应商条件

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| I01 | 外部应用目录、实例/个人身份、目标空间、同步方向与连接状态 | 图 UI10 · [概念对照][mapping] | 部分 | 实现 · [集成页][views] · [供应商基线][providers] | 行为、数据、权限、待联调 | 目录有名称或能建连接器不能证明对应厂商已授权、回调或写回成功。 |
| I02 | 一次性导入、预览、字段/人员/状态映射、作业历史、撤回与导出 | 图 UI15 · [概念对照][mapping] | 部分 | 测试 · [行映射][metadata] · [事务][workspace-tests] | 行为、数据、权限、待联调 | 已有 JSON/映射行导入。参考图中有待开放入口，官方帮助另列已开放的 API/文件向导。核验实际账号、批次取消/恢复/回滚。 |
| I03 | Jira Cloud/自建实例、CSV/API、Webhook、人员和状态映射与写回 | [Jira][i-jira] | 部分 | 测试 · [Jira 事件样例][provider-tests] | 行为、数据、权限、待联调 | 支持入站 Issue。实例授权、评论、附件、账号归属、远端写回和导入回滚需逐环验。 |
| I04 | 已有 Jira 客户端改地址，读取/更新 Issue、执行流转及 JQL 搜索 | [兼容 API][i-jira-api] | 有对应 | 测试 · [Jira HTTP 与 SDK][jira-http] · [部署与用法][jira-doc] | 行为、数据、权限 | 实际 jira-python 3.10.5 操作本地团队服务。v2/v3 的任务、评论、流转、字段裁剪、JQL、分页及权限撤回分别验证。 |
| I05 | 禅道产品线/项目、版本条件、字段映射、状态回写与失败恢复 | [禅道][i-zentao] | 部分 | 实机 · [Windows 集成详情][native] · [REST v1、状态响应丢失和恢复][tracker-http] · [操作与版本门槛][tracker-guide] | 行为、数据、权限、待联调 | 验证实例登录、产品和项目分页、子需求和执行任务、绑定隔离、增量、缺陷原生流转及批次撤销。商业版差异及真实工作流待联调。 |
| I06 | TAPD 项目发现、两种身份凭据、导入、限流及持续同步 | [TAPD][i-tapd] | 部分 | 测试 · [双身份、分页和增量][tapd-http] · [模式与范围][vendor-imports] | 行为、数据、权限、待联调 | 公司及个人项目、已结束项、Story/Bug 历史和增量、429 恢复及批次回滚已在本地 HTTP 验证。真实租户、持续 Webhook、定时同步与出站写入待做。 |
| I07 | Bugly 提单 Webhook、产品范围、可选状态回写 | [Bugly][i-bugly] | 有对应 | 测试 · [提单、签名与重发][vendor-http] · [配置][vendor-imports] | 行为、数据、权限、待联调 | 产品级 URL 轮换、并发重投复用任务、权限受限的任务链接、状态队列和签名回写已在回环上游验证。真实产品的 OpenAPI 许可及送达待验。 |
| I08 | PingCode 企业/项目的工作项发现、导入、同步、回滚 | [PingCode][i-pingcode] | 有对应 | 实机 · [Windows 确认及撤回批次][native] · [三种认证、工作项、发布和回滚][tracker-http] · [设备授权及轮询][tracker-guide] | 行为、数据、权限、待联调 | v2 真实 HTTP 夹具检验企业及项目范围、修改时间增量、保留本地编辑、工作机轮询、原生状态和评论回执。真实企业的 OAuth 授权界面与配额待联调。 |
| I09 | Teambition 组织+项目、两层凭据、状态与删除写回 | [Teambition][i-teambition] | 部分 | 实机 · [Windows 记录][native] · [原生表单][enterprise-native] · [HTTP 与工作机][enterprise-http] | 行为、数据、权限、待联调 | 组织应用、本人项目发现、已授权项目导入及增量、成员可见性、字段和 ID 或唯一名称状态映射、精确确认远端删除已验。新建远端任务、依赖、事件订阅和生产应用仍待对齐。[操作说明][enterprise-guide] |
| I10 | Zoho BugTracker 项目导入、再次导入、轮换凭据和批次历史 | [Zoho][i-zoho] | 有对应 | 测试 · [门户、分页、重导与按批次撤销][vendor-http] · [配置][vendor-imports] | 行为、数据、权限、待联调 | 已验证 OAuth 头、活动与归档项目、长数字 ID、同毫秒增量、本地修改保护及跨空间权限。真实门户授权和线上配额待验。 |
| I11 | Asana 工作区绑定、项目导入/持续读入、限定字段写回 | [Asana][i-asana] | 部分 | 实机 · [Windows 记录][native] · [项目 HTTP 与工作机][asana-http] · [窗口][enterprise-native] | 行为、数据、权限、待联调 | 已验多工作区令牌选择、项目归属与分页、增量、旧任务就地接管、人员和状态映射、标题描述回写、新评论中断后核对、回滚。真实租户权限和线上 webhook 订阅待联调。[操作指引][asana-guide] |
| I12 | GitHub 组织/个人授权、Issue 双向字段、显式来源评论、PR/Commit 与回链 | [GitHub][i-github] | 部分 | 测试 · [三身份 HTTP][forge-http] · [PR/Commit][vcs-http] · [精确编号与作者检查][vcs-regression] · [Windows 入口][forge-native] | 行为、数据、权限、待联调 | 隔离验证安装签名与 OAuth、按授权仓库配对、只接新 Issue、来源回复身份与去重、撤销后保留任务。真实私库和组织审批待联调；历史处理人回补及自动向外同步任务仍有缺口。[边界][forge-guide] |
| I13 | GitLab.com 和自建 GitLab 授权、个人身份、Issue、MR 与项目 Webhook | [GitLab][i-gitlab] | 部分 | 实机 · [Windows 记录][native] · [自建连接与仓库配对窗口][forge-native] · [云/自建 HTTP][forge-http] · [MR 关联][vcs-http] | 行为、数据、权限、待联调 | 隔离覆盖 OAuth、PAT 范围、Maintainer 筛选、署名与组织回退、Issue 字段/评论和撤权。客户网络、企业真实身份待联调；Agent 内网代理、组级 Webhook、历史处理人回补和 MR 讨论待实现。[边界][forge-guide] |
| I14 | CNB 组令牌、仓库配对与 CI 配置，提交/议题/MR 回链 | [CNB][i-cnb] | 部分 | 实机 · [Windows 记录][native] · [原生表单][enterprise-native] · [签名回调及供应商 HTTP][enterprise-http] | 行为、数据、权限、待联调 | 已验子组仓库、组令牌、配对密钥轮换、CI 签名、末次提交/议题/MR 活动及回链丢响应后的去重。生产仓库的受保护密钥、真正的云构建及评论仍待联调。[配置指引][enterprise-guide] |
| I15 | Linear 项目、工作流状态映射、增量与评论 | [Linear][i-linear] · [概念对照][mapping] | 部分 | 测试 · [GraphQL、OAuth、状态及评论核验][cloud-tracker-http] · [协议][cloud-tracker-guide] | 行为、数据、权限、待联调 | 已验证同名状态映射、关系两端归属、OAuth PKCE 与刷新、按项目唤醒、旧任务接管及按批撤回。凭据按设备与身份保存，真实组织授权与配额待联调。 |
| I16 | Gitee、Bitbucket、Codeup 等官网列名平台 | [官网][home] · [项目产品页][p-planning] | 待核 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 按具体平台和账号核验账户绑定、字段映射及 PR 工作流。 |
| I17 | IM 浏览目录、空间任务助手与面向 Agent 的机器人两种关系 | 图 UI10 · 菜单 UI29 · [飞书][i-feishu] · [Discord][i-discord] | 部分 | 测试 · [代表性消息][provider-tests] · [飞书智能体][lark-http] | 行为、数据、权限、待联调 | 显式配置的空间入站连接器可建单。飞书/Lark 的 Agent 入口已验证普通消息与 /new 仅发起对话；官方内置空间助手的独立身份、Runtime、模板及生命周期仍待实现。 |
| I18 | 飞书国内云，任务助手、Agent 机器人、资源连接器、组织助手 | [飞书][i-feishu] · [群聊流程][bp-feishu] | 部分 | 测试 · [实现说明][lark-guide] · [三身份双云][lark-http] · [Windows 操作][lark-native] | 行为、数据、权限、待联调 | 飞书智能体可扫码，处理单聊、群 @、指定群、话题和表情，回复原消息并收发文件。空间助手、组织预览通知与资源连接器各有独立授权，真实租户与前述能力仍需联调。 |
| I19 | Lark 国际云的 OAuth/机器人/任务助手和通知 | 图 补06 · [Lark 与飞书][i-feishu] | 部分 | 测试 · [双云注册与传输][lark-http] · [Windows 双入口][lark-native] | 行为、数据、权限、待联调 | 同一个 Agent 可同时接飞书与国际 Lark，验证了同名应用和会话标识、不同令牌与撤销彼此隔离。正式国际租户扫码、应用审批及客户端呈现待联调。 |
| I20 | 企业微信的空间助手与 Agent 智能体、验签/解密、回复 | [企业微信][i-wecom] | 部分 | 测试 · [解密后的样例][provider-tests] | 行为、数据、权限、待联调 | 需要真实企业权限、回调认证和可见成员；普通入站建单不能证明群/单聊双向。 |
| I21 | 钉钉机器人与任务助手、通知与链接预览、Stream 生命周期 | [钉钉][i-dingtalk] | 部分 | 测试 · [已验证的 Stream/HTTP 载荷][provider-tests] | 行为、数据、权限、待联调 | 需凭据获取、续连、不同身份消息回复、任务卡片及租户授权状态。 |
| I22 | 个人微信扫码绑定一个 Agent、过期重新扫码 | [微信][i-wechat] | 部分 | 测试 · [iLink 与手机流程][personal-bots] · [三身份契约][bot-http] · [Windows 授权控件][bot-native] | 行为、数据、权限、待联调 | 本人扫码、验证码、私有对话、/new 及向原会话回传文字和加密附件。真实手机确认、账号可用性与平台风控需授权联调。 |
| I23 | QQ 官方机器人，单聊/群 @ 绑定 Agent | [QQ][i-qq] | 部分 | 测试 · [官方授权与网关][personal-bots] · [三身份契约][bot-http] · [Windows 入口][bot-native] | 行为、数据、权限、待联调 | 官方扫码、凭据绑定、独立单聊/群 @ 开关、WebSocket 重连与原会话回复。跨绑定用户先解绑；真实 QQ 权限、事件订阅和平台回复限额待联调。 |
| I24 | Slack 工作区安装、任务线程、个人通知、机器人及文件/表情互动 | [Slack][i-slack] | 部分 | 测试 · [签名回调与单选改派][slack-http] · [线程入库][im-regression] | 行为、数据、权限、待联调 | 隔离服务验证线程同步、工作区/频道隔离、已送达卡片单选替换多人、保留 Agent、权限撤销与重放。真实租户安装、个人通知、文件和表情仍待联调。 |
| I25 | Discord Gateway、Agent 机器人/任务助手、私信、频道、文件和命令 | [Discord][i-discord] | 部分 | 测试 · [Discord 事件][provider-tests] | 行为、数据、权限、待联调 | 需要授权的 Gateway、中继、已加入的服务器及合适 intents。真实触发与机器人回复待验。 |
| I26 | Telegram Bot、群或话题、私聊、附件与任务助手 | [Telegram][i-telegram] | 部分 | 测试 · [Telegram 更新][provider-tests] | 行为、数据、权限、待联调 | 需有效 Bot 和测试群，核对新话题、机器人账号抑制和真实回复。 |
| I27 | Microsoft Teams Bot Framework、频道/私聊、文件与空间助手 | [Teams][i-teams] | 部分 | 测试 · [Teams Activity][provider-tests] | 行为、数据、权限、待联调 | 需组织管理员许可、真实安装与认证中继；按会话回复与成员退组后的失权单验。 |
| I28 | 已绑定 IM 话题后，后续消息追加评论，任务进度回复原话题 | [群聊流程][bp-feishu] · [群聊方案][s-chat] | 部分 | 测试 · [入站边界][providers] | 行为、数据、权限、待联调 | 可靠的双向链路必须保留平台话题/本地任务映射、重试幂等及受权的实际出站回执。 |
| I29 | 平台账号映射、撤权/解绑、历史保留、远端与本地编辑冲突 | [概念对照][mapping] · [GitHub][i-github] | 部分 | 测试 · [三方合并][execution] · [来源过滤][provider-tests] | 行为、数据、权限、待联调 | 字段冲突有用例。每个供应商的外部身份、凭据生命周期及远端拒绝写入后的状态需另验。 |
| I30 | ClickUp 工作区连接、按 List 导入和持续同步 | [ClickUp][i-clickup] | 部分 | 测试 · [层级、分页、双向操作][cloud-tracker-http] · [配置][cloud-tracker-guide] | 行为、数据、权限、待联调 | 个人令牌及 OAuth、含无目录 List、多人分配、来源关系、增量和评论丢失回执已验证。Webhook 注册与真实企业审批待接入。 |
| I31 | ONES 团队凭据、项目发现、任务/需求/缺陷导入与同步 | [ONES][i-ones] | 部分 | 测试 · [团队和类型游标、原生流转][cloud-tracker-http] · [范围][cloud-tracker-guide] | 行为、数据、权限、待联调 | 区域 API 核验团队，按项目和类型分页，复核单项归属、缺陷与需求映射、工作流和评论确认。应用中心安装及真实工作流必填字段待联调。 |

## 客户端、服务端与未在 Windows 取证的条件

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| D01 | CLI 结构化查询和写入，与桌面操作同一工作区 | [CLI][h-cli] · [产品介绍][welcome] | 有对应 | 测试 · [CLI 手册][cli] · [协议][protocol] | 行为、数据、权限 | 操作/资源按 ShipForge 自有协议开放。参考 CLI 的命令行兼容性、安装方式和后台服务生命周期分别评估。 |
| D02 | 本机自动发现服务、托盘/守护进程、日志、配置 profile | 图 UI11、UI12 · 壳 · [CLI][h-cli] | 部分 | 实机 · [原生验收][native] · [工作机隔离验收][machine-life-test] · [机器管理][machine-management] | 行为、数据、待联调 | 用户级启动策略、真实托盘图标、进行中的任务在重启时中断、退出后台控制。真实用户注销再登录、整机重启及系统升级过程仍待点验。 |
| D03 | 自托管后台、成员身份、服务令牌、空间权限、撤销 | [私有化][s-private] · [成员][members] | 有对应 | 测试 · [双客户端 HTTP][team] · [团队服务][server] | 行为、数据、权限、待联调 | ShipForge 的自托管服务覆盖邮箱身份与管理员签发的令牌。原版云服务内部的部署与授权实现仍待核对。 |
| D04 | 离线读取和草稿、冲突、重连重送、撤权后访问 | [空间][spaces] · [Windows 对照][coverage] | 有对应 | 测试 · [团队客户端][team] · [本地缓存][client] | 行为、数据、权限 | 是 ShipForge 服务保障。参考同一操作的离线时限、冲突策略及撤权行为尚无配对实测。 |
| D05 | 事件、提醒和多人实时更新，退出后从另一端恢复 | [执行][run-help] · [任务产品页][p-tasks] | 有对应 | 测试 · [两个 Windows 窗口][live-windows] · [HTTP 与增量查询][team] | 行为、数据、权限 | 两个认证窗口同时打开同一私聊。第二端自动收到消息，网络中断时隐藏会话正文，重连后自动打开原对话并补齐唯一一份离线期间的消息；撤销空间成员后自动回退。 |
| D06 | Windows 安装、升级、数据保留、团队部署与恢复 | [CLI][h-cli] · [私有化][s-private] | 有对应 | 测试 · [安装验证][installer] · [恢复指引][windows] | 行为、数据、权限、待联调 | 可验证自有二进制/数据。参考云服务的备份、集群、计费和托管 SLA 需独立证据。 |
| D07 | 移动端、浏览器或邮件中的提醒，点击后返回对应任务 | 图 补06 · [官网][home] | 跨端 | 测试 · [隔离邮箱和浏览器落地页][mail-tests] · [Windows 指南][windows] | 行为、数据、权限、待联调 | 邮件链接可在浏览器打开对应的桌面资源，撤权后原链接失效。系统推送、移动端回跳及商业邮箱最终投递仍待跨端实测。 |
| D08 | Workflow 节点画布、审批和连接编排 | [官网预告][home] | 预告 | 未证 · [规则][automation-code] | 行为、数据、权限 | 官网标为 Coming Soon。普通自动化规则是另一种已存在的功能。 |
| D09 | 语言、本地主题、窄窗/缩放、键盘焦点、输入法与辅助技术 | 图 补04、补05 · [原生验收][native] | 部分 | 实机 · [浅色宽窗记录][native] | 行为 | 宽窗三种画面有记录。窄窗、深色、150% 缩放、完整键盘遍历、读屏与输入法分别复验。 |
| D10 | 特定账号未开放、新组织无数据、功能停用、离线与权限不足 | 图 UI05、UI16、UI23、UI36 · [空间][spaces] | 待核 | 实现 · [状态渲染][views] | 行为、权限 | 实机记录中的“零条”“未启用”“即将推出”需分别留证；对照时切换有数据、已启用及受限角色。 |
| D11 | 人在 Windows 审阅自动测试、API 测试、修复、文档自动化等结果 | [测试方案][s-test] · [实践][practice] | 跨端 | 测试 · [执行端][worker-tests] | 行为、数据、待联调 | 案例依赖模型、业务仓库和外部设备；案例内容与某个页面可达性应分别核证。 |

## 可重复的验收条件

1. **首次使用与成员。** 使用两个全新测试身份和一个只读/访客身份，分别记录邀请前、接受后、撤权后可进入的空间、对话、历史文件和搜索结果。对新手行动只登记触发方式、账户和重开后的完成判据。参考的分数徽章只观察到显示名次。
2. **执行端。** 使用两台获准的机器绑定测试 Git 仓库。在指定时区的工作时段与非工作时段分别指派，记录派发、持租机器、回复、超时、取消和验收。对模型登录、真实计费或第三方仓库写入仅用可以授权的测试账户。
3. **供应商。** 对每个需要的租户分别记录绑定主体、入站投递 ID、绑定话题/任务 ID、出站回执、重复发送和解除授权后的结果。平台名、一条静态样例、一个代表性载荷通过或一次公共 Issue 读取各自只覆盖一段。
4. **Windows。** 使用自有测试数据执行保存、重开、返回、快捷键与失权操作。官网说法与当前版本出现歧义时再查参考窗口。每条实测记录应保留版本、条件、实际操作和观察结果；向公开仓库提交的图只能来自自有程序的测试空间。
5. **核对发现。** 帮助描述了分拣台关闭时的导航行为，实机也能从模块菜单打开“未启用”画面。官网页脚仍有文档库预告，帮助与本机则有文档库入口。供应商营销介绍的双向能力与具体产品帮助中的读写方向也会不同。按实际状态、入口和专用帮助逐条保留这种差异。[分拣][triage] · [文档库][docs-help] · [供应商概念][mapping]

## 覆盖到的资料入口

官网除首页及四个功能页面外，还从站点地图核对了八类方案。以下页面说明适用场景。[Agent][p-agents] · [任务][p-tasks] · [项目管理][p-planning] · [自动化][p-auto] · [敏捷][s-agile] · [自动化测试][s-test] · [群聊到代码][s-chat] · [私有化][s-private] · [产品团队][s-product] · [产品研发流程][s-flow] · [软件研发][s-dev] · [支持团队][s-support]

官方帮助导航中的九篇最佳实践作为跨模块场景纳入。[飞书群转任务][bp-feishu] · [文档自动化][bp-doc] · [全流程自动化测试][bp-full] · [全流程接口测试][bp-api] · [接口测试覆盖][bp-api-cover] · [自动修复][bp-bug] · [测试 Agent 指令][bp-instruction] · [产品研发流程][bp-flow] · [机器与权限][bp-security]

用 `python scripts/check-parity-inventory.py` 检查主键、证据标签、引用和资料覆盖；加 `--online` 重新比较官方帮助导航与站点地图。运行时不会打开应用，也不会证明某项行为已端到端实现。[校验脚本][checker]

[home]: https://hifox.com/
[sitemap]: https://hifox.com/sitemap.xml
[p-agents]: https://hifox.com/features/agents/
[p-auto]: https://hifox.com/features/automations/
[p-planning]: https://hifox.com/features/project-management/
[p-tasks]: https://hifox.com/features/tasks/
[s-agile]: https://hifox.com/solutions/agile/
[s-test]: https://hifox.com/solutions/automated-testing/
[s-chat]: https://hifox.com/solutions/group-chat-to-code/
[s-private]: https://hifox.com/solutions/private-deployment/
[s-product]: https://hifox.com/solutions/product/
[s-flow]: https://hifox.com/solutions/product-development-workflow/
[s-dev]: https://hifox.com/solutions/software-development/
[s-support]: https://hifox.com/solutions/support/
[changes]: https://help.hifox.com/changelog/
[nov13]: https://help.hifox.com/changelog/2026-11-13
[practice]: https://help.hifox.com/practice/
[welcome]: https://help.hifox.com/docs/welcome
[quick]: https://help.hifox.com/docs/quickstart
[agents]: https://help.hifox.com/docs/agent-and-computer/agents
[variants]: https://help.hifox.com/docs/agent-and-computer/agent-variants
[run-help]: https://help.hifox.com/docs/agent-and-computer/agent-task-execution
[directory]: https://help.hifox.com/docs/agent-and-computer/working-directory
[hours]: https://help.hifox.com/docs/agent-and-computer/working-hours
[repos]: https://help.hifox.com/docs/agent-and-computer/repositories
[comments]: https://help.hifox.com/docs/agent-and-computer/comments-and-replies
[inbox]: https://help.hifox.com/docs/agent-and-computer/inbox
[chat]: https://help.hifox.com/docs/agent-and-computer/chat-with-agent
[computers]: https://help.hifox.com/docs/agent-and-computer/computers
[skills]: https://help.hifox.com/docs/agent-and-computer/skills
[crews]: https://help.hifox.com/docs/agent-and-computer/crews
[spaces]: https://help.hifox.com/docs/space/spaces
[tasks]: https://help.hifox.com/docs/space/tasks
[members]: https://help.hifox.com/docs/space/members
[projects]: https://help.hifox.com/docs/space/projects
[hviews]: https://help.hifox.com/docs/space/views
[iterations]: https://help.hifox.com/docs/space/iterations
[triage]: https://help.hifox.com/docs/space/triage
[automations]: https://help.hifox.com/docs/space/automations
[docs-help]: https://help.hifox.com/docs/space/docs
[types]: https://help.hifox.com/docs/task/task-types
[follow]: https://help.hifox.com/docs/task/follow-tasks
[labels]: https://help.hifox.com/docs/task/task-labels
[statuses]: https://help.hifox.com/docs/task/task-statuses
[status-team]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-status-workflows.py
[status-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/status_workflows.rs
[status-window]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/status_ui_test.rs
[relations]: https://help.hifox.com/docs/task/task-relations
[mapping]: https://help.hifox.com/docs/integration/concept-mapping
[h-cli]: https://help.hifox.com/docs/cli/cli
[pat]: https://help.hifox.com/docs/cli/access-tokens
[account]: https://help.hifox.com/docs/other/account-management
[account-tests]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-accounts.py
[verified-identity]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-identity.py
[identity-guide]: account-lifecycle.md
[live-skills]: skills-market.md
[i-jira]: https://help.hifox.com/docs/integration/jira
[i-jira-api]: https://help.hifox.com/docs/integration/jira-api
[jira-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-jira.py
[jira-doc]: jira-api.md
[i-zentao]: https://help.hifox.com/docs/integration/zentao
[i-tapd]: https://help.hifox.com/docs/integration/tapd
[i-bugly]: https://help.hifox.com/docs/integration/bugly
[i-pingcode]: https://help.hifox.com/docs/integration/pingcode
[i-teambition]: https://help.hifox.com/docs/integration/teambition
[i-zoho]: https://help.hifox.com/docs/integration/zoho
[i-asana]: https://help.hifox.com/docs/integration/asana
[i-linear]: https://help.hifox.com/docs/integration/linear
[i-clickup]: https://help.hifox.com/docs/integration/clickup
[i-ones]: https://help.hifox.com/docs/integration/ones
[i-github]: https://help.hifox.com/docs/integration/github
[i-cnb]: https://help.hifox.com/docs/integration/cnb
[i-gitlab]: https://help.hifox.com/docs/integration/gitlab
[i-feishu]: https://help.hifox.com/docs/integration/feishu
[i-wecom]: https://help.hifox.com/docs/integration/wecom
[i-dingtalk]: https://help.hifox.com/docs/integration/dingtalk
[i-wechat]: https://help.hifox.com/docs/integration/wechat
[i-qq]: https://help.hifox.com/docs/integration/qq
[i-slack]: https://help.hifox.com/docs/integration/slack
[i-discord]: https://help.hifox.com/docs/integration/discord
[i-telegram]: https://help.hifox.com/docs/integration/telegram
[i-teams]: https://help.hifox.com/docs/integration/teams
[bp-feishu]: https://help.hifox.com/docs/best-practices/feishu-group-to-task
[bp-doc]: https://help.hifox.com/docs/best-practices/documentation-automation-pipeline
[bp-full]: https://help.hifox.com/docs/best-practices/full-flow-automated-testing
[bp-api]: https://help.hifox.com/docs/best-practices/full-flow-api-testing
[bp-api-cover]: https://help.hifox.com/docs/best-practices/api-testing-full-coverage
[bp-bug]: https://help.hifox.com/docs/best-practices/agent-auto-bugfix
[bp-instruction]: https://help.hifox.com/docs/best-practices/testing-agent-instructions
[bp-flow]: https://help.hifox.com/docs/best-practices/product-development-workflow
[bp-security]: https://help.hifox.com/docs/best-practices/agent-computer-security
[rea]: client-analysis.md
[ui]: interfaces.md
[native]: design-qa.md
[inbox-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-inbox.py
[inbox-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/inbox.rs
[inbox-window]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/inbox_ui_test.rs
[inbox-manual]: inbox.md
[chat-flow-ui]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/chat_ui_test.rs
[chat-flow-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/chat_workflows.rs
[chat-flow-worker]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-chat-workflows.py
[space-window]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/space_ui_test.rs
[space-lifecycle-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/space_lifecycle.rs
[cli]: cli.md
[windows]: windows.md
[coverage]: windows-coverage.md
[features]: features.md
[client]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-client/src/lib.rs
[device-guide]: device-network.md
[device-tests]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-device-network.py
[shell]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/shell.rs
[app]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/app.rs
[pages]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/catalog.rs
[views]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/views.rs
[forms]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/forms.rs
[main]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/main.rs
[team]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-team.py
[personal-token-tests]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-personal-tokens.py
[installer]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-installer.py
[server]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-server/src/main.rs
[auth]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-server/src/auth.rs
[native-worker]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/machine.rs
[machine-management]: machine-management.md
[storage-models]: storage-and-models.md
[storage-model-test]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/tests/storage_and_launch.rs
[machine-life-test]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/tests/machine_management.rs
[worker-manual]: ../crates/shipforge-worker/README.md
[worker-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/tests/native_runtime.rs
[runtime-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/runtime_policy.rs
[execution]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/execution.rs
[directory-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/agent_directory.rs
[directory-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-agent-directory.py
[collab]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/collaboration.rs
[metadata]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/metadata.rs
[workspace-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/workspace.rs
[access]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/access.rs
[domain]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/domain.rs
[protocol]: protocol.schema.json
[query]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/query.rs
[planning]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/planning.rs
[market-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-skill-market.py
[market-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/skill_market.rs
[skill-files]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-client/src/skill_files.rs
[worker-core]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/worker.rs
[providers]: ../crates/shipforge-core/PROVIDERS.md
[enterprise-guide]: teambition-cnb.md
[asana-guide]: asana-projects.md
[asana-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-asana-projects.py
[enterprise-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-teambition-cnb.py
[enterprise-native]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/enterprise_integration_ui_test.rs
[vendor-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-vendor-imports.py
[tapd-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-tapd.py
[vendor-imports]: vendor-imports.md
[tracker-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-trackers.py
[tracker-guide]: tracker-integrations.md
[cloud-tracker-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-cloud-trackers.py
[cloud-tracker-guide]: cloud-trackers.md
[provider-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/providers.rs
[schedule]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/automation/schedule.rs
[automation-code]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/automation.rs
[checker]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/check-parity-inventory.py

[weekly]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/worker/working_hours.rs

[guide-ui]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/onboarding_ui.rs
[guide-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/onboarding.rs
[invites-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/invitations.rs
[invites-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-invitations.py
[mail-tests]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-mail-delivery.py
[chat-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/chat.rs
[assistant-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/personal_assistant.rs
[assistant-native]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/assistant_ui_test.rs
[assistant-team]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-assistant-groups.py
[chat-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-chat.py
[conversation-help]: conversations.md
[navigation-flow]: navigation.md
[nav-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/navigation.rs
[live-windows]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/live_navigation_test.rs
[org-default-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/organization_settings.rs

[guide-manual]: onboarding.md
[personal-ui]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/personal.rs
[personal-doc]: https://github.com/Hubujiu/ShipForge/blob/main/docs/personal-settings.md
[personal-regression]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/personal.rs
[vcs-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-vcs.py
[vcs-regression]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/vcs_links.rs
[slack-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-slack.py
[im-regression]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/im_threads.rs
[assign-ui]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/assignees_ui.rs
[assign-core]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/assignees.rs
[iteration-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/iterations/tests.rs
[iteration-service]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-iterations.py
[iteration-manual]: iterations.md
[work-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/work_collaboration.rs
[work-window]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/workspace_ui_test.rs
[work-service]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-work-collaboration.py
[tag-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/tags.rs
[tag-ui]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/tag_ui.rs
[planning-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/review_planning.rs
[planning-native]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/review_ui_test.rs
[planning-flow]: triage-planning.md
[commands-native]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/conversation_ui_test.rs
[commands-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/conversation_tools.rs
[commands-team]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-conversation-tools.py

[personal-bots]: personal-bots.md
[lark-guide]: lark-agent.md
[lark-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-lark-bots.py
[lark-native]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/lark_ui_test.rs
[bot-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-personal-bots.py
[bot-native]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/personal_bots_ui_test.rs

[forge-guide]: repository-integrations.md
[forge-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-forge.py
[forge-native]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/repository_ui_test.rs
