# Teambition 和 CNB 接入

Windows「集成」中创建对应平台的连接器，打开详情即可执行以下操作。团队模式由团队服务访问供应商，成员可以从另一台已登录设备继续工作。Teambition 应用凭据、CNB 组令牌按 ShipForge 组织和供应商租户隔离，Teambition 个人授权另外绑定当前成员。本机工作区把凭据放在本机资料目录。Windows 使用当前服务账号的 DPAPI，Unix 通过仅限账号读取的文件权限保护。令牌轮换、凭据清除、项目发现和回写均须在线完成。

## Teambition

1. 组织管理员在 Teambition 安装已授权的应用，获取企业 ID、App ID 和 App Secret。在 ShipForge 连接器保存企业 ID，通过「配置 Teambition 组织应用」提交另外两项。服务用应用凭据换取应用令牌并核对企业 ID。
2. 每名需要发现项目或写回的成员打开「生成 Teambition 个人授权地址」，填写应用已登记的回调地址，打开返回的授权网址。收到回调后，在个人授权表单填写 `code` 与 `state`，或使用该成员已有的个人授权令牌。服务从供应商的用户接口确认操作者。一次性状态有效期十分钟，个人授权失效后重新绑定。
3. 「发现我加入的 Teambition 项目」显示当前账号参加的项目。在连接器的远端项目字段保存项目 ID，设置 `mapping.status` 及可选的 `assignee_map`。预览所选项目可供项目成员查看的未归档任务，返回新增、更新、冲突和排除数量。检查结果后提交批次。首次预览成功后，授权的工作机可使用组织应用读取同一项目的变化。
4. 「以我的身份回写 Teambition」处理已导入的公开任务。管理员须先开启 `vendor_write`。选择标题与描述可写回这两个字段。写回状态须配置 `mapping.write_status`，目标以状态 ID 或项目内唯一的名称表示。每次操作复核当前用户的项目成员资格、远端任务可见性及当前值。
5. 删除远端任务另需管理员开启 `vendor_delete`。操作者选择删除，填写原始远端任务 ID 和刚读取的本地任务修订号。服务重新核对远端项目、标题、描述及状态，成功后本地任务仍可打开。在批次历史回滚导入属于本地操作，并会暂停该连接器的后台同步。

```json
{"op":"teambition.projects","space":"engineering","id":"tb","page_size":50}
{"op":"teambition.preview","space":"engineering","id":"tb","import_mode":"history"}
{"op":"imports.commit","space":"engineering","id":"tb","batch":"<预览返回的批次>"}
{"op":"teambition.push","space":"engineering","id":"tb","task_id":"<本地任务>","intent":"status"}
```

服务按页读取任务和项目工作流，使用外部稳定 ID 与批次快照比较变更。用户映射采用 Teambition 用户 ID。当前回写需从客户端明确发起，支持已关联任务的字段、状态和删除。新建远端任务、同步依赖、供应商评论事件及在线安装应用的回调需另行对接。[HiFox 功能说明](https://help.hifox.com/docs/integration/teambition) · [Teambition 官方 SDK 和接口目录](https://github.com/teambition/openapi-sdk-golang)

## CNB

1. 创建 CNB 连接器，填写完整组路径。组织管理员在凭据表单提交该组成员的个人令牌。服务按层检查顶级组织和子组成员资格。查询组织、仓库列表和仓库详情分别需要 `account-engage:r`、`group-resource:r`、`repo-basic-info:r`。读取提交、议题、合并请求和评论分别使用 `repo-code:r`、`repo-issue:r`、`repo-pr:r`、`repo-notes:r`，发送回链评论需 `repo-notes:rw`。[CNB 官方接口定义](https://api.cnb.cool/swagger.json)
2. 空间管理员发现仓库，选取包含组和子组的完整路径，填写流水线可访问的 ShipForge 团队 HTTPS 地址后配对。将一次性显示的仓库签名密钥存入 CNB 的受保护密钥仓库，作为 `SHIPFORGE_CNB_SIGNING_KEY` 导入通知任务。把返回的片段合并到该仓库默认分支的 `.cnb.yml`。议题事件从默认分支读取配置。推送事件以对应分支的配置为准，需按仓库策略授权能够使用该密钥的分支及事件。可以在配置指引再次查看片段，轮换后更新密钥仓库中的值。[CNB 触发规则](https://docs.cnb.cool/zh/build/trigger-rule.html) · [环境变量与密钥导入](https://docs.cnb.cool/zh/build/env.html)
3. 在推送的末次提交说明、议题、合并请求标题或正文、源分支名或受支持的评论中引用本空间的任务编号。流水线传递构建 ID 和对象标识，服务验证签名及时间戳后从 CNB 重新读取源对象。已关联任务的动态中可打开原始提交、议题或合并请求。受限议题与 ShipForge 私有任务跳过关联。默认片段包含议题评论，合并请求评论需要能够安全使用此仓库密钥的独立受信任流水线。[CNB 默认环境变量](https://docs.cnb.cool/zh/build/build-in-env.html)
4. 在连接器开启 `cnb_backlinks` 后，服务向关联的议题和合并请求各发送一次任务链接。打开「查看 CNB 事件与回链记录」查看最近构建和评论编号。若连接中断，可用「发送或核对 CNB 回链」核对原评论。投递状态不明确时，服务读取评论并匹配唯一投递标记，已经生成的回链会被入站处理跳过。

```json
{"op":"cnb.repositories","space":"engineering","id":"code","page_size":50}
{"op":"cnb.pair","space":"engineering","id":"code","repository":"example/subgroup/tool","server_url":"https://shipforge.example.org"}
{"op":"cnb.ci","space":"engineering","id":"code"}
{"op":"cnb.activity","space":"engineering","id":"code"}
```

在验收中，Windows CLI、团队服务、工作机通过回环 HTTP 访问合成供应商，覆盖凭据隔离、翻页、冲突、签名、重放和回链恢复。隐藏 GPUI 窗口验证了连接器表单、凭据输入及后台状态。生产应用审批、真实 CNB 密钥导入、构建配额及仓库的流水线策略仍需使用获授权租户联调。[HiFox CNB 说明](https://help.hifox.com/docs/integration/cnb) · [隔离 HTTP 回归](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-teambition-cnb.py) · [Windows 窗口回归](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/enterprise_integration_ui_test.rs)
