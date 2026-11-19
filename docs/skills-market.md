# 技能市场与小队共享目录

2026 年 11 月 19 日使用 Windows CLI 实际从 GitHub 的 v0.10.0 标签和 Gist 的固定修订导入两个公开技能，重新读取本地记录核对名称和指令。两次导入使用临时工作区，未绑定 Agent 或执行远端内容。[ShipForge 技能样本](https://github.com/Hubujiu/ShipForge/blob/v0.10.0/skills/shipforge/SKILL.md) · [Gist 样本](https://gist.github.com/aulneau/ace152742d7d85de9348da604887abb4/f0323e27e81cfad1945ed8c1444983689dfc8039)

## 从 Windows 安装、更新和发布

打开「市场」，选择「技能」或「Agent 模板」。「搜索市场」支持分类和关键词，「查看详情」显示说明及包内指令。安装技能时可同时选择要绑定的 Agent。安装状态按空间共享。模板的「从模板创建 Agent」可重复操作，每次生成独立成员；模板发布的新版本供下一次创建选用。

「发布市场版本」支持单一技能、最多 16 个技能组成的合集或一个 Agent 模板。选择当前空间或整个组织作为发布范围。再次发布时，以原条目的「发布新版本」作为入口。发布人需要管理收录的源记录。同一发布条目有固定范围和来源空间。模板复制职责、Runtime 名称、模型和并发等设置；原 Agent 绑定的技能会复制到每个新实例。本机环境变量、机器、工具授权由新 Agent 的管理者设置。

```json
{"op":"catalog.publish","space":"design","id":"report-kit","kind":"skill","sources":["review","summary"],"title":"评审和摘要","description":"团队交付复核","category":"engineering","audience":"organization"}
{"op":"catalog.list","space":"delivery","kind":"skill","category":"engineering","query":"摘要"}
{"op":"catalog.detail","space":"delivery","id":"report-kit"}
{"op":"catalog.install","space":"delivery","id":"report-kit","agent":"reviewer"}
{"op":"catalog.publish","space":"design","id":"report-kit","kind":"skill","sources":["review","summary"],"title":"评审和摘要","category":"engineering","expected_version":1}
{"op":"catalog.update","space":"delivery","id":"report-kit"}
{"op":"catalog.uninstall","space":"delivery","id":"report-kit"}
{"op":"catalog.instantiate","id":"documentation-agent","instance_title":"交付文档助手"}
```

更新保留已有技能 ID 和 Agent 绑定，包中移除的技能会从相应 Agent 上解绑。已编辑的空间副本遇到升级冲突时，先检查本地内容和新版本。确实要替换时，以 `force:true` 重发 `catalog.update`。卸载会移除空间副本和绑定。已排队或完成的运行继续保留入队当时的技能版本、正文及辅助文件。

## 导入自己的技能

「技能」页支持手动创建、选择文件、选择文件夹、从 URL 导入、从机器发现以及绑定空间技能，亦可前往「空间连接器」。文件导入支持 UTF-8 的 `SKILL.md`、`.skill` 和 ZIP。项目可包含模板或其他文本。技能的 Markdown 标题或 frontmatter 的 `name` 提供名称。每项最多 96 个文件、512 KiB，其中任一文件最多 192 KiB。单次导入最多 16 项，客户端读取的合集最多 2 MiB。隐藏路径、敏感文件名、链接、重复名称、二进制内容以及不一致的 ZIP 会导致整项导入失败。

```json
{"op":"skill.import","path":"D:/Team/Skills/report"}
{"op":"skill.import","path":"D:/Downloads/reports.zip","agent":"reviewer"}
{"op":"skill.import","url":"https://github.com/TEAM/REPOSITORY/blob/main/skills/report/SKILL.md"}
{"op":"skill.bind","id":"已导入技能的 ID","agent":"reviewer","enabled":true}
```

URL 示例中的组织、仓库与文件路径需替换为公开文件的实际地址。客户端支持公开 GitHub 文件及 Gist raw 地址，读取后向服务端提交内容。直接调用团队 HTTP API 可传 `files:[{"path":"SKILL.md","text":"# 报告\n写报告"}]`，也可用 `packages` 一次提交多个文件树。导入生成独立副本。已存在同名技能时，带 Agent 的请求返回 `linked` 或 `exists`，保留空间副本的内容；未指定 Agent 时返回 `skipped`。

## 从机器和项目发现

工作机首次运行后可按已注册的机器与 Runtime 排队扫描。机器停机时请求保存在团队服务或本地数据库；工作机下次启动或心跳时处理。可在机器配置的 `skill_roots` 中按 Runtime 列出允许发现的绝对目录。已绑定的 `workspace` 和 `profiles` 支持项目下的 `.agent/skills`、`.agents/skills`、`.claude/skills`、`.codex/skills`。共享机器可将这些目录中检测到的技能提供给有使用权的空间成员。

```json
{"op":"skill.discover","machine_id":"studio-box","runtime":"review"}
{"op":"skill.sources","machine_id":"studio-box","runtime":"review","query":"评审"}
{"op":"skill.machine_import","machine_id":"studio-box","runtime":"review","source_id":"上一步返回的 ID","agent":"reviewer"}
```

在「从机器发现技能」中可请求扫描，也可查看扫描结果并选择一项导入。「查看导入进度」或 `skill.sources` 可查询本人最近的请求、导入结果或失败原因。工作机用自己的 `machine_key` 处理相应机器在当前空间的 `skill.jobs`，以 `skill.finish` 回报结果。服务端应用结果前会复核发起成员的权限。运行时的 `fields.config.skill_versions` 含已绑定技能的快照。工作机还会将这些快照写入本次执行专用的临时目录，外部程序可通过 `fields.skill_directories` 的 `entry_file` 和 `directory` 读取指令、模板及辅助文件。任务结束后工作机会回收临时技能目录。

## 共用同一个任务工作树

小队管理者可在小队详情开启「同机共享任务目录」，也可在新建和编辑小队时设置。开关默认关闭。在已开启的小队下，同一空间、任务、机器及批准的源目录使用同一份任务工作目录。不同任务、小队、机器和源仓库各有自己的目录。私有会话与私有发起人也有各自的目录。委派和汇总沿用队长入队时的设置。

```json
{"op":"update","kind":"crew","id":"delivery-team","fields":{"share_task_directory":true}}
```

源仓库已绑定时，工作机首次执行会创建独立 Git worktree，后续成员能读到队长及其他成员的本地修改。成员可并行运行；需要顺序改同一文件时由队长安排委派。执行完成后，可通过「运行历史」的工作目录产物找到保留的修改。 [HiFox 技能规则](https://help.hifox.com/docs/agent-and-computer/skills) · [HiFox 小队目录规则](https://help.hifox.com/docs/agent-and-computer/crews)
