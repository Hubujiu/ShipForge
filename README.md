# ShipForge

ShipForge 是面向团队和 Agent 的开源协作工作台。新版 Windows 工作区采用 Electron、React、Webpack 和 PostgreSQL 协作服务，由独立的 Rust 工作机执行本机 Runtime。可以在同一空间组织任务、讨论、迭代与文档，并查看 Agent 的运行和回复。[新版安装与使用说明](docs/electron-core.md)

**[下载已发布的 Windows x64 版本](https://github.com/Hubujiu/ShipForge/releases/latest) · [旧版 Windows 使用指南](docs/windows.md) · [CLI 手册](docs/cli.md)**

首次打开新版客户端时连接团队服务，创建组织与首个空间，或使用团队邀请码加入。新手指引会检查任务、工作机、Agent 和首次执行。0.16.0 版的本地演示通过 `shipforge-desktop.exe --demo` 打开。[旧版新手指引](docs/onboarding.md)

![Windows 上的 ShipForge 任务看板](docs/images/windows-board.jpg)

| 文档 | 内容 |
| --- | --- |
| [Electron 工作区与 PostgreSQL 服务](docs/electron-core.md) | 技术栈证据、初次运行、部署、CLI 和当前验收范围。 |
| [Windows 功能对照](docs/windows-coverage.md) | 从参考能力定位到 Windows 实际入口，列明本机、团队及第三方接入范围。 |
| [Windows 实机截图与验收](docs/design-qa.md) | 查看原生看板、任务详情和定时规则编辑器，以及附件、重开和文档版本的测试结果。 |
| [功能清单与验收条件](docs/features.md) | 对照 HiFox 官网、帮助中心和 Windows 实机梳理的产品能力，涵盖首次进入与新手指引。 |
| [行为覆盖清单](docs/parity-inventory.md) | 逐项查找官方说明、ShipForge 验收证据、当前差异及第三方待联调范围。 |
| [界面索引](docs/interfaces.md) | 首批实机样本的 36 个页面、面板和表单状态，含导航路径与可公开查看的官方参考。 |
| [客户端与 REA 分析](docs/client-analysis.md) | HiFox 0.4.538 安装包、可读模块恢复结果和桌面桥接职责。 |
| [Rust 与 GPUI 架构](docs/architecture.md) | 单机、团队服务、CLI 和执行机器的职责，以及设计目标与当前实现的区别。 |
| [命令协议](docs/protocol.schema.json) | 桌面、服务、CLI 共享的操作名称和 JSON 请求结构。 |

## 从源码运行新版

在 `platform` 安装锁定依赖并构建。启动 PostgreSQL 17 协作服务后，从同一目录运行桌面应用。

```powershell
Set-Location platform
npm ci
npm run build
npm start
```

## 构建已发布的 GPUI 版

```powershell
cargo build --release --workspace --locked
.\target\release\shipforge-desktop.exe
```

产品参考来自 HiFox 官网和官方帮助。ShipForge 源码按 MIT 许可公开，应用图标授权见仓库随附文件。[HiFox 官网](https://hifox.com/) · [官方帮助](https://help.hifox.com/docs/welcome) · [MIT 许可证](LICENSE)
