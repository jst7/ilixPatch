// "Is there a newer version?" (D54). At startup, at most once a day and unless turned off in Settings › General,
// the app asks GitHub for the latest published release; the About dialog checks on demand. Nothing is downloaded:
// the notice links to the release page. Per-user preferences, in localStorage (may be unavailable / throw).
import { api, type UpdateCheck } from "$lib/api";

const LS_AUTO = "ilixpatch.update.auto";
const LS_LAST = "ilixpatch.update.lastCheck";
const LS_SKIPPED = "ilixpatch.update.skipped";
const DAY_MS = 24 * 60 * 60 * 1000;

function lsGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function lsSet(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // ignore
  }
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e);
}

class UpdateStore {
  /** Check at startup (once a day). */
  auto = $state(lsGet(LS_AUTO, true));
  /** Result of the last check in this session. */
  result = $state.raw<UpdateCheck | null>(null);
  checking = $state(false);
  /** Error of the last manual check (automatic checks fail silently). */
  error = $state<string | null>(null);
  /** Version the user chose to skip: no notice for it. */
  skipped = $state<string | null>(lsGet<string | null>(LS_SKIPPED, null));
  /** Notice closed for this session. */
  hidden = $state(false);

  /** A newer version the startup notice should announce. */
  notice = $derived(
    this.result?.newer && !this.hidden && this.result.latest.version !== this.skipped ? this.result.latest : null,
  );

  setAuto(on: boolean) {
    this.auto = on;
    lsSet(LS_AUTO, on);
  }

  /** At startup: check if enabled and the last attempt is over a day old. Errors are silent (offline, rate limit). */
  async autoCheck() {
    if (!this.auto) return;
    const last = lsGet<number>(LS_LAST, 0);
    if (Date.now() - last < DAY_MS) return;
    // Recorded before asking, so being offline doesn't mean asking on every start.
    lsSet(LS_LAST, Date.now());
    try {
      this.result = await api.updateCheck();
    } catch {
      // Silent: the user didn't ask.
    }
  }

  /** "Check for updates" in the About dialog. */
  async check() {
    if (this.checking) return;
    this.checking = true;
    this.error = null;
    try {
      this.result = await api.updateCheck();
      lsSet(LS_LAST, Date.now());
      // Asking again un-hides a notice closed earlier.
      this.hidden = false;
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.checking = false;
    }
  }

  /** Never announce this version again (a later one is announced). */
  skip(version: string) {
    this.skipped = version;
    lsSet(LS_SKIPPED, version);
  }
}

export const updates = new UpdateStore();
