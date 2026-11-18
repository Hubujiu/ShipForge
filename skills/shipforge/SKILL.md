---
name: shipforge
description: Work with a ShipForge workspace through the installed shipforge CLI and its versioned JSON protocol.
---

# ShipForge

Use `shipforge schema` for the request schema shipped with the running version, and `shipforge --version` to identify that version. The installed manual explains each operation and its inputs. [CLI manual](../../docs/cli.md) · [JSON Schema](../../docs/protocol.schema.json)

Read the current workspace and task before making a change. Use the space and object IDs returned by the API. Send UTF-8 JSON to `shipforge request` on stdin or through `--file`. Successful replies contain `ok: true` and `data`. Surface the error code and message when a command fails. On Windows, invoke `.\shipforge.exe` when running from the program directory.

Include the current `expected_rev` on updates and deletes. For ordinary record writes, retry with the same `request_id` and original body. A conflict calls for reading the current state before applying changes. The local and team identities share permission checks with the desktop client.

Use `iteration.timeline` to read the visible iterations with planning phase, task totals and capacity. Supply an optional `as_of` date in `YYYY-MM-DD` format. Before closing an iteration, inspect `iteration.preview`, then choose the open destination iteration or return remaining tasks to the backlog with `iteration.close`.

Use `shipforge upload` and `shipforge download` for files. The equivalent protocol operations are `attachment.upload` and `attachment.download`. Each file can be up to 4 MiB. Downloads check the SHA-256 and preserve existing destination files. For a historical document attachment, pass its revision with `--version`.

For team drafts, inspect `sync.pending` before `sync.flush`. A conflicting draft remains in the queue. After reading the latest remote record, use `sync.resolve` with the draft ID and current `expected_rev` or an explicit discard choice. Keep tokens in the saved connection or process environment.

Use `agent.resolve` to inspect an Agent's effective settings and `tool.authorize` to check a grant. The machine worker owns the external runtime process, approved directory, lease and report. Use the same rule and `event_id` when retrying `automation.fire`. For calendar triggers supply `frequency`, `local_time` and an IANA `timezone`. For `weekly` include `weekdays` with Monday as 1 through Sunday as 7. [Machine and schedule manual](../../crates/shipforge-worker/README.md)

Preview mapped row imports with `import.preview`. For provider events, call `provider.preview` and `provider.ingest` with the same connector, raw payload and upstream delivery identity. Route messages through the corresponding authenticated relay. A sync reply with `action: "conflict"` contains the fields that need a decision. Push changes to a mapped GitHub Issue only when the user requests a remote write. [Provider formats and examples](../../crates/shipforge-core/PROVIDERS.md)
