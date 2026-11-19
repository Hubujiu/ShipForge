# Electron 工作区与 PostgreSQL 服务

新版 Windows 客户端使用 Electron、React 和 Webpack。服务通过身份验证后的 HTTP JSON 命令与 WebSocket 提供协作，以 PostgreSQL 17 和 pgvector 保存核心数据。独立工作机运行 Rust 程序，领取已授权的工作，启动本机配置的 Runtime，再把运行状态和结果写回原任务或对话。

## 技术依据

| 范围 | 证据与实现 |
| --- | --- |
| 桌面与前端 | REA 6.9.0 对当前安装包的 `main/index.js` 与 `preload/index.js` 完成定向分析。Electron preload 与 IPC 边界可以定位，渲染库包含 React 19.2.3，先前对同一分发线的模块恢复识别为 Webpack 5。ShipForge 使用 Electron、React 19 与 Webpack 5，自行实现交互。 |
| 本地执行 | 安装包附带独立的 Windows 执行程序，程序包含 Rust 编译器路径标记。官方说明 CLI 与本地服务在受控机器上启动 Runtime。Rust 执行器沿用已有程序，经独立机器凭据与新服务交换运行租约。 |
| 团队服务 | 私有化文档明确列出 PostgreSQL 17、pgvector、HTTP API、WebSocket、Docker Compose 与可选 S3 兼容附件存储。应用使用这些核心边界。TypeScript 与 Node.js 是当前服务端实现选择，官方资料尚未确认 HiFox 私有 API 服务的框架和 ORM。 |

[安装样本与 REA 方法](client-analysis.md) · [HiFox 私有化部署](https://hifox.com/solutions/private-deployment/) · [Agent 如何执行任务](https://help.hifox.com/docs/agent-and-computer/agent-task-execution)

## 首次使用

连接一次自托管服务地址。首次初始化需要设置用户名、至少 10 个字符的密码、组织名称和初始空间。部署设置了初始化密钥时，所有者在创建表单输入该密钥。成员可用一次性邀请码加入。

「开始使用」基于实际业务记录显示完成进度。连接本机时可以配置符合 JSON 协议的本机 Runtime，或者使用内置示例 Runtime 测试任务链路。随后创建 Agent，建任务并指派给它。任务在「待整理」时保留队列前的配置，移到「待处理」后由上线的工作机执行。任务详情显示进度和原对话。Agent 完成一次运行后，用户可以继续对话或单独改变任务状态。 [官方快速上手](https://help.hifox.com/docs/quickstart)

看板、列表、当前空间的个人任务、评论、收件箱、迭代、文档修订和空间导航使用同一协作服务。工作机令牌只可以使用工作机端点。用户退出时，客户端关闭事件订阅。Windows 会话凭据由 Electron 的操作系统凭据保护保存。

## 运行服务与客户端

Docker Compose 需要 `SHIPFORGE_POSTGRES_PASSWORD` 和 `SHIPFORGE_BOOTSTRAP_KEY`。由部署者保存两项随机密钥。在 `platform` 目录运行 `docker compose up -d --build`。默认服务地址为 `http://127.0.0.1:4790`。团队部署可接入 HTTPS 反向代理，并转发 `/v1/events` 的 WebSocket 升级。

已有 PostgreSQL 17 服务时，可在一份用于 ShipForge 的数据库上安装 pgvector，将其连接串通过 `DATABASE_URL` 提供给服务。在 `platform` 目录运行 `npm ci`、`npm run build`、`npm run server`。启动迁移需要创建表与安装 vector 扩展的数据库权限。桌面开发运行 `npm start`。Windows 打包将独立机器程序复制到 `platform/build/worker`，然后运行 `npm run package:windows`。

## CLI 与验收

`npm run cli -- schema` 输出基于运行时验证器生成的 JSON Schema。`npm run cli -- request [文件]` 处理同一份命令，分别通过 `SHIPFORGE_SERVER` 与 `SHIPFORGE_TOKEN` 选择服务和身份。以 `{"op":"workspace.read"}` 读取最近进入的工作区，以 `{"op":"task.create","spaceId":"<返回的空间 ID>","title":"准备发布"}` 新建任务。具有版本的更新需携带读回的 `revision`。

`npm test` 使用 `DATABASE_URL` 创建隔离的临时 PostgreSQL 数据库，并通过 `SHIPFORGE_WORKER_EXE` 启用独立工作机。测试覆盖新用户、成员邀请、两个身份、非法修改拒绝、协作事件、真实工作机接单、结果回写、文档冲突、进程重启后的数据及 CLI。

Windows 安装程序已在含空格和中文的目录内安装并启动。安装后的真实窗口使用随包工作机和独立的 PostgreSQL 17.12、pgvector 0.8.7 服务，验证了创建组织、建任务、评论、连接工作机、创建 Agent、指派、完成回写和四项引导。示例 Runtime 由工作机作为独立进程启动。回归脚本位于 `platform/tests/electron-flow.js`。 [生成的请求协议](electron-protocol.schema.json)

目前优先完成组织、空间、任务、Agent 和执行闭环。文件存储适配、较复杂的工作流、迁移验证以及更完整的多成员交付流程分阶段推进。应用集成可沿服务端新增命令，与本机 Runtime 凭据使用独立授权。
