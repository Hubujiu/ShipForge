# Windows 产品入口与行为盘点

参考样本为 2026-11-18 核对的 HiFox Windows 0.4.538。Windows 对照以 ShipForge 0.6.0 的验收记录为起点。本表的单位是可单独安排验证的用户操作或条件分支。一个页面可产生多行，一条跨端行为可涉及多个页面。表的行数、帮助篇数和截图张数都不能作为产品的页面总数或完成率。

## 取证范围

各模块的验收目的见[功能基线][features]，通用记录的创建、更新和校验入口见[业务核心][domain]。

| 来源 | 本次实际核对的范围 | 能支持的结论 |
| --- | --- | --- |
| 官网 | 从 robots 指向的 sitemap 取得 26 个 URL，中英文各有 13 个对应页面。主页、4 个功能页和 8 个方案页均计入来源清单。 | 产品承诺、平台清单、预告和应用场景。解决方案中的工作流程不能计作额外客户端入口。[官网][home] · [站点地图][sitemap] |
| 官方帮助 | 欢迎页的导航包含 62 篇不同的 `/docs/` 页面。帮助子域的 `/sitemap.xml` 当日返回 404，采用实际 HTML 导航。九篇最佳实践列在文末。 | 操作前提、平台方向、角色和限制。更新日志的目录有 79 个日期，重点复核影响本表的 11 月更新。文章的日期与本机安装版本独立。[帮助导航][welcome] · [更新日志][changes] |
| 安装与 REA | 在安装目录重新核对两份 ASAR 的 SHA-256，与既有分析一致。对派生渲染模块的可见路由常量及桌面桥接能力作只读复核。 | 壳处理登录、注册、登出、组织、认证、邀请、引导、配对等路径前缀。静态线索无法证明该路径可达、业务授权或服务端行为。[安装样本与边界][rea] |
| 参考窗口记录 | 首批 UI01–UI36、补01–补07 的图像文件逐个匹配其本地清单的 SHA-256。另核对新手指引专门批次的侧栏、全页及展开项记录。 | “图”表示既有实机画面里观察到的状态。“菜单”仅指另一个已打开页面中出现了该名称。单个空态、已保存的截图和成功点击后的结果分别记载。[首批索引][ui] |
| ShipForge Windows | 既有公开验收记录分列看板、任务详情、附件、文档正文和自动化表单。核心测试、客户端协议、机器子进程和隔离 HTTP 服务分别给出定位。 | 只有具名实机操作记“实机”。“测试”指列出的特定协议或进程场景，“实现”仅表示能定位源码，“未证”表示该项还没有正向证据。三种标记均不能推断跨供应商端到端一致。[Windows 操作记录][native] · [协议][cli] |

### 怎样读表

“有对应”表示该行所述的具体操作已经有相应实现。“部分”说明实现覆盖了其中一段。“缺口”表示在本次核查范围内缺少该专用流程。“待核”用于前提、行为细节或版本对应仍待确认的项目。“预告”和“跨端”分别记录官方未来方向和 Windows 无法独立核证的场景。各行“验收维度”标出可观察行为、数据或权限。末栏写出能重复检验的输入、角色、触发及结果。功能一致性可以通过官方帮助的行为契约与自有实现的可重复验证建立；截图用于厘清含糊的入口或状态。当前 HiFox 组织只读，多成员写入、向外发信、绑定真实账号或执行真实任务均标为待联调。

“图 UIxx”沿用原图库编号。“图 补01–补07”依次对应组织设置、任务标签、Git 设置、偏好、键盘、通知渠道和桌面通知事件。新手指引另用“图 指引”。文中的本机标记均指参考方，ShipForge 的实操证据单列在 Windows 一栏。公开表仅记用户能观察的行为和证据定位。

## 工作台、身份与新手状态

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| A01 | 启动、登录、注册、重新认证、登出 | 壳 · [账号][account] | 部分 | 实现 · [客户端][client] | 行为、数据、权限、待联调 | 本地工作区和团队令牌可用。要验证浏览器登录返回桌面、账号过期重登和登出后的落地页。 |
| A02 | 新账号首次进入组织、选择或创建组织 | 壳 · [快速上手][quick] | 部分 | 测试 · [全新 Windows 工作区][guide-ui] · [升级与重复初始化][guide-tests] | 行为、数据、权限、待联调 | 新工作区可从组织名与地址进入默认空间，再按工作类型保存任务类型和 Agent 默认值。参考的账号注册、微信和邮箱验证码待联调。 |
| A03 | 多账号与多个组织间切换，重启恢复最后位置 | [账号][account] | 部分 | 实现 · [客户端][client] | 行为、数据、权限、待联调 | 可连接/断开团队服务。多账号列表、分账号路由和失效账号回退需单独验证。 |
| A04 | 邀请链接、授权返回与机器配对入口 | 壳 · [成员][members] · [机器][computers] | 部分 | 测试 · [邀请协议][invites-tests] · [机器配对][execution] | 行为、数据、权限、待联调 | 邮箱和通用邀请覆盖身份、空间范围、过期与撤销，安装版通过系统链接打开接受窗口。真实认证回跳、邮件投递和设备配对另行核对。 |
| A05 | 永久注销账号并退出各组织 | [账号][account] | 缺口 | 未证 · [服务][server] | 行为、数据、权限、待联调 | 需要可回收的测试身份、二次核验以及注销后跨设备失效记录。 |
| A06 | 标题栏后退、前进、刷新，恢复空间和对象 | 图 UI02、UI03、UI25 | 有对应 | 实现 · [导航][shell] · [桌面状态][app] | 行为 | 分别从抽屉、搜索结果和跨空间位置返回，检查筛选、焦点及滚动位置。 |
| A07 | 全局搜索及命令菜单，按对象、操作、权限定位 | 图 UI25 · [空间][spaces] | 部分 | 测试 · [工作区][workspace-tests] | 行为、权限 | 有对象搜索及权限检查。核验命令分类、快捷跳转、撤权后结果和无结果提示。 |
| A08 | 首页的快捷新建、双层导航、自定义导航、折叠更多模块 | 图 UI01、UI21 · 菜单 UI01 | 部分 | 实现 · [导航][shell] | 行为 | 核验新建任务/多种对话的逐级选择，以及自定义导航保存后重启。 |
| A09 | 侧栏新手清单，展开四个快捷行动、更多、隐藏 | 图 指引 · 图 UI01 | 有对应 | 测试 · [原生侧栏操作][guide-ui] · [成员设置持久化][guide-tests] | 行为、数据 | 自建 Windows 隔离库实测四项行动、展开、隐藏、恢复和重开。 |
| A10 | 新手全页，六组 36 项、进度、积分和名次 | 图 指引 · [快速上手][quick] | 有对应 | 测试 · [原生完整向导][guide-ui] · [36 项计分][guide-tests] · [用户手册][guide-manual] | 行为、数据 | Windows 原生窗口展开六组。公开业务操作可完成 36 项并达到 560 分。旧档恢复、同分名次及成员隔离分别核对。 |
| A11 | 从新手行动进入工作流并返回，跨设备完成相同步骤 | 图 指引 · [快速上手][quick] | 部分 | 测试 · [真实业务计分][guide-tests] · [Windows 工作流][guide-ui] | 行为、数据、权限、待联调 | ShipForge 各步骤可通过实际业务操作完成。原版多人、第三方事件的积分归属和异地登录一致性待独立组织联调。 |
| A12 | 本地机器入口、帮助与交流群、更新/关于 | 菜单 UI01、UI33 · 壳 · [CLI][h-cli] | 部分 | 实现 · [执行端控制][native-worker] · [桌面入口][main] | 行为 | 可启动本机 Worker。帮助、更新可用性、离线与当前版本信息分别核对。 |
| A13 | 在浏览器或手机查看、回复阻塞、派发到桌面机器 | [任务产品页][p-tasks] · [官网][home] | 跨端 | 测试 · [团队 API][team] | 行为、数据、权限、待联调 | 两客户端 HTTP 验证限于服务。移动推送、手机应用内打开与跨端接续需真实设备。 |
| A14 | 专用网络环境、代理、登录失败和重新进入工作区 | 菜单 UI33 · 壳 · [私有化方案][s-private] | 待核 | 实现 · [客户端][client] | 行为、数据、权限、待联调 | 核验代理鉴权、证书错误、不可达时旧视图与出错重试。菜单文字未证明代理配置页已打开。 |

## 设置、组织与成员

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| S01 | 组织设置中的默认语言、任务优先级档数与危险操作区 | 图 补01 · [成员][members] | 部分 | 实现 · [通用表单][forms] | 行为、数据、权限 | 已有组织对象。分别验证优先级档数对已有任务/创建表单的影响、语言级联及危险操作可见角色。 |
| S02 | 组织空间目录、公开加入和退出、可选模块、个人固定、停用与恢复 | 菜单 UI33 · 图 UI21 · [空间][spaces] | 有对应 | 实机 · [Windows 空间验收][native] · [原生窗口用例][space-window] · 测试 · [成员与状态][space-lifecycle-tests] | 行为、数据、权限 | Windows 实测临时入口、固定、启用客户请求、预览未完成任务、停用后历史只读与恢复。隔离数据库覆盖公开加入、私有权限、迁移回滚、运行租约失效及个人固定跨重启。 |
| S03 | 组织成员清单、角色、邀请、邀请链接及外部账号 | 图 UI33 · [成员][members] | 部分 | 测试 · [邀请与成员权限][invites-tests] · [HTTP 真实身份][invites-http] | 行为、数据、权限、待联调 | 管理员可按邮箱或共享链接邀人并同时选择空间。停用立即生效，重新启用后旧链接失效；地址错误或只读令牌均不能接受。真实邮件投递、外部账号合并待联调。 |
| S04 | 空间成员与组织角色并存，访客仅能访问获邀空间 | [成员][members] · [分拣][triage] | 有对应 | 测试 · [成员与空间][workspace-tests] · [权限实现][access] | 行为、数据、权限、待联调 | 合成 Guest 获取显式成员资格前后、撤销后分别验证列表、搜索和直接读取。未加入的组织管理员可管理私有空间元数据，成员及任务仍需空间权限。 |
| S05 | 成员 rail 区分 Agent、小队、真人、机器，检索并打开成员抽屉 | 图 UI17 · [成员改版][nov13] | 部分 | 实现 · [导航][shell] · [对象页][views] | 行为、数据、权限、待联调 | 成员与机器等页面存在。分类树、跨空间同名项、快捷新建、真人详情直达对话仍需点击。 |
| S06 | 管理范围的 Agent、小队、机器与空间内的同名入口 | 图 UI06、UI11、UI13 · 菜单 UI33 | 部分 | 实现 · [目录][pages] | 行为、数据、权限 | 检查共享/私有及本人/空间过滤，管理动作对不同身份可见范围。 |
| S07 | 全局与空间任务标签、名称筛选、分组、互斥、合并及归档 | 图 补02 · [标签][labels] | 有对应 | 测试 · [标签范围与批量事务][tag-tests] · [Windows 标签表单][tag-ui] | 行为、数据、权限 | 组织与空间标签可并用，同组后选替换先选。回归覆盖归档保留历史、合并更新任务和视图、多标签交集筛选、跨空间保留组织标签及批量失败回滚。 |
| S08 | 组织状态库的类别、用量、顺序、默认值及多语言名 | 图 UI14 · [状态][statuses] · [11 月更新][changes] | 部分 | 测试 · [元数据][metadata] | 行为、数据、权限 | 修改名称有用例。需检查多语言回退、组织默认值与已有类型的特殊工作流。 |
| S09 | 组织默认的字段、关联类型及空间任务类型管理 | 图 UI34、UI35 · [类型][types] · [关联][relations] | 有对应 | 测试 · [工作区][workspace-tests] | 行为、数据、权限 | 实际看过列表/空态。用两种任务类型验证自定义必填字段、双向关系名与停用回退。 |
| S10 | 组织、空间、Agent 三级代码库与提交作者设置 | 图 补03 · 菜单 UI29 · [代码库][repos] | 部分 | 测试 · [工作树][worker-tests] | 行为、数据、权限 | 本地已批准仓库能执行。核验空间继承、Agent 覆盖、作者邮箱、共同署名和实际 Git 提交作者。 |
| S11 | 个人资料、第三方账号绑定、重新绑定与外部账号归属 | 菜单 UI33 · [账号][account] · [GitHub][i-github] | 部分 | 实现 · [身份][auth] | 行为、数据、权限、待联调 | 服务令牌不证明个人头像、账号关联或外部评论归属。验证换绑冲突和撤销旧身份。 |
| S12 | 偏好设置含五种主题、界面语言、任务会话自动打开开关 | 图 补04 · [更新日志][changes] | 有对应 | 测试 · [原生偏好和输入][personal-ui] · [个人设置手册][personal-doc] | 行为、数据 | 已在 Windows 原生窗口切换全部主题与 English、验证两种发送设置。设置按成员持久化。操作系统主题切换仍受 Windows 配色控制。 |
| S13 | 快捷键，发送消息组合、录制、冲突与恢复默认 | 图 补05 | 有对应 | 测试 · [原生键盘录制与输入法][personal-ui] · [设置持久化][personal-regression] | 行为 | 已验证组合冲突、恢复、重开及实际消息文本框的 Enter/Shift+Enter 输入与提交。 |
| S14 | 通知设置，桌面、移动、浏览器、邮件、收件箱与四个 IM 渠道 | 图 补06 · [收件箱][inbox] | 部分 | 测试 · [核心收件箱][workspace-tests] | 行为、数据、权限、待联调 | 参考页列九个渠道。列名与配置值仅证明入口；设备/租户实际送达分别计证。 |
| S15 | 桌面通知详情总开关、事件组、恢复默认 | 图 补07 · 壳 · [收件箱][inbox] | 部分 | 实现 · [桌面][app] | 行为、数据、权限、待联调 | 参考图展开分派 5 类、状态变更 10 类。须用事件源与真机通知检验总开关、还原、点击定位、按人隔离。 |
| S16 | 个人访问令牌，创建、到期、仅一次展示明文、吊销 | 菜单 UI33 · [个人令牌][pat] | 部分 | 测试 · [服务令牌][team] | 行为、数据、权限、待联调 | ShipForge 支持管理员发放有范围的令牌；核验成员自助、过期选择、关联机器、吊销后的重连。 |
| S17 | 设置中的开始使用、网络代理、关于、地区/账号条件 | 菜单 UI33 · [账号][account] | 待核 | 未证 · [目录][pages] | 行为、权限 | 个人设置中看见菜单，逐页记录表单默认值、按钮、错误、保存及重启；代理属于设备设置。 |
| S18 | 组织全局默认与空间、用户、设备的配置归属 | 图 补01、补03、补04、补07 · [代码库][repos] | 部分 | 实现 · [客户端][client] · [权限][access] | 行为、数据、权限、待联调 | 用两个成员两台机器测跨空间继承、按账号同步与设备独享。偏好持久化本身无法证明其生效。 |

## 任务、迭代、专项与分拣

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| T01 | 空间内新建任务，填写标题、描述、属性、处理人及负责人 | 图 UI02、UI03、UI24 · [任务][tasks] | 有对应 | 实机 · [建单及重开][native] · [多成员指派][assign-ui] | 行为、数据、权限 | 已在原生空间设置开启多人，从表单同时选中成员与独立执行者并验证保存。两客户端跨角色创建、同一编号重开仍须按配对数据比对。 |
| T02 | 空间看板、类型切换、折叠空列、列表、表格和分组 | 图 UI02 · [视图][hviews] | 有对应 | 实机 · [看板][native] · [多人分组][assign-ui] · 实现 · [列表][views] | 行为 | 已验同一任务显示在两个人员列，页面总数仍是一项。校验拖放、键盘操作、多列滚动、两种布局下相同筛选结果。 |
| T03 | 我的任务中指派给我/我创建的，查找与仪表盘 | 图 UI18、UI19 · [视图][hviews] | 有对应 | 实现 · [个人页与统计][views] · [查询][query] · [按多人集合查询][assign-core] | 行为、权限 | 个人归属、逾期边界、近期到期及按状态/优先级统计需用多成员数据比对。 |
| T04 | 任务详情、编辑、关注切换、上传下载附件 | 图 UI03 · [关注][follow] · [评论][comments] | 有对应 | 实机 · [关注及附件往返][native] | 行为、数据、权限 | 已实测单份附件字节与重开。仍需比对大文件、长评论、富文本、图片粘贴及多附件错误反馈。 |
| T05 | 任务类型绑定字段和状态，切换类型时处理值 | 图 UI14、UI34 · [类型][types] | 部分 | 测试 · [字段与状态][workspace-tests] | 行为、数据 | 已覆盖必填、约束与类型状态。还需有值的任务切换类型时对丢失信息的提示与确认。 |
| T06 | 子任务、阻塞、相关、重复与自定义方向，从两端追踪 | 图 UI35 · [任务关联][relations] | 有对应 | 测试 · [关系约束][workspace-tests] | 行为、数据、权限 | 以跨空间可见/不可见的另一端验证关系、删除、去重和重新打开。 |
| T07 | 保存个人或空间视图，新建中实时预览、复制、个人置顶和排序 | 图 UI05、UI28 · [视图][hviews] | 部分 | 测试 · [保存与实时查询][workspace-tests] | 行为、数据、权限 | 筛选与预览有实现。核对个人排序、可分享链接，临时筛选和更新已保存视图的分界。 |
| T08 | 迭代时间轴、待规划、日期、容量、完成数、当前和未来 | 图 UI04 · [迭代][iterations] | 有对应 | 测试 · [时间轴][collab] · 实现 · [原生迭代][planning] | 行为、数据、权限 | 已核验日期和任务计数。比对有数据/无日期、关闭、未启动和不同角色的实际窗口。 |
| T09 | 手动结项前预览，将未完成项移入下期或待规划 | [迭代][iterations] | 有对应 | 测试 · [原子转移][workspace-tests] | 行为、数据 | 跨端并发结项、存在取消状态与目标关闭时核对失败提示、剩余任务去向和编号。 |
| T10 | 启用自动迭代、按周期建未来迭代、自动加入和顺延 | [迭代][iterations] | 有对应 | 测试 · [固定时钟与成员隔离][iteration-tests] · [Windows 服务补跑][iteration-service] · [使用手册][iteration-manual] | 行为、数据、权限 | 固定时钟跨过夏令时和当地午夜，验证冷却、仅顺延未完成任务、只回填窗口内的匹配变化。临时服务离线一周后在首次客户端请求前完成结项。停用后检查历史及未来任务。 |
| T11 | 专项空态、新建、多空间范围、负责人及汇总进度 | 图 UI22 · [专项][projects] | 有对应 | 测试 · [跨空间进度][collab] | 行为、数据、权限 | 参考图为空态。使用多空间成员与管理员打开有数据的专项，核对分母、失权和隐藏任务。 |
| T12 | 专项时间线，按任务开始、结束或计划区间查看排期 | [专项][projects] · [项目产品页][p-planning] | 部分 | 实现 · [专项查询][query] | 行为、数据、权限 | 已有专项对象及进度。需核对图形排期、重叠日期、无日期任务与可见范围。 |
| T13 | 分拣台未启用提示、进入设置、启用后的来源与类型 | 图 UI23 · [分拣][triage] | 部分 | 测试 · [来源筛选][collab] | 行为、数据、权限 | 截图仅有未启用态。需检验开关、私有空间访客可投递、关闭后再投递与旧队列的处置。 |
| T14 | 待分拣任务接受、搁置、拒绝、并为重复，设置责任与规则 | [分拣][triage] | 部分 | 测试 · [分拣操作][workspace-tests] | 行为、数据、权限 | 可处置基础队列。轮值责任、多个来源规则、批处理与权限可见前后变化需验证。 |
| T15 | 任务编号、链接分享、外部 Issue 编号及历史来源 | [任务][tasks] · [Jira][i-jira] | 部分 | 测试 · [版本与幂等][workspace-tests] · [导入][metadata] | 行为、数据、权限、待联调 | 单机编号与外部来源可持久。测跨组织链接、强制匹配远端编号和深链接在另一设备打开。 |
| T16 | 任务活动、讨论线程、@、回复、快捷审阅与解决/重开 | 图 UI03 · [评论][comments] | 部分 | 测试 · [通知与关注][workspace-tests] | 行为、数据、权限、待联调 | 评论回复在核心存在。需验证定位某条评论、解决/重开线程、审阅按钮与已撤权人的历史可见性。 |
| T17 | 关注者、自动关注与只通知相关人 | [关注][follow] · [收件箱][inbox] | 部分 | 实机 · [本人关注][native] · 测试 · [按身份关注][collab] | 行为、数据、权限、待联调 | 分别试参与讨论、取消关注后被 @、更换处理人和另一成员的收件箱。 |
| T18 | 任务指派 Agent/小队，待规划后进入可执行状态、回收结果 | 图 UI03 · [任务执行][run-help] | 有对应 | 测试 · [工作流与租约][execution] · [真实进程][worker-tests] | 行为、数据、待联调 | 用已授权的真实外部 Runtime 验证请求人工判断与验收状态；示例程序不能代表所有模型。 |
| T19 | 收件箱重要/普通、未读、稍后唤醒、清理、空间搜索 | 图 UI09 · [收件箱][inbox] | 部分 | 测试 · [通知动作][workspace-tests] | 行为、数据、权限、待联调 | 已测聚合和本人操作。待比对重要分类、定时重现、操作打开任务后返回列表原位置。 |
| T20 | 任务与对话用量、运行中/阻塞状态与侧栏增量更新 | 图 UI36 · [任务执行][run-help] · [11 月日志][changes] | 部分 | 测试 · [进度/取消][worker-tests] | 行为、数据、待联调 | 参考 Agent 详情仅见零条历史。“运行中”持续时间、其他成员收到更新和空态退场需实际运行。 |

## 对话、文档、市场与 Agent

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| C01 | 独立对话、个人小助手、任务内对话与真人私聊 | 图 UI16、UI32 · [对话][chat] | 部分 | 实现 · [对话表单][forms] · 测试 · [私有运行][execution] | 行为、数据、权限、待联调 | 空会话和禁用发送可见。核验真人私聊、Agent 运行、任务上下文、跨身份检索和个人小助手默认配置。 |
| C02 | 新建 Agent/真人对话、群聊和新任务的选择器 | 菜单 UI01 · [成员与群聊][nov13] | 部分 | 实现 · [导航][shell] | 行为、数据、权限、待联调 | 已提供任务/对话对象。逐个打开对象选择、筛选同名成员，核对默认空间与群聊首次发送。 |
| C03 | 群聊添加/移除人或 Agent、头像名称描述、退出与群主解散 | [群聊管理][nov13] | 部分 | 测试 · [群成员与所有者][chat-tests] · 实现 · [Windows 入口][conversation-help] | 行为、数据、权限、待联调 | 合成成员验证添加、移出、私有 Agent 可见性、群主转让、退出、解散和解散后权限。自定义群头像及真实组织的表现待核。 |
| C04 | 群聊免打扰、@所有人、@我穿透和仅清自己历史 | [群聊偏好][nov13] | 有对应 | 测试 · [三人提醒与历史][chat-tests] | 行为、数据、权限、待联调 | A 发普通消息、群体提及、单独提及 B。B 开免打扰且忽略群体提及，仅收到单独提及。B 清空后 A 和 C 历史仍可读。真实客户端系统推送另行联调。 |
| C05 | 私有会话授权管理、邀请/撤回、任务内私有会话 | [会话授权][nov13] · [对话][chat] | 有对应 | 测试 · [两个独立登录的本地团队客户端][chat-http] · [任务权限][chat-tests] · [隐私继承][execution] | 行为、数据、权限、待联调 | 接受前、撤回后，直接读取、附件下载、搜索、事件和产物各按当前身份验证。任务可见性收紧也收回会话访问。离线不能重开此前取过的消息，参考生产租户的跨账号步骤待联调。 |
| C06 | 对话列表单人置顶、不显示、撤销、新消息恢复 | [侧栏更新][changes] · [对话][chat] | 缺口 | 未证 · [对话页][views] | 行为、数据、权限、待联调 | 两用户应能有不同顺序和隐藏状态；发来新消息时只恢复符合规则的会话。 |
| C07 | 从指定消息 Fork、分享已完成片段、转任务、Slash 与快捷指令 | [对话][chat] | 部分 | 测试 · [冻结分享与撤销][chat-http] · [选区与编辑后内容][chat-tests] | 行为、数据、权限、待联调 | 指定消息后的链接固定正文与附件名称，组织成员和持链者的权限、同选区链接共同撤销均已验证。图片分享、分叉上下文、Slash 和对话转任务回链需独立验收。 |
| C08 | 对话工具调用、流式进度、人类回复、同目录继续 | [对话][chat] · [任务执行][run-help] | 部分 | 测试 · [MCP 子进程与进度][worker-tests] | 行为、数据、待联调 | 工具回报与续跑有测试。实际长对话工具卡、流式输出和中途停单在双端的展示待验证。 |
| C09 | 文档库个人/团队区、新建、检索、分享链接与实时权限 | 图 UI08 · [文档][docs-help] | 部分 | 测试 · [正文搜索及历史可见性][workspace-tests] | 行为、数据、权限、待联调 | 可管理文档与授权。检验嵌入预览、打开链接、移出空间后链接与历史附件失效。 |
| C10 | 文档历史、恢复、对话产物存入库与附件下载 | 图 UI08 · [文档][docs-help] | 有对应 | 实机 · [文档版本][native] · 测试 · [文件及产物][collab] | 行为、数据、权限 | 实机编辑形成第二版。历史恢复、保存产物后删除源、另一成员下载是独立的测试场景。 |
| C11 | 市场的技能/Agent 模板两个入口、分类、搜索、详情和安装 | 图 UI07 · [技能][skills] · [官网][home] | 部分 | 测试 · [内置安装][execution] · 实现 · [市场页][views] | 行为、数据、权限 | 内置目录能安装模板和技能。分类/关键词、远端条目、版本与多人共享安装状态需分开验。 |
| C12 | 技能包合集、团队发布、更新/卸载、从模板创建独立 Agent | 图 UI07 · [市场][home] · [技能][skills] | 部分 | 测试 · [内置市场][execution] | 行为、数据、权限、待联调 | 需真实多成员及可更新包验证发布、版本切换、已绑定 Agent 及历史运行快照。 |
| C13 | 空间技能与连接器切换，URL、文件/文件夹或机器技能导入 | 图 UI20 · [技能][skills] · [项目技能目录][nov13] | 部分 | 实现 · [表单][forms] · [执行配置][worker-core] | 行为、数据、权限、待联调 | 已有技能对象。需在离线机器、项目级目录、ZIP/文件夹和 URL 上检验排队、导入副本及失败回滚。 |
| C14 | 空间 Agent、全部/我的/共享/私有、设置与运行历史 | 图 UI06、UI29、UI36 · [Agent][agents] | 部分 | 实现 · [原生目录][pages] · [解析][worker-core] | 行为、数据、权限 | 身份与配置已存在。逐页实测同名跨空间 Agent、过滤、运行账单/用量及非空历史。 |
| C15 | Agent 指令、归档与恢复，归档后停止新指派/提及 | 图 UI29 · [Agent][agents] | 部分 | 实现 · [Agent 模型][worker-core] | 行为、数据、权限 | 参考记录显示归档动作。验证保留旧历史、阻止新增指派/提及及恢复后重新可用。 |
| C16 | 分身跨空间复用、逐字段覆盖、恢复继承及父配置更新 | [分身][variants] · [Agent 产品页][p-agents] | 部分 | 测试 · [执行配置][execution] | 行为、数据、权限、待联调 | 需跨空间在两个真实账号依次改父/子字段，验证新旧运行快照及可见范围。 |
| C17 | 小队分组、队长分派、成员结果汇总、真人协作 | 图 UI13 · [小队][crews] | 部分 | 测试 · [委派及汇总][execution] | 行为、数据、权限、待联调 | Agent 子运行与汇总可追踪。验证人机混编、在同一任务中 @ 真人及中途替换成员。 |
| C18 | 小队聊天、收藏、专属 Agent、共享成员工作目录及归档 | [小队][crews] | 缺口 | 未证 · [目录与配置][worker-core] | 行为、数据、权限、待联调 | 验证专属 Agent 无法独立接单/私聊，归档仍读旧任务，跨机器工作目录符合预期。 |
| C19 | 工具、知识库、MCP 连接、授权与真正调用外部服务 | 图 UI31 · [Agent][agents] | 部分 | 测试 · [受限 MCP 子进程][worker-tests] · [文档权限][execution] | 行为、数据、权限、待联调 | 参考画面仅展开工具分组。需有凭据的第三方工具验证授权、撤销、错误回传与轮换。 |
| C20 | Agent 设置的触发器与消息、任务列表、关联仓库及自动化 | 菜单 UI29 · [Agent][agents] | 部分 | 实现 · [设置表单][forms] | 行为、数据、权限、待联调 | 逐项打开设置后验证变更是否生效；面板中显示名称不代表各平台授权成功。 |

## 执行机器、日程与自动化

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| W01 | 安装/连接本机或远程机器，个人、空间、共享范围及心跳 | 图 UI11、UI12 · [机器][computers] | 部分 | 测试 · [真实执行端][worker-tests] | 行为、数据、权限、待联调 | 需在两台主机验证配对、所有者与共享访问、机器离线和重新绑定。 |
| W02 | 单个 Runtime 探测、重扫、错误、安装/升级及模型登录 | 图 UI12 · 壳 · [机器][computers] · [官网][home] | 部分 | 实现 · [机器控制][native-worker] · [机器手册][worker-manual] | 行为、数据、权限、待联调 | 可探测/运行配置好的程序。逐家检验一键安装、下载失败、版本兼容和真实账号。 |
| W03 | 选 Runtime、模型/推理深度、环境、批准模式与额外参数 | 图 UI30 · [Agent][agents] | 部分 | 测试 · [外部进程][worker-tests] | 行为、数据、权限、待联调 | 固定程序参数和环境白名单可运行。需逐种 Runtime 验证选项翻译、凭据遮蔽、失败与配置继承。 |
| W04 | 按任务/聊天分别限并发，最大运行时长、排队超时和取消 | 图 UI30 · [运行][run-help] · [更新日志][changes] | 部分 | 测试 · [并发、取消][worker-tests] | 行为、数据、待联调 | 任务租约、机器容量可测。参考分开设置等待时间与单次运行上限，须含超时重试和排队恢复。 |
| W05 | Agent 工作时段按星期、每日一至两段、IANA 时区 | 菜单 UI30 · [工作时间][hours] | 有对应 | 测试 · [逐日排班与夏令时][weekly] · [真实领取][execution] | 行为、数据、待联调 | 本轮按 IANA 时区与星期校验 0–2 段、上下界、两次秋季重复时刻及分身恢复继承；旧单段固定偏移兼容。参考实际运行待联调。 |
| W06 | 空闲时段外的等待原因、立即触发、已运行任务跨下班时刻 | [工作时间][hours] | 部分 | 实现 · [调度][worker-core] | 行为、数据、待联调 | 待验证不同触发类型、手动覆盖、排队超时豁免与下一可用时间。 |
| W07 | 独立 Git 工作树或空目录，多任务并行与清理 | 图 UI30 · [目录][directory] | 有对应 | 测试 · [两个真正工作树][worker-tests] | 行为、数据、权限、待联调 | 已验证隔离仓库。还需在桌面展示 Git 改动、review、合并/丢弃及清理状态。 |
| W08 | 选择目标机器目录、多路径择空闲、直接目录互斥 | 图 UI30 · [目录][directory] | 部分 | 测试 · [直接目录互斥][worker-tests] | 行为、数据、权限、待联调 | 机器配置可批准单目录/profile。需两条路径、占满等待和远程机器目录选择器。 |
| W09 | 组织/空间/Agent 绑定仓库、平台凭据、默认分支及 PR/MR | 图 补03 · [代码库][repos] · [官网][home] | 部分 | 实现 · [工作机][worker-manual] | 行为、数据、权限、待联调 | 本地 Git 工作树与托管平台自动拉取/推送、生成 PR、独立作者设置分别核验。 |
| W10 | 运行状态、租约、心跳、人工回复、取消、产物和重试 | 图 UI36 · [执行][run-help] | 部分 | 测试 · [租约][execution] · [实际进程][worker-tests] | 行为、数据、权限、待联调 | 核心/进程路径已测。参考只有空历史截图；需非空、运行中、等待、失败、离线重连同场比对。 |
| W11 | 模型错误、限流、额度、凭据失效、机器诊断和本机日志 | [执行排障][run-help] · [CLI][h-cli] | 部分 | 实现 · [机器手册][worker-manual] | 行为、数据、权限、待联调 | 外部退出状态和结果可报。需要合法故障夹具验证分类、可读诊断、后台进程明细的可见边界。 |
| W12 | 本地存储占用、清理已结束的工作目录、迁移和升级 | [CLI][h-cli] · [变更记录][changes] | 部分 | 实现 · [Windows 指南][windows] | 行为、数据、权限、待联调 | 已验证安装覆盖与备份。需核验磁盘用量、活跃租约保护及用户确认后的清理。 |
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
| I05 | 禅道产品线/项目、版本条件、字段映射、状态回写与失败恢复 | [禅道][i-zentao] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 须用符合官方门槛的自建版本、企业凭据及允许写的测试项目；通用导入缺少专用向导。 |
| I06 | TAPD 项目发现、两种身份凭据、导入、限流及持续同步 | [TAPD][i-tapd] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 需 API 凭据、速率限制恢复和远端字段检查。 |
| I07 | Bugly 提单 Webhook、产品范围、可选状态回写 | [Bugly][i-bugly] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 使用测试产品的缺陷提单与回写权限；应该测按产品识别、重投和远端关闭。 |
| I08 | PingCode 企业/项目的工作项发现、导入、同步、回滚 | [PingCode][i-pingcode] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 需要授权项目、用户与状态映射和单批回滚结果。 |
| I09 | Teambition 组织+项目、两层凭据、状态与删除写回 | [Teambition][i-teambition] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 需检验只读授权与有写授权的差别，有映射/无映射状态分别执行。 |
| I10 | Zoho BugTracker 项目导入、再次导入、轮换凭据和批次历史 | [Zoho][i-zoho] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 官方描述通过重复导入跟进，不承诺实时双向。以可授权的测试项目验证增量与回滚。 |
| I11 | Asana 工作区绑定、项目导入/持续读入、限定字段写回 | [Asana][i-asana] | 部分 | 测试 · [Asana 事件与任务详情样例][provider-tests] | 行为、数据、权限、待联调 | 事件需中继补取完整任务。上游真实订阅、人员映射、标题描述回写与状态只入站须分别验。 |
| I12 | GitHub 组织/个人授权、Issue 拉取、显式写入、PR/Commit 和评论关联 | [GitHub][i-github] | 部分 | 测试 · [签名回调与跨仓隔离][vcs-http] · [受控任务关联][vcs-regression] | 行为、数据、权限、待联调 | 已验证稳定任务编号关联提交和 PR、重复事件只关联一次、受信作者的可选审查运行，以及合并后任务状态。真实私库授权、平台身份和远端写入待联调。 |
| I13 | GitLab 云端/自建、个人身份、Issue、MR、Webhook 与同步 | [GitLab][i-gitlab] | 部分 | 测试 · [GitLab 投递][provider-tests] · [本机服务器与 MR 回链][vcs-http] | 行为、数据、权限、待联调 | 已验证 Issue 入站、提交和 MR 引用、撤销后拒收及重复投递。OAuth 授权、讨论回写、企业自建网络待联调。 |
| I14 | CNB 组令牌、仓库配对与 CI 配置，提交/议题/MR 回链 | [CNB][i-cnb] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 需要可写测试组验证回链，普通 Git checkout 无法证明 CNB 接入。 |
| I15 | Linear 任务的导入、更新与平台身份 | [官网][home] · [概念对照][mapping] | 部分 | 测试 · [Linear 事件][provider-tests] | 行为、数据、权限、待联调 | 专用工单事件已用代表性载荷验证。帮助导航无独立 Linear 文章，写回、权限和版本差异待租户验证。 |
| I16 | Gitee、Bitbucket、Codeup、ONES 等官网列名平台 | [官网][home] · [项目产品页][p-planning] | 待核 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 官网列名和通用 Git/行映射不足以断言逐个平台的账户绑定、字段映射或 PR 工作流；按具体账户另验。 |
| I17 | IM 浏览目录、空间任务助手与面向 Agent 的机器人两种关系 | 图 UI10 · 菜单 UI29 · [飞书][i-feishu] · [Discord][i-discord] | 部分 | 测试 · [代表性消息][provider-tests] | 行为、数据、权限、待联调 | 七种普通文字载荷可建单。官方部分平台的普通消息仅为对话，需显式建单或绑定话题；先核对触发语义。 |
| I18 | 飞书国内云，任务助手、Agent 机器人、资源连接器、组织助手 | [飞书][i-feishu] · [群聊流程][bp-feishu] | 部分 | 测试 · [飞书事件][provider-tests] | 行为、数据、权限、待联调 | 缺独立租户授权、用户身份、话题建单/回复、群内卡片和资源工具的同租户实测。 |
| I19 | Lark 国际云的 OAuth/机器人/任务助手和通知 | 图 补06 · [Lark 与飞书][i-feishu] | 部分 | 实现 · [供应商解析][providers] | 行为、数据、权限、待联调 | 载荷可共用，国内/国际账号及云端隔离不能由格式别名证明。需要两个租户独立绑定、撤销。 |
| I20 | 企业微信的空间助手与 Agent 智能体、验签/解密、回复 | [企业微信][i-wecom] | 部分 | 测试 · [解密后的样例][provider-tests] | 行为、数据、权限、待联调 | 需要真实企业权限、回调认证和可见成员；普通入站建单不能证明群/单聊双向。 |
| I21 | 钉钉机器人与任务助手、通知与链接预览、Stream 生命周期 | [钉钉][i-dingtalk] | 部分 | 测试 · [已验证的 Stream/HTTP 载荷][provider-tests] | 行为、数据、权限、待联调 | 需凭据获取、续连、不同身份消息回复、任务卡片及租户授权状态。 |
| I22 | 个人微信扫码绑定一个 Agent、过期重新扫码 | [微信][i-wechat] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 扫码的个人账号与企业微信是两类接入；需经授权验证扫码、会话和失效重绑。 |
| I23 | QQ 官方机器人，单聊/群 @ 绑定 Agent | [QQ][i-qq] | 缺口 | 未证 · [供应商基线][providers] | 行为、数据、权限、待联调 | 需授权机器人、平台允许的会话、真实回复与解绑验证。 |
| I24 | Slack 工作区安装、任务线程、个人通知、机器人及文件/表情互动 | [Slack][i-slack] | 部分 | 测试 · [签名回调与单选改派][slack-http] · [线程入库][im-regression] | 行为、数据、权限、待联调 | 隔离服务验证线程同步、工作区/频道隔离、已送达卡片单选替换多人、保留 Agent、权限撤销与重放。真实租户安装、个人通知、文件和表情仍待联调。 |
| I25 | Discord Gateway、Agent 机器人/任务助手、私信、频道、文件和命令 | [Discord][i-discord] | 部分 | 测试 · [Discord 事件][provider-tests] | 行为、数据、权限、待联调 | 需要授权的 Gateway、中继、已加入的服务器及合适 intents。真实触发与机器人回复待验。 |
| I26 | Telegram Bot、群或话题、私聊、附件与任务助手 | [Telegram][i-telegram] | 部分 | 测试 · [Telegram 更新][provider-tests] | 行为、数据、权限、待联调 | 需有效 Bot 和测试群，核对新话题、机器人账号抑制和真实回复。 |
| I27 | Microsoft Teams Bot Framework、频道/私聊、文件与空间助手 | [Teams][i-teams] | 部分 | 测试 · [Teams Activity][provider-tests] | 行为、数据、权限、待联调 | 需组织管理员许可、真实安装与认证中继；按会话回复与成员退组后的失权单验。 |
| I28 | 已绑定 IM 话题后，后续消息追加评论，任务进度回复原话题 | [群聊流程][bp-feishu] · [群聊方案][s-chat] | 部分 | 测试 · [入站边界][providers] | 行为、数据、权限、待联调 | 可靠的双向链路必须保留平台话题/本地任务映射、重试幂等及受权的实际出站回执。 |
| I29 | 平台账号映射、撤权/解绑、历史保留、远端与本地编辑冲突 | [概念对照][mapping] · [GitHub][i-github] | 部分 | 测试 · [三方合并][execution] · [来源过滤][provider-tests] | 行为、数据、权限、待联调 | 字段冲突有用例。每个供应商的外部身份、凭据生命周期及远端拒绝写入后的状态需另验。 |

## 客户端、服务端与未在 Windows 取证的条件

| 编号 | 入口、操作与条件 | HiFox 证据 | 对照判断 | ShipForge 可查证据 | 验收维度 | 可重复条件、待核结果或已知差异 |
| --- | --- | --- | --- | --- | --- | --- |
| D01 | CLI 结构化查询和写入，与桌面操作同一工作区 | [CLI][h-cli] · [产品介绍][welcome] | 有对应 | 测试 · [CLI 手册][cli] · [协议][protocol] | 行为、数据、权限 | 操作/资源按 ShipForge 自有协议开放。参考 CLI 的命令行兼容性、安装方式和后台服务生命周期分别评估。 |
| D02 | 本机自动发现服务、托盘/守护进程、日志、配置 profile | 图 UI11、UI12 · 壳 · [CLI][h-cli] | 部分 | 测试 · [Worker 进程][worker-tests] · [Windows 指南][windows] | 行为、数据、待联调 | 工作进程可启动、停止。需核验系统开机、关闭桌面、重启机器、服务升级及日志可达范围。 |
| D03 | 自托管后台、成员身份、服务令牌、空间权限、撤销 | [私有化][s-private] · [成员][members] | 有对应 | 测试 · [双客户端 HTTP][team] | 行为、数据、权限、待联调 | ShipForge 有独立服务端验证。参考安装包仅含桌面壳与本机配套程序，无法从中证明两方后端一致。 |
| D04 | 离线读取和草稿、冲突、重连重送、撤权后访问 | [空间][spaces] · [Windows 对照][coverage] | 有对应 | 测试 · [团队客户端][team] · [本地缓存][client] | 行为、数据、权限 | 是 ShipForge 服务保障。参考同一操作的离线时限、冲突策略及撤权行为尚无配对实测。 |
| D05 | 事件、提醒和多人实时更新，退出后从另一端恢复 | [执行][run-help] · [任务产品页][p-tasks] | 部分 | 测试 · [HTTP 与增量查询][team] | 行为、数据、权限、待联调 | 重新请求和运行上报可用。两端打开同一视图后测无手动刷新时延、断线期间丢失事件和去重。 |
| D06 | Windows 安装、升级、数据保留、团队部署与恢复 | [CLI][h-cli] · [私有化][s-private] | 有对应 | 测试 · [安装验证][installer] · [恢复指引][windows] | 行为、数据、权限、待联调 | 可验证自有二进制/数据。参考云服务的备份、集群、计费和托管 SLA 需独立证据。 |
| D07 | 移动端、浏览器或邮件中的提醒，点击后返回对应任务 | 图 补06 · [官网][home] | 跨端 | 未证 · [Windows 指南][windows] | 行为、数据、权限、待联调 | 必须分别提供真实移动系统、浏览器、邮箱及消息落地页；Windows 渠道设置项只能证明配置入口。 |
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
[relations]: https://help.hifox.com/docs/task/task-relations
[mapping]: https://help.hifox.com/docs/integration/concept-mapping
[h-cli]: https://help.hifox.com/docs/cli/cli
[pat]: https://help.hifox.com/docs/cli/access-tokens
[account]: https://help.hifox.com/docs/other/account-management
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
[space-window]: ../crates/shipforge-desktop/src/space_ui_test.rs
[space-lifecycle-tests]: ../crates/shipforge-core/tests/space_lifecycle.rs
[cli]: cli.md
[windows]: windows.md
[coverage]: windows-coverage.md
[features]: features.md
[client]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-client/src/lib.rs
[shell]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/shell.rs
[app]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/app.rs
[pages]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/catalog.rs
[views]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/views.rs
[forms]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/forms.rs
[main]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/main.rs
[team]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-team.py
[installer]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-installer.py
[server]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-server/src/main.rs
[auth]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-server/src/auth.rs
[native-worker]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/machine.rs
[worker-manual]: ../crates/shipforge-worker/README.md
[worker-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-worker/tests/native_runtime.rs
[execution]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/execution.rs
[collab]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/collaboration.rs
[metadata]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/metadata.rs
[workspace-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/workspace.rs
[access]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/access.rs
[domain]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/domain.rs
[protocol]: protocol.schema.json
[query]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/query.rs
[planning]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/planning.rs
[worker-core]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/worker.rs
[providers]: ../crates/shipforge-core/PROVIDERS.md
[provider-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/providers.rs
[schedule]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/automation/schedule.rs
[automation-code]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/automation.rs
[checker]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/check-parity-inventory.py

[weekly]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/src/worker/working_hours.rs

[guide-ui]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/onboarding_ui.rs
[guide-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/onboarding.rs
[invites-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/invitations.rs
[invites-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-invitations.py
[chat-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/chat.rs
[chat-http]: https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-chat.py
[conversation-help]: conversations.md

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
[tag-tests]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-core/tests/tags.rs
[tag-ui]: https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/tag_ui.rs
