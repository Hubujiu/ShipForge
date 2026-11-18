# 用 Jira 客户端访问 ShipForge

团队服务可为一个组织提供 Jira REST v2 和 v3。已有脚本保留 Jira 的资源路径，将站点地址和认证切换为 ShipForge 即可。客户端写入任务、流转与评论后，桌面、CLI、动态和已配置的自动化会读取同一条记录。[HiFox 接口说明](https://help.hifox.com/docs/integration/jira-api) · [Jira REST 资源](https://developer.atlassian.com/cloud/jira/platform/rest/v2/api-group-issues/)

## 服务与成员

1. 通过 `organization.list` 查组织 ID。在空间设置里为各空间填写独立的「任务编号前缀」，例如 `OPS`。前缀采用 1 到 12 位英文字母或数字，已有任务保留原编号。同组织内两个可见任务若共用同一完整编号，按编号访问会返回 409。
2. 管理员启动团队服务时传入 `--jira-organization <组织 ID>`。在反向代理后，用 `--jira-base-url` 填客户端实际访问的 HTTPS 站点，让返回的 `self` 链接可直接访问。代理应把相对站点根的 `/rest/api/` 转给服务。下例中的域名供部署时替换。

```powershell
.\shipforge-server.exe --data-dir D:\ShipForgeTeam serve --listen 127.0.0.1:47138 --jira-organization default --jira-base-url https://issues.example.org
```

3. 在桌面「连接设置」的访问令牌页签发目标成员自己的 `read` 或 `write` 令牌。也可在服务机运行 `shipforge-server --data-dir D:\ShipForgeTeam token issue --actor alex --scope read`。该账号需要已加入组织，访问私有空间时还需空间成员资格。可在同一页撤销令牌。机器和 Webhook 凭据仅用于本身的服务。
4. 在 Jira 客户端填入站点根地址，例如 `https://issues.example.org`。发送 `Authorization: Bearer <个人令牌>`，或用 HTTP Basic 发送成员 ID 与此人的个人令牌。发起请求的身份取自凭据。反向代理对外提供 TLS。

`SHIPFORGE_JIRA_ORGANIZATION` 和 `SHIPFORGE_JIRA_BASE_URL` 可代替启动参数。每个服务进程绑定一个组织。可为多个组织分别部署内网监听器，由反向代理按组织域名分发。未配置组织时，此进程的 Jira 路由返回 404。

## 客户端示例

下例使用 Python 的 `jira` 包。读请求需要个人读权限，写入还需要该成员对任务的操作权限。任务编号在对应空间内生成，例如 `OPS-42`。[jira-python 用法](https://jira.readthedocs.io/examples.html)

```python
import os
from jira import JIRA

client = JIRA(server=os.environ["JIRA_URL"], token_auth=os.environ["JIRA_TOKEN"])
issues = client.search_issues(
    'project = OPS AND assignee = currentUser() ORDER BY priority DESC',
    startAt=0, maxResults=20,
)
issue = client.issue("OPS-42")
issue.update(summary="许可已核对")
client.add_comment(issue.key, "完成复核。")
for transition in client.transitions(issue.key):
    print(transition["id"], transition["name"])
```

原始详情地址为 `GET /rest/api/2/issue/OPS-42?fields=summary,status,assignee`。获取可选流转后，向 `POST /rest/api/2/issue/OPS-42/transitions` 提交 `{"transition":{"id":"done"}}`。返回 204 表示事务已提交。读取或提交富文本时切换到 `/rest/api/3/`。v2 返回文本，v3 返回 Atlassian Document Format。v3 写入的段落、换行、文本标记在正文未另行编辑时保留。桌面编辑纯文本后，v3 按新正文生成对应的段落。

| 资源 | 方法及作用 |
| --- | --- |
| `myself`、`serverInfo` | GET 当前成员及站点信息 |
| `project`、`project/{key}`、`issue/createmeta` | GET 可读空间、任务类型和创建字段 |
| `issue`、`issue/{key}` | POST 创建，GET 读取，PUT 更新，DELETE 删除。创建返回 201。成功更新和删除返回 204 |
| `issue/{key}/comment` | GET 分页评论，POST 添加评论 |
| `issue/{key}/transitions` | GET 当前任务类型可用的其他状态，POST 流转。空间分拣、类型工作流和编辑权限均生效 |
| `search` | GET 或 POST 经典 JQL 搜索 |
| `priority`、`resolution`、`status`、`issuetype`、`field` | GET 当前站点的字段元数据 |
| `user/search`、`user/assignable/search` | GET 成员搜索，可用 `query` 和 `project` 缩小范围 |

写入支持 `summary`、`description`、`issuetype`、`priority`、`assignee`、`labels`、`duedate` 和同一空间的 `parent.key`。创建还需指定 `project.key` 或 `project.id`。Jira 的 `assignee` 显示首位人员，设置它会替换本地任务的人员集合，传 `null` 会清空人员。已有 Agent 或小队执行者独立保存。标签按空间和组织目录解析，缺少的名称按空间规则创建。Jira 的五档优先级依次映射紧急、高、中、低和无。终态的 resolution 显示为 Done。

## JQL 和分页

可查询的字段包括 `project`、`issuetype`、`status`、`statusCategory`、`resolution`、`priority`、`assignee`、`reporter`、`creator`、`labels`、`summary`、`description`、`text`、`created`、`updated`、`duedate`、`key` 和 `id`。支持比较运算符、`~`、`!~`、`IN`、`NOT IN`、`IS [NOT] EMPTY`、括号和 `AND`、`OR`、`NOT`。成员值可使用 `currentUser()`。日期按 UTC 解释，可写绝对日期或 `-7d`、`-2w`、`-1M`、`-1y` 等相对值。

`ORDER BY` 支持 `key`、`created`、`updated`、`priority`、`status`、`assignee`、`summary` 和 `duedate`，可指定多列及 ASC/DESC。搜索与评论列表接收 `startAt` 和 `maxResults`，默认从 0 开始返回 50 项，最多 100 项。传 `maxResults=0` 可只获取可见结果总数。查询先过滤组织、空间和任务权限，再计算总数、排序和分页。

`400` 表示字段、JQL 或参数有误，`401` 表示令牌缺失、无效或已撤销，`403` 表示账号或令牌缺少操作权限，`404` 表示资源对当前成员不可用，`409` 表示标识冲突，`413` 表示 JSON 超过 1 MiB。服务按直连来源 IP 每分钟接收 600 次 Jira 请求，达到上限返回 `429` 和 `Retry-After`。经同一反向代理转发的请求共享该来源的限额。错误体含 `errorMessages` 和 `errors`。

本页覆盖经典 Jira issue API。Agile 看板、工时、附件、自定义字段和 Jira Cloud 的增强搜索属于独立接口。验收使用独立的本地组织、原生团队服务及实际发行的 jira-python 3.10.5 客户端。[HTTP 与 SDK 验收脚本](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-jira.py)
