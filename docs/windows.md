# Windows 使用指南

## 安装和第一条任务

1. 从 GitHub Release 下载 `shipforge-0.6.0-windows-x64-setup.exe`，运行后在开始菜单打开 ShipForge。也可以将同一版本 ZIP 解压到可写目录，双击 `shipforge-desktop.exe`。[发布页](https://github.com/Hubujiu/ShipForge/releases)
2. 在「空间任务」点击「新建任务」，填写标题并保存。点击任务打开详情，可调整状态、负责人、描述，或添加评论。从列表切回看板能看到同一条任务。
3. 在程序目录打开 PowerShell，执行下面的命令。在同一个 Windows 用户下，桌面与 CLI 默认共用工作区。使用 `--data-dir` 可以另建独立的本地工作区。

```powershell
.\shipforge.exe list task
.\shipforge-desktop.exe --demo
```

演示窗口使用内存数据，关闭后丢弃。程序文件与项目数据库分开存放。安装新版时可选择原程序目录。安装流程保留原工作区、用户自建文件及指定安装目录。核对下载文件时，在 PowerShell 运行 `Get-FileHash -Algorithm SHA256 文件名`，与 Release 中同名 `.sha256` 文件的值比较。

## 两台设备共享一个空间

在服务机的程序目录打开 PowerShell，指定一个团队专用数据目录。首条命令返回供首位管理员使用的令牌。第二条命令启动服务，默认监听本机回环端口。

```powershell
.\shipforge-server.exe --data-dir D:\ShipForgeTeam token issue --actor owner --scope write --label initial-admin
.\shipforge-server.exe --data-dir D:\ShipForgeTeam serve --listen 127.0.0.1:47138
```

在同机桌面的「连接设置」输入 `http://127.0.0.1:47138` 和令牌。跨设备时，在服务器前配置 HTTPS 反向代理，再在成员设备输入对应的 HTTPS 地址。每位成员使用各自的身份和令牌。Windows 用 DPAPI 保存本机连接凭据。服务机保留工作区和令牌摘要。用安装所有者 `owner` 的写令牌连接团队后，可在 PowerShell 中建立只读成员，再把第二条命令返回的令牌单独交给该成员。

```powershell
'{"op":"create","kind":"member","id":"alex","title":"Alex","fields":{"actor":"alex","role":"viewer"}}' | .\shipforge.exe request
'{"op":"access.issue","actor":"alex","scope":"read","label":"Alex Windows"}' | .\shipforge.exe request
```

CLI 也可从标准输入接收令牌，例如 `$teamToken | .\shipforge.exe connect https://team.example.org --token-stdin`。使用 `.\shipforge.exe sync status` 检查待发送数，使用 `.\shipforge.exe sync flush` 提交离线草稿。桌面在连接恢复后会尝试发送可同步的草稿。若有修订冲突，在连接设置或 `sync pending` 中查看草稿，再读取最新记录，选择重新提交或舍弃。[命令与离线草稿](cli.md)

## 接入第一台执行机器

以下命令在你控制的机器上建立本地机器配置，并启用随包附带的示例程序。Worker 会为这次运行创建独立空目录。示例程序读取任务上下文并生成运行结果。

```powershell
$config = "$env:LOCALAPPDATA\ShipForgeExample\machine.json"
.\shipforge-worker.exe init --config $config --example
.\shipforge-worker.exe run --config $config
```

让上面的进程保持运行。在同一空间的机器列表中确认已登记的机器，然后建立使用 `inspect-context` Runtime 的 Agent，把处于待办的任务指派给它。在运行历史查看返回结果。运行自有工具时，在机器配置的 `runtimes` 中填写固定程序、参数、超时和允许传给子进程的环境变量。使用 Git 工作树隔离时选择本机已批准的仓库。直接目录执行需在机器配置中显式开启。

团队执行端需连接同一服务。为执行身份分配目标空间的成员权限，并单独签发 `worker` 令牌。在执行端的 PowerShell 中设置 `$env:SHIPFORGE_SERVER` 和 `$env:SHIPFORGE_TOKEN` 后再启动 Worker。服务机、桌面与执行机可以分别运行。通过 `.\shipforge-worker.exe --help` 查看探测、同步和停止命令。

## 给规则安排固定时间

打开空间的「自动化」，新建规则，填写名称、执行指令并选择 Agent 或小队。在触发器中选定时，可按分钟间隔、每天或每周执行。每日和每周计划都可指定当地时间与时区，例如上海时间 09:30。每周计划还可选择星期。保存启用的规则后，让此空间至少有一台 Worker 正在运行以扫描到期计划。每次运行都能从空间的运行历史追踪。[自动化 JSON 示例](cli.md)

## 上传和保存附件

在程序目录运行 `.\shipforge.exe upload .\report.pdf` 可创建一份带附件的私人文档。`.\shipforge.exe upload .\report.pdf --kind task --id TASK_ID` 把文件添加到已有任务。命令返回的 `fields.attachments` 含每个附件的 ID、文件名、字节数和 SHA-256。把真实 ID 代入 `.\shipforge.exe download task TASK_ID ATTACHMENT_ID .\report-copy.pdf`，即可将校验后的文件保存到新路径。单个附件可达 4 MiB。

## 外部系统

在「外部应用集成」建立连接器，指定平台与来源范围。GitLab、Jira、Linear、Asana 支持工单输入。飞书、Slack、Telegram、Microsoft Teams、Discord、企业微信、钉钉支持相应的用户文字消息。可用 `provider.preview` 查看样例映射，再用 `provider.ingest` 录入。与实际租户连接时，按平台要求在受信中继上完成鉴权、解密、事件订阅或补取详情。

服务端的入站地址为 `POST /v1/integrations/{space}/{connector}`，可用绑定到该空间和连接器的 hook 令牌。GitLab Issue Webhook 可以将该令牌作为 Secret token。GitHub Issues 使用机器端的 `github-sync` 拉取，以及 `github-push` 回写已经映射的单条 Issue。读取私有仓库和写入远端需给执行命令的用户配置适当的 GitHub 仓库权限。

机器手册列出 GitHub、MCP、本机运行目录及小队回报的参数。入站连接器手册说明每个平台的载荷、重复事件标识和中继前提。[执行机器手册](../crates/shipforge-worker/README.md) · [入站连接器和样例](../crates/shipforge-core/PROVIDERS.md)

## 备份及恢复

关闭所有使用该目录的桌面、服务、CLI 和执行端进程，再复制整个数据目录，包括 SQLite 主文件和旁侧文件。团队服务器备份中也含令牌摘要与附件原始数据。恢复到独立目录后，用桌面或服务的 `--data-dir` 打开并查看已知任务。另有空间记录的 JSON 迁移方式，使用 `export`、`import.preview` 和 `import`。JSON 导出保留附件元数据，完整附件随数据库备份恢复。[CLI 手册](cli.md)
