// Small cross-component UI requests (e.g. "open Settings on a given section") so components
// that don't own a dialog can still ask for it.

/** Settings sections other components may deep-link to (subset of SettingsDialog's SectionId). */
export type SettingsSection = "general" | "appearance" | "colors" | "history" | "https" | "proxy" | "requests";

const LS_WRAP = "ilixpatch.response.wrap";

function readWrap(): boolean {
  try {
    return localStorage.getItem(LS_WRAP) !== "false";
  } catch {
    return true;
  }
}

class UiStore {
  /** Wrap long lines in the response body viewer (on by default; per user). */
  wrapResponse = $state(readWrap());

  setWrapResponse(on: boolean) {
    this.wrapResponse = on;
    try {
      localStorage.setItem(LS_WRAP, String(on));
    } catch {
      // ignore
    }
  }

  /** Settings dialog requested by a component (Sidebar owns the dialog and reads this). */
  settingsOpen = $state(false);
  settingsSection = $state<SettingsSection | undefined>(undefined);

  openSettings(section?: SettingsSection) {
    this.settingsSection = section;
    this.settingsOpen = true;
  }

  closeSettings() {
    this.settingsOpen = false;
    this.settingsSection = undefined;
  }
}

export const ui = new UiStore();
