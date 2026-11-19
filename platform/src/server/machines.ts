import { z } from "zod";
import { type Principal, digest } from "./auth";
import { type Db, changed, conflict, forbidden, invalid, one, rows, transaction } from "./database";

type Worker = Extract<Principal, { kind: "worker" }>;
const inputSchema = z.object({
  op: z.string(),
  machine_id: z.uuid(),
  machine_key: z.string().min(32).max(256),
}).loose();
type Packet = z.infer<typeof inputSchema>;

async function checkKey(db: Db, worker: Worker, q: Packet) {
  if (q.machine_id !== worker.id) throw forbidden("The credential belongs to another machine");
  const hash = digest(q.machine_key);
  const machine = await one<{ key_digest: string | null }>(db, "SELECT key_digest FROM machines WHERE id=$1 FOR UPDATE", [worker.id]);
  if (!machine || (machine.key_digest ? machine.key_digest !== hash : q.op !== "machine.register")) {
    throw forbidden("Machine authentication failed");
  }
  return hash;
}

async function expireRuns(db: Db, worker: Worker) {
  const failed = await rows<{ chat_id: string; id: string; detail: string }>(db, `UPDATE runs SET
    status=CASE WHEN status='canceling' THEN 'canceled' ELSE 'failed' END,
    detail='工作机未及时续租。本次运行已停止，可以重新发起。',updated_at=now()
    WHERE machine_id=$1 AND status IN ('running','canceling') AND lease_until<now() RETURNING chat_id,id,detail`, [worker.id]);
  for (const row of failed) {
    await db.query("INSERT INTO messages(chat_id,run_id,sender,body) VALUES($1,$2,'系统',$3)", [row.chat_id, row.id, row.detail]);
  }
  if (failed.length) await changed(db, worker.orgId);
}

async function claim(db: Db, worker: Worker) {
  await expireRuns(db, worker);
  const run = await one<{
    id: string; chat_id: string; task_id: string | null; lease: string; requested_by: string;
    instruction: string; runtime: string; instructions: string; agent_name: string; owner_id: string;
  }>(db, `WITH next AS (
      SELECT r.id FROM runs r JOIN machines m ON m.id=r.machine_id JOIN agents a ON a.id=r.agent_id
      WHERE r.machine_id=$1 AND r.status='queued' AND m.runtimes ? a.runtime
        AND (m.owner_id=r.requested_by OR m.shared)
        AND m.capacity>(SELECT count(*) FROM runs running
          WHERE running.machine_id=m.id AND running.status IN ('running','canceling') AND running.lease_until>now())
      ORDER BY r.created_at,r.id LIMIT 1 FOR UPDATE OF r SKIP LOCKED
    )
    UPDATE runs r SET status='running',lease=gen_random_uuid(),lease_until=now()+interval '90 seconds',updated_at=now()
    FROM next,agents a,chats c WHERE r.id=next.id AND a.id=r.agent_id AND c.id=r.chat_id
    RETURNING r.id,r.chat_id,r.task_id,r.lease,r.requested_by,r.instruction,a.runtime,a.instructions,a.name AS agent_name,c.owner_id`,
  [worker.id]);
  if (!run) return null;
  const task = run.task_id ? await one(db, `SELECT id,number,title,description,status,priority,type
    FROM tasks WHERE id=$1`, [run.task_id]) : undefined;
  const comments = run.task_id ? (await rows(db, `SELECT u.name AS author,left(c.body,4000) AS body,c.created_at
    FROM comments c JOIN users u ON u.id=c.author_id WHERE c.task_id=$1 ORDER BY c.created_at DESC LIMIT 20`, [run.task_id])).reverse() : [];
  const messages = (await rows(db, "SELECT sender,left(body,4000) AS body FROM messages WHERE chat_id=$1 ORDER BY created_at DESC LIMIT 20",
    [run.chat_id])).reverse();
  await changed(db, worker.orgId, run.task_id ? undefined : run.owner_id);
  return { lease: run.lease, run: { id: run.id, kind: "run", title: run.instruction.slice(0, 160), fields: {
    runtime: run.runtime, requested_by: run.requested_by, handler_kind: "agent",
    instruction: run.instruction, task, comments, messages, chat_id: run.chat_id,
    config: { name: run.agent_name, instructions: run.instructions, directory_mode: "temporary" },
  } } };
}

async function execution(db: Db, worker: Worker, q: Packet) {
  const { id, lease } = z.object({ id: z.uuid(), lease: z.uuid() }).parse(q);
  const current = await one<{ status: string; detail: string; task_id: string | null; chat_id: string; owner_id: string;
    report_digest: string | null; valid: boolean }>(db, `SELECT r.status,r.detail,r.task_id,r.chat_id,r.report_digest,
      r.lease_until>now() AS valid,c.owner_id FROM runs r JOIN chats c ON c.id=r.chat_id
      WHERE r.id=$1 AND r.lease=$2 AND r.machine_id=$3 FOR UPDATE OF r`, [id, lease, worker.id]);
  if (!current) throw conflict("stale lease");
  const active = ["running", "canceling"].includes(current.status);
  if (q.op === "run.heartbeat") {
    if (!active || !current.valid) throw conflict("stale lease");
    const progress = z.object({ message: z.string().max(2048) }).safeParse(q.progress);
    const detail = progress.success ? progress.data.message : current.detail;
    await db.query("UPDATE runs SET lease_until=now()+interval '90 seconds',detail=$2 WHERE id=$1", [id, detail]);
    if (detail !== current.detail) await changed(db, worker.orgId, current.task_id ? undefined : current.owner_id);
    return { cancel: current.status === "canceling" };
  }
  const report = z.object({
    status: z.enum(["done", "failed", "blocked", "waiting", "cancelled", "canceled"]),
    result: z.string().max(120_000).optional(),
  }).parse(q);
  const reportKey = digest(JSON.stringify(report));
  if (!active && current.report_digest === reportKey) return { id, status: current.status };
  if (!active || !current.valid) throw conflict("stale lease");
  const status = current.status === "canceling" || report.status === "cancelled" ? "canceled"
    : report.status === "waiting" ? "blocked" : report.status;
  const detail = report.result?.trim() || {
    done: "运行已完成。", failed: "运行失败。", blocked: "等待补充信息。", canceled: "运行已取消。",
  }[status];
  await db.query("UPDATE runs SET status=$2,detail=$3,report_digest=$4,updated_at=now() WHERE id=$1",
    [id, status, detail, reportKey]);
  await db.query(`INSERT INTO messages(chat_id,run_id,sender,body)
    SELECT $1,$2,a.name,$3 FROM runs r JOIN agents a ON a.id=r.agent_id WHERE r.id=$2`,
  [current.chat_id, id, detail]);
  await db.query("UPDATE chats SET updated_at=now() WHERE id=$1", [current.chat_id]);
  if (current.task_id) {
    await db.query(`INSERT INTO notifications(user_id,task_id,title)
      SELECT r.requested_by,t.id,$2||' · '||t.title FROM runs r JOIN tasks t ON t.id=r.task_id
      WHERE r.id=$1 AND t.archived_at IS NULL`, [id, status === "done" ? "Agent 已完成运行" : "Agent 运行状态更新"]);
  }
  await changed(db, worker.orgId, current.task_id ? undefined : current.owner_id);
  return { id, status };
}

export function machineCommand(worker: Worker, input: unknown) {
  const q = inputSchema.parse(input);
  return transaction(async (db) => {
    const key = await checkKey(db, worker, q);
    switch (q.op) {
      case "machine.register": {
        const data = z.object({
          runtimes: z.array(z.string().min(1).max(160)).max(128),
          capacity: z.number().int().min(1).max(64),
        }).parse(q);
        await db.query("UPDATE machines SET key_digest=$2,runtimes=$3::jsonb,capacity=$4,last_seen=now() WHERE id=$1",
          [worker.id, key, JSON.stringify(data.runtimes), data.capacity]);
        await changed(db, worker.orgId);
        return { id: worker.id };
      }
      case "machine.heartbeat": {
        const row = await one<{ online: boolean }>(db, "SELECT coalesce(last_seen>now()-interval '60 seconds',false) AS online FROM machines WHERE id=$1", [worker.id]);
        await db.query("UPDATE machines SET last_seen=now() WHERE id=$1", [worker.id]);
        await expireRuns(db, worker);
        if (!row?.online) await changed(db, worker.orgId);
        return { id: worker.id };
      }
      case "machine.stopped":
        await db.query("UPDATE machines SET last_seen=NULL WHERE id=$1", [worker.id]);
        await changed(db, worker.orgId);
        return { id: worker.id };
      case "run.claim":
        return claim(db, worker);
      case "run.heartbeat":
      case "run.report":
        return execution(db, worker, q);
      // Capability pollers from the standalone worker accept an empty queue.
      case "automation.tick": return { runs: [] };
      case "machine.runtime.jobs":
      case "machine.storage.take":
      case "skill.jobs": return null;
      case "machine.runtime.publish":
      case "skill.sources.publish": return { id: worker.id };
      default: throw invalid("This service does not support that machine operation");
    }
  });
}
