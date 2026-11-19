import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { ZodError } from "zod";
import { WebSocket, WebSocketServer } from "ws";
import type { Actor } from "../protocol";
import type { Reply } from "../transport";
import { acceptInvitation, authenticate, login, logout, setup } from "./auth";
import { RequestError, eventsChannel, invalid, migrate, missing, one, pool } from "./database";
import { machineCommand } from "./machines";
import { workspace } from "./workspace";

const maximumBody = 512 * 1024;
const attempts = new Map<string, { count: number; expires: number }>();

function rateCheck(address: string) {
  const now = Date.now();
  if (attempts.size > 1024) {
    for (const [key, value] of attempts) if (value.expires <= now) attempts.delete(key);
  }
  const previous = attempts.get(address);
  if (!previous || previous.expires <= now) {
    if (attempts.size > 1024) throw new RequestError(429, "limited", "Try signing in again shortly");
    attempts.set(address, { count: 1, expires: now + 60_000 });
    return;
  }
  if (++previous.count > 15) throw new RequestError(429, "limited", "Try signing in again shortly");
}

async function body(req: IncomingMessage) {
  if (!/^application\/json(?:\s*;|$)/i.test(req.headers["content-type"] ?? "")) {
    throw invalid("Expected application/json");
  }
  if (Number(req.headers["content-length"]) > maximumBody) throw invalid("The request is too large");
  const chunks: Buffer[] = [];
  let length = 0;
  // Keep the HTTP response available when a chunked request exceeds the limit.
  for await (const part of req.iterator({ destroyOnReturn: false })) {
    const bytes = Buffer.isBuffer(part) ? part : Buffer.from(part);
    length += bytes.length;
    if (length > maximumBody) throw invalid("The request is too large");
    chunks.push(bytes);
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown;
  } catch {
    throw invalid("Invalid JSON request");
  }
}

function respond(res: ServerResponse, reply: Reply<unknown>, status: number) {
  res.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
    "X-Content-Type-Options": "nosniff",
  });
  res.end(JSON.stringify(reply));
}

function translate(error: unknown) {
  if (error instanceof RequestError) return error;
  if (error instanceof ZodError) return invalid(error.issues.map((issue) => issue.path.join(".") || "request").join(", ") + " is invalid");
  if (typeof error === "object" && error !== null && "code" in error && error.code === "23505") {
    return new RequestError(409, "conflict", "That name or identifier is already in use");
  }
  console.error("Request failed", error instanceof Error ? error.name : typeof error);
  return new RequestError(500, "internal", "The service could not complete the request");
}

async function dispatch(req: IncomingMessage) {
  if (req.method === "GET" && req.url === "/healthz") {
    const database = await one<{ initialized: boolean; postgres: string; vector: string | null }>(pool, `SELECT
      EXISTS(SELECT 1 FROM organizations) AS initialized, current_setting('server_version') AS postgres,
      (SELECT extversion FROM pg_extension WHERE extname='vector') AS vector`);
    return database;
  }
  if (req.method !== "POST") throw missing("Endpoint is unavailable");
  const address = req.socket.remoteAddress;
  if (req.url === "/v1/setup" || req.url === "/v1/login" || req.url === "/v1/join") {
    rateCheck(address ?? "unknown");
    const input = await body(req);
    if (req.url === "/v1/setup") return setup(input, address);
    if (req.url === "/v1/login") return login(input);
    return acceptInvitation(input);
  }
  const person = await authenticate(req.headers.authorization);
  if (req.url === "/v1/logout") {
    await logout(person);
    return { signedOut: true };
  }
  if (req.url !== "/v1/command") throw missing("Endpoint is unavailable");
  const command = await body(req);
  if (person.kind === "worker") return machineCommand(person, command);
  return workspace(person.actor, command);
}

async function start() {
  await migrate();
  const server = createServer({ requestTimeout: 16_000, headersTimeout: 10_000 }, (req, res) => {
    dispatch(req).then((data) => respond(res, { protocol: 1, ok: true, data }, 200)).catch((error: unknown) => {
      const local = translate(error);
      if (req.readable && !req.complete) {
        res.setHeader("Connection", "close");
        req.resume();
      }
      respond(res, { protocol: 1, ok: false, error: { code: local.code, message: local.message } }, local.status);
    });
  });
  const websocket = new WebSocketServer({ noServer: true, maxPayload: 1024 });
  const subscribers = new Map<WebSocket, { actor: Actor; session: string }>();
  server.on("upgrade", (req, socket, head) => {
    socket.on("error", () => undefined);
    if (req.url !== "/v1/events" || subscribers.size >= 256) {
      socket.destroy();
      return;
    }
    void authenticate(req.headers.authorization).then((person) => {
      if (person.kind !== "member") {
        socket.destroy();
        return;
      }
      websocket.handleUpgrade(req, socket, head, (channel) => {
        subscribers.set(channel, person);
        channel.on("close", () => subscribers.delete(channel));
        channel.on("error", () => channel.terminate());
        channel.send(JSON.stringify({ type: "ready" }));
      });
    }).catch(() => socket.destroy());
  });

  let stopping = false;
  async function listenToChanges() {
    if (stopping) return;
    const subscriber = await pool.connect();
    subscriber.once("error", () => {
      subscriber.release(true);
      for (const socket of subscribers.keys()) socket.terminate();
      reportListenerFailure();
    });
    subscriber.on("notification", ({ payload }) => {
      if (!payload) return;
      let event: { orgId: string; actorId?: string };
      try { event = JSON.parse(payload); } catch { return; }
      for (const [socket, info] of subscribers) {
        if (socket.readyState === WebSocket.OPEN && info.actor.orgId === event.orgId
              && (!event.actorId || info.actor.id === event.actorId)) {
          socket.send(JSON.stringify({ type: "changed" }));
        }
      }
    });
    try {
      await subscriber.query(`LISTEN ${eventsChannel}`);
    } catch (error) {
      subscriber.release(true);
      throw error;
    }
    return subscriber;
  }
  function reportListenerFailure() {
    if (!stopping) setTimeout(() => void listenToChanges().then((value) => { listener = value; }).catch(reportListenerFailure), 2000).unref();
  }
  let listener = await listenToChanges();
  const verifySessions = setInterval(() => {
    if (!subscribers.size) return;
    const sessions = [...new Set([...subscribers.values()].map((item) => item.session))];
    void pool.query<{ digest: string }>("SELECT digest FROM sessions WHERE digest=ANY($1) AND expires_at>now()", [sessions])
      .then((active) => {
        const valid = new Set(active.rows.map((row) => row.digest));
        for (const [socket, info] of subscribers) {
          if (!valid.has(info.session)) socket.terminate();
          else socket.ping();
        }
      }).catch(() => undefined);
  }, 30_000);
  verifySessions.unref();
  server.listen(Number(process.env.SHIPFORGE_PORT ?? 4790), process.env.SHIPFORGE_HOST ?? "127.0.0.1", () => {
    const address = server.address();
    console.log(JSON.stringify({ service: "shipforge", port: typeof address === "object" ? address?.port : undefined }));
  });
  const stop = () => {
    if (stopping) return;
    stopping = true;
    clearInterval(verifySessions);
    for (const socket of subscribers.keys()) socket.terminate();
    server.close();
    if (listener) listener.release(true);
    void pool.end().catch(() => undefined);
  };
  process.on("SIGTERM", stop);
  process.on("SIGINT", stop);
}

void start().catch((error: Error) => {
  console.error("Unable to start the PostgreSQL-backed service.", "code" in error ? error.code : error.name);
  process.exitCode = 1;
  void pool.end();
});
