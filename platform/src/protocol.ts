import { z } from "zod";

export const taskStates = ["backlog", "todo", "doing", "review", "done"] as const;
export const priorities = ["none", "low", "medium", "high", "urgent"] as const;
export const taskTypes = ["task", "feature", "bug"] as const;
const uuid = z.uuid();
const text = z.string().trim().min(1).max(160);
const body = z.string().max(64_000);
const taskFields = {
  title: text,
  description: body,
  status: z.enum(taskStates),
  priority: z.enum(priorities),
  type: z.enum(taskTypes),
  memberId: uuid.nullable(),
  agentId: uuid.nullable(),
  iterationId: uuid.nullable(),
  dueOn: z.iso.date().nullable(),
};
export const loginSchema = z.object({
  username: z.string().trim().toLowerCase().regex(/^[a-z0-9][a-z0-9._-]{2,63}$/),
  password: z.string().min(10).max(128),
});
export const setupSchema = loginSchema.extend({
  name: text,
  organization: text,
  firstSpace: text,
  bootstrapKey: z.string().max(256).optional(),
});

export const commandSchema = z.discriminatedUnion("op", [
  z.object({ op: z.literal("workspace.read"), spaceId: uuid.optional() }),
  z.object({ op: z.literal("space.create"), name: text, description: body.optional() }),
  z.object({ op: z.literal("task.create"), spaceId: uuid, ...taskFields }).partial({
    description: true, status: true, priority: true, type: true,
    memberId: true, agentId: true, iterationId: true, dueOn: true,
  }),
  z.object({
    op: z.literal("task.update"), id: uuid, revision: z.number().int().positive(),
    changes: z.object(taskFields).partial().strict(),
  }),
  z.object({ op: z.literal("task.comment"), id: uuid, body: body.trim().min(1) }),
  z.object({ op: z.literal("task.archive"), id: uuid, revision: z.number().int().positive() }),
  z.object({
    op: z.literal("iteration.create"), spaceId: uuid, name: text,
    startsOn: z.iso.date(), endsOn: z.iso.date(),
  }),
  z.object({
    op: z.literal("iteration.state"), id: uuid, status: z.enum(["planned", "active", "closed"]),
  }),
  z.object({
    op: z.literal("document.save"), spaceId: uuid, id: uuid.optional(),
    revision: z.number().int().positive().optional(), title: text, body,
  }),
  z.object({
    op: z.literal("agent.create"), spaceId: uuid, name: text,
    instructions: body, machineId: z.string().min(1).max(80), runtime: text,
  }),
  z.object({ op: z.literal("run.start"), taskId: uuid }),
  z.object({ op: z.literal("run.cancel"), id: uuid }),
  z.object({ op: z.literal("chat.start"), agentId: uuid, body: body.trim().min(1) }),
  z.object({ op: z.literal("chat.send"), id: uuid, body: body.trim().min(1) }),
  z.object({ op: z.literal("inbox.read"), id: uuid }),
  z.object({ op: z.literal("onboarding.dismiss"), dismissed: z.boolean() }),
  z.object({ op: z.literal("member.invite"), role: z.enum(["admin", "member"]), expiresInDays: z.number().int().min(1).max(30) }),
  z.object({ op: z.literal("machine.enroll"), name: text, shared: z.boolean().optional() }),
]);

export type Command = z.infer<typeof commandSchema>;
export type TaskState = (typeof taskStates)[number];
export type TaskPriority = (typeof priorities)[number];
export type TaskType = (typeof taskTypes)[number];

export interface Actor {
  id: string;
  username: string;
  name: string;
  orgId: string;
  orgName: string;
  role: "owner" | "admin" | "member";
}

export interface Task {
  id: string;
  number: number;
  title: string;
  description: string;
  status: TaskState;
  type: TaskType;
  priority: TaskPriority;
  memberId: string | null;
  agentId: string | null;
  iterationId: string | null;
  dueOn: string | null;
  revision: number;
  updatedAt: string;
}

export interface Workspace {
  actor: Actor;
  spaces: { id: string; name: string; description: string }[];
  spaceId: string;
  tasks: Task[];
  comments: { id: string; taskId: string; author: string; body: string; createdAt: string }[];
  members: { id: string; name: string; username: string; role: Actor["role"] }[];
  agents: { id: string; name: string; instructions: string; runtime: string; machineId: string; spaceId: string }[];
  machines: { id: string; name: string; ownerId: string; online: boolean; runtimes: string[]; shared: boolean }[];
  runs: { id: string; taskId: string | null; chatId: string; agentId: string; status: string; detail: string; createdAt: string }[];
  chats: { id: string; agentId: string; taskId: string | null; title: string; updatedAt: string }[];
  messages: { id: string; chatId: string; sender: string; body: string; createdAt: string }[];
  documents: { id: string; title: string; body: string; revision: number; updatedAt: string }[];
  iterations: { id: string; name: string; startsOn: string; endsOn: string; status: string }[];
  inbox: { id: string; title: string; taskId: string; read: boolean; createdAt: string }[];
  guide: { dismissed: boolean; task: boolean; machine: boolean; agent: boolean; execution: boolean };
}
