import type { Actor, Command } from "../protocol";

export interface SessionInfo {
  origin: string;
  signedIn: boolean;
}

export interface HealthInfo {
  initialized: boolean;
  postgres: string;
  vector: string;
}

export interface WorkerOptions {
  name: string;
  example: boolean;
  shared: boolean;
  program?: string;
  runtime?: string;
  args?: string[];
}

export interface DesktopBridge {
  session(): Promise<SessionInfo>;
  health(origin: string): Promise<HealthInfo>;
  signIn(kind: "login" | "setup" | "join", origin: string, data: Record<string, string>): Promise<Actor>;
  signOut(): Promise<void>;
  command<T>(command: Command): Promise<T>;
  onChange(listener: () => void): () => void;
  pickProgram(): Promise<string | undefined>;
  startWorker(options: WorkerOptions): Promise<{ id: string; name: string }>;
  copy(text: string): Promise<void>;
}

declare global {
  interface Window {
    shipforge: DesktopBridge;
  }
}
