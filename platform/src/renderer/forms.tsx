import { useState, type FormEvent } from "react";
import { Check, Clipboard, FileCode, LoaderCircle } from "lucide-react";
import type { Command, Task, TaskState, Workspace } from "../protocol";
import { ErrorLine, Field, Layer, priorityLabel, statusLabel, typeLabel } from "./components";

export type Modal =
  | { kind: "task"; task?: Task; state?: TaskState }
  | { kind: "space" }
  | { kind: "machine" }
  | { kind: "agent" }
  | { kind: "iteration" }
  | { kind: "document"; document?: Workspace["documents"][number] }
  | { kind: "invite" }
  | { kind: "archive"; task: Task };

const titles = {
  task: "新建任务", space: "创建空间", machine: "连接本机", agent: "创建 Agent", iteration: "新建迭代",
  document: "新建文档", invite: "邀请成员", archive: "归档任务",
};
const valueOf = (data: FormData, key: string) => String(data.get(key) ?? "");

export function WorkForm({ modal, workspace, close, done }: {
  modal: Modal;
  workspace: Workspace;
  close: () => void;
  done: (selection?: { taskId?: string; spaceId?: string }) => void;
}) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [example, setExample] = useState(true);
  const [program, setProgram] = useState("");
  const [invitation, setInvitation] = useState("");
  const [copied, setCopied] = useState(false);
  const [machineId, setMachineId] = useState(workspace.machines.find((machine) => machine.online && machine.runtimes.length)?.id ?? "");
  const task = modal.kind === "task" ? modal.task : undefined;
  const doc = modal.kind === "document" ? modal.document : undefined;
  const selectedMachine = workspace.machines.find((machine) => machine.id === machineId);
  const label = task ? "编辑任务" : doc ? "编辑文档" : titles[modal.kind];
  const mutation = <T,>(q: Command) => window.shipforge.command<T>(q);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (saving) return;
    setSaving(true);
    setError("");
    const data = new FormData(event.currentTarget);
    const val = (key: string) => valueOf(data, key);
    try {
      switch (modal.kind) {
        case "space": {
          const space = await mutation<{ id: string }>({ op: "space.create", name: val("name"), description: val("description") });
          done({ spaceId: space.id });
          break;
        }
        case "task": {
          const assignee = val("assignee");
          const changes = {
            title: val("title"), description: val("description"), status: val("status") as Task["status"],
            priority: val("priority") as Task["priority"], type: val("type") as Task["type"],
            memberId: assignee.startsWith("member:") ? assignee.slice(7) : null,
            agentId: assignee.startsWith("agent:") ? assignee.slice(6) : null,
            iterationId: val("iterationId") || null, dueOn: val("dueOn") || null,
          };
          const row = await mutation<{ id: string }>(task
            ? { op: "task.update", id: task.id, revision: task.revision, changes }
            : { op: "task.create", spaceId: workspace.spaceId, ...changes });
          done({ taskId: row.id });
          break;
        }
        case "machine": {
          const args: unknown = example ? [] : JSON.parse(val("args") || "[]");
          if (!Array.isArray(args) || args.some((item) => typeof item !== "string")) throw new Error("启动参数应是字符串数组");
          await window.shipforge.startWorker({ name: val("name"), shared: Boolean(data.get("shared")), example,
            program, runtime: val("runtime"), args });
          done();
          break;
        }
        case "agent":
          await mutation({ op: "agent.create", spaceId: workspace.spaceId, name: val("name"),
            instructions: val("instructions"), machineId, runtime: val("runtime") });
          done();
          break;
        case "iteration":
          await mutation({ op: "iteration.create", spaceId: workspace.spaceId, name: val("name"), startsOn: val("startsOn"), endsOn: val("endsOn") });
          done();
          break;
        case "document":
          await mutation({ op: "document.save", spaceId: workspace.spaceId, id: doc?.id, revision: doc?.revision, title: val("title"), body: val("body") });
          done();
          break;
        case "archive":
          await mutation({ op: "task.archive", id: modal.task.id, revision: modal.task.revision });
          done({});
          break;
        case "invite": {
          const result = await mutation<{ code: string }>({ op: "member.invite", role: val("role") as "member" | "admin", expiresInDays: 7 });
          setInvitation(result.code);
          break;
        }
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSaving(false);
    }
  }
  return <Layer title={label} onClose={close}>
    {invitation ? <div className="form-body">
      <p className="muted">邀请在 7 天内可使用一次。接收方在同一服务地址选择「接受邀请」。</p>
      <Field label="邀请码"><input aria-label="邀请码" readOnly value={invitation} /></Field>
      <button className="button primary" onClick={() => void window.shipforge.copy(invitation).then(() => setCopied(true))}>
        {copied ? <Check size={16} /> : <Clipboard size={16} />}{copied ? "已复制" : "复制邀请码"}
      </button>
    </div> : <form onSubmit={submit} className="form-body">
      {modal.kind === "task" && <>
        <Field label="任务标题"><input name="title" defaultValue={task?.title} required maxLength={160} autoFocus placeholder="填写要交付的结果" /></Field>
        <Field label="任务描述"><textarea name="description" rows={5} defaultValue={task?.description} placeholder="补充背景、验收条件或所需文件" /></Field>
        <div className="form-grid">
          <Field label="状态"><select name="status" defaultValue={task?.status ?? modal.state ?? "backlog"}>{Object.entries(statusLabel).map(([id, title]) => <option key={id} value={id}>{title}</option>)}</select></Field>
          <Field label="优先级"><select name="priority" defaultValue={task?.priority ?? "none"}>{Object.entries(priorityLabel).map(([id, title]) => <option key={id} value={id}>{title}</option>)}</select></Field>
          <Field label="任务类型"><select name="type" defaultValue={task?.type ?? "task"}>{Object.entries(typeLabel).map(([id, title]) => <option key={id} value={id}>{title}</option>)}</select></Field>
          <Field label="处理人或 Agent"><select name="assignee" defaultValue={task?.agentId ? `agent:${task.agentId}` : task?.memberId ? `member:${task.memberId}` : ""}>
            <option value="">未指派</option>
            <optgroup label="成员">{workspace.members.map((member) => <option key={member.id} value={`member:${member.id}`}>{member.name}</option>)}</optgroup>
            <optgroup label="Agent">{workspace.agents.map((agent) => <option key={agent.id} value={`agent:${agent.id}`}>{agent.name}</option>)}</optgroup>
          </select></Field>
          <Field label="迭代"><select name="iterationId" defaultValue={task?.iterationId ?? ""}>
            <option value="">未排期</option>{workspace.iterations.map((iteration) => <option key={iteration.id} value={iteration.id}>{iteration.name}</option>)}
          </select></Field>
          <Field label="截止日期"><input type="date" name="dueOn" defaultValue={task?.dueOn ?? ""} /></Field>
        </div>
      </>}
      {modal.kind === "space" && <>
        <Field label="空间名称"><input name="name" autoFocus required maxLength={160} /></Field>
        <Field label="说明"><textarea name="description" rows={3} /></Field>
      </>}
      {modal.kind === "machine" && <>
        <Field label="机器名称"><input name="name" autoFocus required maxLength={160} placeholder="本机工作机" /></Field>
        <Field label="运行方式"><select value={example ? "example" : "program"} onChange={(event) => setExample(event.currentTarget.value === "example")}>
          <option value="example">示例 Runtime（检查任务链路）</option><option value="program">已有本机 Runtime（JSON 协议）</option>
        </select></Field>
        {!example && <>
          <button type="button" className="button subtle" onClick={() => void window.shipforge.pickProgram().then((file) => file && setProgram(file)).catch((cause) => setError(String(cause)))}>
            <FileCode size={16} />{program || "选择本机程序"}
          </button>
          <Field label="Runtime 名称"><input name="runtime" required maxLength={100} /></Field>
          <Field label="启动参数" hint={'字符串 JSON 数组，例如 ["local-adapter.cjs"]'}><input name="args" placeholder="[]" /></Field>
        </>}
        {workspace.actor.role !== "member" && <label className="check"><input type="checkbox" name="shared" />允许当前组织的成员调度这台机器</label>}
      </>}
      {modal.kind === "agent" && <>
        <Field label="Agent 名称"><input name="name" autoFocus required maxLength={160} /></Field>
        <Field label="工作机"><select value={machineId} onChange={(event) => setMachineId(event.currentTarget.value)} required>
          <option value="">选择可用工作机</option>{workspace.machines.filter((machine) => machine.runtimes.length).map((machine) =>
            <option value={machine.id} key={machine.id}>{machine.name}{machine.online ? "" : "（离线）"}</option>)}
        </select></Field>
        <Field label="Runtime"><select name="runtime" required key={machineId}>{selectedMachine?.runtimes.map((runtime) => <option key={runtime}>{runtime}</option>)}</select></Field>
        <Field label="执行指令"><textarea name="instructions" rows={4} placeholder="说明任务开始前要检查的内容，以及交付标准" /></Field>
      </>}
      {modal.kind === "iteration" && <>
        <Field label="迭代名称"><input name="name" required autoFocus maxLength={160} /></Field>
        <div className="form-grid">
          <Field label="开始日期"><input type="date" name="startsOn" required /></Field>
          <Field label="结束日期"><input type="date" name="endsOn" required /></Field>
        </div>
      </>}
      {modal.kind === "document" && <>
        <Field label="文档标题"><input name="title" defaultValue={doc?.title} required autoFocus maxLength={160} /></Field>
        <Field label="正文"><textarea rows={12} name="body" defaultValue={doc?.body} placeholder="可输入 Markdown 正文" /></Field>
      </>}
      {modal.kind === "invite" && <Field label="成员角色"><select name="role">
        <option value="member">组织成员</option>{workspace.actor.role === "owner" && <option value="admin">管理员</option>}
      </select></Field>}
      {modal.kind === "archive" && <p className="muted">归档「{modal.task.title}」将从当前任务视图移除它，并停止仍在执行的 Agent 运行。历史记录会保存在服务中。</p>}
      <ErrorLine message={error} />
      <div className="form-footer"><button className="button subtle" type="button" onClick={close}>取消</button>
        <button type="submit" className={`button ${modal.kind === "archive" ? "danger" : "primary"}`} disabled={saving}>
          {saving && <LoaderCircle className="spin" size={16} />}{modal.kind === "invite" ? "生成邀请" : modal.kind === "machine" ? "启用工作机" : modal.kind === "archive" ? "确认归档" : "保存"}
        </button>
      </div>
    </form>}
  </Layer>;
}
