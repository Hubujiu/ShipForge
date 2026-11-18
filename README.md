# ShipForge

ShipForge 是以 Rust 和 GPUI 设计的开源团队协作工作台。团队在一个空间里管理任务、迭代和文档，把工作指派给同事、Agent 或小队，并在原任务上追踪执行结果。

当前交付为第一版产品与界面基线。桌面端、CLI、协作服务与机器执行端列入开发路线，首个可运行里程碑是本机任务看板与详情。

| 文档 | 可以查到什么 |
| --- | --- |
| [功能清单与验收条件](docs/features.md) | 逐项对照官网、帮助中心和 Windows 实机，标出已开放、需配置、仅有文档和预告的能力。 |
| [界面索引](docs/interfaces.md) | 31 个页面、面板和表单状态，导航路径以及可直接查看的官方截图。 |
| [客户端与 REA 分析](docs/client-analysis.md) | 0.4.538 安装包内容、可读模块的恢复结果、桌面桥接证据和工具覆盖边界。 |
| [Rust + GPUI 架构与实施顺序](docs/architecture.md) | core、原生桌面、CLI、团队服务和执行机器的职责、协议、数据边界及五个可验收里程碑。 |

产品参考为 [HiFox 官网](https://hifox.com/) 和[官方帮助](https://help.hifox.com/docs/welcome)。ShipForge 采用独立实现。项目内容使用 [MIT 许可证](LICENSE)。
