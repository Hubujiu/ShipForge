const assert = require("node:assert/strict");
const { spawn } = require("node:child_process");
const { randomBytes } = require("node:crypto");
const { once } = require("node:events");
const { mkdtemp, readFile, realpath, rm, writeFile } = require("node:fs/promises");
const { tmpdir } = require("node:os");
const { basename, join, resolve, sep } = require("node:path");
const { test } = require("node:test");
const { Pool } = require("pg");
const { WebSocket } = require("ws");

const workbench = resolve(__dirname, "..");
const password = () => randomBytes(18).toString("base64url");

async function command(program, args, options = {}) {
  const child = spawn(program, args, { ...options, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8").on("data", (chunk) => { stdout += chunk; });
  child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
  child.stdin.end(options.input);
  const [code] = await once(child, "close");
  assert.equal(code, 0, stderr);
  return JSON.parse(stdout);
}

async function startServer(database) {
  const child = spawn(process.execPath, [join(workbench, "build/server/index.js")], {
    env: { ...process.env, DATABASE_URL: database, SHIPFORGE_PORT: "0", SHIPFORGE_HOST: "127.0.0.1", SHIPFORGE_BOOTSTRAP_KEY: "" },
    windowsHide: true, stdio: ["ignore", "pipe", "pipe"],
  });
  const closed = once(child, "close");
  let output = "";
  let error = "";
  child.stderr.setEncoding("utf8").on("data", (chunk) => { error += chunk; });
  const announce = new Promise((ready) => {
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      output += chunk;
      const line = output.split(/\r?\n/).find((item) => item.startsWith('{"service":'));
      if (line) ready(JSON.parse(line).port);
    });
  });
  const port = await Promise.race([announce, closed.then(() => { throw new Error(error || "Service stopped before listening"); })]);
  return {
    address: `http://127.0.0.1:${port}`,
    async stop() {
      if (child.exitCode === null) child.kill();
      await closed;
    },
  };
}

async function ask(address, path, input, token, status = 200) {
  const reply = await fetch(address + path, {
    method: input === undefined ? "GET" : "POST",
    headers: { ...(input !== undefined && { "Content-Type": "application/json" }), ...(token && { Authorization: `Bearer ${token}` }) },
    body: input === undefined ? undefined : JSON.stringify(input),
    signal: AbortSignal.timeout(12_000),
  });
  const payload = await reply.json();
  assert.equal(reply.status, status, `${path} ${input?.op ?? ""} ${JSON.stringify(payload.error ?? {})}`);
  assert.equal(payload.protocol, 1);
  return payload.ok ? payload.data : payload.error;
}

async function monitor(address, token) {
  const events = [];
  const socket = new WebSocket(address.replace(/^http/, "ws") + "/v1/events", {
    headers: { Authorization: `Bearer ${token}` }, handshakeTimeout: 5000,
  });
  socket.on("message", (data) => events.push(JSON.parse(data.toString())));
  await once(socket, "message");
  assert.equal(events[0].type, "ready");
  return { events, close: () => socket.terminate() };
}

test("PostgreSQL workspaces, authorization, live collaboration and a real machine process", { timeout: 100_000 }, async () => {
  assert.ok(process.env.DATABASE_URL, "Set DATABASE_URL to a disposable PostgreSQL 17 instance with pgvector");
  assert.ok(process.env.SHIPFORGE_WORKER_EXE, "Set SHIPFORGE_WORKER_EXE to the independently built machine utility");
  const base = process.env.DATABASE_URL;
  const admin = new Pool({ connectionString: base });
  const database = `shipforge_acceptance_${randomBytes(9).toString("hex")}`;
  const temporary = await mkdtemp(join(tmpdir(), "shipforge-workbench-"));
  const sockets = [];
  let running;
  let created = false;
  try {
    await admin.query(`CREATE DATABASE "${database}"`);
    created = true;
    const url = new URL(base);
    url.pathname = "/" + database;
    running = await startServer(url.href);
    const address = running.address;
    const health = await ask(address, "/healthz");
    assert.equal(health.initialized, false);
    assert.match(health.postgres, /^17\./);
    assert.match(health.vector, /^\d+\.\d+/);
    const ownerLogin = {
      username: "owner-fixture", password: password(), name: "验收所有者", organization: "交付小组", firstSpace: "研发",
    };
    const owner = await ask(address, "/v1/setup", ownerLogin);
    assert.equal(owner.actor.role, "owner");
    assert.equal((await ask(address, "/v1/setup", ownerLogin, null, 409)).code, "conflict");
    const call = (op, params = {}, token = owner.token, status) =>
      ask(address, "/v1/command", { op, ...params }, token, status);
    const first = await call("workspace.read");
    assert.deepEqual(first.guide, { dismissed: false, task: false, machine: false, agent: false, execution: false });
    const spaceId = first.spaceId;
    const invite = await call("member.invite", { role: "member", expiresInDays: 1 });
    const memberLogin = { username: "teammate-fixture", password: password(), name: "验收成员", code: invite.code };
    const member = await ask(address, "/v1/join", memberLogin);
    assert.equal(member.actor.role, "member");
    assert.equal((await ask(address, "/v1/join", memberLogin, null, 403)).code, "forbidden");
    assert.equal((await call("space.create", { name: "越权空间" }, member.token, 403)).code, "forbidden");
    const listener = await monitor(address, member.token);
    sockets.push(listener);
    const second = await call("space.create", { name: "隔离空间" });
    const task = await call("task.create", { spaceId, title: "验收 · 任务分派", description: "保留评论和执行记录", memberId: member.actor.id });
    assert.equal((await call("workspace.read", { spaceId: second.id })).tasks.length, 0);
    assert.ok((await call("workspace.read", {}, member.token)).inbox.some((item) => item.taskId === task.id && !item.read));
    await call("task.comment", { id: task.id, body: "两端可见的评论" }, member.token);
    assert.ok((await call("workspace.read")).comments.some((item) => item.body === "两端可见的评论"));
    const ownerSession = await ask(address, "/v1/login", ownerLogin);
    assert.notEqual(owner.token, ownerSession.token);
    const iteration = await call("iteration.create", { spaceId, name: "验收迭代", startsOn: "2026-11-20", endsOn: "2026-11-27" });
    await call("iteration.state", { id: iteration.id, status: "active" });
    const doc = await call("document.save", { spaceId, title: "验收说明", body: "# 决策\n验证 PostgreSQL 版本记录" });
    await call("document.save", { spaceId, ...doc, title: "验收说明", body: "第二版" });
    assert.equal((await call("document.save", { spaceId, ...doc, title: "过期写入", body: "" }, owner.token, 409)).code, "conflict");
    const issued = await call("machine.enroll", { name: "合成工作机", shared: true });
    const config = join(temporary, "测试机器.json");
    const workerEnv = { ...process.env, SHIPFORGE_SERVER: address, SHIPFORGE_TOKEN: issued.token, SHIPFORGE_DATA_DIR: temporary };
    const worker = process.env.SHIPFORGE_WORKER_EXE;
    await command(worker, ["init", "--config", config, "--example"], { env: workerEnv });
    const settings = JSON.parse(await readFile(config, "utf8"));
    await writeFile(config, JSON.stringify({ ...settings, machine_id: issued.id }), "utf8");
    const runWorker = () => command(worker, ["run", "--config", config, "--once"], { env: workerEnv });
    assert.deepEqual((await runWorker()).runs, []);
    const remote = (await call("workspace.read")).machines.find((item) => item.id === issued.id);
    assert.ok(remote.online && remote.runtimes.length, remote);
    assert.equal((await call("workspace.read", {}, issued.token, 400)).code, "invalid");
    const agent = await call("agent.create", { spaceId, name: "测试Agent", instructions: "回传工作项摘要", machineId: issued.id, runtime: remote.runtimes[0] });
    const assigned = await call("task.update", { id: task.id, revision: task.revision, changes: { memberId: null, agentId: agent.id, iterationId: iteration.id } });
    assert.equal((await call("workspace.read")).runs.length, 0, "Backlog must remain idle");
    const latest = await call("task.update", { id: task.id, revision: assigned.revision, changes: { status: "todo" } });
    assert.equal((await call("task.update", { id: task.id, revision: assigned.revision, changes: { status: "done" } }, owner.token, 409)).code, "conflict");
    assert.equal((await call("run.start", { taskId: task.id }, owner.token, 409)).code, "conflict");
    const result = await runWorker();
    assert.equal(result.runs.length, 1, result);
    assert.equal(result.runs[0].status, "done", result);
    const saved = await call("workspace.read");
    assert.equal(saved.tasks.find((item) => item.id === task.id).status, "todo");
    assert.equal(saved.runs[0].taskId, task.id);
    assert.ok(saved.messages.some((item) => item.chatId === saved.runs[0].chatId));
    assert.deepEqual(saved.guide, { dismissed: false, task: true, machine: true, agent: true, execution: true });
    assert.ok(listener.events.some((event) => event.type === "changed"));
    const privateChat = await call("chat.start", { agentId: agent.id, body: "独立会话验收" });
    assert.ok(!(await call("workspace.read", {}, member.token)).chats.some((item) => item.id === privateChat.chatId));
    assert.equal((await call("chat.send", { id: privateChat.chatId, body: "越权内容" }, member.token, 404)).code, "not_found");
    await runWorker();
    const followUp = await call("chat.send", { id: privateChat.chatId, body: "继续对话" });
    assert.equal(followUp.chatId, privateChat.chatId);
    await call("run.cancel", { id: followUp.runId });
    assert.equal((await runWorker()).runs.length, 0);
    const export_ = await command(process.execPath, [join(workbench, "build/cli.js"), "request"], {
      input: JSON.stringify({ op: "workspace.read", spaceId }), env: { ...process.env, SHIPFORGE_SERVER: address, SHIPFORGE_TOKEN: owner.token },
    });
    assert.equal(export_.data.tasks[0].revision, latest.revision);
    const schema = await command(process.execPath, [join(workbench, "build/cli.js"), "schema"]);
    assert.ok(schema.data.oneOf.some((entry) => entry.properties.op.const === "task.create"));
    await call("onboarding.dismiss", { dismissed: true });
    listener.close();
    await running.stop();
    running = await startServer(url.href);
    const after = await ask(running.address, "/v1/command", { op: "workspace.read", spaceId }, owner.token);
    assert.ok(after.guide.dismissed);
    assert.equal(after.documents[0].body, "第二版");
    assert.equal(after.tasks[0].status, "todo");
    assert.ok(after.runs.some((run) => run.status === "canceled"));
  } finally {
    for (const socket of sockets) socket.close();
    await running?.stop();
    if (created) await admin.query(`DROP DATABASE "${database}" WITH (FORCE)`);
    await admin.end();
    const root = await realpath(tmpdir());
    const actual = await realpath(temporary);
    assert.ok(actual.startsWith(root + sep) && basename(actual).startsWith("shipforge-workbench-"), actual);
    await rm(actual, { recursive: true, force: true });
  }
});
