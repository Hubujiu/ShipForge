# 自动化 Webhook

空间里的每个 Webhook 触发器都可以有自己的私密 URL。调用方用 `POST` 发送一个 JSON 对象即可派发规则。保存规则时会出现「立即复制此 URL」。同一条规则有多个新触发器时，弹窗里每个 URL 都有自己的复制按钮。关闭弹窗后可为原触发器重新生成 URL。境外或企业内网调用方需要能够访问团队服务的 HTTPS 地址。[HiFox 自动化 Webhook](https://help.hifox.com/docs/space/automations#webhook)

## 设置与使用

1. 在「自动化」创建规则，选择 Agent 或小队、填写指令、添加 Webhook 触发器并保存。将弹窗里的 URL 放进调用方的秘密配置中。
2. 编辑已有规则，在相应触发器点「安全设置」。也可从规则详情为指定触发器进入设置与投递记录。首次签发、重新生成和吊销只作用于选中的触发器。旧 URL 在轮换或吊销后立即返回 `404`。轮换会保留签名开关、既有密钥和 IP 白名单。
3. 签名校验和 IP 白名单默认关闭。编辑者点「保存安全设置」后，开关和名单才生效。首次启用签名会生成以 `whsec_` 开头的密钥，默认掩码显示。「查看签名密钥」可显示、复制当前密钥。「重置签名密钥」在确认后立即更换密钥，关闭再启用签名则继续使用原密钥。
4. 在「Webhook 投递记录」查看来源 IP、时间、请求结果、签名状态和拒绝原因。较早记录可翻页。规则和该触发器均启用时才会派发，暂停或禁用的投递记为已跳过。当前用户需要规则读取权限才能查看记录，还要有运行读取权限才能从协议结果取得关联运行 ID。

新地址位于 `/v1/automation-webhooks/` 下，私密部分由系统生成。部署到代理后可用 `SHIPFORGE_PUBLIC_URL` 指定外部 HTTPS 基址及代理路径前缀。团队服务用配置的基址或实际监听地址生成 URL。本地模式默认使用 `http://127.0.0.1:47138`，需要用同一个数据目录启动团队服务，外部调用方才可访问。原有 `POST /v1/hooks/{space}/{rule}` 继续使用限定空间及规则的 Bearer 凭据、事件 ID 与原响应格式。升级已有规则时可从安全设置签发新的私密入口。

## 签名与来源

| 请求方式 | 校验 |
| --- | --- |
| 自有发送端 | 以完整 `whsec_…` 字符串的 UTF-8 字节为密钥，计算 `HMAC-SHA256`。输入依次是时间戳头的原字符串、一个换行字节和实际发送的原始请求体字节。将摘要用带补位的标准 Base64 编码，在 `X-Webhook-Signature` 中加 `v1=` 前缀，并发送原 `X-Webhook-Timestamp`。 |
| 时间窗口 | 接受 Unix 秒及 13 位毫秒，允许与服务端相差最多 300 秒。同一签名在 600 秒内受理一次。毫秒签名计算使用完整毫秒字符串。 |
| GitHub 兼容发送端 | `X-Hub-Signature-256` 使用 `sha256=` 加原始请求体 HMAC-SHA256 的十六进制字符串。此格式没有时间戳和签名防重放。两套头同时存在时，优先验证 `X-Webhook-Signature`。 |
| IP 白名单 | 每行一个 IPv4、IPv6 或 CIDR，最多 20 项。保存时规范化网段并去重，IPv4-mapped IPv6 按 IPv4 比对。开启时需至少一项。`/0` 通配网段会被拒绝。来源取 TCP 对端地址，代理的 `Forwarded`、`X-Forwarded-For` 不参与判断。使用反向代理时，服务端看到的是代理地址。 |

以下 Python 示例从环境读取凭据，用同一份字节签名和发送。

```python
import base64
import hashlib
import hmac
import json
import os
import time
from urllib.request import Request, urlopen

body = json.dumps({"event": "build.finished", "build": 42}).encode("utf-8")
stamp = str(time.time_ns() // 1_000_000)
signed = hmac.new(
    os.environ["WEBHOOK_SECRET"].encode("utf-8"),
    stamp.encode("ascii") + b"\n" + body,
    hashlib.sha256,
).digest()
headers = {
    "Content-Type": "application/json",
    "X-Webhook-Timestamp": stamp,
    "X-Webhook-Signature": "v1=" + base64.b64encode(signed).decode("ascii"),
}
with urlopen(Request(os.environ["WEBHOOK_URL"], data=body, headers=headers)) as response:
    print(response.status)
```

验签使用原始字节，序列化后若改动空格、编码或字段顺序，需要重新计算签名。来源 IP 在应用读取请求体之前检查。查看具体拒绝原因可读响应中的 `data.reason`。[签名和兼容格式](https://help.hifox.com/docs/space/automations#签名校验) · [来源规则](https://help.hifox.com/docs/space/automations#ip-白名单)

| HTTP 状态 | 处理结果 |
| --- | --- |
| 202 | 已受理。`data.status` 为 `accepted` 或 `skipped`，`deliveryId` 可和投递记录的 `id` 核对。 |
| 401、403、409 | 签名缺失、签名错误、时间超窗、IP 不在名单或签名重放。被拒请求仅记录元数据。 |
| 400、413 | 请求体不是有效 JSON 对象，或超过 ShipForge 的 128 KiB 限制。空请求体按空对象处理。 |
| 404 | 私密 URL 无效，已轮换、吊销，或所属触发器已删除。 |
| 429 | 每个私密 URL 每 60 秒允许至多 60 次请求。按 `Retry-After` 等待，限流请求不产生投递记录。这是 ShipForge 的配额。 |

## CLI

用 `shipforge request` 的标准输入提交单个 JSON。新建并签发时使用 `automation.hook.save`，返回的 `issued[]` 含每个新触发器的一次性链接。通用 `create` 和 `update` 照常返回普通规则记录，之后可显式签发。一般读取、事件日志、导出和历史只显示业务数据与投递状态。

```json
{"op":"automation.hook.save","space":"default","action":"create","id":"build-events","title":"构建结果","fields":{"enabled":true,"handler_kind":"agent","handler_id":"helper","instruction":"整理构建结果","triggers":[{"id":"ci","type":"webhook","enabled":true}]}}
```

```json
{"op":"automation.hook.configure","space":"default","id":"build-events","trigger_id":"ci","signing":true,"ip_filter":true,"allowlist":["203.0.113.7/24","2001:db8::7"]}
```

```json
{"op":"automation.hook.history","space":"default","id":"build-events","trigger_id":"ci","limit":25}
```

后续页传 `before` 为上一页的 `next_before`。安全设置的 `revision` 可作为变更请求的 `expected_version`，在其他编辑者先保存时返回冲突。涉及配置修改、签发及轮换的请求可携带唯一 `request_id`。重投已成功的同一请求返回冲突，避免再次轮换或再次输出秘密。编辑者可以单独查看当前签名密钥。

## 验收

独立发送端用标准库计算两种签名。团队服务、CLI 和三个身份在 IPv4 与 IPv6 环回地址验证来源过滤、URL 及签名轮换、单次展示、并发重放、限流、分页和历史命名凭据升级。发送端另行声明 16 字节正文，只发请求头，确认未知 URL 立即返回 404、受限来源立即返回 403。带 `Expect: 100-continue` 的 100,000 字节请求同样在发送正文前被拒绝。Windows 隐藏的原生 D3D 窗口另测表单、密钥显示和投递记录。公网反向代理的 TLS 和出口配置仍需在实际部署确认。[HTTP 验收](https://github.com/Hubujiu/ShipForge/blob/main/scripts/verify-automation-webhooks.py) · [Windows 用例](https://github.com/Hubujiu/ShipForge/blob/main/crates/shipforge-desktop/src/webhook_ui_test.rs)
