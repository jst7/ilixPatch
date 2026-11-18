// Mock servers (D62): running servers, their settings and the log of served requests, per target.
import { listen } from "@tauri-apps/api/event";
import { api, type MockLogEntry, type MockStatus } from "$lib/api";

const MAX_LOG = 500;

interface MockConfig {
  port: number;
  delay_ms: number;
}

class MockStore {
  running = $state<Record<string, MockStatus>>({});
  logs = $state<Record<string, MockLogEntry[]>>({});
  config = $state<Record<string, MockConfig>>({});
  error = $state<Record<string, string | null>>({});
  busy = $state<Record<string, boolean>>({});
  private listening = false;

  async init() {
    if (this.listening) return;
    this.listening = true;
    await listen<{ target: string; entry: MockLogEntry }>("mock://request", (e) => {
      const { target, entry } = e.payload;
      this.logs[target] = [entry, ...(this.logs[target] ?? [])].slice(0, MAX_LOG);
    });
    try {
      for (const s of await api.mockList()) this.running[s.target] = s;
    } catch {
      // ignore: nothing running
    }
  }

  /** Create the settings of `target` if missing. Call it outside derived/template code (it writes state). */
  ensure(target: string): MockConfig {
    if (!this.config[target]) this.config[target] = { port: 4000, delay_ms: 0 };
    return this.config[target];
  }

  async start(target: string) {
    const c = this.ensure(target);
    this.busy[target] = true;
    this.error[target] = null;
    try {
      this.running[target] = await api.mockStart(target, Number(c.port) || 0, Number(c.delay_ms) || 0);
      this.config[target].port = this.running[target].port;
    } catch (e) {
      this.error[target] = String(e);
    } finally {
      this.busy[target] = false;
    }
  }

  async stop(target: string) {
    try {
      await api.mockStop(target);
    } finally {
      delete this.running[target];
    }
  }

  clearLog(target: string) {
    this.logs[target] = [];
  }
}

export const mocks = new MockStore();
