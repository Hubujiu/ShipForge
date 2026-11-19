import { type Actor, type Command, commandSchema } from "../protocol";
import { digest, secret } from "./auth";
import { type Db, changed, conflict, forbidden, invalid, missing, one, transaction } from "./database";
import { readWorkspace } from "./workspace-read";

export interface TaskRow {
  id: string;
  space_id: string;
  title: string;
  description: string;
  status: string;
  member_id: string | null;
  agent_id: string | null;
  iteration_id: string | null;
  created_by: string;
  revision: number;
}

interface AgentRow {
  id: string;
  space_id: string;
  machine_id: string;
  name: string;
}

interface ChatRow {
  id: string;
  task_id: string | null;
  agent_id: string;
  owner_id: string;
}

function admin(actor: Actor) {
  if (actor.role === "member") throw forbidden("Organization administration is required");
}

async function requireSpace(db: Db, actor: Actor, spaceId: string) {
  if (!await one(db, "SELECT 1 FROM spaces WHERE id=$1 AND org_id=$2", [spaceId, actor.orgId])) throw missing();
}

async function taskById(db: Db, actor: Actor, id: string) {
  const task = await one<TaskRow>(db, `SELECT t.* FROM tasks t JOIN spaces s ON s.id=t.space_id
    WHERE t.id=$1 AND s.org_id=$2 AND t.archived_at IS NULL FOR UPDATE OF t`, [id, actor.orgId]);
  if (!task) throw missing("Task is unavailable");
  return task;
}

async function agentById(db: Db, actor: Actor, id: string) {
  const agent = await one<AgentRow>(db, `SELECT a.* FROM agents a JOIN spaces s ON s.id=a.space_id
    JOIN machines m ON m.id=a.machine_id WHERE a.id=$1 AND s.org_id=$2 AND (m.owner_id=$3 OR m.shared)`,
  [id, actor.orgId, actor.id]);
  if (!agent) throw missing("Agent or work machine is unavailable to your account");
  return agent;
}

async function validateTaskLinks(db: Db, actor: Actor, spaceId: string, memberId: string | null, agentId: string | null, iterationId: string | null) {
  if (memberId && agentId) throw invalid("Choose one member or one Agent");
  if (memberId && !await one(db, "SELECT 1 FROM members WHERE org_id=$1 AND user_id=$2", [actor.orgId, memberId])) throw missing();
  if (agentId && (await agentById(db, actor, agentId)).space_id !== spaceId) throw missing();
  if (iterationId && !await one(db, "SELECT 1 FROM iterations WHERE id=$1 AND space_id=$2", [iterationId, spaceId])) throw missing();
}

function executable(task: TaskRow) {
  return task.agent_id && ["todo", "doing", "review"].includes(task.status);
}

async function enqueue(db: Db, actor: Actor, agent: AgentRow, chat: ChatRow, instruction: string) {
  const run = await one<{ id: string }>(db, `INSERT INTO runs(task_id,chat_id,agent_id,machine_id,requested_by,instruction)
    VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(chat_id) WHERE status IN ('queued','running','canceling') DO NOTHING RETURNING id`,
  [chat.task_id, chat.id, agent.id, agent.machine_id, actor.id, instruction]);
  if (!run) throw conflict("This conversation already has an active run");
  await db.query("UPDATE chats SET updated_at=now() WHERE id=$1", [chat.id]);
  return { runId: run.id, chatId: chat.id };
}

async function enqueueTask(db: Db, actor: Actor, task: TaskRow) {
  if (!executable(task)) throw conflict("Assign an Agent and move the task to an executable status first");
  const agent = await agentById(db, actor, task.agent_id!);
  const chat = await one<ChatRow>(db, `INSERT INTO chats(space_id,owner_id,agent_id,task_id,title)
    VALUES($1,$2,$3,$4,$5) ON CONFLICT(task_id,agent_id) DO UPDATE SET title=EXCLUDED.title RETURNING *`,
  [agent.space_id, actor.id, agent.id, task.id, task.title]);
  return enqueue(db, actor, agent, chat!, task.title);
}

async function cancelRuns(db: Db, taskId: string) {
  await db.query(`UPDATE runs SET status=CASE WHEN status='queued' THEN 'canceled' ELSE 'canceling' END, updated_at=now()
    WHERE task_id=$1 AND status IN ('queued','running')`, [taskId]);
}

async function notification(db: Db, target: string | null, actor: Actor, task: TaskRow, headline: string) {
  if (target && target !== actor.id) {
    await db.query("INSERT INTO notifications(user_id,task_id,title) VALUES($1,$2,$3)", [target, task.id, `${headline} · ${task.title}`]);
  }
}

async function saveTask(db: Db, actor: Actor, q: Extract<Command, { op: "task.create" | "task.update" }>) {
  if (q.op === "task.create") {
    await requireSpace(db, actor, q.spaceId);
    await validateTaskLinks(db, actor, q.spaceId, q.memberId ?? null, q.agentId ?? null, q.iterationId ?? null);
    const next = await one<{ number: number }>(db, "UPDATE spaces SET next_task_number=next_task_number+1 WHERE id=$1 RETURNING next_task_number-1 AS number", [q.spaceId]);
    const task = await one<TaskRow>(db, `INSERT INTO tasks(space_id,number,title,description,status,priority,type,member_id,agent_id,iteration_id,due_on,created_by)
      VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING *`,
    [q.spaceId, next!.number, q.title, q.description ?? "", q.status ?? "backlog", q.priority ?? "none", q.type ?? "task",
      q.memberId ?? null, q.agentId ?? null, q.iterationId ?? null, q.dueOn ?? null, actor.id]);
    await notification(db, task!.member_id, actor, task!, "任务指派");
    if (executable(task!)) await enqueueTask(db, actor, task!);
    return { id: task!.id, revision: task!.revision };
  }
  const before = await taskById(db, actor, q.id);
  if (before.revision !== q.revision) throw conflict("The task has changed. Reload before saving");
  const columns = { title: "title", description: "description", status: "status", priority: "priority", type: "type",
    memberId: "member_id", agentId: "agent_id", iterationId: "iteration_id", dueOn: "due_on" } as const;
  const changes = Object.entries(q.changes) as [keyof typeof columns, string | null][];
  if (!changes.length) return { id: before.id, revision: before.revision };
  const nextMember = q.changes.memberId === undefined ? before.member_id : q.changes.memberId;
  const nextAgent = q.changes.agentId === undefined ? before.agent_id : q.changes.agentId;
  const nextIteration = q.changes.iterationId === undefined ? before.iteration_id : q.changes.iterationId;
  if (q.changes.memberId !== undefined || q.changes.agentId !== undefined || q.changes.iterationId !== undefined) {
    await validateTaskLinks(db, actor, before.space_id, nextMember, nextAgent, nextIteration);
  }
  const set = changes.map(([field], index) => `${columns[field]}=$${index + 2}`).join(",");
  const next = await one<TaskRow>(db, `UPDATE tasks SET ${set},revision=revision+1,updated_at=now() WHERE id=$1 RETURNING *`,
    [q.id, ...changes.map(([, value]) => value)]);
  if (before.member_id !== nextMember) await notification(db, nextMember, actor, next!, "任务指派");
  if (before.agent_id !== nextAgent || next!.status === "backlog") await cancelRuns(db, before.id);
  if (executable(next!) && (!executable(before) || before.agent_id !== nextAgent)) await enqueueTask(db, actor, next!);
  return { id: next!.id, revision: next!.revision };
}

async function mutate(db: Db, actor: Actor, q: Exclude<Command, { op: "workspace.read" }>) {
  switch (q.op) {
    case "space.create":
      admin(actor);
      return one(db, "INSERT INTO spaces(org_id,name,description) VALUES($1,$2,$3) RETURNING id",
        [actor.orgId, q.name, q.description ?? ""]);
    case "member.invite": {
      admin(actor);
      if (q.role === "admin" && actor.role !== "owner") throw forbidden("Only the owner can invite an administrator");
      const code = secret();
      const link = await one(db, `INSERT INTO invitations(digest,org_id,role,expires_at)
        VALUES($1,$2,$3,now()+make_interval(days => $4)) RETURNING expires_at AS "expiresAt"`,
      [digest(code), actor.orgId, q.role, q.expiresInDays]);
      return { ...link, code };
    }
    case "machine.enroll": {
      if (q.shared) admin(actor);
      const token = secret();
      const machine = await one(db, `INSERT INTO machines(org_id,owner_id,name,worker_digest,shared)
        VALUES($1,$2,$3,$4,$5) RETURNING id,name`, [actor.orgId, actor.id, q.name, digest(token), q.shared ?? false]);
      return { ...machine, token };
    }
    case "agent.create": {
      await requireSpace(db, actor, q.spaceId);
      if (!/^[0-9a-f-]{36}$/i.test(q.machineId)) throw invalid("Invalid work machine ID");
      const machine = await one(db, `SELECT 1 FROM machines WHERE id=$1 AND org_id=$2 AND (owner_id=$3 OR shared)
        AND runtimes ? $4`, [q.machineId, actor.orgId, actor.id, q.runtime]);
      if (!machine) throw missing("Select a machine with the requested runtime");
      return one(db, `INSERT INTO agents(space_id,name,instructions,machine_id,runtime,owner_id)
        VALUES($1,$2,$3,$4,$5,$6) RETURNING id`, [q.spaceId, q.name, q.instructions, q.machineId, q.runtime, actor.id]);
    }
    case "task.create":
    case "task.update":
      return saveTask(db, actor, q);
    case "task.comment": {
      const task = await taskById(db, actor, q.id);
      const comment = await one(db, "INSERT INTO comments(task_id,author_id,body) VALUES($1,$2,$3) RETURNING id",
        [q.id, actor.id, q.body]);
      for (const target of new Set([task.created_by, task.member_id])) {
        await notification(db, target, actor, task, "任务收到评论");
      }
      return comment;
    }
    case "task.archive": {
      const task = await taskById(db, actor, q.id);
      if (task.created_by !== actor.id) admin(actor);
      if (q.revision !== task.revision) throw conflict("The task has changed. Reload before archiving");
      await db.query("UPDATE tasks SET archived_at=now(),updated_at=now(),revision=revision+1 WHERE id=$1", [q.id]);
      await cancelRuns(db, q.id);
      return { id: q.id };
    }
    case "iteration.create":
      await requireSpace(db, actor, q.spaceId);
      if (q.startsOn > q.endsOn) throw invalid("The end date must be on or after the start date");
      return one(db, "INSERT INTO iterations(space_id,name,starts_on,ends_on) VALUES($1,$2,$3,$4) RETURNING id",
        [q.spaceId, q.name, q.startsOn, q.endsOn]);
    case "iteration.state": {
      const row = await one(db, `UPDATE iterations i SET status=$1 FROM spaces s
        WHERE i.id=$2 AND s.id=i.space_id AND s.org_id=$3 RETURNING i.id`, [q.status, q.id, actor.orgId]);
      if (!row) throw missing();
      return row;
    }
    case "document.save": {
      await requireSpace(db, actor, q.spaceId);
      const document = q.id
        ? await one<{ id: string; revision: number }>(db, `UPDATE documents SET title=$1,body=$2,revision=revision+1,updated_at=now()
          WHERE id=$3 AND space_id=$4 AND revision=$5 RETURNING id,revision`, [q.title, q.body, q.id, q.spaceId, q.revision])
        : await one<{ id: string; revision: number }>(db, "INSERT INTO documents(space_id,title,body) VALUES($1,$2,$3) RETURNING id,revision",
          [q.spaceId, q.title, q.body]);
      if (!document) throw conflict("The document has changed. Reload before saving");
      await db.query(`INSERT INTO document_versions(document_id,revision,title,body,author_id) VALUES($1,$2,$3,$4,$5)`,
        [document.id, document.revision, q.title, q.body, actor.id]);
      return document;
    }
    case "run.start":
      return enqueueTask(db, actor, await taskById(db, actor, q.taskId));
    case "run.cancel": {
      const row = await one<{ id: string; task_id: string | null }>(db, `UPDATE runs r SET
        status=CASE WHEN r.status='queued' THEN 'canceled' ELSE 'canceling' END,updated_at=now()
        FROM chats c JOIN spaces s ON s.id=c.space_id
        WHERE r.id=$1 AND r.chat_id=c.id AND s.org_id=$2 AND (c.task_id IS NOT NULL OR c.owner_id=$3)
          AND r.status IN ('queued','running') RETURNING r.id,c.task_id`, [q.id, actor.orgId, actor.id]);
      if (!row) throw conflict("The run has already stopped or is unavailable");
      return { id: row.id, private: row.task_id === null };
    }
    case "chat.start": {
      const agent = await agentById(db, actor, q.agentId);
      const chat = await one<ChatRow>(db,
        "INSERT INTO chats(space_id,owner_id,agent_id,title) VALUES($1,$2,$3,$4) RETURNING *",
        [agent.space_id, actor.id, agent.id, [...q.body].slice(0, 42).join("")]);
      await db.query("INSERT INTO messages(chat_id,sender,body) VALUES($1,$2,$3)", [chat!.id, actor.name, q.body]);
      return enqueue(db, actor, agent, chat!, q.body);
    }
    case "chat.send": {
      const chat = await one<ChatRow>(db, `SELECT c.* FROM chats c JOIN spaces s ON s.id=c.space_id
        WHERE c.id=$1 AND s.org_id=$2 AND (c.owner_id=$3 OR c.task_id IS NOT NULL)`, [q.id, actor.orgId, actor.id]);
      if (!chat) throw missing();
      const agent = await agentById(db, actor, chat.agent_id);
      if (chat.task_id) {
        const task = await taskById(db, actor, chat.task_id);
        if (task.agent_id !== agent.id) throw conflict("The task is assigned to another Agent");
      }
      await db.query("SELECT 1 FROM chats WHERE id=$1 FOR UPDATE", [chat.id]);
      await db.query("INSERT INTO messages(chat_id,sender,body) VALUES($1,$2,$3)", [chat.id, actor.name, q.body]);
      return { ...await enqueue(db, actor, agent, chat, q.body), private: chat.task_id === null };
    }
    case "inbox.read": {
      const row = await one(db, "UPDATE notifications SET opened_at=coalesce(opened_at,now()) WHERE id=$1 AND user_id=$2 RETURNING id", [q.id, actor.id]);
      if (!row) throw missing();
      return row;
    }
    case "onboarding.dismiss":
      await db.query("UPDATE members SET guide_dismissed=$3 WHERE org_id=$1 AND user_id=$2", [actor.orgId, actor.id, q.dismissed]);
      return { dismissed: q.dismissed };
  }
}

export function workspace(actor: Actor, input: unknown) {
  const q = commandSchema.parse(input);
  if (q.op === "workspace.read") return transaction((db) => readWorkspace(db, actor, q.spaceId), "read");
  return transaction(async (db) => {
    const result = await mutate(db, actor, q);
    const personal = q.op === "chat.start" || q.op === "onboarding.dismiss" || q.op === "inbox.read"
      || (q.op === "machine.enroll" && !q.shared)
      || (result && "private" in result && result.private === true);
    await changed(db, actor.orgId, personal ? actor.id : undefined);
    return result;
  });
}
