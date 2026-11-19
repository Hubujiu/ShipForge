import { useState, type FormEvent } from "react";
import { Archive, Bot, CalendarDays, CircleDot, LayoutGrid, List, MessageCircle, Play, Plus, Search, Square, UserRound } from "lucide-react";
import type { Command, Task, TaskState, Workspace } from "../protocol";
import { dateTime, Empty, ErrorLine, Field, Layer, priorityLabel, statusLabel, Status, typeLabel } from "./components";

export function TaskBoard({ workspace, open, create, update, mine = false }: {
  workspace: Workspace;
  open: (id: string) => void;
  create: (status?: TaskState) => void;
  update: (task: Task, status: TaskState) => void;
  mine?: boolean;
}) {
  const [layout, setLayout] = useState<"board" | "list">(mine ? "list" : "board");
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState("");
  const [iteration, setIteration] = useState("");
  const phrase = query.trim().toLocaleLowerCase();
  const tasks = workspace.tasks.filter((task) => (!mine || task.memberId === workspace.actor.id)
    && (!status || task.status === status) && (!iteration || task.iterationId === iteration)
    && (!phrase || `${task.title} ${task.description} SF-${task.number}`.toLocaleLowerCase().includes(phrase)));
  const memberName = (task: Task) => workspace.members.find((member) => member.id === task.memberId)?.name;
  const agentName = (task: Task) => workspace.agents.find((agent) => agent.id === task.agentId)?.name;
  const lanes = (Object.keys(statusLabel) as TaskState[]).filter((state) => !status || state === status);
  return <section className="task-content">
    <div className="tool-row">
      <label className="search-input"><Search size={16} /><input aria-label="筛选任务" placeholder="按名称或编号筛选" value={query} onChange={(event) => setQuery(event.currentTarget.value)} /></label>
      <select aria-label="筛选状态" value={status} onChange={(event) => setStatus(event.currentTarget.value)}>
        <option value="">全部状态</option>{Object.entries(statusLabel).map(([key, title]) => <option key={key} value={key}>{title}</option>)}
      </select>
      {!!workspace.iterations.length && <select aria-label="筛选迭代" value={iteration} onChange={(event) => setIteration(event.currentTarget.value)}>
        <option value="">所有迭代</option>{workspace.iterations.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
      </select>}
      <span className="tool-spacer" />
      <div className="segment" aria-label="任务布局">
        <button aria-pressed={layout === "board"} onClick={() => setLayout("board")}><LayoutGrid size={16} />看板</button>
        <button aria-pressed={layout === "list"} onClick={() => setLayout("list")}><List size={17} />列表</button>
      </div>
    </div>
    {layout === "board" ? <div className="board-grid">
      {lanes.map((state) => {
        const items = tasks.filter((task) => task.status === state);
        return <section key={state} className="lane"
          onDragOver={(event) => event.preventDefault()}
          onDrop={(event) => {
            event.preventDefault();
            const id = event.dataTransfer.getData("application/x-shipforge-task");
            const item = workspace.tasks.find((task) => task.id === id);
            if (item && item.status !== state) update(item, state);
          }}>
          <div className="lane-title"><Status state={state} /><span className="lane-count">{items.length}</span>
            <button className="icon-button" onClick={() => create(state)} aria-label={`在${statusLabel[state]}新建任务`}><Plus size={16} /></button></div>
          <div className="lane-cards">
            {items.map((task) => <button key={task.id} className="task-card" draggable
              onDragStart={(event) => {
                event.dataTransfer.effectAllowed = "move";
                event.dataTransfer.setData("application/x-shipforge-task", task.id);
              }}
              onClick={() => open(task.id)}>
              <span className="card-kicker"><span>SF-{task.number}</span><span className={`task-type type-${task.type}`}>{typeLabel[task.type]}</span></span>
              <strong>{task.title}</strong>
              {task.description && <span className="task-summary">{task.description}</span>}
              <span className="card-meta">
                {agentName(task) ? <span><Bot size={14} />{agentName(task)}</span>
                  : memberName(task) ? <span><UserRound size={14} />{memberName(task)}</span> : <span>未指派</span>}
                {task.priority !== "none" && <span className={`priority priority-${task.priority}`}>{priorityLabel[task.priority]}</span>}
              </span>
            </button>)}
            {!items.length && <button className="empty-lane" onClick={() => create(state)}><Plus size={16} />添加任务</button>}
          </div>
        </section>;
      })}
    </div> : tasks.length ? <div className="task-table">
      <div className="table-head"><span>编号 / 任务</span><span>状态</span><span>处理人</span><span>截止日期</span></div>
      {tasks.map((task) => <button key={task.id} className="table-row" onClick={() => open(task.id)}>
        <span><small>SF-{task.number}</small><strong>{task.title}</strong></span>
        <Status state={task.status} /><span>{agentName(task) ?? memberName(task) ?? "未指派"}</span>
        <span>{task.dueOn || "—"}</span>
      </button>)}
    </div> : <Empty title={mine ? "暂无分配给你的任务" : "没有符合筛选的任务"} hint={mine ? "任务分配给你后会出现在这里。" : "修改筛选条件，或创建一条任务。"}
      action={mine ? undefined : () => create()}>新建任务</Empty>}
  </section>;
}

export function TaskDetails({ task, workspace, close, edit, archive, run, openChat, pending, error }: {
  task: Task;
  workspace: Workspace;
  close: () => void;
  edit: () => void;
  archive: () => void;
  run: (command: Command) => Promise<boolean>;
  openChat: (id: string) => void;
  pending: boolean;
  error: string;
}) {
  const [comment, setComment] = useState("");
  const live = workspace.runs.filter((item) => item.taskId === task.id);
  const chats = workspace.chats.filter((item) => item.taskId === task.id);
  const assignee = workspace.agents.find((agent) => agent.id === task.agentId)?.name
    ?? workspace.members.find((member) => member.id === task.memberId)?.name;
  const iteration = workspace.iterations.find((item) => item.id === task.iterationId);
  async function reply(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (comment.trim() && await run({ op: "task.comment", id: task.id, body: comment })) setComment("");
  }
  return <Layer title={`任务 SF-${task.number}`} onClose={close} drawer>
    <div className="drawer-content">
      <div className="drawer-title"><span className={`task-type type-${task.type}`}>{typeLabel[task.type]}</span><h2>{task.title}</h2></div>
      <div className="detail-meta">
        <Field label="状态"><select value={task.status} onChange={(event) => void run({ op: "task.update", id: task.id, revision: task.revision, changes: { status: event.currentTarget.value as TaskState } })} disabled={pending}>
          {Object.entries(statusLabel).map(([id, name]) => <option key={id} value={id}>{name}</option>)}
        </select></Field>
        <div><small>处理人</small><span>{assignee || "未指派"}</span></div>
        <div><small>优先级</small><span>{priorityLabel[task.priority]}</span></div>
        <div><small>排期</small><span><CalendarDays size={14} />{iteration?.name ?? task.dueOn ?? "未排期"}</span></div>
      </div>
      <div className="tool-row detail-actions"><button className="button subtle" onClick={edit}>编辑任务</button>
        <button className="button ghost" onClick={archive}><Archive size={16} />归档</button></div>
      <section className="detail-section"><h3>任务描述</h3><p className={task.description ? "prose-text" : "muted"}>{task.description || "暂无任务描述。"}</p></section>
      <section className="detail-section"><div className="detail-heading"><h3><Bot size={17} />执行记录</h3>
        {task.agentId && ["todo", "doing", "review"].includes(task.status) && !live.some((item) => ["queued", "running", "canceling"].includes(item.status))
          && <button className="button subtle" disabled={pending} onClick={() => void run({ op: "run.start", taskId: task.id })}><Play size={15} />发起运行</button>}</div>
        {live.map((item) => <div key={item.id} className="run-item">
          <Status state={item.status} /><span className="muted">{dateTime(item.createdAt)}</span>
          <button className="text-button" onClick={() => openChat(item.chatId)}>查看对话</button>
          {["queued", "running"].includes(item.status) && <button className="icon-button" aria-label="停止运行" disabled={pending}
            onClick={() => void run({ op: "run.cancel", id: item.id })}><Square size={15} /></button>}
          {item.detail && <p>{item.detail}</p>}
        </div>)}
        {!live.length && <p className="muted">{task.agentId ? "任务进入可执行状态后会派发给 Agent。" : "将任务指派给 Agent 后，可在这里查看进度。"}</p>}
        {chats.length > 0 && !live.length && <button className="text-button" onClick={() => openChat(chats[0].id)}>查看历史对话</button>}
      </section>
      <section className="detail-section"><div className="detail-heading"><h3><MessageCircle size={17} />评论</h3></div>
        {workspace.comments.filter((item) => item.taskId === task.id).map((item) => <article className="comment" key={item.id}>
          <strong>{item.author}</strong><small>{dateTime(item.createdAt)}</small><p>{item.body}</p>
        </article>)}
        <form onSubmit={reply} className="inline-form">
          <Field label="添加评论"><textarea rows={3} name="comment" value={comment} onChange={(event) => setComment(event.currentTarget.value)} placeholder="与团队讨论这项任务" /></Field>
          <button className="button primary" disabled={!comment.trim() || pending} type="submit"><CircleDot size={15} />发送评论</button>
        </form>
      </section>
      <ErrorLine message={error} />
    </div>
  </Layer>;
}
