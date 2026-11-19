# 邮箱登录、个人资料与注销

在 Windows「设置 → 账号列表」选择「使用邮箱登录或注册」，输入团队服务地址和自己的邮箱。打开邮件，在浏览器输入桌面显示的八位配对码，回到桌面选择「完成登录」。首次验证会创建独立账号及其私有组织，并进入新手设置。已有账号回到上次有权访问的位置。[HiFox 账号管理](https://help.hifox.com/docs/other/account-management)

在账号列表可编辑姓名、HTTPS 头像地址及个人简介。「退出当前账号」撤销此设备的会话，其他设备继续使用各自的会话。「从本机移除」清理本机凭据。管理员签发的团队令牌可从「添加账号」连接。

## 自托管邮件服务

为 `shipforge-server serve` 的进程配置以下环境变量。服务使用 Resend 的邮件发送协议；团队地址与邮件端点接受 HTTPS 或本机回环 HTTP。[Resend 发信接口](https://resend.com/docs/api-reference/emails/send-email)

| 环境变量 | 含义 |
| --- | --- |
| `SHIPFORGE_IDENTITY_BASE_URL` | 收件人的浏览器能访问的团队根地址，例如 `https://team.example.org`。反向代理将该地址下的 `/identity/`、`/open/` 和 `/v1/` 转发给团队服务。 |
| `SHIPFORGE_IDENTITY_MAIL_FROM` | 已获得发信权限的地址。 |
| `SHIPFORGE_IDENTITY_MAIL_TOKEN` | 保存在服务端的发信凭据。 |
| `SHIPFORGE_IDENTITY_MAIL_URL` | 默认为 `https://api.resend.com/emails`。可指定兼容同一格式的组织邮件网关。 |

每次验证有十分钟有效期。收件人需在浏览器输入发起设备上显示的配对码，点击「打开 ShipForge 桌面版」可回到安装版继续操作。便携版可以直接回到原窗口。Windows 用当前用户的 DPAPI 保存凭据。每次登录形成独立会话。已验证账号可以接受发给自己邮箱的组织邀请。

## 注销与组织数据

「永久注销账号」先列出将退出或解散的组织、待转让的子空间及共享机器。单人、自有组织会随注销删除。多人组织的唯一所有者须显式选择解散。有其他所有者的组织会保留，先到空间管理将自己持有的子空间转给已加入的成员。仍有共享机器权限时应先取消共享、退出相应空间或联系机器所有者处理。服务在最后一步重新核对。完成邮箱验证后，在桌面输入「注销」确认。

确认后，个人资料和成员身份被清除，已登录的设备及个人令牌失效。共同参与且由其他人持有的组织保留任务和讨论。用同一邮箱重新注册会得到新的账号 ID、私有组织和新手进度。

团队服务签发的邮箱设备会话有效期为 30 天。到期后可从原设备的账号列表重新验证同一邮箱，保留该账号的上次位置。个人访问令牌按创建时选择的期限生效。

## CLI

```json
{"op":"profiles.login.start","url":"https://team.example.org","email":"member@example.org"}
{"op":"profiles.login.finish","id":"请求 ID"}
{"op":"access.account.profile"}
{"op":"access.account.update","expected_rev":1,"name":"示例成员","bio":"负责版本验收"}
{"op":"access.account.impact"}
```

验证后使用同一设备上返回的请求 ID 完成登录。重新认证时向 `profiles.login.start` 提供设备保存的 `account` ID，身份和服务地址均须匹配。使用 `profiles.logout` 撤销本设备登录。注销使用 `profiles.close.start`、浏览器确认和 `profiles.close.finish`，最后一步带 `confirmation`，多人组织解散还需 `dissolve`。[命令行手册](cli.md)
