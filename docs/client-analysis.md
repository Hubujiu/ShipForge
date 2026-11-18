# HiFox 0.4.538 桌面客户端分析

本次使用 [morluto/rea](https://github.com/morluto/rea) 的 `rea-agents 6.9.0`，对 Windows 安装版的两个 ASAR 做静态分析，并在 Linux x64 上调用 REA 的源码恢复提供器。安装包回答“桌面如何连接本机能力”，界面观察回答“当前账号实际能看到什么”。团队存储、调度和部署的设计依据另见[架构](architecture.md)。[REA 固定版本](https://github.com/morluto/rea/tree/rea-agents-6.9.0)

## 样本和覆盖面

| 对象 | 发布物里的内容 | 样本 SHA-256 |
| --- | --- | --- |
| `resources/app.asar` | 两个文件。包元数据声明版本 0.4.538，入口是 `dist-entry/entry.js`。 | `e888ac7837414fed57fea8524489058c0e86803bdfd2756308b0b522b627b8a6` |
| `resources/HiFox.asar` | 产品包包含主进程加载器、V8 缓存、preload、HTML、JS、CSS 及字体等 21 个条目。 | `db2e557c4ef7107b5db5bb83d56658978118780f4ea612ae790296ad52b4cbc2` |
| `main/index.jsc` | 产品包中的 V8 缓存，2,984,000 字节；相邻的可读入口将其交给字节码加载器。 | `699a4ffc56b63c0fcf5af0b9ecc7a8ee5a60a7fab1917777900a532686ab5518` |

本地清单逐项登记两个归档合计 23 个文件的路径、字节数和哈希，另存 11 份可读文本。归档内未发现随产品发布的 source map。另有独立的 `resources/resources/bin/hifox.exe`；机器详情展示 CLI 常驻服务与 Runtime 探测状态。可执行文件在本次分析中只用于辨认分发形态。

| REA 工作 | 已验证的结果 | 可追溯的产物 |
| --- | --- | --- |
| 启动包静态分析 | 1 个 JS 完成解析，8,190 个 AST 节点，30 条观察，0 项解析失败。 | `rea/bootstrap-evidence.json` |
| 产品包定向分析 | 明确选择 `main/index.js`、`preload/index.js`、渲染主入口。3 个 JS 完成解析、42,253 个 AST 节点、261 条观察，0 项解析失败。 | `rea/client-evidence.json` |
| 产品包扩大范围 | 去掉路径筛选后，5 个 JS 完成解析，195,131 个 AST 节点、1,584 条观察。React 库的静态分析在 1 GiB 及 3 GiB 堆上限下都跳过了 1 个文件。各次报告均未发现归档完整性矛盾。 | `rea/full-client-evidence.json`、`rea/expanded-client-evidence.json` |
| 可读模块恢复 | 4 个输入为渲染主入口、另一个 JS 分块、React 分块及 preload。REA 识别 Webpack 5，Wakaru 1.14.0 在 structural / standard 模式写出 232 个 JS 模块和 232 张派生映射，报告 0 个模块失败、2 个 CommonJS 导出改写告警。 | `client/recovered/manifest.json`、`modules/`、`reports/provenance.json`、`rea/recovery-evidence.json` |

静态分析的“观察条数”是工具输出。模块恢复产物含第三方库。REA 将运行时等价性记为 `unknown`，派生映射只描述恢复产物与发布包的对应关系。能交付的是发布版可读 JS、重组模块与证据；原工程 TypeScript、服务端源码和数据库结构仍然未知。[REA 静态重建指南](https://github.com/morluto/rea/blob/rea-agents-6.9.0/docs/guides/javascript/javascript-artifact-reconstruction.md) · [REA 恢复指南](https://github.com/morluto/rea/blob/rea-agents-6.9.0/docs/guides/javascript/javascript-recovery.md)

## 从证据到工程决策

| 问题 | 直接观察 | ShipForge 的独立设计 |
| --- | --- | --- |
| 界面在哪里运行？ | 分发包带 Electron preload 与 React 渲染资源。打开已登录桌面后，原生窗口可访问性树把任务、成员、设置等页面的文档源标识为 `app.hifox.com`。客户端同时包含本地资源和线上业务页面。 | GPUI 绘制导航、表单及列表，按对象和路由查询协作服务。缓存策略按对象设计。 |
| 如何选择本地目录？ | 在发布版 JS 中，渲染层检查 `pickDirectory`、`validateLocalDirectory` 能力是否存在；preload 通过 `contextBridge` 暴露接口，并用 IPC 发起相应请求。本次未执行目录变更。 | GPUI 调用本机目录选择器，选定路径后交给 core 校验。远程机器目录由目标机器确认。 |
| Runtime 状态如何更新？ | 渲染层存在 `rescanRuntimes` 调用；preload 提供重新扫描和状态变化订阅，含取消订阅路径。实机机器详情将本地服务状态和各 Runtime 探测结果分别列出。 | worker 发布带机器、版本和时间的能力快照，GPUI 订阅状态；切机或离页释放订阅。 |
| 通知如何连接桌面？ | 发布 JS 中可定位到 `showNotification` 的调用。实机收件箱呈现重要性与处理状态。桌面提醒的调用证据不确定服务器的分组实现。 | 服务端持久化收件人状态，桌面用事件标识去重系统通知。 |
| Agent 的工作在哪里发生？ | 本机服务与 Runtime 可见于桌面设置。官方描述工作目录、工作时段、队列和启动外部工具，安装包证据未给出服务端队列代码。 | 将机器服务与 GPUI 分进程，租约、容量、取消和回报写入稳定协议。[机器](https://help.hifox.com/docs/agent-and-computer/computers) · [任务执行](https://help.hifox.com/docs/agent-and-computer/agent-task-execution) |
| 为什么某些页面没有记录？ | 实机分别观察到空列表、空间功能未启用、创建表单缺字段和“即将推出”的导入项。这些是四种可区分的交互状态。 | 用不同状态与可执行动作表达，避免将账号数据、功能开关和产品路线图混为一谈。[界面索引](interfaces.md) |

## 如何在同版本复核

1. 对两个 ASAR 计算 SHA-256，与样本表比对。读取归档索引，逐项计算文件摘要，保留有可读文本的发布文件和本地清单。
2. 使用固定版本的 `rea analyze-javascript-application` 分别分析启动包和产品包。先限定上述三条路径，再去掉路径限制，记录 `statistics`、`integrity_contradictions` 与被跳过文件的原因。将原始证据束与摘要分开保存。
3. 在 Linux x64 的私有工作目录提供 Node 22.23.4、Wakaru 1.14.0 和 `prlimit`。用 `rea recover-javascript-sources` 以渲染主入口为主文件，另用三个 `--additional-path` 指定上述输入。保留输入摘要、模块清单、来源映射和告警，以便独立核验。Wakaru 二进制的 SHA-256 为 `664fa06782a61eda7ed15885134b52525fd565dd77607e642062ceb6cd927c38`。[REA 恢复说明](https://github.com/morluto/rea/blob/rea-agents-6.9.0/docs/guides/javascript/javascript-recovery.md) · [Wakaru 发布版](https://github.com/pionxzh/wakaru/releases/tag/v1.14.0)
4. 对照同一安装版本的界面，把“找到能力探测”“找到实际调用”“界面出现”和“操作已端到端成功”逐项记录。本次取证保留未填写的表单和空态，未向示例空间提交运行、连接器授权或测试数据。

公开仓库存放独立编写的规格、分析结论与官方参考链接。原始安装包派生文件和带账号内容的图片保存在本地研究目录，公开项目的 MIT 许可适用于 ShipForge 自身内容。
