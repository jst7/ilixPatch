// Notifications (D65): short, stacked messages saying what just happened. Successes and infos close by
// themselves; errors stay until dismissed (they may need reading).
export type ToastKind = "success" | "info" | "warn" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
  /** Optional action ("Open", "Undo"…). */
  action?: { label: string; run: () => void };
}

const MAX = 4;
const LIFETIME: Record<ToastKind, number | null> = { success: 3000, info: 4500, warn: 7000, error: null };

class Toasts {
  list = $state<Toast[]>([]);
  private seq = 0;

  push(kind: ToastKind, text: string, action?: Toast["action"]) {
    // The same message again replaces the old one instead of stacking.
    this.list = this.list.filter((t) => t.text !== text);
    const id = ++this.seq;
    this.list = [...this.list, { id, kind, text, action }].slice(-MAX);
    const life = LIFETIME[kind];
    if (life) setTimeout(() => this.dismiss(id), life);
    return id;
  }

  success(text: string, action?: Toast["action"]) {
    return this.push("success", text, action);
  }
  info(text: string, action?: Toast["action"]) {
    return this.push("info", text, action);
  }
  warn(text: string, action?: Toast["action"]) {
    return this.push("warn", text, action);
  }
  error(text: string, action?: Toast["action"]) {
    return this.push("error", text, action);
  }

  dismiss(id: number) {
    this.list = this.list.filter((t) => t.id !== id);
  }
}

export const toasts = new Toasts();
