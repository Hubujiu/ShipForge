# 工作机存储、模型与运行参数

## 查看工作目录占用

打开「机器」，选择自己管理的工作机，再打开「管理存储空间」。首次使用时选择「重新统计」。工作机在后台计算数据目录的实际字节数、工作副本的文件数量与内容指纹。表中显示最近一次成功统计的结果，离线时可以查阅。机器配置绑定的原始仓库和外部文件不计入工作副本容量。[HiFox 变更记录](https://help.hifox.com/changelog)

选择一个可清理的工作副本，检查所属运行或任务和大小，再按「确认清理」。这会删除该副本中的本地更改。已登记且全部成功结束的一次性运行、所属任务已经完结的隔离副本可以清理。仍可继续的对话、开放任务、运行中的进程、待恢复的运行、未上报的结果、Git 锁定目录、包含链接或其他仓库的目录以及无法确认归属的旧副本均有保护原因。升级前的工作副本可以通过历史运行产物识别。

执行前工作机会再次读取内容并比对指纹，服务端同时复核权限、运行租约和任务状态。确认后再次编辑文件会产生冲突，可以重新统计并重新选择。执行端将通过检查的单个目录移至同一状态目录下的隔离位置，再移除文件和对应的 Git worktree 记录。文件暂时被操作系统占用时，剩余字节记入 `pending_reclaim_bytes`；下次重新统计会再次尝试释放已确认的目录。

将以下 JSON 分别传给安装包内的 `shipforge exec`。用返回的真实标识和 64 位十六进制指纹替换示例占位符。工作机所有者可以发起统计和清理。相同输入重试可沿用 `request_id`。

```json
{"op":"machine.storage.request","id":"my-machine","action":"scan","request_id":"scan-01"}
{"op":"machine.storage.preview","id":"my-machine"}
{"op":"machine.storage.request","id":"my-machine","action":"clean","workspace":"<工作副本标识>","expected_snapshot":"<本次预览指纹>","confirm":true,"request_id":"cleanup-01"}
```

预览返回 `entries`、`total_bytes`、`other_bytes`、`pending_reclaim_bytes`、`scanned_at` 和最近的 `jobs`。每次最多列出 2,048 个副本，超出数量计入 `omitted`。返回项可以用 `title`、`category`、`bytes`、`files` 和 `protection` 判断用途。受托执行端用 `machine.storage.take` 获取工作，用 `machine.storage.reserve` 在移动目录前复核快照，然后用 `machine.storage.finish` 提交结果。存储预览使用不透明标识。历史运行中已保存的本机路径供工作机识别旧副本。

## 为 Agent 配置模型和进程参数

从「Agents」进入 Agent 详情，打开「模型与运行参数」。面板按机器展示所用 Runtime 公布的模型和模型支持的思考深度。可以跟随默认模型或选择明确的模型和深度。机器启动参数中固定了模型时，面板显示固定值。Agent 编辑器还可填写参数数组和环境变量对象，环境值默认遮蔽。自定义参数中的每个值按一个进程参数传递，含空格的值保持完整。[HiFox Agent 设置](https://help.hifox.com/docs/agent-and-computer/agents)

机器所有者在机器配置文件的 `runtime_launch` 下为已配置的 JSON 适配器声明可用选项，重启工作机后生效。例如，下面的 `fixture` 适配器接收模型、思考深度和输出样式，子进程可以读取 `OUTPUT_STYLE`。

```json
{
  "runtime_launch": {
    "fixture": {
      "models": {"compact": ["low", "high"], "analysis": ["medium", "high"]},
      "default_model": "compact",
      "model_flag": "--model",
      "effort_flag": "--effort",
      "allowed_arguments": {"--format": ["short", "detailed report"]},
      "default_arguments": {"--format": "short"},
      "environment": {"OUTPUT_STYLE": "plain"},
      "allowed_environment": ["OUTPUT_STYLE"]
    }
  }
}
```

`agent.runtime.options` 按可见机器返回该 Agent 的可用设置。`machine.runtimes` 中每个工具的 `launch` 对象只公布可选模型、参数和允许覆盖的环境键。机器本地环境值只交给指定适配器。凭据可由机器端原有的 `runtimes.<名称>.inherit_env` 指定允许传入的变量。Agent 在共享配置中填写的普通环境值遵循该 Agent 的可见性，分身继续遵循原 Agent 的环境继承开关。

一项运行优先使用临时模型，其次使用 Agent 选择，再使用机器默认模型。操作示例如下。机器端固定的模型优先生效。OpenClaw 由执行适配器读取模型选择。选择不同临时模型并省略思考深度时，使用新模型自身的默认值。没有声明启动参数映射的旧适配器继续从运行 JSON 获取配置。

```json
{"op":"agent.runtime.options","id":"my-agent"}
{"op":"update","kind":"agent","id":"my-agent","fields":{"model":"analysis","reasoning_effort":"medium","arguments":["--format","detailed report"],"environment":{"OUTPUT_STYLE":"color"}}}
{"op":"run.enqueue","handler_id":"my-agent","instruction":"检查当前需求","model":"compact","reasoning_effort":"low"}
```
