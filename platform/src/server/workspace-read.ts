import type { Actor, Workspace } from "../protocol";
import { type Db, missing, one, rows } from "./database";

export async function readWorkspace(db: Db, actor: Actor, spaceId?: string): Promise<Workspace> {
  const spaces = await rows<Workspace["spaces"][number]>(db,
    "SELECT id,name,description FROM spaces WHERE org_id=$1 ORDER BY created_at,id", [actor.orgId]);
  const selected = spaceId ?? spaces[0]?.id;
  if (!spaces.some((space) => space.id === selected)) throw missing("Space is unavailable");
  const id = selected!;
  const [tasks, comments, members, agents, machines, runs, chats, messages, documents, iterations, inbox, guide] =
    await Promise.all([
      rows<Workspace["tasks"][number]>(db, `SELECT id,number,title,description,status,type,priority,member_id AS "memberId",
        agent_id AS "agentId",iteration_id AS "iterationId",due_on::text AS "dueOn",revision,updated_at AS "updatedAt"
        FROM tasks WHERE space_id=$1 AND archived_at IS NULL ORDER BY number DESC`, [id]),
      rows<Workspace["comments"][number]>(db, `SELECT c.id,c.task_id AS "taskId",u.name AS author,c.body,c.created_at AS "createdAt"
        FROM comments c JOIN tasks t ON t.id=c.task_id JOIN users u ON u.id=c.author_id
        WHERE t.space_id=$1 AND t.archived_at IS NULL ORDER BY c.created_at,c.id`, [id]),
      rows<Workspace["members"][number]>(db, `SELECT u.id,u.name,u.username,m.role
        FROM members m JOIN users u ON u.id=m.user_id WHERE m.org_id=$1 ORDER BY u.name,u.id`, [actor.orgId]),
      rows<Workspace["agents"][number]>(db, `SELECT a.id,a.name,a.instructions,a.machine_id AS "machineId",a.space_id AS "spaceId",a.runtime
        FROM agents a JOIN machines m ON m.id=a.machine_id WHERE a.space_id=$1 AND (m.owner_id=$2 OR m.shared)
        ORDER BY a.created_at,a.id`, [id, actor.id]),
      rows<Workspace["machines"][number]>(db, `SELECT id,name,owner_id AS "ownerId",runtimes,shared,
        coalesce(last_seen > now()-interval '60 seconds',false) AS online FROM machines
        WHERE org_id=$1 AND (owner_id=$2 OR shared) ORDER BY created_at,id`, [actor.orgId, actor.id]),
      rows<Workspace["runs"][number]>(db, `SELECT r.id,r.task_id AS "taskId",r.chat_id AS "chatId",r.agent_id AS "agentId",
        r.status,r.detail,r.created_at AS "createdAt" FROM runs r JOIN chats c ON c.id=r.chat_id
        WHERE c.space_id=$1 AND (c.task_id IS NOT NULL OR c.owner_id=$2) ORDER BY r.created_at DESC,r.id`, [id, actor.id]),
      rows<Workspace["chats"][number]>(db, `SELECT c.id,c.agent_id AS "agentId",c.task_id AS "taskId",c.title,c.updated_at AS "updatedAt"
        FROM chats c WHERE c.space_id=$1 AND (c.task_id IS NOT NULL OR c.owner_id=$2) ORDER BY c.updated_at DESC,c.id`, [id, actor.id]),
      rows<Workspace["messages"][number]>(db, `SELECT m.id,m.chat_id AS "chatId",m.sender,m.body,m.created_at AS "createdAt"
        FROM messages m JOIN chats c ON c.id=m.chat_id WHERE c.space_id=$1 AND (c.task_id IS NOT NULL OR c.owner_id=$2)
        ORDER BY m.created_at,m.id`, [id, actor.id]),
      rows<Workspace["documents"][number]>(db, `SELECT id,title,body,revision,updated_at AS "updatedAt" FROM documents
        WHERE space_id=$1 ORDER BY updated_at DESC,id`, [id]),
      rows<Workspace["iterations"][number]>(db, `SELECT id,name,starts_on::text AS "startsOn",ends_on::text AS "endsOn",status
        FROM iterations WHERE space_id=$1 ORDER BY starts_on DESC,id`, [id]),
      rows<Workspace["inbox"][number]>(db, `SELECT n.id,n.title,n.task_id AS "taskId",n.opened_at IS NOT NULL AS read,n.created_at AS "createdAt"
        FROM notifications n JOIN tasks t ON t.id=n.task_id WHERE n.user_id=$1 AND t.space_id=$2 AND t.archived_at IS NULL
        ORDER BY n.created_at DESC,n.id`, [actor.id, id]),
      one<Workspace["guide"]>(db, `SELECT m.guide_dismissed AS dismissed,
        EXISTS(SELECT 1 FROM tasks t JOIN spaces s ON s.id=t.space_id WHERE s.org_id=$1 AND t.created_by=$2) AS task,
        EXISTS(SELECT 1 FROM machines WHERE org_id=$1 AND owner_id=$2 AND last_seen IS NOT NULL) AS machine,
        EXISTS(SELECT 1 FROM agents a JOIN spaces s ON s.id=a.space_id WHERE s.org_id=$1 AND a.owner_id=$2) AS agent,
        EXISTS(SELECT 1 FROM runs r JOIN chats c ON c.id=r.chat_id JOIN spaces s ON s.id=c.space_id
          WHERE s.org_id=$1 AND r.requested_by=$2 AND r.status='done') AS execution
        FROM members m WHERE m.org_id=$1 AND m.user_id=$2`, [actor.orgId, actor.id]),
    ]);
  if (!guide) throw missing();
  return { actor, spaces, spaceId: id, tasks, comments, members, agents, machines, runs, chats, messages, documents, iterations, inbox, guide };
}
