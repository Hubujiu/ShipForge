import { useEffect, useState, type FormEvent } from "react";
import { ArrowRight, ChevronLeft, CircleCheck, Layers3, LoaderCircle, Radio, Server } from "lucide-react";
import { createRoot } from "react-dom/client";
import type { HealthInfo } from "../desktop/bridge";
import type { Workspace } from "../protocol";
import { ErrorLine, Field, Mark } from "./components";
import { WorkspaceApp } from "./workspace";

type AuthMode = "connect" | "login" | "setup" | "join";
const headings = { connect: "连接协作服务", login: "登录工作区", setup: "创建工作区", join: "接受团队邀请" };

function Welcome({ onEnter }: { onEnter: (workspace: Workspace) => void }) {
  const [origin, setOrigin] = useState("http://127.0.0.1:4790");
  const [mode, setMode] = useState<AuthMode>("connect");
  const [health, setHealth] = useState<HealthInfo>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let alive = true;
    void window.shipforge.session().then(async (session) => {
      if (!alive) return;
      setOrigin(session.origin);
      if (session.signedIn) {
        try {
          const workspace = await window.shipforge.command<Workspace>({ op: "workspace.read" });
          if (alive) onEnter(workspace);
        } catch {
          if (alive) setError("无法恢复工作区。请确认服务在线并重新登录。");
        }
      }
    }).catch((cause: Error) => setError(cause.message));
    return () => { alive = false; };
  }, [onEnter]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    const fields = Object.fromEntries([...new FormData(event.currentTarget)].map(([key, value]) => [key, String(value)]));
    try {
      if (mode === "connect") {
        const status = await window.shipforge.health(origin);
        setHealth(status);
        setMode(status.initialized ? "login" : "setup");
      } else {
        await window.shipforge.signIn(mode, origin, fields);
        onEnter(await window.shipforge.command<Workspace>({ op: "workspace.read" }));
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }
  return <div className="welcome">
    <div className="welcome-side">
      <div className="welcome-brand"><Mark /><strong>ShipForge</strong></div>
      <div className="welcome-art" aria-hidden="true"><div className="art-orbit orbit-one" /><div className="art-orbit orbit-two" /><div className="art-orbit orbit-three" /><Layers3 size={98} strokeWidth={1.1} /></div>
      <div className="welcome-copy"><h1>团队和 Agent 的<br />交付工作台</h1><p>从任务规划到运行结果，都在同一个工作区。</p></div>
    </div>
    <main className="welcome-main">
      <form className="welcome-form" onSubmit={submit} key={mode}>
        {mode !== "connect" && <button className="text-button welcome-back" type="button" onClick={() => { setMode("connect"); setError(""); }}><ChevronLeft size={17} />更改服务地址</button>}
        <span className="resource-icon"><Server size={24} /></span>
        <h2>{headings[mode]}</h2>
        {mode === "connect" ? <p className="muted">输入团队部署的 ShipForge 服务地址。单机开发默认使用下面的地址。</p>
          : <p className="welcome-status"><CircleCheck size={16} />{origin}</p>}
        {mode === "connect" && <Field label="服务地址"><input type="url" name="origin" value={origin} onChange={(event) => setOrigin(event.currentTarget.value)} required autoFocus /></Field>}
        {mode === "join" && <Field label="邀请码"><input name="code" required autoFocus /></Field>}
        {(mode === "setup" || mode === "join") && <Field label="显示名称"><input name="name" required maxLength={160} /></Field>}
        {mode !== "connect" && <>
          <Field label="用户名" hint={mode === "login" ? undefined : "使用字母、数字、点、下划线或连字符。"}><input name="username" autoComplete="username" required minLength={3} maxLength={64} autoFocus={mode === "login"} /></Field>
          <Field label="密码"><input name="password" type="password" minLength={10} maxLength={128}
            autoComplete={mode === "login" ? "current-password" : "new-password"} required /></Field>
        </>}
        {mode === "setup" && <>
          <Field label="组织名称"><input name="organization" required maxLength={160} /></Field>
          <Field label="初始空间"><input name="firstSpace" required defaultValue="产品研发" maxLength={160} /></Field>
          <Field label="部署初始化密钥" hint="如果服务启用了初始化密钥，请填写管理员提供的值。"><input name="bootstrapKey" type="password" maxLength={256} /></Field>
        </>}
        <ErrorLine message={error} />
        <button className="button primary welcome-submit" type="submit" disabled={busy}>
          {busy ? <LoaderCircle size={17} className="spin" /> : <ArrowRight size={17} />}
          {mode === "connect" ? "检查并继续" : mode === "setup" ? "创建并进入" : mode === "join" ? "加入组织" : "登录"}
        </button>
        {mode === "login" && <button className="text-button welcome-secondary" type="button" onClick={() => { setMode("join"); setError(""); }}>使用邀请码</button>}
        {mode === "join" && <button className="text-button welcome-secondary" type="button" onClick={() => { setMode("login"); setError(""); }}>已有账号，返回登录</button>}
        {health && <small className="health"><Radio size={14} />PostgreSQL {health.postgres} · pgvector {health.vector}</small>}
      </form>
    </main>
  </div>;
}

function App() {
  const [workspace, setWorkspace] = useState<Workspace>();
  return workspace ? <WorkspaceApp initial={workspace} signOut={() => void window.shipforge.signOut().finally(() => setWorkspace(undefined))} />
    : <Welcome onEnter={setWorkspace} />;
}

createRoot(document.getElementById("root")!).render(<App />);
