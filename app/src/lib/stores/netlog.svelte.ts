// Network console (D46): exchanges reported by the backend (`netlog://entry`), newest last.
import { listen } from "@tauri-apps/api/event";
import { api, type NetEntry } from "$lib/api";

const MAX = 500;
const LS_KEY = "ilixpatch.netlog";

function load(): { open: boolean; height: number } {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) return { open: false, height: 280, ...JSON.parse(raw) };
  } catch {
    // ignore
  }
  return { open: false, height: 280 };
}

class NetLogStore {
  entries = $state.raw<NetEntry[]>([]);
  open = $state(load().open);
  height = $state(load().height);
  selected = $state<number | null>(null);
  private started = false;

  async init() {
    if (this.started) return;
    this.started = true;
    try {
      this.entries = await api.netlogList();
    } catch {
      this.entries = [];
    }
    await listen<NetEntry>("netlog://entry", (e) => {
      const next = this.entries.length >= MAX ? this.entries.slice(-(MAX - 1)) : this.entries.slice();
      next.push(e.payload);
      this.entries = next;
    });
  }

  save() {
    try {
      localStorage.setItem(LS_KEY, JSON.stringify({ open: this.open, height: this.height }));
    } catch {
      // ignore
    }
  }

  toggle() {
    this.open = !this.open;
    this.save();
  }

  async clear() {
    try {
      await api.netlogClear();
    } catch {
      // ignore
    }
    this.entries = [];
    this.selected = null;
  }
}

export const netlog = new NetLogStore();
