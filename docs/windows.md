# Windows 使用指南

## 安装和第一条任务

1. 从 GitHub Release 下载 `shipforge-0.6.0-windows-x64-setup.exe`，运行后在开始菜单打开 ShipForge。也可以将同一版本 ZIP 解压到可写目录，双击 `shipforge-desktop.exe`。[发布页](https://github.com/Hubujiu/ShipForge/releases)
2. 在「空间任务」点击「新建任务」，填写标题并保存。点击任务打开详情，可调整状态、负责人、描述，或添加评论。从列表切回看板能看到同一条任务。
3. 在程序目录打开 PowerShell，执行下面的命令。在同一个 Windows 用户下，桌面与 CLI 默认共用工作区。使用 `--data-dir` 可以另建独立的本地工作区。

```powershell
.\shipforge.exe list task
.\shipforge-desktop.exe --demo
```

演示窗口使用内存数据，关闭后丢弃。程序文件与项目数据库分开存放。更新时运行新版本安装程序；覆盖安装的验证包括保留原工作区、用户自建文件及指定安装目录。核对下载文件时，在 PowerShell 运行 `Get-FileHash -Algorithm SHA256 文件名`，与 Release 中同名 `.sha256` 文件的值比较。

## 两台设备共享一个空间

在服务机的程序目录打开 PowerShell，指定一个团队专用数据目录。首条命令返回供首位管理员使用的令牌。第二条命令启动服务，默认监听本机回环端口。

```powershell
.\shipforge-server.exe --data-dir D:\ShipForgeTeam token issue --actor owner --scope write --label initial-admin
.\shipforge-server.exe --data-dir D:\ShipForgeTeam serve --listen 127.0.0.1:47138
```

在同机桌面的团队连接表单输入 `http://127.0.0.1:47138` 和令牌。跨设备时，在服务器前配置 HTTPS 反向代理，再在成员设备输入对应的 HTTPS 地址。每位成员使用各自的身份和令牌。Windows 用 DPAPI 保存本机连接凭据。服务机保留工作区与令牌摘要。管理员可以建立只读成员，例如在已连接团队的 PowerShell 中执行下列命令，再将第二条命令返回的令牌单独交给该成员。

```powershell
'{"op":"create","kind":"member","id":"alex","title":"Alex","fields":{"actor":"alex","role":"viewer"}}' | .\shipforge.exe request
'{"op":"access.issue","actor":"alex","scope":"read","label":"Alex Windows"}' | .\shipforge.exe request
```

可在 CLI 以标准输入传递令牌，例如 `$teamToken | .\shipforge.exe connect https://team.example.org --token-stdin`。使用 `shipforge sync status` 检查待发送数，使用 `shipforge sync flush` 提交离线草稿。并发修改返回当前版本冲突，可先读取最新记录再决定如何提交。[命令与离线草稿](cli.md)

## 接入第一台执行机器

在你控制的机器上先选择一个可用于执行任务的工作区。下例会创建独立的空目录和本地机器配置，并只启用随包附带的示例程序。示例程序读取任务上下文并生成运行结果。

```powershell
$work = New-Item -ItemType Directory -Force "$env:LOCALAPPDATA\ShipForgeExample\work"
$config = "$env:LOCALAPPDATA\ShipForgeExample\machine.json"
.\shipforge-worker.exe init --config $config --workspace $work.FullName --example
.\shipforge-worker.exe run --config $config
```

让上面的进程保持运行。在同一空间的机器列表中确认已登记的机器，然后建立使用 `inspect-context` Runtime 的 Agent，把处于待办的任务指派给它。可以在运行历史查看返回结果。运行自有工具时，在机器配置的 `runtimes` 中填写固定程序、参数、超时和允许传给子进程的环境变量。使用仓库隔离时选择本机已批准的 Git 路径；直接目录执行需在机器配置中显式开启。

团队部署中执行端也需连接同一服务，使用为该执行身份单独签发的 `worker` 令牌。服务机、桌面与执行机可以分别运行。通过 `shipforge-worker --help` 查看已安装版本提供的探测、同步和停止命令。

## 上传和保存附件

在命令行中，`shipforge upload .\report.pdf` 创建一份带附件的私人文档。`shipforge upload .\report.pdf --kind task --id TASK_ID` 把文件添加到已有任务。命令返回的 `fields.attachments` 含每个附件的 ID、文件名、字节数和 SHA-256。`shipforge download task TASK_ID ATTACHMENT_ID .\report-copy.pdf` 把附件校验后保存到新文件。单个附件可达 4 MiB。

## 外部系统

创建带有 `fields.provider` 的连接器后，可用 `provider.preview` 检查传入的 GitLab、Jira、飞书、Slack 或 Telegram 事件如何映射为任务，再用 `provider.ingest` 应用。服务端支持 `POST /v1/integrations/{space}/{connector}`，使用绑定到该空间和连接器的 hook 令牌。GitLab Issue Webhook 可以把该令牌作为 Secret token，其他平台可通过自己的受信中继转发事件。数据格式、幂等要求及冲突处理见 CLI 手册。GitHub Issues 拉取与显式回写使用执行端中的 GitHub 命令。

执行端如何配置 GitHub 双向同步、MCP、本机运行目录和小队回报可查阅机器手册；各供应商的事件标识与身份边界见入站连接器文档。[执行机器手册](../crates/shipforge-worker/README.md) · [入站连接器和样例](../crates/shipforge-core/PROVIDERS.md)

## 备份及恢复

关闭本地客户端，或停止使用相应数据目录的团队服务与执行端，然后复制整个数据目录，包含 SQLite 主文件和旁侧文件。恢复到独立目录后，先通过 `--data-dir` 打开并查看已知任务。团队服务目录同时含令牌摘要，按成员数据同等保护。可使用 CLI 的 `export` 和 `import.preview` 对空间数据做可审阅的迁移。[CLI 手册](cli.md)
