# Windows 网络代理与版本检查

在「设置 → 网络代理」选择连接方式。默认使用系统设置。直接连接会忽略代理；手动配置支持 HTTP、HTTPS 和 SOCKS5，地址示例为 `http://proxy.example.org:8080`。用户名和密码分别填写。在原地址、同一用户名下留空密码，可继续使用已保存的凭据。更换地址或用户名后需要重新输入密码。Windows 当前用户的 DPAPI 保护保存的密码。

「测试连接」使用当前表单中的设置向指定地址发送 HEAD 请求，显示 HTTP 状态及耗时。测试成功后选择「保存并应用」，设置立即用于后续连接。测试地址可填写团队的 HTTPS 地址或本机回环 HTTP 地址。收到 401 或 404 说明 HTTP 服务有响应，成员登录另行验证。代理要求认证、TLS 验证失败和超时分别报告。

设备上的账号共用这项设置。团队连接、邮箱登录、公开技能文件下载、版本检查以及本进程的第三方 HTTP 请求会使用它。`localhost`、IPv4 和 IPv6 回环服务始终直接连接。设置保存在设备资料目录中。团队服务部署在另一台机器时，该进程使用自己的网络环境。独立工作机的长连接与外部 CLI 遵循该执行环境的配置。

系统模式识别进程的代理环境变量。Windows 上可读取当前用户的代理规则、排除列表并按目标解析自动代理。手动 SOCKS5 将目标域名交给代理解析，TLS 验证使用平台信任设置。实际组织网络中的 PAC 脚本、企业证书及代理认证策略需与其管理员联调。[Windows 当前用户代理](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpgetieproxyconfigforcurrentuser) · [Windows 自动代理](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpgetproxyforurl)

## 关于与更新

「设置 → 关于 ShipForge」显示安装版本、平台、架构和许可证。在已安装版本中，「使用手册」打开随包文档。「开始使用」进入当前成员的新手指引。「检查更新」从项目正式发布页读取最新稳定版本，显示可用的 Windows 安装包及其校验文件。点击「查看发布说明」可查看版本差异并自行下载。离线时仍能查看已安装版本并阅读随包手册。[正式发布](https://github.com/Hubujiu/ShipForge/releases)

## CLI

读取 `network.settings` 后，将返回的 `rev` 作为 `network.save` 的 `expected_rev`。并发编辑会提示重新读取。在修改前可用 `network.test` 测试草稿参数。这些命令以及 `device.about`、`device.update` 可在项目数据库损坏或服务未连接时使用。

```json
{"op":"network.settings"}
{"op":"network.test","mode":"manual","endpoint":"http://proxy.example.org:8080","test_url":"https://team.example.org"}
{"op":"network.save","expected_rev":0,"mode":"direct"}
{"op":"device.about"}
{"op":"device.update"}
```
