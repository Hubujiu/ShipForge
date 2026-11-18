# Zoho BugTracker、腾讯 Bugly 与 TAPD

空间管理员可以在桌面端的集成目录新建连接器，在详情打开对应平台的操作。团队模式使用已连接成员的令牌和当前空间权限。Zoho 的访问令牌和 Bugly 的签名凭据保存在发起操作的本机配置中，按服务器、账号、组织和门户或产品区分。Windows 使用 DPAPI，Unix 配置文件权限为 0600。CLI 的凭据操作可通过标准输入读取 JSON。

## Zoho BugTracker

1. 选择 Zoho BugTracker 并填写十进制字符串格式的门户 ID。打开「保存或更新 Zoho 凭据」。客户端先用新令牌校验门户，再替换本机凭据。申请 `ZohoBugTracker.portals.READ`、`ZohoBugTracker.projects.READ`、`ZohoBugTracker.bugs.READ` 三个 OAuth 权限。访问令牌到期后可再次保存。[OAuth](https://www.zoho.com/projects/help/rest-api/bugtracker-api.html) · [门户接口](https://www.zoho.com/projects/help/rest-api/bugtracker-portal-api.html)
2. 打开「发现 Zoho 项目」查看有权访问的活动及归档项目，把选中的项目 ID 填入连接器。每个组织内，同一门户的同一项目只关联一个目标空间。同批创建的任务全部撤销后，其他空间可以导入该项目。[项目接口](https://www.zoho.com/projects/help/rest-api/bugtracker-projects-api.html)
3. 「预览 Zoho 导入」返回批次 ID、每个来源问题的预期动作和计数。确认批次后才创建任务。客户端使用 BugTracker 公开的美国域名和 `Zoho-oauthtoken` 授权头，按 `index/range` 分页，必要时获取单个缺陷详情。一批最多 5,000 项、7 MB。任一页取数失败时可重试完整预览。[缺陷接口](https://www.zoho.com/projects/help/rest-api/bugtracker-bugs-api.html) · [官方域名与路径示例](https://help.zoho.com/portal/en/community/topic/extension-pointers-for-integrating-zoho-crm-with-zoho-products-6-handle-and-track-your-zoho-bugtracker-issues-easily-from-within-zoho-crm)
4. 再次预览时，客户端从最近已确认的来源修改时间向前重叠一毫秒。来源未提供修改时间时自动全量检查。勾选「重新检查全部缺陷」可主动全量检查。核心保留完整数字 ID，按字段比较上一次来源、本次来源与当前本地值。两方都修改同一字段时保留本地值，批次中列出冲突字段。其他没有冲突的字段可以更新。已有任务在上游消失时会保留。
5. 可用「查看导入历史」查批次号。「撤销导入批次」取消待确认预览，或删除该批次首次创建且仍归其所有的任务。撤销后续只更新已有任务的批次，删除数为零，先前任务及其更新保留。保存新预览后，如连接器或已确认的账本发生变化，需重新预览。[参考行为](https://help.hifox.com/docs/integration/zoho)

缺陷导入为 `bug` 类型。来源的状态和严重性可用 `fields.mapping.status` 和 `fields.mapping.priority` 按原始英文名称覆盖。原始提单人和未匹配处理人保留在任务上。有稳定外部用户 ID 时，可在 `fields.assignee_map` 中明确映射到空间成员。后续写入仍通过空间的类型、状态和人员校验。

```json
{"op":"create","kind":"connector","id":"zo","title":"Zoho","fields":{"provider":"zoho","portal_id":"2063927","enabled":true}}
{"op":"zoho.credential","id":"zo","access_token":"<access token>"}
{"op":"zoho.projects","id":"zo"}
{"op":"update","kind":"connector","id":"zo","expected_rev":1,"fields":{"zoho_project":"170876000003152069"}}
{"op":"zoho.preview","id":"zo"}
{"op":"imports.commit","id":"zo","batch":"<preview batch id>"}
{"op":"imports.history","id":"zo"}
{"op":"imports.rollback","id":"zo","batch":"<batch id>"}
```

客户端取数后调用核心 `imports.stage`。自建客户端可用 `imports.binding` 取得 `revision`、`checkpoint`、`project`，把它们与原始 Zoho `issues` 数组一并提交为 `binding_rev`、`checkpoint`、`project` 与 `issues`。无界面环境可设置 `SHIPFORGE_ZOHO_TOKEN_<门户ID>`。本机保存的凭据优先。发送 `{"op":"zoho.credential","id":"zo","clear":true}` 移除本机凭据。

## 腾讯 Bugly

1. 在接收提单的空间选择「腾讯 Bugly」，填写产品 ID，可设置默认任务类型。在团队模式打开「生成或轮换提单地址」，将显示的完整地址填入 Bugly 控制台产品的缺陷管理项目。每次轮换立即使旧地址失效。单机模式返回相对路径，可与使用同一数据库的 ShipForge 服务地址组合。服务部署在反向代理后或监听所有接口时，应为服务设置 `SHIPFORGE_PUBLIC_URL`，例如 `https://work.example.com`。[HiFox 绑定说明](https://help.hifox.com/docs/integration/bugly)
2. 上游提交包含 `ProductID`、`IssueKey`、`IssueType`、`ProjectName` 和 `FieldValues`。成功返回 `code`、`message` 与可打开的任务地址。在浏览器提示中使用 ShipForge 用户令牌作密码，或通过 Bearer 令牌读取。任务页按当前身份核验空间与任务可见性。同一绑定内，相同产品和问题的重复提单返回原任务。提单人、处理人及「1天」等相对时限保留为来源信息。[腾讯提单格式](https://bugly.tds.tencent.com/docs/tutorial/issue_manage/)
3. 需要回写时，勾选「启用 Bugly 状态回写」，在「配置 Bugly 回写凭据」保存公司 API ID、公司 API Key、当前产品 ProductKey。组织管理员可在组织所属空间建立 ID 为 `bugly-defaults` 的 `settings` 记录，设置 `fields.mapping.priority` 与 `fields.mapping.status`。连接器的同名映射优先。只启用提单时可直接使用前两步。
4. 本地任务状态变为已映射状态后进入出站队列。点击「投递待发状态」，或用工作机命令持续投递。工作机会用请求原文、随机 nonce 和秒级时间戳签署 `POST /v1/issue/modify_issue_status`。以供应商成功应答确认投递，失败保留在队列中。单个绑定按顺序处理。网络中断后回写可能重试同一个目标状态。「查看状态回写记录」会显示投递次数和结果；「断开 Bugly 提单」使提单地址失效并取消待发状态。[腾讯 OpenAPI 签名](https://bugly.tds.tencent.com/docs/tutorial/others/openapi/)

```json
{"op":"create","kind":"connector","id":"mobile","title":"移动端异常","fields":{"provider":"bugly","product_id":"p1","default_task_type":"bug","enabled":true,"bugly_outbound":true,"mapping":{"priority":{"P1":"urgent"},"status":{"in_progress":"repairing"}}}}
{"op":"bugly.rotate","id":"mobile"}
{"op":"bugly.credentials","id":"mobile","api_id":"<company ID>","api_key":"<company key>","product_key":"<product key>"}
{"op":"bugly.deliver","id":"mobile"}
{"op":"bugly.outbox","id":"mobile"}
{"op":"bugly.revoke","id":"mobile"}
```

在已配置团队账号的机器上，可运行 `shipforge-worker bugly-deliver --space default --connector mobile --watch`。也可在该进程指定 `SHIPFORGE_BUGLY_API_ID`、`SHIPFORGE_BUGLY_API_KEY`、`SHIPFORGE_BUGLY_PRODUCT_KEY_<产品ID>`。核心的 `bugly.binding` 返回组织和产品。其他投递程序可用 `bugly.claim` 领取两分钟租约，按返回的投递编号及租约调用 `bugly.settle`，用布尔 `success` 记录结果。

## TAPD

在目标空间选择 TAPD，按实际身份选择公司 API 账号或个人访问令牌。公司模式填写公司 ID，保存 API 账号、密码后通过 HTTP Basic 查询公司项目。个人模式仅保存个人令牌，客户端用 Bearer 令牌读取当前用户的 nick，再查询这个用户参与的项目。已结束项目保留在发现列表。凭据验证成功后才替换本机凭据。腾讯 TAPD 团队的官方 MCP 也用用户信息接口识别个人令牌的用户。[身份及权限](https://help.hifox.com/docs/integration/tapd) · [腾讯 TAPD 官方客户端](https://pypi.org/project/mcp-server-tapd/8.0.83/) · [公司项目](https://open.tapd.cn/document/api-doc/API文档/api_reference/workspace/projects.html) · [参与项目](https://open.tapd.cn/document/api-doc/API文档/api_reference/workspace/user_participant_projects.html)

将发现的项目 ID 保存到连接器，再选「预览 TAPD 导入与更新」。预览提供三种方式，都可以通过批次号确认。

- **补导历史**读取项目的需求和缺陷，跳过已有映射。新任务保留来源映射的状态，包括已开启分拣的空间。
- **检查增量变更**按 TAPD 的 `modified` 查询，游标向前重叠一秒。来自持续更新的新记录遵循空间分拣设置。
- **重新检查全部条目**获取完整列表，通过逐字段比较保护本地编辑。相同数字 ID 的 Story 和 Bug 具有独立映射。需求和缺陷可通过 `fields.mapping.status`、`fields.mapping.priority` 及 `fields.assignee_map` 映射。多名处理人按 TAPD 的分号分隔，未关联的 nick 保留为外部人员。

客户端向 `GET /stories` 和 `GET /bugs` 传递绑定的 `workspace_id`、`page`、`limit`，同时核验响应项的工作区及业务状态码。HTTP 429 时按 `Retry-After` 等待，缺少秒数则等 60 秒。每次请求最多尝试三次。预计等待超过 60 秒或次数用完时，返回 `rate_limited` 和建议等待秒数，管理员可重新发起预览。[需求接口](https://open.tapd.cn/document/api-doc/API文档/api_reference/story/get_stories.html) · [缺陷接口](https://open.tapd.cn/document/api-doc/API文档/api_reference/bug/get_bugs.html) · [限流状态](https://open.tapd.cn/document/api-doc/API文档/API错误码.html)

```json
{"op":"create","kind":"connector","id":"tapd","title":"研发协作","fields":{"provider":"tapd","tapd_auth":"personal","enabled":true}}
{"op":"tapd.credential","id":"tapd","access_token":"<personal token>"}
{"op":"tapd.projects","id":"tapd"}
{"op":"update","kind":"connector","id":"tapd","expected_rev":1,"fields":{"tapd_workspace":"20003270"}}
{"op":"tapd.preview","id":"tapd","import_mode":"history"}
{"op":"imports.commit","id":"tapd","batch":"<preview batch id>"}
{"op":"tapd.preview","id":"tapd","import_mode":"changes"}
```

公司连接采用 `fields.tapd_auth:"company"`、`fields.company_id` 和 `tapd.credential` 的 `api_user`、`api_password`。本机也可通过 `SHIPFORGE_TAPD_TOKEN`，或 `SHIPFORGE_TAPD_USER_<公司ID>`、`SHIPFORGE_TAPD_PASSWORD_<公司ID>` 提供凭据。两个身份均支持本版的手动历史导入和入站变更检测。实时 Webhook、定时同步及远程写回尚待实现，连接器会拒绝开启这几类配置。

## 验证范围

源码仓库中运行 `python scripts/verify-vendor-imports.py target/debug`。测试启动实际团队服务、CLI 和工作机，以隔离数据库和合成账户访问回环 HTTP 上游。覆盖凭据更新、跨空间绑定、授权、分页、同毫秒变更、冲突、按批次回滚、并发重投、密钥轮换、签名和失败后重发。供应商真实租户的授权、字段扩展、线上限流与投递仍需在对应租户验收。

运行 `python scripts/verify-tapd.py target/debug` 复验公司与个人身份、已关闭项目、跨项目来源拒绝、历史重导保护、分拣、同秒增量和 429 恢复。
