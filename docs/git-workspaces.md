# Git 仓库与提交作者

从组织全局配置、空间设置或 Agent 详情打开「Git 仓库与设置」。组织设置提供提交作者默认值，也可打开本人加入的空间。空间管理者添加本空间使用的仓库。Agent 管理者可为该 Agent 配置仓库和作者。三层设置按字段继承。

## 仓库和工作目录

- 空间和 Agent 可以添加 HTTPS 或 SSH 仓库 URL，填写说明和可选的基线分支。Git 使用工作机当前用户的凭据。填表时把凭据留在工作机的 Git 凭据管理器或 SSH 配置。[Git URL 格式](https://git-scm.com/docs/git-clone#_git_urls)
- Agent 有自有仓库时，可同时启用全部或部分空间仓库。未配置自有仓库时，默认使用空间仓库。选择「全部」会跟随空间之后新增的仓库。分身默认继承本体，单独改动某项后以自己的设置为准。
- 同一 URL 的分支优先级从高到低为本次运行指定分支、Agent 分支、空间分支和远端默认分支。工作机缓存对象，为运行创建独立 worktree。可继续的任务或对话使用已有工作目录。
- Agent 明确指定的本地目录优先于绑定的远端仓库。选择「纯临时目录」时从空目录开始。Git 缓存和隔离工作目录由工作机管理。清理目录前检查运行状态及待保留的改动。

## 提交作者

作者名称支持触发成员、触发成员与 Agent 名称、Agent 所有者、所有者与 Agent 名称、机器 Git 配置或自填。作者邮箱可以选触发成员、所有者、机器 Git 配置或自填。留空表示继承上一层。各层均留空时使用触发成员与 Agent 名称，邮箱取机器配置，缺省时使用平台的 noreply 地址。

使用已有 Git 目录时，优先读取该仓库的本地 `user.name` 和 `user.email`。运行时通过 `GIT_AUTHOR_NAME` 与 `GIT_AUTHOR_EMAIL` 获得作者。Git 提交者和签名遵循机器 Git 配置。Co-authored-by 默认开启，可按层关闭。运行时临时追加尾注并保留已有 Git 钩子。

## CLI

通过 `git.settings` 读取指定层的 `rev`、当前设置和生效来源。 `git.configure` 需要该修订号和完整的新设置，提交冲突时先重新读取。层级用 `organization`、`space` 或 `agent`，Agent 层需要 `id`。 `settings: null` 清除当前层配置。Agent 的仓库字段缺省时允许继承本体，空数组明确覆盖本体的仓库列表。

```json
{
  "op": "git.configure",
  "level": "agent",
  "space": "dev",
  "id": "builder",
  "expected_rev": 8,
  "settings": {
    "author": {"name": {"mode": "trigger_and_agent"}, "coauthor": true},
    "repositories": [{"url": "ssh://git@example.org/team/project.git", "branch": "develop"}],
    "include_space": {"mode": "all"}
  }
}
```

`run.enqueue` 接受 `checkout_branch`。工作机取得租约时解析作者与仓库。租约下发的已验证个人邮箱仅用于该次执行。 [CLI 手册](cli.md) · [机器配置](windows.md)
