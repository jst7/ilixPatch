// Global network settings (D29): loaded from the backend, edited in Settings, saved debounced.
import { api, type NetworkSettings } from "$lib/api";

const SAVE_DELAY = 600;

class NetworkStore {
  settings = $state<NetworkSettings | null>(null);
  loading = $state(false);
  saving = $state(false);
  /** Last load error. */
  loadError = $state<string | null>(null);
  /** Backend validation error of the last save (names the bad field). */
  saveError = $state<string | null>(null);
  /** True briefly after a successful save. */
  saved = $state(false);

  private timer: ReturnType<typeof setTimeout> | undefined;
  private savedTimer: ReturnType<typeof setTimeout> | undefined;
  private loadPromise: Promise<void> | null = null;

  /** Load once (call again with force to refresh). */
  load(force = false): Promise<void> {
    if (this.loadPromise && !force) return this.loadPromise;
    this.loading = true;
    this.loadPromise = api
      .networkGetSettings()
      .then((s) => {
        this.settings = s;
        this.loadError = null;
      })
      .catch((e) => {
        this.loadError = String(e);
        this.loadPromise = null;
      })
      .finally(() => (this.loading = false));
    return this.loadPromise;
  }

  /** Call after mutating `settings`; saves after a short pause. */
  changed() {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.save(), SAVE_DELAY);
  }

  async save() {
    clearTimeout(this.timer);
    if (!this.settings) return;
    this.saving = true;
    try {
      await api.networkSetSettings($state.snapshot(this.settings) as NetworkSettings);
      this.saveError = null;
      this.saved = true;
      clearTimeout(this.savedTimer);
      this.savedTimer = setTimeout(() => (this.saved = false), 1500);
    } catch (e) {
      this.saveError = String(e);
    } finally {
      this.saving = false;
    }
  }
}

export const network = new NetworkStore();
