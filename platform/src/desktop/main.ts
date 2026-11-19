import { app, BrowserWindow, clipboard, dialog, ipcMain, safeStorage, type IpcMainInvokeEvent } from "electron";
import { execFile, spawn, type ChildProcess } from "node:child_process";
import { mkdir, readFile, readdir, rename, rm, stat, writeFile } from "node:fs/promises";
import { hostname } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { promisify } from "node:util";
import { WebSocket } from "ws";
import type { Actor, Workspace } from "../protocol";
import { originOf, request } from "../transport";
import type { WorkerOptions } from "./bridge";

const execute = promisify(execFile);
const entry = join(__dirname, "../renderer/index.html");
const rendererUrl = pathToFileURL(entry).href;
let origin = "http://127.0.0.1:4790";
let token: string | undefined;
let currentWindow: BrowserWindow | undefined;
let subscription: WebSocket | undefined;
let reconnect: ReturnType<typeof setTimeout> | undefined;
const workers = new Map<string, { api: string; program: string; config: string; process: ChildProcess }>();
const userDirectory = () => app.getPath("userData");
const profileFile = () => join(userDirectory(), "collaboration-session.json");
const machineDirectory = () => join(userDirectory(), "work-machines");

async function workerExecutable() {
  const suffix = process.platform === "win32" ? ".exe" : "";
  const file = resolve(process.env.SHIPFORGE_WORKER_EXE ?? join(process.resourcesPath, "worker", `shipforge-worker${suffix}`));
  if (!(await stat(file).catch(() => undefined))?.isFile()) throw new Error("The ShipForge work machine program is missing");
  return file;
}

function workerEnv(api: string, key: string, directory: string) {
  return { ...process.env, SHIPFORGE_SERVER: api, SHIPFORGE_TOKEN: key, SHIPFORGE_DATA_DIR: directory };
}

function launchMachine(id: string, api: string, key: string, program: string, directory: string) {
  if (workers.has(id)) return;
  const config = join(directory, "machine.json");
  const child = spawn(program, ["run", "--config", config], { env: workerEnv(api, key, directory), windowsHide: true, stdio: "ignore" });
  workers.set(id, { api, program, config, process: child });
  const remove = () => {
    if (workers.get(id)?.process === child) workers.delete(id);
    currentWindow?.webContents.send("sf:changed");
  };
  child.once("error", remove);
  child.once("exit", remove);
}

async function restoreMachines() {
  const api = origin;
  const credential = token;
  if (!credential || !safeStorage.isEncryptionAvailable()) return;
  try {
    const workspace = await request<Workspace>(api, "/v1/command", { op: "workspace.read" }, credential);
    const entries = await readdir(machineDirectory(), { withFileTypes: true });
    const executable = await workerExecutable();
    for (const entry of entries) {
      if (quitting || origin !== api || token !== credential) return;
      if (!entry.isDirectory() || !/^[a-f0-9-]{36}$/i.test(entry.name)
          || !workspace.machines.some((item) => item.id === entry.name && item.ownerId === workspace.actor.id)) continue;
      const directory = join(machineDirectory(), entry.name);
      try {
        const profile = JSON.parse(await readFile(join(directory, "machine-session.json"), "utf8"));
        if (profile.origin !== api) continue;
        const key = safeStorage.decryptString(Buffer.from(profile.credential, "base64"));
        launchMachine(entry.name, api, key, executable, directory);
      } catch {
        // A machine may have been connected for this session only.
      }
    }
  } catch {
    // The account form and machine list report connection status.
  }
}

async function readProfile() {
  try {
    const saved = JSON.parse(await readFile(profileFile(), "utf8"));
    origin = originOf(saved.origin);
    if (typeof saved.credential === "string" && safeStorage.isEncryptionAvailable()) {
      token = safeStorage.decryptString(Buffer.from(saved.credential, "base64"));
    }
  } catch {
    // An unset or invalid profile opens the connection form.
  }
}

async function writeProfile() {
  await mkdir(userDirectory(), { recursive: true });
  const credential = token && safeStorage.isEncryptionAvailable()
    ? safeStorage.encryptString(token).toString("base64") : undefined;
  await writeFile(`${profileFile()}.tmp`, JSON.stringify({ origin, credential }), { mode: 0o600 });
  await rename(`${profileFile()}.tmp`, profileFile());
}

function disconnectEvents() {
  clearTimeout(reconnect);
  const old = subscription;
  subscription = undefined;
  if (old && [WebSocket.CONNECTING, WebSocket.OPEN].includes(old.readyState as 0 | 1)) old.close();
}

function connectEvents(delay = 700) {
  disconnectEvents();
  if (!token || quitting) return;
  const target = new URL("/v1/events", origin);
  target.protocol = target.protocol === "https:" ? "wss:" : "ws:";
  const channel = new WebSocket(target, {
    headers: { Authorization: `Bearer ${token}` },
    followRedirects: false, handshakeTimeout: 8000, maxPayload: 1024,
  });
  subscription = channel;
  channel.on("message", () => currentWindow?.webContents.send("sf:changed"));
  channel.on("error", () => undefined);
  channel.on("close", () => {
    if (subscription === channel && token) reconnect = setTimeout(() => connectEvents(Math.min(delay * 2, 10_000)), delay);
  });
}

function handle<Input extends unknown[]>(name: string, action: (...args: Input) => unknown) {
  ipcMain.handle(name, (event: IpcMainInvokeEvent, ...args: Input) => {
    if (event.sender !== currentWindow?.webContents || event.senderFrame?.url !== rendererUrl
        || event.senderFrame !== event.sender.mainFrame) throw new Error("Unauthorized local request");
    return action(...args);
  });
}

async function startWorker(options: WorkerOptions) {
  if (!token) throw new Error("Sign in first");
  if (!options || typeof options.name !== "string" || !options.name.trim()) throw new Error("Enter a machine name");
  const executable = await workerExecutable();
  const init: string[] = [];
  if (options.example) {
    init.push("--example");
  } else {
    if (!options.program || !isAbsolute(options.program) || !(await stat(options.program).catch(() => undefined))?.isFile()
      || !options.runtime?.trim() || !Array.isArray(options.args) || options.args.length > 40
      || options.args.some((item) => typeof item !== "string" || item.includes("\0") || item.length > 1024)) {
      throw new Error("Select a local runtime executable and valid startup arguments");
    }
    if (process.platform === "win32" && !options.program.toLowerCase().endsWith(".exe")) {
      throw new Error("Select an executable .exe for the local runtime");
    }
    init.push("--program", options.program, "--runtime", options.runtime.trim());
    for (const argument of options.args) init.push(`--arg=${argument}`);
  }
  const api = origin;
  const issued = await request<{ id: string; name: string; token: string }>(api, "/v1/command",
    { op: "machine.enroll", name: options.name.trim(), shared: options.shared === true }, token);
  const directory = join(machineDirectory(), issued.id);
  await mkdir(directory, { recursive: true });
  const config = join(directory, "machine.json");
  // The machine implements a narrow, machine-scoped credential over the existing Rust worker protocol.
  await execute(executable, ["init", "--config", config, ...init], {
    windowsHide: true, timeout: 15_000, env: workerEnv(api, issued.token, directory),
  });
  const data = JSON.parse(await readFile(config, "utf8"));
  await writeFile(config, JSON.stringify({ ...data, name: issued.name, machine_id: issued.id }, null, 2), { mode: 0o600 });
  if (safeStorage.isEncryptionAvailable()) {
    const credential = safeStorage.encryptString(issued.token).toString("base64");
    await writeFile(join(directory, "machine-session.json"), JSON.stringify({ origin: api, credential }), { mode: 0o600 });
  }
  launchMachine(issued.id, api, issued.token, executable, directory);
  return { id: issued.id, name: issued.name };
}

function stopWorkers() {
  for (const machine of workers.values()) {
    void execute(machine.program, ["stop", "--config", machine.config], { windowsHide: true, timeout: 8000 }).catch(() => undefined);
  }
}

let quitting = false;
function register() {
  handle("sf:session", () => ({ origin, signedIn: !!token, deviceName: hostname() }));
  handle("sf:health", (address: string) => request(originOf(address), "/healthz"));
  handle("sf:signin", async (kind: string, address: string, fields: Record<string, string>) => {
    if (!["login", "setup", "join"].includes(kind)) throw new Error("Choose an account action");
    const selected = originOf(address);
    const result = await request<{ token: string; actor: Actor }>(selected, `/v1/${kind}`, fields);
    disconnectEvents();
    token = result.token;
    origin = selected;
    await writeProfile();
    connectEvents();
    void restoreMachines();
    return result.actor;
  });
  handle("sf:signout", async () => {
    if (token) await request(origin, "/v1/logout", {}, token).catch(() => undefined);
    token = undefined;
    disconnectEvents();
    stopWorkers();
    await rm(profileFile(), { force: true });
  });
  handle("sf:command", (command: unknown) => {
    if (!token) throw new Error("Sign in first");
    return request(origin, "/v1/command", command, token);
  });
  handle("sf:program", async () => {
    const selection = await dialog.showOpenDialog(currentWindow!, {
      title: "选择本机 Runtime 程序",
      properties: ["openFile"],
      filters: process.platform === "win32" ? [{ name: "Windows 程序", extensions: ["exe"] }] : undefined,
    });
    return selection.filePaths[0];
  });
  handle("sf:worker", startWorker);
  handle("sf:copy", (value: string) => {
    if (typeof value !== "string" || value.length > 16_384) throw new Error("Invalid clipboard text");
    clipboard.writeText(value);
  });
}

function openWindow() {
  const window = new BrowserWindow({
    title: "ShipForge",
    width: 1360,
    height: 900,
    minWidth: 820,
    minHeight: 600,
    show: true,
    backgroundColor: "#f4f2ed",
    webPreferences: {
      preload: join(__dirname, "preload.js"),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      spellcheck: false,
    },
  });
  currentWindow = window;
  window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  window.webContents.on("will-navigate", (event) => event.preventDefault());
  window.on("closed", () => { if (currentWindow === window) currentWindow = undefined; });
  void window.loadFile(entry);
}

if (process.env.SHIPFORGE_ELECTRON_PROFILE) app.setPath("userData", resolve(process.env.SHIPFORGE_ELECTRON_PROFILE));
else app.setPath("userData", join(app.getPath("appData"), "ShipForge Workspace"));
if (!app.requestSingleInstanceLock()) app.quit();
else {
  app.on("second-instance", () => {
    if (currentWindow?.isMinimized()) currentWindow.restore();
    currentWindow?.focus();
  });
  void app.whenReady().then(async () => {
    await readProfile();
    register();
    openWindow();
    connectEvents();
    void restoreMachines();
  });
  app.on("activate", () => { if (!currentWindow) openWindow(); });
  app.on("window-all-closed", () => { if (process.platform !== "darwin") app.quit(); });
  app.on("before-quit", (event) => {
    if (quitting) return;
    quitting = true;
    disconnectEvents();
    if (!workers.size) return;
    event.preventDefault();
    const endings = [...workers.values()].map((worker) =>
      execute(worker.program, ["stop", "--config", worker.config], { windowsHide: true, timeout: 8000 }));
    void Promise.allSettled(endings).then(() => app.quit());
  });
}
