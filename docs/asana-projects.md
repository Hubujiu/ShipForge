# Asana 项目同步

组织管理员在 Windows「集成」里创建 Asana 连接器。打开「发现 Asana 工作区」，输入个人访问令牌。在「绑定 Asana 工作区令牌」里选择返回的工作区 GID。如果令牌只可见一个工作区，可以留空供服务自动选择。令牌按 ShipForge 组织与 Asana 工作区保存，Windows 使用服务账号的 DPAPI。变更组织令牌前，先核对将共用该凭据的连接器。团队服务执行请求，设备缓存只保留非机密的项目数据。[HiFox 接入流程](https://help.hifox.com/docs/integration/asana) · [Asana 工作区权限](https://developers.asana.com/reference/getworkspaces)

空间管理员打开「发现 Asana 项目」，选择项目并填写连接器的远端项目 GID。可按 `fields.assignee_map` 将 Asana 用户 GID 对应到 ShipForge 成员，在 `fields.mapping.status` 配置 `open` 与 `completed` 对应的本地状态。未映射的处理人保留外部 GID。预览按页读取项目中的任务，随后提交返回的批次。一个任务属于多个项目时，仍须同时符合已绑定工作区和所选项目。[Asana 项目发现](https://developers.asana.com/reference/getprojectsforworkspace) · [任务列表和修改时间过滤](https://developers.asana.com/reference/gettasks)

```json
{"op":"asana.projects","space":"engineering","id":"asana","page_size":50}
{"op":"asana.preview","space":"engineering","id":"asana","import_mode":"history"}
{"op":"imports.commit","space":"engineering","id":"asana","batch":"<预览批次>"}
{"op":"asana.synchronize","space":"engineering","id":"asana"}
```

单次同步和工作机轮询使用上次已提交变更的时间，以重叠窗口重新读取并按稳定 GID 比较。工作机示例为 `shipforge-worker tracker-sync --space engineering --connector asana --watch`。完整历史、重新检查和手动增量可在预览表单选择。原来由 Asana 中继创建的任务在首次导入时保留原本地 ID、讨论和本地修改。已绑项目收到合规的中继事件后唤醒工作机，从官方 API 重新读取。批次回滚会移除本批新建的任务，释出接管的旧任务并暂停同步。后续重新开启可继续读取。

## 写回字段和评论

管理员须在连接器开启 `vendor_write`。空间可编辑成员可写回已关联公开任务的标题与描述。服务逐字段比较当前远端内容与同步基线，向 Asana 发送实际更改，并核对结果。完成状态只用于读取。在任务的「添加评论」勾选「同时发到 Asana」后，服务先保存本地评论，再以组织绑定的 Asana 账号发布。供应商响应中断时，从投递历史查找原评论，在连接器回写表单填写 Asana story GID，核对内容后完成投递。[更新任务](https://developers.asana.com/reference/updatetask) · [发送评论](https://developers.asana.com/reference/createstoryfortask)

```json
{"op":"asana.push","id":"asana","task_id":"<本地任务>","intent":"fields"}
{"op":"asana.push","id":"asana","task_id":"<本地任务>","intent":"comment","body":"已复核验收步骤。","request_id":"0dab31fa-2aff-4ff9-be0f-556d04877d80"}
{"op":"asana.push","id":"asana","task_id":"<本地任务>","intent":"comment","comment_id":"<已保存的本地评论>","confirm_comment":"<Asana story GID>"}
```

每条新评论需要独立的请求 UUID。网络重试应复用同一 UUID 和正文。直接发布已有评论可提供 `comment_id`。评论回执按本地评论保存，编辑或删除评论不会更新 Asana。PAT 权限遵循供应商账号，接口使用 `workspaces:read`、`projects:read`、`tasks:read`，写入需要相应的 `tasks:write` 和 `stories:write`，投递核对需要 `stories:read`。[读取任务评论](https://developers.asana.com/reference/getstoriesfortask)

Windows CLI、团队服务和工作机已通过隔离的 Asana HTTP 样本复验。商业租户的速率限制、真实 PAT 授权和 webhook 订阅仍需使用授权测试账号联调。当前持续同步使用轮询，现有中继可提交带同一任务详情的事件并唤醒同步。[HTTP 回归](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-asana-projects.py) · [Windows 表单](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/enterprise_integration_ui_test.rs)
