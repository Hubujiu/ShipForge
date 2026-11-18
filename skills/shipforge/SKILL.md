---
name: shipforge
description: Work with a ShipForge workspace through the installed shipforge CLI and its versioned JSON protocol.
---

# ShipForge

List available `tag` and `tag_group` records in the current space. Use `tag.apply` to add, remove or replace tags for a list of task IDs, or clear a named group. Group selection replaces the previous member of that group. Archive a tag to preserve existing task references. When an administrator requests a merge, use `tag.merge` with source and target IDs from the same scope. [Label operations](../../docs/cli.md)

Discover spaces with `space.list`. An eligible actor can use `space.join` for a public space or `space.leave` for their own membership. Use member invitations for private spaces. Check `space.navigation` before opening a module and use `space.pin` to change the caller's sidebar. Before deactivation, inspect `space.deactivate.preview`. Apply the user's decision to move or cancel unfinished work with `space.deactivate`, its revision and the destination if needed. Use `space.restore` to reopen the archived space. [Space lifecycle and modules](../../docs/space-lifecycle.md)

Read the authenticated member's organization progress with `onboarding.get` and the checklist with `onboarding.catalog`. Use `onboarding.display` for personal sidebar visibility. Apply `onboarding.setup` when the member chooses their first organization name and space defaults. Successful business operations earn the corresponding credit. [Onboarding](../../docs/onboarding.md)

Organization administrators can inspect `invitation.list`, issue a scoped email or shared invitation with `invitation.issue`, and use `invitation.copy` or `invitation.revoke` on its ID. Copying a disabled shared invitation reopens it with a new link. A recipient uses their own authenticated identity for `invitation.accept`. Keep bearer links out of shared logs. [Member invitation operations](../../docs/cli.md)

Use `shipforge schema` for the request schema shipped with the running version, and `shipforge --version` to identify that version. The installed manual explains each operation and its inputs. [CLI manual](../../docs/cli.md) · [JSON Schema](../../docs/protocol.schema.json)

Read the current workspace and task before making a change. Use the space and object IDs returned by the API. Send UTF-8 JSON to `shipforge request` on stdin or through `--file`. Successful replies contain `ok: true` and `data`. Surface the error code and message when a command fails. On Windows, invoke `.\shipforge.exe` when running from the program directory.

Include the current `expected_rev` on updates and deletes. For ordinary record writes, retry with the same `request_id` and original body. A conflict calls for reading the current state before applying changes. The local and team identities share permission checks with the desktop client.

Use `iteration.timeline` to read the visible iterations with planning phase, task totals and capacity. Supply an optional `as_of` date in `YYYY-MM-DD` format. Before closing an iteration, inspect `iteration.preview`, then choose the open destination iteration or return remaining tasks to the backlog with `iteration.close`.

Read `iteration.settings` to identify the space's iteration mode, time zone and revision. For an authorized schedule change, send `iteration.configure` with a partial `fields` object and the returned `expected_rev`. Automatic mode uses an IANA zone, an anchor date, 1–8 weeks per cycle, cooldown days and a count of future periods. Auto-join tracks status and due-date changes within the relevant window. Use `iteration.start` for a manual start or to bring the next automatic period forward. In manual mode, filter the timeline with `owner:"@me"`. Use `iteration.history` to inspect the visible closure snapshot. Disabling the module ends started periods, removes unstarted ones and keeps historical scope. [Iteration policy and examples](../../docs/iterations.md)

Use `shipforge upload` and `shipforge download` for files. The equivalent protocol operations are `attachment.upload` and `attachment.download`. Each file can be up to 4 MiB. Downloads check the SHA-256 and preserve existing destination files. For a historical document attachment, pass its revision with `--version`.

Use `chat.info` for the caller's role and `chat.invitations` for pending invitations. Use `chat.manage` with the chat ID and action. Human group members use `add` or `remove`. An Agent also needs `target_kind:"agent"`. Private access uses `invite`, then `accept` or `decline` as the recipient. The owner can `revoke` or `transfer`. A participant can `leave` or `clear` their own history. Only a group owner can `dissolve` a group. Set `muted` or `ignore_all` for the caller with `chat.preferences`. Direct mentions still notify them.

Create a frozen snapshot with `chat.share.create` only for messages the user selected. Choose `scope:"organization"` unless the user wants anyone holding the link to read it. Read it with `chat.share.read` and its `share` key. Use `chat.share.revoke` with the chat ID and key to invalidate every link for that same selection. Team reads check live membership and share state. [Conversation operations](../../docs/conversations.md)

For team drafts, inspect `sync.pending` before `sync.flush`. A conflicting draft remains in the queue. After reading the latest remote record, use `sync.resolve` with the draft ID and current `expected_rev` or an explicit discard choice. Keep tokens in the saved connection or process environment.

Use `agent.resolve` to inspect an Agent's effective settings and `tool.authorize` to check a grant. The machine worker owns the external runtime process, approved directory, lease and report. Use the same rule and `event_id` when retrying `automation.fire`. For calendar triggers supply `frequency`, `local_time` and an IANA `timezone`. For `weekly` include `weekdays` with Monday as 1 through Sunday as 7. [Machine and schedule manual](../../crates/shipforge-worker/README.md)

Preview mapped row imports with `import.preview`. For provider events call `provider.preview` and `provider.ingest` with the connector, raw payload and stable upstream delivery ID. Use the native signed Slack or GitHub route, a bound GitLab secret, or the corresponding authenticated relay. Supported message threads add comments to their existing task. A sync reply with `action: "conflict"` contains the fields that need a decision. Push changes to a mapped GitHub Issue only when the user requests a remote write. [Provider formats and examples](../../crates/shipforge-core/PROVIDERS.md)

For a Jira client reading ShipForge tasks, use the organization's configured Jira site and the member's personal token. Use `jira.read` with the organization ID in `space`, `api_version: 2` or `3`, and a resource such as `issue/OPS-42` or `search`. Put JQL and pagination in `params`. For an authorized change use `jira.write` with the HTTP method and Jira-shaped `payload`. Read the issue's transitions before writing its target status. [Jira API and examples](../../docs/jira-api.md)

For a requested external Slack reply, list the task’s authorized targets with `im.threads`, then send `im.reply` with the selected connector ID, task ID, exact thread object, body and a stable request ID. Use a normal task comment for internal discussion. An administrator can bind a known thread with `im.bind`, remove it with `im.unbind`, or inspect delivery history with `im.outbox`. The delivery worker uses `im.claim` and `im.ack`. Before using `im.retry` on an uncertain delivery, check the original Slack thread and confirm its message timestamp or request a resend. [Thread routing and delivery](../../crates/shipforge-core/PROVIDERS.md)

GitHub and GitLab connectors match commit messages and pull or merge requests to exact task numbers when the authorized repository has `fields.link_tasks:true`. Read `links` for the source URLs and task activities. An explicitly configured `fields.review_rule_id` routes eligible new review requests to an enabled webhook automation. Inspect `review_runs` for its run IDs. [Repository activity and review automation](../../crates/shipforge-core/PROVIDERS.md)

For an Agent's weekly availability, put an IANA `timezone` and a `week` object under `fields.working_hours`. Use ISO weekday keys `"1"` through `"7"` and up to two nonoverlapping local `start`/`end` intervals per day. An omitted day is closed. Older daily integer-hour windows with `utc_offset_minutes` remain supported. Check the effective configuration with `agent.resolve`. [Working hours](../../crates/shipforge-worker/README.md)

When working across spaces, inspect the authenticated identity's membership. An organization guest needs an explicit invitation to a child space. An organization administrator can see private space metadata; opening its tasks or members requires membership in that space. The installation `owner` is the service operator.

Read the caller's settings with `preferences.get`. Patch only requested fields with `preferences.update` and the returned `expected_rev`. Reset one shortcut by sending a null value for that command. Use `task.move` with a source task and writable destination. `task.conversation` reopens an assignee thread. `task.command` returns the linked chat and run and accepts a stable request ID for retries. [Personal settings and task commands](../../docs/personal-settings.md)

Assign human members and a single agent or crew independently with `task.assign`. Use `assignees` for replacement, `mode:"append"` to add people, or `executor:{"kind":"agent","id":"…"}` for execution. Keep the current revision in `expected_rev`. Read the space’s `fields.allow_multiple_assignees` before adding multiple people to a new task. Query `filter.assignees:"@me"` to include every task assigned to the current member.

For Slack's native single-user assignment control, enable `fields.slack_assignment` on an outbound Slack connector and map both the clicking and selected Slack users through `fields.assignee_map`. A click replaces the task's human assignees under the clicking member's permissions and preserves its executor. [Slack integration setup](../../crates/shipforge-core/PROVIDERS.md)
