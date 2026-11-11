// Small cross-component UI requests (e.g. "open Settings on a given section") so components
// that don't own a dialog can still ask for it.

/** Settings sections other components may deep-link to (subset of SettingsDialog's SectionId). */
export type SettingsSection = "general" | "appearance" | "colors" | "history" | "https" | "proxy" | "requests";

class UiStore {
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
