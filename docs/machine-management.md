# Windows 本机服务与运行时

机器详情的「管理存储空间」可统计和清理已完成的工作副本。Agent 详情可选择机器公布的模型、思考深度和运行参数。[存储及运行参数](storage-and-models.md)

在「机器」启动一次工作机，记下所用配置文件。打开「本机服务设置」，选择该文件并读取配置。服务状态显示机器名称、启动方式、后台运行和托盘设置。设置只影响登录的 Windows 用户和选中的 ShipForge 数据目录。[HiFox 机器帮助](https://help.hifox.com/docs/agent-and-computer/computers) · [HiFox CLI 帮助](https://help.hifox.com/docs/cli/cli)

| 启动方式 | 生效时机 |
| --- | --- |
| 手动启动 | 从桌面或本机命令启动。 |
| 打开桌面时启动 | 同一数据目录下的桌面下次打开时启动已登记的工作机。本次手动停止后可从机器页面再次启动。 |
| 登录 Windows 时启动 | 在当前用户的启动文件夹创建可辨识的 ShipForge 快捷方式。下次登录时启动工作机。选择别的模式会移除这个快捷方式。 |

默认关闭主窗口会收起到系统托盘。点击托盘图标可以回到窗口，右键可以选择退出。关闭「关闭窗口时收起到托盘」后，关窗会进入退出确认。退出确认与服务设置共用「退出桌面后继续运行工作机」。保留后台时仍可接单，取消该选项会停止此数据目录中登记的工作机并中断在执行的任务。手动「重新启动工作机」同样会中断在执行的任务。选择登录启动需要完整安装包内的桌面程序与工作机程序。[Windows 当前用户启动文件夹说明](https://learn.microsoft.com/en-us/windows/win32/shell/knownfolderid)

从安装目录的 PowerShell 运行以下示例。将配置和数据目录改成自己的实际路径。查询会返回运行状态、服务 PID 和保存的策略。

```powershell
.\shipforge-worker.exe service status --config "D:\ShipForge\machine.json" --directory "D:\ShipForge\data"
.\shipforge-worker.exe service configure --config "D:\ShipForge\machine.json" --directory "D:\ShipForge\data" --mode desktop --keep-background true --close-to-tray true
.\shipforge-worker.exe service start --config "D:\ShipForge\machine.json" --directory "D:\ShipForge\data"
.\shipforge-worker.exe service restart --config "D:\ShipForge\machine.json" --directory "D:\ShipForge\data"
.\shipforge-worker.exe service stop --config "D:\ShipForge\machine.json" --directory "D:\ShipForge\data"
```

旧的 `shipforge-worker stop --config FILE` 允许当前工作完成后退出。`service stop` 会请求立即停止并等待该监督进程释放锁。本机「进程与日志」提供执行诊断，监督进程的启动错误写入配置文件旁的 `*.worker.log`。

## 每台机器的运行时能力

打开一台机器的详情，选择「运行时能力」。工作机会在启动时发现本机 Claude Code、Codex、Gemini CLI、OpenCode 以及 Git、Node、npm、Python、uv 和 Bun。机器配置中指定的适配器也会出现。首次发现展示程序存在情况。「重新检测」会单独执行版本命令。检测失败后可再次检测。机器离线时保留最近的版本和结果。

机器所有者可以在已连接的工作机上为一个工具发起一项操作。可安装的工具会显示「安装运行时」，安装后显示受支持的「升级运行时」。Claude Code 在 Windows 上使用本机 WinGet 的固定软件包 `Anthropic.ClaudeCode`。其他运行时可以通过详情中的供应商指南安装、登录或配置，也可以由机器所有者在本机配置中指定安装和升级程序。发布到团队服务的内容包括名称、状态、版本和结果代码，本机路径、输出和登录材料保留在本地。[Claude Code 安装说明](https://code.claude.com/docs/en/setup) · [Gemini CLI 安装说明](https://geminicli.com/docs/get-started/installation/) · [Codex CLI](https://developers.openai.com/codex/cli/) · [OpenCode](https://opencode.ai/docs/)

「测试模型连接」支持 Claude Code 和接受 ShipForge JSON 输入的已配置适配器。测试发送随机短句，要求模型完整回传，并在独立的临时目录运行。详情会区分成功、登录问题、程序失败、超时和模型回复不匹配。测试使用机器当前账户为该工具配置的模型连接，供应商可按实际请求计费。

以下是传给 `shipforge exec` 的 JSON 请求。`machine.runtimes` 返回 `runtimes`、最近操作 `recent` 和当前访问者的 `can_manage`。同一个 `request_id` 可以重试，返回同一项操作。任务期间刷新详情可见进度。

```json
{"op":"machine.runtimes","space":"default","id":"my-machine"}
{"op":"machine.runtime.request","space":"default","id":"my-machine","runtime":"claude","action":"rescan","request_id":"refresh-claude-1"}
```

高级机器配置可添加 `runtime_management.<name>`。可选的 `executable` 指向该工具的本机文件。`probe`、`install`、`upgrade` 各接受固定 `program`、`args` 和 `timeout_seconds`。`probe` 输出可辨识的版本号，安装和升级后会自动再检测。已有的 `runtimes.<name>` 负责实际 JSON 执行。本机配置属于机器所有者。工作区成员只可选择已发布的操作名。修改固定程序后，重新启动工作机。

受托工作机使用 `machine.runtime.publish` 发布能力，使用 `machine.runtime.jobs` 领取有期限的租约，并使用 `machine.runtime.finish` 汇报结果。三者都需机器凭据。显式停机时 `machine.stopped` 更新在线状态。
