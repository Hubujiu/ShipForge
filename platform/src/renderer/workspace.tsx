import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowRight, ArrowUpRight, Bell, BookOpen, Bot, CalendarRange, Check, ChevronDown, CircleDot,
  Cpu, FolderKanban, Gauge, LayoutGrid, ListTodo, LogOut, MessageSquare, Plus, Search, Users } from "lucide-react";
import type { Command, TaskState, Workspace } from "../protocol";
import { ChatHub } from "./chats";
import { dateTime, Empty, ErrorLine, Field, iterationLabel, Layer, Mark, SectionTitle, Status } from "./components";
import { WorkForm, type Modal } from "./forms";
import { TaskBoard, TaskDetails } from "./tasks";

const navigation = [
  { group: "工作", items: [["home", "工作台", Gauge], ["tasks", "空间任务", FolderKanban], ["mine", "我的任务", ListTodo],
    ["inbox", "收件箱", Bell], ["chats", "对话", MessageSquare]] },
  { group: "规划", items: [["iterations", "迭代", CalendarRange], ["documents", "文档库", BookOpen]] },
  { group: "管理", items: [["agents", "Agent", Bot], ["machines", "工作机", Cpu], ["members", "成员", Users]] },
] as const;
type View = (typeof navigation)[number]["items"][number][0] | "guide";
type NavItem = (typeof navigation)[number]["items"][number];

export function WorkspaceApp({ initial, signOut }: { initial: Workspace; signOut: () => void }) {
  const [workspace, setWorkspace] = useState(initial);
  const [view, setView] = useState<View>(initial.guide.dismissed || initial.guide.execution ? "home" : "guide");
  const [modal, setModal] = useState<Modal>();
  const [taskId, setTaskId] = useState<string>();
  const [agentId, setAgentId] = useState<string>();
  const [chatId, setChatId] = useState<string>();
  const [search, setSearch] = useState<string>();
  const [spaceOpen, setSpaceOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [syncing, setSyncing] = useState(false);
  const busy = useRef(false);
  const generation = useRef(0);
  const currentSpace = useRef(initial.spaceId);
  const action = <T,>(q: Command) => window.shipforge.command<T>(q);
  const refresh = useCallback(async (scope = currentSpace.current) => {
    const sequence = ++generation.current;
    currentSpace.current = scope;
    try {
      const result = await window.shipforge.command<Workspace>({ op: "workspace.read", spaceId: scope });
      if (generation.current === sequence) {
        setWorkspace(result);
        setSyncing(false);
      }
      return result;
    } catch (cause) {
      if (generation.current === sequence) setSyncing(true);
      throw cause;
    }
  }, []);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const safeRefresh = () => void refresh().catch(() => undefined);
    const unsubscribe = window.shipforge.onChange(() => {
      clearTimeout(timer);
      timer = setTimeout(safeRefresh, 160);
    });
    const periodic = setInterval(safeRefresh, 20_000);
    return () => { clearTimeout(timer); clearInterval(periodic); unsubscribe(); };
  }, [refresh]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearch("");
      }
      if (event.key === "Escape") {
        if (search !== undefined) setSearch(undefined);
        else if (modal) setModal(undefined);
        else if (taskId) setTaskId(undefined);
        else setSpaceOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [modal, search, taskId]);
  const admin = workspace.actor.role !== "member";
  const space = workspace.spaces.find((item) => item.id === workspace.spaceId);
  const task = workspace.tasks.find((item) => item.id === taskId);
  const unread = workspace.inbox.filter((item) => !item.read).length;
  const activeRuns = workspace.runs.filter((run) => ["queued", "running", "canceling"].includes(run.status)).length;
  const canCreateAgent = workspace.machines.some((machine) => machine.runtimes.length > 0);
  const done = [workspace.guide.task, workspace.guide.machine, workspace.guide.agent, workspace.guide.execution].filter(Boolean).length;
  const section = navigation.flatMap<NavItem>((entry) => entry.items).find(([id]) => id === view)?.[1] ?? "开始使用";
  const prefix = workspace.actor.name.trim().slice(0, 1).toUpperCase();
  const createTask = (state?: TaskState) => setModal({ kind: "task", state });

  function navigate(target: View) {
    setView(target);
    setTaskId(undefined);
    setError("");
  }
  function openChat(id: string) {
    navigate("chats");
    setChatId(id);
  }
  function startChat(id: string) {
    navigate("chats");
    setAgentId(id);
    setChatId(undefined);
  }
  async function run<T>(q: Command): Promise<T | undefined> {
    if (busy.current) return undefined;
    setError("");
    setPending(true);
    busy.current = true;
    try {
      const result = await action<T>(q);
      await refresh();
      return result;
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      return undefined;
    } finally {
      busy.current = false;
      setPending(false);
    }
  }
  async function switchSpace(id: string) {
    const previous = workspace.spaceId;
    setSpaceOpen(false);
    setChatId(undefined);
    setAgentId(undefined);
    setTaskId(undefined);
    setError("");
    try { await refresh(id); }
    catch (cause) {
      currentSpace.current = previous;
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  const pageActions = {
    home: <button className="button primary" onClick={() => createTask()}><Plus size={17} />新建任务</button>,
    tasks: <button className="button primary" onClick={() => createTask()}><Plus size={17} />新建任务</button>,
    mine: <button className="button primary" onClick={() => createTask()}><Plus size={17} />新建任务</button>,
    inbox: undefined, chats: undefined, guide: undefined,
    iterations: <button className="button primary" onClick={() => setModal({ kind: "iteration" })}><Plus size={17} />新建迭代</button>,
    documents: <button className="button primary" onClick={() => setModal({ kind: "document" })}><Plus size={17} />新建文档</button>,
    agents: <button className="button primary" disabled={!canCreateAgent} onClick={() => setModal({ kind: "agent" })}><Plus size={17} />创建 Agent</button>,
    machines: <button className="button primary" onClick={() => setModal({ kind: "machine" })}><Plus size={17} />连接本机</button>,
    members: admin ? <button className="button primary" onClick={() => setModal({ kind: "invite" })}><Plus size={17} />邀请成员</button> : undefined,
  };
  const guide = [
    { icon: ListTodo, title: "创建第一项任务", hint: "记录目标和验收条件，先放在待整理。", complete: workspace.guide.task,
      action: () => createTask(), label: "新建任务" },
    { icon: Cpu, title: "让工作机上线", hint: "连接本机，并验证至少一个 Runtime。", complete: workspace.guide.machine,
      action: () => setModal({ kind: "machine" }), label: "连接本机" },
    { icon: Bot, title: "为工作机配置 Agent", hint: "设置角色指令，明确它应该如何开始。", complete: workspace.guide.agent,
      action: () => navigate("agents"), label: "查看 Agent" },
    { icon: Check, title: "完成一次任务运行", hint: "把任务交给 Agent 并移到待处理，查看原任务里的运行结果。", complete: workspace.guide.execution,
      action: () => navigate("tasks"), label: "打开任务看板" },
  ];
  const pages: Record<View, string> = {
    home: "查看当前空间的工作、工作机和执行活动。", tasks: "跟踪从待整理到验收的交付进度。",
    mine: "查看当前空间指派给你的工作。", inbox: "查看任务指派、评论和 Agent 运行通知。",
    chats: "", guide: "用一项真实任务跑通团队和 Agent 的协作过程。",
    iterations: "按时间安排任务，在迭代中查看进度。", documents: "保存团队说明、决策与任务背景。",
    agents: "配置工作角色，并选择运行它的工作机。", machines: "管理可接收任务的机器和本机 Runtime。",
    members: "查看组织成员与可用角色。",
  };
  const searchPhrase = search?.trim().toLocaleLowerCase();
  const taskMatches = searchPhrase ? workspace.tasks.filter((item) =>
    `${item.title} ${item.description} SF-${item.number}`.toLocaleLowerCase().includes(searchPhrase)).slice(0, 12) : [];
  const docMatches = searchPhrase ? workspace.documents.filter((item) => item.title.toLocaleLowerCase().includes(searchPhrase)).slice(0, 8) : [];
  return <div className="application">
    <aside className="rail">
      <div className="brand"><Mark /><div><strong>ShipForge</strong><small>{workspace.actor.orgName}</small></div></div>
      <div className="space-switch">
        <button className="space-trigger" onClick={() => setSpaceOpen(!spaceOpen)} aria-expanded={spaceOpen}><LayoutGrid size={16} /><span>{space?.name}</span><ChevronDown size={15} /></button>
        {spaceOpen && <div className="space-menu">
          {workspace.spaces.map((item) => <button key={item.id} onClick={() => void switchSpace(item.id)}>{item.name}{item.id === space?.id && <Check size={15} />}</button>)}
          {admin && <button onClick={() => { setSpaceOpen(false); setModal({ kind: "space" }); }}><Plus size={15} />创建空间</button>}
        </div>}
      </div>
      <nav aria-label="工作区导航">
        {navigation.map(({ group, items }) => <div className="nav-group" key={group}>
          <span>{group}</span>
          {items.map(([id, label, Icon]) => <button className={view === id ? "active" : ""} onClick={() => navigate(id)} key={id}
              aria-current={view === id ? "page" : undefined}><Icon size={17} /><span>{label}</span>{id === "inbox" && unread > 0 && <b>{unread}</b>}</button>)}
        </div>)}
      </nav>
      <div className="rail-footer">
        <button className={`guide-nav ${view === "guide" ? "active" : ""}`} onClick={() => navigate("guide")}><CircleDot size={17} />开始使用<span>{done}/4</span></button>
        <div className="account"><div className="avatar">{prefix}</div><div><strong>{workspace.actor.name}</strong><small>{workspace.actor.role === "owner" ? "所有者" : workspace.actor.role === "admin" ? "管理员" : "成员"}</small></div>
          <button className="icon-button" aria-label="退出登录" onClick={signOut}><LogOut size={16} /></button></div>
      </div>
    </aside>
    <main className="main">
      <header className="topbar"><div className="breadcrumbs"><span>{space?.name}</span><span>/</span><strong>{section}</strong></div>
        <button className="top-search" onClick={() => setSearch("")}><Search size={16} />搜索工作区<kbd>Ctrl K</kbd></button>
        <span title={syncing ? "服务暂时不可用" : "已连接协作服务"} className={`connection-dot ${syncing ? "disconnected" : ""}`} />
      </header>
      <div className={`page page-${view}`}>
        {view !== "chats" && <SectionTitle title={section} subtitle={pages[view]} actions={pageActions[view]} />}
        {!task && <ErrorLine message={error} />}
        {view === "home" && <>
          <div className="dashboard-banner"><div><span className="dashboard-label"><span className="live-dot" />{workspace.machines.filter((machine) => machine.online).length} 台工作机在线</span>
            <h2>{space?.name}</h2><p>{space?.description || "在这里安排工作，跟进交付进度。"}</p></div>
            <button className="button banner-button" onClick={() => navigate("tasks")}>打开任务看板<ArrowUpRight size={17} /></button></div>
          <div className="metrics">
            <button onClick={() => navigate("tasks")}><FolderKanban size={18} /><span>活跃任务</span><strong>{workspace.tasks.filter((item) => item.status !== "done").length}</strong></button>
            <button onClick={() => navigate("chats")}><Bot size={18} /><span>Agent 运行</span><strong>{activeRuns}</strong></button>
            <button onClick={() => navigate("inbox")}><Bell size={18} /><span>未读通知</span><strong>{unread}</strong></button>
          </div>
          <section className="surface recent"><div className="surface-heading"><h2>最近任务</h2><button className="text-button" onClick={() => navigate("tasks")}>查看全部<ArrowRight size={15} /></button></div>
            {!workspace.tasks.length && <Empty title="还没有任务" action={() => createTask()}>创建任务</Empty>}
            {workspace.tasks.slice(0, 6).map((item) => <button className="recent-row" key={item.id} onClick={() => setTaskId(item.id)}>
              <span className="task-number">SF-{item.number}</span><strong>{item.title}</strong><Status state={item.status} /><ArrowUpRight size={16} />
            </button>)}
          </section>
        </>}
        {view === "guide" && <section className="guide-list">
          <div className="guide-progress"><strong>{done} 项已完成</strong><progress aria-label="引导完成进度" value={done} max={4} /><span>{done}/4</span></div>
          {guide.map(({ icon: Icon, title, hint, complete, action: start, label }) => <section className="guide-step" key={title}>
            <span className={`resource-icon ${complete ? "complete" : ""}`}>{complete ? <Check size={21} /> : <Icon size={21} />}</span>
            <div><h2>{title}</h2><p>{hint}</p></div><button className="button subtle" onClick={start}>{label}<ArrowUpRight size={16} /></button>
          </section>)}
          <button className="text-button" onClick={() => void run({ op: "onboarding.dismiss", dismissed: true }).then(() => navigate("home"))}>返回工作台<ArrowRight size={16} /></button>
        </section>}
        {(view === "tasks" || view === "mine") && <TaskBoard key={`${view}-${workspace.spaceId}`} workspace={workspace} mine={view === "mine"}
          open={setTaskId} create={createTask} update={(item, state) => void run({ op: "task.update", id: item.id, revision: item.revision, changes: { status: state } })} />}
        {view === "chats" && <ChatHub workspace={workspace} selected={chatId} initialAgent={agentId} choose={setChatId} chooseAgent={setAgentId}
          run={run} busy={pending} openTask={setTaskId} />}
        {view === "inbox" && (workspace.inbox.length ? <div className="surface">
          {workspace.inbox.map((item) => <button className={`resource-row inbox-row ${item.read ? "" : "unread"}`} key={item.id}
            onClick={() => { setTaskId(item.taskId); if (!item.read) void run({ op: "inbox.read", id: item.id }); }}>
            <span className="resource-icon"><Bell size={18} /></span><span><strong>{item.title}</strong><small>{dateTime(item.createdAt)}</small></span><ArrowUpRight size={16} />
          </button>)}
        </div> : <Empty title="暂时没有通知" hint="新的任务指派和运行结果会出现在这里。" />)}
        {view === "agents" && (workspace.agents.length > 0 ? <div className="resource-grid">
          {workspace.agents.map((item) => <article className="resource-card" key={item.id}><span className="resource-icon"><Bot size={23} /></span><h2>{item.name}</h2>
            <p>{item.runtime} · {workspace.machines.find((machine) => machine.id === item.machineId)?.name ?? "工作机"}</p>
            {item.instructions && <p className="resource-description">{item.instructions}</p>}
            <button className="button subtle" onClick={() => startChat(item.id)}>发起对话<ArrowUpRight size={16} /></button>
          </article>)}
        </div> : <Empty title="还没有 Agent" hint="为一个工作角色配置指令和工作机。" action={canCreateAgent ? () => setModal({ kind: "agent" }) : () => navigate("machines")}>
          {canCreateAgent ? "创建 Agent" : "查看工作机"}</Empty>)}
        {view === "machines" && (workspace.machines.length ? <div className="resource-grid">
          {workspace.machines.map((machine) => <article className="resource-card" key={machine.id}><span className="resource-icon"><Cpu size={23} /></span>
            <div className="resource-card-title"><h2>{machine.name}</h2><Status state={machine.online ? "done" : "backlog"} label={machine.online ? "在线" : "离线"} /></div>
            <p>{machine.shared ? "组织共享" : "个人工作机"}</p><div className="tags">{machine.runtimes.map((name) => <span key={name}>{name}</span>)}</div>
          </article>)}
        </div> : <Empty title="连接一台工作机" hint="工作机会领取任务并在对应机器上启动已配置的 Runtime。" action={() => setModal({ kind: "machine" })}>连接本机</Empty>)}
        {view === "iterations" && (workspace.iterations.length ? <div className="surface">
          {workspace.iterations.map((item) => {
            const tasks = workspace.tasks.filter((row) => row.iterationId === item.id);
            return <div className="resource-row" key={item.id}><span className="resource-icon"><CalendarRange size={19} /></span>
              <span><strong>{item.name}</strong><small>{item.startsOn} — {item.endsOn} · {tasks.filter((row) => row.status === "done").length}/{tasks.length} 已完成</small></span>
              <select aria-label={`${item.name}的状态`} value={item.status} disabled={pending}
                onChange={(event) => void run({ op: "iteration.state", id: item.id, status: event.currentTarget.value as "planned" | "active" | "closed" })}>
                {Object.entries(iterationLabel).map(([value, label]) => <option value={value} key={value}>{label}</option>)}
              </select>
            </div>;
          })}
        </div> : <Empty title="创建一个迭代" hint="用开始日期和结束日期为任务排期。" action={() => setModal({ kind: "iteration" })}>新建迭代</Empty>)}
        {view === "documents" && (workspace.documents.length ? <div className="surface">
          {workspace.documents.map((item) => <button className="resource-row" key={item.id} onClick={() => setModal({ kind: "document", document: item })}>
            <span className="resource-icon"><BookOpen size={19} /></span><span><strong>{item.title}</strong><small>修订 {item.revision} · {dateTime(item.updatedAt)}</small></span><ArrowUpRight size={16} />
          </button>)}
        </div> : <Empty title="文档库为空" hint="把项目背景或团队说明保存在当前空间。" action={() => setModal({ kind: "document" })}>新建文档</Empty>)}
        {view === "members" && <div className="surface">
          {workspace.members.map((item) => <div className="resource-row" key={item.id}><span className="avatar member-avatar">{item.name.slice(0, 1)}</span>
            <span><strong>{item.name}</strong><small>@{item.username}</small></span><span className="muted">{item.role === "owner" ? "所有者" : item.role === "admin" ? "管理员" : "成员"}</span></div>)}
        </div>}
      </div>
      {syncing && <button className="sync-notice" onClick={() => void refresh().catch(() => undefined)}><ArrowDown size={15} />连接中断 · 点击重试</button>}
    </main>
    {task && <TaskDetails key={task.id} task={task} workspace={workspace} close={() => setTaskId(undefined)}
      edit={() => setModal({ kind: "task", task })} archive={() => setModal({ kind: "archive", task })}
      openChat={openChat} pending={pending} error={error} run={async (q) => Boolean(await run(q))} />}
    {modal && <WorkForm modal={modal} workspace={workspace} close={() => setModal(undefined)} done={(selection) => {
      setModal(undefined);
      if (selection?.spaceId) void switchSpace(selection.spaceId);
      else {
        if (selection?.taskId) setTaskId(selection.taskId);
        if (modal.kind === "archive") setTaskId(undefined);
        void refresh().catch(() => undefined);
      }
    }} />}
    {search !== undefined && <Layer title="搜索工作区" onClose={() => setSearch(undefined)}>
      <div className="form-body search-results"><Field label="任务或文档"><input autoFocus value={search} onChange={(event) => setSearch(event.currentTarget.value)} placeholder="输入名称或任务编号" /></Field>
        {taskMatches.map((item) => <button className="recent-row" key={item.id} onClick={() => { setTaskId(item.id); setSearch(undefined); }}><span className="task-number">SF-{item.number}</span><strong>{item.title}</strong><ArrowUpRight size={16} /></button>)}
        {docMatches.map((item) => <button className="recent-row" key={item.id} onClick={() => { setModal({ kind: "document", document: item }); setSearch(undefined); }}><BookOpen size={16} /><strong>{item.title}</strong><ArrowUpRight size={16} /></button>)}
        {!!searchPhrase && !taskMatches.length && !docMatches.length && <p className="muted">没有找到匹配的任务或文档。</p>}
      </div>
    </Layer>}
  </div>;
}
