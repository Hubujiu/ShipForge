---
name: shipforge-workspace
description: Work in an Electron and PostgreSQL ShipForge organization through its JSON CLI.
---

Use the service address and the authenticated member's session. Provide them via `SHIPFORGE_SERVER` and `SHIPFORGE_TOKEN`. Run `npm run cli -- request [file]` from the ShipForge platform directory. Omit the filename for UTF-8 JSON on stdin. Use `npm run cli -- schema` to read the accepted fields.

Call `workspace.read` first, optionally with a known `spaceId`. Use the returned task ID and revision for `task.update` or `task.archive`. Keep task status, comments, and run results in their original workspace. Record new discussion with `task.comment`.

Assign a visible Agent via `task.update`. A task in Backlog waits. Moving it to Todo can enqueue an execution. An existing eligible task can be explicitly started with `run.start`. Read the task's runs and chat before reporting completion. Use `chat.send` with the existing chat ID to continue the conversation.

Choose a registered work machine and a runtime it advertises when creating an Agent. The work machine owns its credential and process environment. Shared machine enrollment and team invitation require an authorized role. Never return a machine credential in discussion.

[Service guide and CLI](../../../docs/electron-core.md) · [Command schema](../../../docs/electron-protocol.schema.json)
