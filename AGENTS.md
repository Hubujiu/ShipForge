# ShipForge 开发约定

- 使用功能或修复分支提 PR。初始化后保持 main 受保护，Windows 和 Linux 两个必需检查均通过后再合并。
- 将业务规则放在 `shipforge-core`，GPUI 与 CLI 共用操作。运行 AI 工作时由机器服务调用外部工具。演示数据仅存内存，本机工作区通过 core 读写数据。
- 新业务能力要覆盖 CLI JSON 操作、生成的 schema、CLI 手册及随附 Skill。升级保留项目数据，程序更新进程独立于项目库的打开与迁移。元数据管理只记录绑定目录，编辑或执行源文件遵循用户请求。
- 提交 Rust 变更前运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 与 `cargo test --workspace --locked`。
- 原生文本框和选择触发器遵循共享 beUI 焦点规则，布局使用共享 spring 动效及项目范围稳定键。实现 HTML 参照和 GPUI 演示时同步校验夹具及图标，运行前端 lint 和 UI 规则测试。
- 打包与更新变更需构建 Release，通过打包脚本及 `python scripts/verify-package.py dist` 验证产物。后续正式版本更新 workspace 及 lockfile，先合并发布 PR，再由稳定版 Release 工作流从该提交生成并附上已验证的 Windows ZIP 和 SHA-256。
- 仓库中保留项目源文件、规格与测试。不提交用户数据库、凭据、构建目录、下载的发行物或含私有账号信息的参考材料。
- 产品基线变更在仓库根目录运行 `python scripts/check-docs.py`。
