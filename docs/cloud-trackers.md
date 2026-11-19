# Linear、ClickUp 与 ONES

Windows「集成」支持按 Linear 项目、ClickUp List、ONES 项目拉取任务。管理员可先查看预计创建、更新和冲突的项目，再确认导入。在连接器详情可以映射状态、类型和成员，按批次撤销，也可以选择一条本地公开评论发布到原任务。[Linear 接入行为](https://help.hifox.com/docs/integration/linear) · [ClickUp 接入行为](https://help.hifox.com/docs/integration/clickup) · [ONES 接入行为](https://help.hifox.com/docs/integration/ones)

## 连接并选择来源

1. 新建连接器并选择平台。Linear 和 ClickUp 支持个人凭据或 OAuth。ONES 填入区域或私有部署的租户根地址，如 `https://sz.ones.cn`。ONES 的团队读取需要 `read:account:teams`，还需允许读取项目、工作项类型和工作项。「来源产品或项目 ID」可先留空。
2. 在连接器详情选择「读取可访问的供应商账号」。使用个人凭据时将 Key 填入「供应商访问令牌」。Linear 使用令牌查询组织，ClickUp 展示已授权的工作区，ONES 列出此 Key 可见的团队。打开「绑定或轮换供应商凭据」并填写返回的账号 ID，只有一个可见账号时可直接绑定。
3. 选择「发现可访问项目」或「发现 ClickUp 列表」。ClickUp 显示工作区中的 Space、Folder 和 List，保留无文件夹的 List。将目标项目或 List ID 保存到连接器。入站绑定根据供应商账号、项目或 List、组织和空间校验。轮换凭据会核对已绑定账号。
4. OAuth 模式在「生成 OAuth 授权地址」填写自己的应用 ID 和已注册的回调地址，按需选择写入权限。在浏览器访问返回的地址，完成授权，将回调中的 `code` 和 `state` 填入凭据表单。ClickUp 还需填写应用密钥。客户端验证十分钟内有效的一次性 state，Linear 同时使用 PKCE。Linear 原生刷新令牌可在组织核验收到 HTTP 401 后续期。已有合法 OAuth 访问令牌也可直接填写。[Linear OAuth](https://linear.app/developers/oauth-2-0-authentication) · [ClickUp 官方认证](https://developer.clickup.com/docs/authentication) · [ONES 个人 API Key](https://docs.ones.com/developer/openapi/auth/personal-api-keys)

团队服务保存来源绑定、成员映射、批次和回执。凭据保存在当前设备，按 ShipForge 服务器、登录身份、组织、供应商账号与认证方式隔离。ONES 凭据额外绑定到经过验证的租户地址。相同身份在本机同一供应商账号下的多个项目链路可以共用已保存凭据。Windows 使用当前用户 DPAPI，Unix 设备文件权限为 0600。其他账号和其他工作机需要自行配置凭据。「移除本机凭据」会影响该设备身份使用同一供应商账号的链接。

以下 JSON 每次单独送入 `shipforge request` 的标准输入。凭据字段的示例值须替换，修订号取自 `get` 或创建结果。

```json
{"op":"create","kind":"connector","id":"linear-release","title":"Linear 发布项目","fields":{"provider":"linear","cloud_auth":"key","enabled":true}}
{"op":"linear.accounts","id":"linear-release","access_token":"<个人 Key>"}
{"op":"linear.credential","id":"linear-release","account_id":"<已发现组织 ID>"}
{"op":"linear.projects","id":"linear-release","page_size":50}
{"op":"update","kind":"connector","id":"linear-release","expected_rev":2,"fields":{"remote_scope":"<Linear 项目 ID>","assignee_map":{"<来源用户 ID>":"<本地成员 ID>"},"mapping":{"status":{"<工作流状态 ID>":"in_progress"},"write_status":{"done":"<远端完成状态 ID>"}},"vendor_write":true}}
{"op":"linear.preview","id":"linear-release","import_mode":"history"}
```

ClickUp 使用 `clickup.accounts`、`clickup.credential`、`clickup.projects`、`clickup.preview`。OAuth 可使用 `clickup.authorize` 和 `linear.authorize`。ONES 使用 `ones.accounts`、`ones.credential`、`ones.projects`、`ones.preview`。ONES 新建连接器时填写 `fields.ones_url`。直接在 `*.credential` 输入个人令牌时也可同时提供 `account_id`。

## 数据与分页

| 来源 | 可读数据 | 状态、类型和人员 |
| --- | --- | --- |
| Linear | 固定 `https://api.linear.app/graphql`。组织和项目按令牌权限发现，Issue 通过项目 ID 过滤。项目、Issue 和来源关联系统使用游标分页，增量使用有重叠的 `updatedAt` 下界。 | `mapping.status` 按工作流状态 ID 对应，两条同名状态可以映射到不同本地状态。处理人、优先级、正文、链接和编号随任务保留。 |
| ClickUp | 使用 v2 `/team`、`/team/{id}/space`、`/space/{id}/folder`、Folder 和无目录 List 接口。按 `/list/{id}/task` 的页码导入主任务与子任务，包含已关闭项。增量使用 `date_updated_gt` 毫秒下界。 | 处理人可对应多名本地成员，未匹配的 ID 保留为来源人员。工作量估算、未支持的自定义字段和自定义任务类型在预览逐项提示。 |
| ONES | 租户 OpenAPI v2 使用 Bearer Key。先请求 `/account/teams`，按 `teamID` 获取项目和工作项类型。请求 `/project/issues` 时同时提供 `projectID`、`issueTypeID` 和游标，需要正文时读取单项详情并复核归属。 | 可在 `mapping.type` 中指定任务、需求或缺陷。从标准类型名识别需求和缺陷，未知类型采用空间的默认配置。官方列表没有修改时间查询参数，工作机按内容差异同步。 |

同一空间内、同一来源账号的两端任务都已导入时，确认批次会建立受支持的层级、阻塞或相关关系。目标未导入、关系成环或与已有关系冲突时，回执的 `relations.unavailable_or_conflicting` 给出数量。导入由本地事务完成，任何分页错误、部分 GraphQL 错误或不符的项目归属都会中止预览。单次限 5,000 项和 7 MB。[Linear GraphQL](https://linear.app/developers/graphql) · [ClickUp 列表任务](https://developer.clickup.com/reference/gettasks) · [ONES 项目](https://docs.ones.com/developer/openapi/api/01-get-team-project-list) · [ONES 工作项筛选](https://docs.ones.com/developer/openapi/api/01-get-a-list-of-issues)

## 导入、持续同步与撤回

```json
{"op":"imports.commit","id":"linear-release","batch":"<预览批次>"}
{"op":"imports.synchronize","id":"linear-release"}
{"op":"imports.history","id":"linear-release"}
{"op":"imports.sync_control","id":"linear-release","action":"pause"}
{"op":"imports.rollback","id":"linear-release","batch":"<已完成或预览批次>"}
{"op":"imports.sync_control","id":"linear-release","action":"resume"}
```

确认批次后，同一来源 ID 始终对应原来的任务。重复确认返回同一结果。本地和上游独立修改了同一字段时，轮询返回 `needs_review`，保留本地编辑，等管理员重新预览并核对。工作机运行 `shipforge-worker tracker-sync --connector linear-release --watch --interval 300` 后定期接收变化。在暂停状态下，工作机跳过供应商请求，已有任务、凭据和配置保留。撤回仅删掉由该批次新建且仍属原映射的任务。若实际撤回了新建任务，同步自动暂停，可显式恢复。已发送的外部评论和状态保留在供应商处。取消预览可使用同一 `imports.rollback`，之后可重新预览当前来源。

已绑定组织和项目的 Linear 连接器还可接收经过验签的中继。`provider.ingest` 核对事件的组织与项目，按 `Linear-Delivery` 去重，并返回 `sync_requested`。监听中的工作机每五秒检查本机或团队服务上的投递序号，收到新事件即从 Linear 重新读取该项目。回调和按时轮询共用同一份来源映射。原来通过同一个连接器建立的 Linear 任务会按 Issue ID 保留本地编号和编辑，撤回项目批次会解除对这些旧任务的接管。已暂停的连接维持暂停。[Linear Webhooks](https://linear.app/developers/webhooks)

## 状态和新评论回写

在连接器勾选「允许显式回写来源平台」，配置 `mapping.write_status`，并授予上游项目相应写权限。选择「回写关联工作项」发布已关联任务的状态或所选的本地公开评论。项目归属、当前上游状态及任务身份会再次检查。独立的上游变更会提示先重新预览。每次操作带有任务级回执和短时租约，失败时可以从 `imports.push_history` 核对。

- Linear 调用 `issueUpdate` 修改工作流状态或 `commentCreate` 新建评论。状态使用原生工作流状态 ID。[Linear 写入接口](https://linear.app/developers/sdk-fetching-and-modifying-data)
- ClickUp 先读取 List 的状态字典。可以配置状态 ID 或该 List 上唯一的完整状态名称。只返回名称的状态也能导入和回写。客户端以 `PUT /task/{id}` 写入正式名称，新评论通过 `POST /task/{id}/comment` 发布。[任务状态](https://developer.clickup.com/reference/updatetask) · [创建任务评论](https://developer.clickup.com/reference/createtaskcomment)
- ONES 先以 `action=executableWorkflow` 查该工作项当前可执行的转换，再以 `action=executeWorkflow` 提交明确的转换 ID。起止状态相同的多条转换须在 `mapping.write_workflow` 指定其中一条。交互式必填字段可在上游处理。ONES 的创建评论接口接受纯文本，客户端采用文档列出的 128 字符上限。供应商只返回成功确认时，本地回执的 `remote_id` 为空。[执行工作流](https://docs.ones.com/developer/openapi/api/02-execute-workflow-for-issue/) · [新建工作项评论](https://docs.ones.com/developer/openapi/api/02-send-issue-comment/)

```json
{"op":"imports.push","id":"linear-release","task_id":"<本地已导入任务 ID>"}
{"op":"imports.push","id":"linear-release","task_id":"<本地任务 ID>","intent":"comment","comment_id":"<本地公开评论 ID>"}
{"op":"imports.push_history","id":"linear-release"}
{"op":"imports.push","id":"linear-release","task_id":"<本地任务 ID>","intent":"comment","comment_id":"<本地公开评论 ID>","confirm_comment":"<供应商已收到的评论 ID>"}
```

状态响应丢失时，重试会先读上游确认是否已经生效。新评论响应不明时回执显示 `uncertain`。提供供应商已生成的评论 ID 后，客户端核对原任务和正文后记为完成。评论编辑、删除及向上游创建依赖关系尚未开放。

本轮验收使用回环上游、临时团队数据库和真实 Windows 客户端。真实租户的浏览器同意页、企业审批、配额和自定义流转表单待联调。ClickUp 官方签名 Webhook 与 ONES 应用中心实时事件安装需另行接入，现有连接器可持续轮询。[隔离 HTTP 验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-cloud-trackers.py) · [ClickUp Webhook](https://developer.clickup.com/docs/webhooks) · [ONES 部署条件](https://help.hifox.com/docs/integration/ones)
