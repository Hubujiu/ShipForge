import { contextBridge, ipcRenderer } from "electron";
import type { DesktopBridge } from "./bridge";

const api: DesktopBridge = {
  session: () => ipcRenderer.invoke("sf:session"),
  health: (address) => ipcRenderer.invoke("sf:health", address),
  signIn: (kind, address, fields) => ipcRenderer.invoke("sf:signin", kind, address, fields),
  signOut: () => ipcRenderer.invoke("sf:signout"),
  command: (command) => ipcRenderer.invoke("sf:command", command),
  onChange: (listener) => {
    const relay = () => listener();
    ipcRenderer.on("sf:changed", relay);
    return () => ipcRenderer.removeListener("sf:changed", relay);
  },
  pickProgram: () => ipcRenderer.invoke("sf:program"),
  startWorker: (options) => ipcRenderer.invoke("sf:worker", options),
  copy: (value) => ipcRenderer.invoke("sf:copy", value),
};
contextBridge.exposeInMainWorld("shipforge", Object.freeze(api));
