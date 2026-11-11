// Request history (D22): list/search/pagination state for the sidebar view, plus settings and stats.
// The backend records every send; the UI only reads, deletes and configures.
// Every call degrades gracefully: if the backend command fails (e.g. not available yet), we show an
// empty list with a small error instead of throwing.
import { api, type HistoryEntrySummary, type HistorySettings, type HistoryStats } from "$lib/api";

export const HISTORY_PAGE = 100;

class HistoryStore {
  items = $state<HistoryEntrySummary[]>([]);
  query = $state("");
  loading = $state(false);
  error = $state<string | null>(null);
  hasMore = $state(false);
  /** Ids ticked for bulk deletion. */
  selected = $state<Record<number, true>>({});
  settings = $state<HistorySettings | null>(null);
  stats = $state<HistoryStats | null>(null);

  selectedIds = $derived(Object.keys(this.selected).map(Number));

  /** Bumped on every refresh so stale responses (older query) are ignored. */
  private seq = 0;

  async refresh() {
    const seq = ++this.seq;
    this.loading = true;
    try {
      const list = await api.historyList(this.query.trim() || null, null, HISTORY_PAGE);
      if (seq !== this.seq) return;
      this.items = list;
      this.hasMore = list.length === HISTORY_PAGE;
      this.error = null;
      // Drop selections of entries that are gone.
      const ids = new Set(list.map((e) => e.id));
      for (const id of this.selectedIds) if (!ids.has(id)) delete this.selected[id];
    } catch (e) {
      if (seq !== this.seq) return;
      this.items = [];
      this.hasMore = false;
      this.error = String(e);
    } finally {
      if (seq === this.seq) this.loading = false;
    }
  }

  /** Next page (older entries), appended. */
  async loadMore() {
    if (this.loading || !this.hasMore || this.items.length === 0) return;
    const seq = this.seq;
    this.loading = true;
    try {
      const before = this.items[this.items.length - 1].id;
      const list = await api.historyList(this.query.trim() || null, before, HISTORY_PAGE);
      if (seq !== this.seq) return;
      const known = new Set(this.items.map((e) => e.id));
      this.items.push(...list.filter((e) => !known.has(e.id)));
      this.hasMore = list.length === HISTORY_PAGE;
    } catch (e) {
      if (seq !== this.seq) return;
      this.hasMore = false;
      this.error = String(e);
    } finally {
      if (seq === this.seq) this.loading = false;
    }
  }

  async loadSettings() {
    try {
      this.settings = await api.historyGetSettings();
    } catch {
      this.settings = null;
    }
    await this.loadStats();
  }

  async loadStats() {
    try {
      this.stats = await api.historyStats();
    } catch {
      this.stats = null;
    }
  }

  /** Persist settings; returns an error message or null. */
  async saveSettings(settings: HistorySettings): Promise<string | null> {
    try {
      this.stats = await api.historySetSettings(settings);
      this.settings = settings;
      return null;
    } catch (e) {
      return String(e);
    }
  }

  /** Delete entries; throws on failure so the caller can report it. */
  async remove(ids: number[]) {
    if (ids.length === 0) return;
    await api.historyDelete(ids);
    const gone = new Set(ids);
    this.items = this.items.filter((e) => !gone.has(e.id));
    for (const id of ids) delete this.selected[id];
    void this.loadStats();
  }

  async clear() {
    await api.historyClear();
    this.items = [];
    this.selected = {};
    this.hasMore = false;
    void this.loadStats();
  }

  toggleSelected(id: number, value = !this.selected[id]) {
    if (value) this.selected[id] = true;
    else delete this.selected[id];
  }

  clearSelection() {
    this.selected = {};
  }
}

export const history = new HistoryStore();
