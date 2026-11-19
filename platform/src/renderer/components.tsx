import type { ReactNode } from "react";
import { ArrowUpRight, CircleAlert, Command, Layers3, X } from "lucide-react";
import type { TaskPriority, TaskState, TaskType } from "../protocol";

export const statusLabel: Record<TaskState, string> = {
  backlog: "待整理", todo: "待处理", doing: "进行中", review: "待验收", done: "已完成",
};
export const priorityLabel: Record<TaskPriority, string> = {
  none: "无", low: "低", medium: "中", high: "高", urgent: "紧急",
};
export const typeLabel: Record<TaskType, string> = { task: "任务", feature: "需求", bug: "缺陷" };
export const runLabel: Record<string, string> = {
  queued: "排队中", running: "运行中", canceling: "正在停止", blocked: "等待补充",
  done: "已完成", failed: "失败", canceled: "已停止",
};
export const iterationLabel: Record<string, string> = { planned: "待开始", active: "进行中", closed: "已完成" };
const time = new Intl.DateTimeFormat("zh-CN", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
export const dateTime = (value: string) => time.format(new Date(value));

export function Mark() {
  return <span className="mark" aria-hidden="true"><Layers3 size={21} strokeWidth={2.2} /></span>;
}

export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return <label className="field"><span>{label}</span>{children}{hint && <small>{hint}</small>}</label>;
}

export function Status({ state, label }: { state: string; label?: string }) {
  return <span className={`status status-${state}`}><i />{label ?? statusLabel[state as TaskState] ?? runLabel[state] ?? state}</span>;
}

export function ErrorLine({ message }: { message?: string }) {
  if (!message) return null;
  return <p className="error-line" role="alert"><CircleAlert size={16} />{message.replace(/^Error invoking remote method '[^']+': Error: /, "")}</p>;
}

export function Empty({ title, hint, action, children }: { title: string; hint?: string; action?: () => void; children?: ReactNode }) {
  return <div className="empty">
    <span className="empty-mark"><Command size={23} strokeWidth={1.55} /></span>
    <strong>{title}</strong>{hint && <p>{hint}</p>}
    {action && <button className="button subtle" onClick={action}>{children}<ArrowUpRight size={15} /></button>}
  </div>;
}

export function SectionTitle({ title, subtitle, actions }: { title: string; subtitle?: string; actions?: ReactNode }) {
  return <header className="section-title"><div><h1>{title}</h1>{subtitle && <p>{subtitle}</p>}</div>{actions && <div className="title-actions">{actions}</div>}</header>;
}

export function Layer({ title, onClose, children, drawer = false }: { title: string; onClose: () => void; children: ReactNode; drawer?: boolean }) {
  return <div className={`overlay ${drawer ? "drawer-overlay" : ""}`} onMouseDown={(event) => {
    if (event.currentTarget === event.target) onClose();
  }}>
    <section role="dialog" aria-modal="true" aria-label={title} className={drawer ? "drawer" : "dialog"}>
      <header className="layer-heading"><h2>{title}</h2><button className="icon-button" aria-label="关闭" onClick={onClose}><X size={19} /></button></header>
      {children}
    </section>
  </div>;
}
