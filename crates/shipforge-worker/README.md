# ShipForge 机器执行器

`shipforge-worker.exe` 把空间中的任务交给本机安装的程序。它与桌面、CLI 共用工作区协议。单机模式使用当前用户的 ShipForge 数据目录，可用 `SHIPFORGE_DATA_DIR` 指定独立配置。团队模式读取该配置中保存的连接，也可同时设置 `SHIPFORGE_SERVER` 和 `SHIPFORGE_TOKEN`。持续领任务的进程可使用 `worker` 范围令牌，执行身份还需目标空间的编辑权限。运行、回复、产物及关联任务可以从客户端的执行历史中打开。

## 第一次运行

在 Windows PowerShell 中，将配置文件放在绑定仓库外。以下命令会启用一个可独立运行的上下文检查程序，适合首次验证。

```powershell
.\shipforge-worker.exe init --config D:\ShipForge\machine.json --space default --example
'{"op":"create","kind":"agent","id":"inspector","title":"Inspector","fields":{"runtime":"inspect-context"}}' | .\shipforge.exe request
'{"op":"create","kind":"task","id":"acceptance","title":"Check worker","fields":{"status":"todo","assignee_kind":"agent","assignee":"inspector"}}' | .\shipforge.exe request
.\shipforge-worker.exe run --config D:\ShipForge\machine.json --once
.\shipforge.exe list run
```

持续工作时运行 `shipforge-worker.exe run --config <配置文件>`。使用同一配置的 `stop --config <配置文件>` 会停止接新任务，等待已领取的工作结束。关闭窗口前可以通过 `run.cancel` 取消一条正在执行的任务。`probe` 列出本机可找到的常见可执行文件。

## 配置外部 Runtime

小队管理者可以开启 `share_task_directory`，使同一任务的队长、成员和汇总在同一机器及绑定仓库中复用独立工作树。每次执行的技能文件可从标准输入 `fields.skill_directories` 的 `entry_file` / `directory` 读取。可在机器配置设置 `"skill_roots":{"review":["D:/Team/Skills"]}`，允许已注册的 `review` Runtime 从指定目录发现技能。工作机也识别绑定项目中的 `.agent/skills` 等目录。 [技能发现与共享目录](../../docs/skills-market.md)

`init --config <文件> --program <exe> --runtime <名称> --arg <固定参数> --workspace <绝对目录>` 可以创建机器配置，每个固定参数使用一个 `--arg`。已有配置请编辑 JSON，重启 Worker 后生效。示例中的 `code-review.exe` 和 `mcp-docs.exe` 代表操作者自行安装的程序。

```json
{
  "machine_id": "workstation-01",
  "name": "研发工作站",
  "space": "default",
  "shared": false,
  "capacity": 2,
  "allow_direct": false,
  "workspace": "D:/Work/Project",
  "profiles": { "website": "D:/Work/Website" },
  "runtimes": {
    "review": {
      "program": "D:/Tools/code-review.exe",
      "args": ["--stdin"],
      "inherit_env": [],
      "timeout_seconds": 1800
    }
  },
  "approved_tools": {
    "reference-search": {
      "tool_name": "search_reference",
      "program": "D:/Tools/mcp-docs.exe",
      "args": ["--stdio"],
      "timeout_seconds": 15
    }
  }
}
```

机器容量取 1–64。个人机器执行归属本人的工作。空间管理员可注册共享机器。Agent 的 `runtime`、`machine_id`、`profile`、`max_parallel`、`chat_parallel`、`working_hours` 决定匹配范围。变体通过 `parent_id` 继承，`working_hours` 设为 `null` 时重新采用上级设置。

周排班按 IANA 时区和当地时钟查看能否开始新运行。下例安排上海时间周一两个时段和周六全天。星期键为周一 `"1"` 至周日 `"7"`；未列出或空数组的日期休息。每个日期最多两段，开始时刻包含在内，结束时刻排除在外。午夜结束可填 `24:00`，跨天班次分别填写两天。`week:{}` 一周都不接新运行。

```json
{"timezone":"Asia/Shanghai","week":{"1":[{"start":"09:00","end":"12:00"},{"start":"13:30","end":"18:00"}],"6":[{"start":"00:00","end":"24:00"}]}}
```

夏令时跳过的当地分钟没有对应运行时刻；回拨后重复出现的当地分钟，两次均可按该段接单。运行使用入队时保存的有效配置，已开始的运行跨过结束时刻仍可继续。旧式配置 `{"start":21,"end":3,"utc_offset_minutes":480}` 仍按每天固定偏移、整点判断，支持跨午夜，同一开始与结束表示全天。

隔离模式会针对绑定的 Git 仓库从 `HEAD` 创建独立分离工作树，也可省略 `workspace` 让每次运行使用空目录。工作树及产物保存在配置文件旁，例如 `machine.json.state/work`，数据库记录机器路径。直接使用已有目录时，同时设置机器 `allow_direct: true` 和 Agent `directory_mode: "direct"`。涉及同一规范化路径的直接运行通过系统文件锁串行访问。[Git worktree 文档](https://git-scm.com/docs/git-worktree)

## 输入、回复与产物

外部 Runtime 从标准输入读取一条 JSON 运行记录，可在 `fields` 下读取 `config`、`task_context`、`instruction`、`reply`、`child_results` 和 `tool_results`。技能内容和配置版本在排队时记录。程序在批准的目录内运行，默认继承系统基础环境，额外变量从本机 `inherit_env` 清单读取。

程序向标准输出写一条 JSON，退出码用于判定进程失败。正常文本输出也会作为结果保存。产物路径应是当前工作目录内已经存在的文件。每个输出流最多记录 96 KiB。

需要显示运行进度时，将 `{"message":"正在整理报告","percent":60}` 写入 `fields.progress_file` 提供的当前运行专用路径。Worker 会通过租约心跳上报进展。明确列入产物的文件每个最多 256 KiB、每份回报合计最多 1 MiB 时，原始内容会随回报保存到工作区，可按附件下载。更大的文件可通过所属机器的产物路径读取。

```json
{"status":"done","result":"已生成变更报告","artifacts":[{"name":"报告","path":"report.md"}]}
```

`waiting` 表示等待人工回复；发送 `{"op":"run.reply","id":"<运行 ID>","text":"补充信息"}` 后，下一次启动的 Runtime 会收到回复。小队首轮可以返回 `delegates` 数组，元素包括 `handler_id` 与 `instruction`。所有小队成员终止后，组长收到 `phase: "summary"` 及成员结果。机器断连导致租约到期时，运行标记为中断，可核实产物后执行 `run.retry`。机器重新连接会重送本地持久化的回报，服务端按租约校验重复回报。

## 文档与 MCP 工具

文档工具记录的字段形如 `{"type":"document","document":"<文档 ID>"}`，本地 MCP 工具的字段形如 `{"type":"mcp"}`。Agent 的 `tools` 字段存放授权工具 ID。文档调用按当前用户的文档权限取正文。MCP 调用还要求本机配置中的 `approved_tools` 有相同 ID，程序路径、固定参数与 MCP 工具名从本机配置读取。

Runtime 提交 `{"tool_calls":[{"tool_id":"reference-search","arguments":{"query":"部署"}}]}` 后，Worker 校验当前租约、快照授权和最新 Agent 授权，把实际工具 ID 和修订号记入运行记录。工具响应随 `fields.tool_results` 交给同一工作目录中下一轮的 Runtime。单次运行支持最多四轮，每轮最多八次调用。命令 `tool-call --config <文件> --agent <ID> --tool <ID> --arguments-file <JSON 文件>` 可由已登录用户试调。支持通过标准输入输出初始化服务器并调用已配置的工具。[MCP 标准传输协议](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)

## 自动化与外部同步

空间规则使用 `handler_id`、`handler_kind`、`instruction`、`enabled`、`notify` 和 `triggers`。`notify` 是结束时接收通知的成员 ID 数组。每个触发器可以单独启停。类型为 `status` 时可指定具体状态，`comment` 可指定 `keyword`，`webhook` 接受绑定该规则的回调。

定时触发器均使用 `type:"schedule"`。日历计划按 IANA 时区计算，`weekdays` 中 1 代表周一，7 代表周日。以下三个 JSON 对象各自可作为规则的一个触发器。

```json
[
  {"type":"schedule","interval_minutes":120},
  {"type":"schedule","frequency":"daily","local_time":"17:45","timezone":"Asia/Shanghai"},
  {"type":"schedule","frequency":"weekly","weekdays":[1,4],"local_time":"09:15","timezone":"America/Los_Angeles"}
]
```

运行中的 Worker 约每 20 秒扫描到期计划。游标保存在工作区，重新连接后每次为每个触发器补算最多 64 个错过的时间点。夏令时跳过指定时刻时选当天随后第一个有效分钟，时钟回拨造成同一时刻出现两次时选较早的那次。规则记录每次派发的来源和计划执行时间。[授权回调及 CLI 规则示例](../../docs/cli.md)

GitHub 连接器的字段为 `{"provider":"github","repository":"OWNER/REPO","enabled":true}`。用具有空间管理权的 ShipForge 身份运行以下同步命令。在团队模式可为这次操作单独使用管理员的 `write` 范围令牌。可拉取单个 Issue 或分页拉取仓库的全部 Issue，拉取时跳过 Pull Request。显式回写支持已映射 Issue 的标题、正文与打开或关闭状态，写前核对上次同步的远端修订。读取私有库及远端写入还需在执行命令的进程环境中提供具备相应仓库权限的 `GITHUB_TOKEN`。[GitHub Issues REST API](https://docs.github.com/en/rest/issues/issues)

```powershell
.\shipforge-worker.exe github-sync --config D:\ShipForge\machine.json --connector github-team --issue 42
.\shipforge-worker.exe github-sync --config D:\ShipForge\machine.json --connector github-team
.\shipforge-worker.exe github-push --config D:\ShipForge\machine.json --connector github-team --task <任务ID>
```

外部数据通过 `connector.ingest` 传入 `id`（连接器）、`external_id`、`event_id`、`source_version`、`title`、`description` 和 `closed`。修订号应使用可按字典序比较的同格式 UTC 时间或补零编号。系统按连接器和外部 ID 映射任务，逐字段合并独立变更。同一字段在两端同时修改时返回 `action: "conflict"`，可由管理员明确指定 `resolve: "source"` 或 `"local"`。命中空间分拣来源与类型要求的外部任务会进入分拣台。GitLab 等十一种专用输入由服务端和 CLI 的 `provider.ingest` 处理。[入站供应商对照](../shipforge-core/PROVIDERS.md)

在已配置的 Runtime 对象中，`slash_commands` 可列出该程序实际支持的命令，例如 `["/plan", "/help"]`。工作机会随 `machine.register` 上报该列表。缺省表示机器尚未报告，空数组表示已报告且没有内置命令。工作区只向有权使用 Agent 或小队的成员显示适用条目。[对话指令](../../docs/conversation-commands.md)
