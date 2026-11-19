import { useState, type FormEvent } from "react";
import { Bot, MessageSquare, Send, Square } from "lucide-react";
import type { Command, Workspace } from "../protocol";
import { dateTime, Empty, Field, Status } from "./components";

export function ChatHub({ workspace, selected, initialAgent, choose, chooseAgent, run, busy, openTask }: {
  workspace: Workspace;
  selected?: string;
  initialAgent?: string;
  choose: (id?: string) => void;
  chooseAgent: (id?: string) => void;
  run: <T>(command: Command) => Promise<T | undefined>;
  busy: boolean;
  openTask: (id: string) => void;
}) {
  const [text, setText] = useState("");
  const chat = workspace.chats.find((item) => item.id === selected);
  const agentId = initialAgent ?? workspace.agents[0]?.id;
  const agent = workspace.agents.find((item) => item.id === (chat?.agentId ?? agentId));
  const latest = workspace.runs.find((item) => item.chatId === selected);
  const active = latest && ["running", "queued", "canceling"].includes(latest.status);
  async function send(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const body = text.trim();
    if (!body || busy || (chat && active)) return;
    if (chat) {
      if (await run({ op: "chat.send", id: chat.id, body })) setText("");
    } else if (agentId) {
      const result = await run<{ chatId: string }>({ op: "chat.start", agentId, body });
      if (result) {
        setText("");
        choose(result.chatId);
      }
    }
  }
  return <section className="chat-layout">
    <aside className="chat-list">
      <button className="button primary" onClick={() => { choose(); setText(""); }}><MessageSquare size={17} />新对话</button>
      {workspace.chats.map((item) => <button key={item.id} className={`chat-row ${item.id === selected ? "selected" : ""}`}
        onClick={() => { choose(item.id); setText(""); }}>
        <strong>{item.title}</strong><small>{dateTime(item.updatedAt)}</small>
      </button>)}
    </aside>
    <div className="conversation">
      <header className="conversation-head">
        <span className="resource-icon"><Bot size={20} /></span>
        <div><strong>{chat?.title ?? "与 Agent 对话"}</strong><small>{agent?.name ?? "选择一个 Agent"}</small></div>
        {latest && <Status state={latest.status} />}
        {chat?.taskId && <button className="button subtle" onClick={() => openTask(chat.taskId!)}>查看任务</button>}
        {latest && active && latest.status !== "canceling" && <button className="icon-button" aria-label="停止运行"
          onClick={() => void run({ op: "run.cancel", id: latest.id })}><Square size={16} /></button>}
      </header>
      <div className="messages">
        {!chat && <Empty title="从一条消息开始" hint="发送问题或工作要求。Agent 在自己的工作机运行，答复会回到这里。" />}
        {workspace.messages.filter((item) => item.chatId === chat?.id).map((item) => <article className="message" key={item.id}>
          <span className="avatar avatar-message">{item.sender.slice(0, 1).toUpperCase()}</span>
          <div><strong>{item.sender}</strong><small>{dateTime(item.createdAt)}</small><p>{item.body}</p></div>
        </article>)}
        {latest && active && <div className="running-note"><span className="live-dot" />{latest.detail || (latest.status === "queued" ? "等待工作机接受这次运行" : "工作机已接收任务")}</div>}
      </div>
      <form className="composer" onSubmit={send}>
        {!chat && <Field label="接收 Agent"><select value={agentId ?? ""} onChange={(event) => chooseAgent(event.currentTarget.value)}>
          {!workspace.agents.length && <option value="">先创建 Agent</option>}{workspace.agents.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}
        </select></Field>}
        <Field label="消息"><textarea rows={3} placeholder={active ? "本轮完成后可以继续回复" : "输入问题或补充说明"} value={text}
          onChange={(event) => setText(event.currentTarget.value)} disabled={!!active} /></Field>
        <button className="button primary" type="submit" disabled={!text.trim() || busy || !!active || (!chat && !agentId)}><Send size={16} />发送消息</button>
      </form>
    </div>
  </section>;
}
