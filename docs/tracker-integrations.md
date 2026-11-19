# 禅道与 PingCode

空间管理员在 Windows「集成」新建连接器，选择平台，填写实例或企业，保存后在连接器详情配置凭据。项目 ID 可以留空，先用「发现」查看当前凭据能读取的范围，再编辑连接器选择项目。需要预览、确认、回滚或发布时使用该连接器的操作。命令行用 `shipforge request` 从标准输入读取单个 JSON 对象。[HiFox 禅道接入](https://help.hifox.com/docs/integration/zentao) · [HiFox PingCode 接入](https://help.hifox.com/docs/integration/pingcode)

## 禅道 REST v1

安装根地址填写如 `https://pm.example.org/zentao`。开源版支持从 16.5 起、企业版从 6.5 起、旗舰版从 3.0 起。表单接受用户提供的版本号并按版本类型检查下限，连接时通过实际接口发放 Token 和发现可见范围验证兼容性。工作机须能访问该实例的 HTTPS 地址。本机开发夹具可用回环 HTTP。[版本要求](https://help.hifox.com/docs/integration/zentao) · [官方鉴权文档](https://www.zentao.net/book/api/1397.html)

在「连接禅道实例」输入禅道账号和密码，客户端以 `POST /api.php/v1/tokens` 验证。每次读取或写入前取新令牌，通过 `Token` 请求头访问 API。「禅道来源」可选产品或项目，分别从 `/products`、`/projects` 查询。产品导入产品的研发需求、启用的用户需求和缺陷。项目导入关联研发需求、缺陷，以及各执行下的任务。列表分页从 1 开始，有独立 ID 的子项一并处理。范围内的数据缺少正文时再读取单项详情。[供应商路由](https://github.com/easysoft/zentaopms/blob/zentaopms_18.12/config/routes.php) · [官方 v1 实现与字段](https://github.com/easysoft/zentaopms/tree/zentaopms_18.12/api/v1/entries)

```json
{"op":"create","kind":"connector","id":"zen","title":"研发产品","fields":{"provider":"zentao","instance_url":"https://pm.example.org/zentao","scope_kind":"product","zentao_edition":"opensource","zentao_version":"18.12","enabled":true}}
{"op":"zentao.credential","id":"zen","account":"<账号>","password":"<密码>"}
{"op":"zentao.projects","id":"zen","page_size":50}
{"op":"update","kind":"connector","id":"zen","expected_rev":1,"fields":{"remote_scope":"7","vendor_write":true,"mapping":{"write_status":{"done":"resolved","cancelled":"closed","in_progress":"active"},"resolution":"fixed","resolved_build":"trunk"}}}
{"op":"zentao.preview","id":"zen","import_mode":"history"}
```

示例中的 `expected_rev` 应取自当前连接器。缺陷默认映射为「缺陷」，研发和用户需求映射为「需求」，执行任务映射为「任务」。映射支持来源状态、类型、优先级和处理人账号。来源的缺陷、需求与任务编号分别保存，避免相同数字撞号。同一组织内，一个安装根地址、维度和范围只能由一个连接器持有。

打开「允许显式回写来源平台」后，选择「回写关联工作项」发布一个已关联缺陷的本地状态。支持解决、关闭和激活，依次对应原生 `resolve`、`close` 和 `active` 动作。关闭前缺陷须已解决。默认解决方案为 `fixed`，构建为 `trunk`，可在 `mapping` 中修改。执行前核对归属、当前状态及映射，执行后重新读取状态。若网络断开，重试同一操作会先检查远端已有的状态。需求和执行任务当前可导入。禅道项目调整、移动或删除来源记录后，可重新预览并人工核对。[官方缺陷操作](https://github.com/easysoft/zentaopms/tree/zentaopms_18.12/api/v1/entries)

## PingCode OpenAPI v2

填写 PingCode 企业 ID。平台提供访问令牌、企业应用凭据和用户授权码三种接入方式。直接令牌填入 `access_token`。企业应用填入 `client_id`、`client_secret`。用户授权在已注册应用的授权页完成，将回调中的 `code` 填入客户端，连同同一应用的 ID 和密钥换取访问令牌。每次访问都用 `GET /v2/directory/team` 核对凭据对应的企业。项目发现需要 `pcp:read:global:team` 和 `pcp:read:pjm:project`，工作项读取需要 `pcp:read:pjm:workitem`，状态写入和工作项评论需要对应主体的写权限。企业核验返回 401 时，应用模式重新签发企业令牌，用户授权模式使用原生 `refresh_token` 流程续期。直接令牌或已吊销的授权可经原入口更新。[官方授权与 API 文档](https://open.pingcode.com/) · [官方 API 目录及作用域](https://open.pingcode.com/api_data.json)

```json
{"op":"create","kind":"connector","id":"pc","title":"PingCode 研发","fields":{"provider":"pingcode","enabled":true,"enterprise_id":"<企业 ID>","pingcode_auth":"token"}}
{"op":"pingcode.credential","id":"pc","access_token":"<访问令牌>"}
{"op":"pingcode.projects","id":"pc","page_size":50}
{"op":"update","kind":"connector","id":"pc","expected_rev":1,"fields":{"remote_scope":"<项目 ID>","assignee_map":{"<PingCode 人员 ID>":"<本地成员 ID>"},"mapping":{"type":{"<类型 ID>":"bug"},"status":{"<办理中状态 ID>":"in_progress"},"write_status":{"done":"<完成状态 ID>"}},"vendor_write":true}}
{"op":"pingcode.preview","id":"pc","page_size":50}
```

原生列表使用 `GET /v2/pjm/projects` 和 `GET /v2/pjm/workitems?project_id=…`，保留归档项目，过滤已删除工作项。分页使用从 0 起的 `page_index`。工作项缺少类型映射时采用当前空间的默认任务类型。可按来源状态 ID、名称和优先级映射。未匹配人员保留为来源人员，自定义属性保存在任务的 `source_properties` 中，预览会列出。来源项目不符时整批失败。

启用回写权限并配置 `mapping.write_status` 后可以发布本地状态。客户端核对项目并用 `PATCH /v2/pjm/workitems/{id}` 写入 `state_id`。选择「指定评论」可发布当前任务的一条本地公开评论，使用 `POST /v2/comments` 指定工作项和正文。若响应丢失，回写历史显示 `uncertain`。在 PingCode 确认实际评论后提供评论 ID，客户端向对应工作项查询该评论并验证正文后补记回执。此流程避免重复发布。两种平台的回写记录都可在连接器详情查看。[官方工作项和评论接口](https://open.pingcode.com/)

```json
{"op":"imports.push","id":"pc","task_id":"<已导入任务 ID>"}
{"op":"imports.push","id":"pc","task_id":"<已导入任务 ID>","intent":"comment","comment_id":"<本地评论 ID>"}
{"op":"imports.push","id":"pc","task_id":"<已导入任务 ID>","intent":"comment","comment_id":"<本地评论 ID>","confirm_comment":"<远端评论 ID>"}
{"op":"imports.push_history","id":"pc"}
```

## 批次、增量与回滚

1. 从连接器详情预览，结果包括预计新建、更新、保留和冲突数量及批次 ID。使用 `imports.commit` 确认该 ID，重复确认返回既有结果。禅道的历史补导跳过已关联记录，`refresh` 对整批逐字段比较。PingCode 的历史导入可以重新核对已有记录。两者都保留独立发生的本地字段改动。
2. 选择「立即同步工作项」或执行 `imports.synchronize`，可以一次拉取并应用增量。PingCode 按已提交记录中最近的更新时间构造有重叠的 `updated_between` 秒区间，接住同一秒的变更。禅道分页读取绑定范围，使用本地快照比较差异。增量模式的新记录遵循空间分拣设置。发生字段冲突或原任务脱离映射时返回 `needs_review`，可改用预览和人工确认。没有差异的轮询返回 `unchanged`，批次账本不增长。
3. 有网络访问权的工作机可以执行 `shipforge-worker tracker-sync --space default --connector pc --watch --interval 300`。去掉 `--watch` 只执行一次。工作机使用当前设备、空间、服务器和账号对应的连接器凭据，逐次复核成员权限。可在工作机停止前台进程，下一次运行从已提交数据继续。
4. 在历史列表使用 `imports.rollback` 和批次 ID。未提交预览会取消，已提交批次仅移除由本批创建、仍保有原身份的本地任务。此前已有任务收到的更新及已发送至供应商的状态或评论会保留。可重复调用撤销取得相同结果。回滚所有持有任务后才可改绑项目。

```json
{"op":"imports.commit","id":"pc","batch":"<预览 ID>"}
{"op":"imports.history","id":"pc"}
{"op":"imports.synchronize","id":"pc"}
{"op":"imports.rollback","id":"pc","batch":"<待撤销批次 ID>"}
```

团队服务要求空间管理员权限和相应读写令牌。自动化客户端可经 `imports.prepare_push` 领取绑定到单个任务的短时租约，按计划核对并发布，再通过 `imports.finish_push` 提交结果。开启回写须拥有供应商项目的原生写权限。Windows 凭据由当前 Windows 用户的 DPAPI 保护，Unix 使用只读写于当前账号的设备文件。在连接器的凭据表单勾选「移除本机凭据」可撤销该设备的使用。上游授权也可在相应供应商管理台吊销。

发布验收使用本地 HTTP 上游、临时团队数据库与独立凭据。测试覆盖产品/项目分页、来源范围、撤权、冲突、同秒更新、评论响应丢失和批次撤销。商业版本、真实租户的自定义工作流、OAuth 同意回调及配额仍需在各部署中联调。[专项测试](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-trackers.py)
