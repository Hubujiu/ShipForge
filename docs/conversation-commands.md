# 对话指令、审阅和文档链接

## 从任务发起快捷指令

在 Windows 任务详情选择「快捷指令」。主列表按本人的收藏顺序排列。「更多快捷指令」显示其他可见条目。「管理快捷指令」可以新建、编辑、收藏、上移或下移。新建的条目排在本人收藏首位。空间成员各自保留自己的顺序。私有指令只有创建者可见，共享指令只能由创建者编辑或删除。[HiFox 对话说明](https://help.hifox.com/docs/agent-and-computer/chat-with-agent)

每次点击指令会新建一条与该任务关联的私有会话，冻结任务标题、描述、状态、版本和当时可读的最近评论。可选择 Agent 或小队。省略提示词时，执行者收到任务上下文，消息用任务编号和标题标识。Agent 有可用的在线工作机时可以发送，忙碌时进入队列。没有可用的在线工作机时，发起操作返回冲突，任务上不会留下空会话。在历史会话里重新打开上次选择可以继续追问，刷新和删除指令都保留已有会话。

```json
{"op":"shortcut.save","space":"default","title":"整理验收项","prompt":"根据当前任务列出验收项","handler_kind":"agent","handler_id":"inspector","visibility":"space","request_id":"make-inspection"}
{"op":"shortcut.catalog","space":"default"}
{"op":"shortcut.organize","space":"default","id":"<快捷指令 ID>","pinned":true}
{"op":"shortcut.run","space":"default","task":"ticket","id":"<快捷指令 ID>","request_id":"ticket-first-inspection"}
{"op":"shortcut.history","space":"default","task":"ticket"}
```

`shortcut.save` 的首建请求省略 `id`，返回新 ID。修改和 `shortcut.delete` 可传 `expected_rev`。调整排序时给 `shortcut.organize` 传 `order`，按目标顺序列入本人当前可见的全部收藏 ID。回放同一写请求使用原 `request_id`，每次有意发起新会话请换一个 ID。

## 在对话输入框使用 Slash

在 Agent 或小队的消息编辑器中输入 `/`，按当前 Runtime 的技能与命令分组筛选。单击或按 Tab 将选项插入输入框，Esc 关闭菜单，发送后才进入执行队列。界面显示离线或尚未上报的原因，工作机恢复后可点「重新获取」。这里的 Runtime 内置命令由工作机配置中的 `runtimes.<名称>.slash_commands` 上报。旧配置继续工作，未上报名单时只列出已绑定、可用的技能。如果同一 Agent 可使用多台在线工作机，内置命令取这些机器所上报名单的交集。[HiFox Slash 菜单](https://help.hifox.com/docs/agent-and-computer/chat-with-agent#slash-命令)

```json
{"op":"chat.slash","id":"<会话 ID>","query":"/pla"}
{"op":"chat.slash.send","id":"<会话 ID>","body":"/plan 处理下一步","request_id":"first-plan"}
```

调用方收到 `online`、`busy`、`commands_reported` 和分组条目。发出时会重新检查会话权限及命令是否仍可用。外部程序对某个命令的具体解释由相应 Runtime 决定。独立测试使用本地合成运行配置。

## 确认审阅

工作机成功完成任务运行后，能读取该运行的人可以在任务列表、看板及详情看到「待审阅」。工作机需要在某次完成后跳过审阅提醒时，可在 `run.report` 传 `needs_review:false`。同一任务和会话保留最新的一轮。收件箱进入任务后可选「已审阅，没问题」。确认发送后，回复和清除本次可见的待审记录在同一事务完成。任务状态保持原值。失败发送或普通反馈会保留待审记录。可指定待审的 `run`，避免多轮同时完成时误清新一轮。[HiFox 收件箱](https://help.hifox.com/docs/agent-and-computer/inbox) · [HiFox 任务评论](https://help.hifox.com/docs/agent-and-computer/comments-and-replies)

```json
{"op":"task.review.state","id":"ticket"}
{"op":"task.review","id":"ticket","run":"<待审运行 ID>","expected_rev":3,"request_id":"review-1"}
```

在会话详情选择「将对话及关联任务标为已完成」使用 `chat.complete`。此操作将对话标记完成，有关联任务时更新到该任务工作流的完成状态。此操作不会发送确认消息。后续新消息会清除对话的完成标记。产品更新日志另行说明了这项会话操作。[HiFox 更新日志](https://help.hifox.com/changelog/)

## 分享消息和图片

对话所有者选择已完成的消息，创建时明确选择范围。Windows 将所选回复关联的提问一并纳入快照。JSON 客户端可显式设置 `include_questions:true`。附件名称和能解码的 PNG、JPEG、WebP、GIF 缩略图随所选消息保存。文本接口仅显示正文、作者和附件名。原图与其他文件的下载权限继续由工作区管理。图片内容、文本及顺序保持创建时的版本。[HiFox 对话分享](https://help.hifox.com/docs/agent-and-computer/chat-with-agent#分享已完成的对话内容)

```json
{"op":"chat.share.create","id":"<会话 ID>","message_ids":["<已完成的回复 ID>"],"scope":"organization","include_questions":true}
{"op":"chat.share.image","share":"<64 位分享码>"}
{"op":"chat.share.revoke","id":"<会话 ID>","share":"<64 位分享码>"}
```

Windows 从对话详情的已有分享选择「保存分享图片」，指定一个新的 PNG 文件名。CLI 支持 `shipforge share-image <分享码或分享链接> <新文件.png>`，检查文件长度与 SHA-256。HTTP 客户端可用 `GET /share/<分享码>/image.png`。该路径和文本页面共用范围、撤销、成员检查。组织内链接要求成员的 Bearer 令牌，公开链接可匿名访问。已撤销的选区再次访问返回 404。已另存的文件独立于服务端链接。

## 当前权限下打开文档

文档详情的「复制文档链接」生成包含工作区标识的 `shipforge://document/…` 链接。在文档库选择「打开文档链接」，或运行 `shipforge-desktop --open-link <链接>`。任务描述、评论和文档正文里的引用可在详情预览，点击预览标题在应用内打开。预览展示当前标题、正文摘要和版本。较长的 Markdown 在宽窗口自动展示标题层级及所在行。窄窗口可用「文档大纲」展开。代码块中的示例标题不会列入。

```json
{"op":"document.share","space":"team","id":"notes"}
{"op":"document.open","link":"<document.share 返回的 link>"}
{"op":"document.preview","link":"<document.share 返回的 link>"}
{"op":"document.embeds","space":"default","kind":"comment","id":"review-notes"}
```

链接固定指向文档身份，读取时按当前成员关系和文档可见性计算权限。撤权后，原有引用可以显示不可访问状态，标题、摘要、附件与大纲均不返回。团队 CLI 还返回 `http_url`。请求该地址须带当前团队令牌，正文按纯文本返回。团队模式的文档正文、历史版本、嵌入与链接需要在线重新核权。[HiFox 文档库](https://help.hifox.com/docs/space/docs)

运行 `cargo test -p shipforge-core --test conversation_tools --locked` 检查执行、权限与历史。构建 CLI 和团队服务后，运行 `python scripts/verify-conversation-tools.py target/debug` 验证本地 HTTP 和独立身份。Windows 桌面运行 `cargo test -p shipforge-desktop --locked hidden_conversation_commands_and_document_window -- --ignored --test-threads=1` 验证原生输入、分享文件、链接和任务状态。
