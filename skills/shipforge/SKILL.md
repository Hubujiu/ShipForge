---
name: shipforge
description: Work with a ShipForge workspace through the installed shipforge CLI and its versioned JSON protocol.
---

# ShipForge

Use `shipforge schema` for the request schema shipped with the running version, and `shipforge --version` to identify that version. The installed manual explains each operation and its inputs. [CLI manual](../../docs/cli.md) · [JSON Schema](../../docs/protocol.schema.json)

Read the current workspace and task before making a change. Use the space and object IDs returned by the API. Send UTF-8 JSON to `shipforge request` on stdin or through `--file`. Successful replies contain `ok: true` and `data`. Surface the error code and message when a command fails.

Include the current revision on updates and deletes. When retrying an uncertain write, retain the same request ID and original request body. A conflict calls for reading the current state and deciding what to apply. Use the configured local or team identity; permissions and visibility are enforced by the same core as the desktop client.

Agents read only the task and capabilities granted to them. The machine worker owns its runtime process, directory policy and lease. Document and task changes should go through the CLI. Before syncing external changes, preview the import or provider mapping and retain the upstream event ID.

Use `shipforge upload` and `shipforge download` for files. The equivalent protocol operations are `attachment.upload` and `attachment.download`. Each file can be up to 4 MiB. Downloads check the SHA-256 and preserve existing destination files. Use `shipforge schema` for the full current operation list, including tool grants, run leases, schedules, vendor inputs and offline draft resolution.
