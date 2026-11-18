// Master password (D60). The app starts locked when the password isn't the default one.
import { api, type VaultStatus } from "$lib/api";

class VaultStore {
  status = $state<VaultStatus | null>(null);
  /** "Later" on the default-password notice (this session only). */
  noticeHidden = $state(false);
  error = $state<string | null>(null);
  busy = $state(false);

  get locked() {
    return this.status !== null && !this.status.unlocked;
  }

  get defaultPassword() {
    return !!this.status?.default_password;
  }

  async refresh() {
    try {
      this.status = await api.vaultStatus();
    } catch (e) {
      this.error = String(e);
      this.status = { unlocked: false, default_password: false };
    }
  }

  private async run(f: () => Promise<VaultStatus>): Promise<boolean> {
    this.busy = true;
    this.error = null;
    try {
      this.status = await f();
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    } finally {
      this.busy = false;
    }
  }

  unlock(password: string) {
    return this.run(() => api.vaultUnlock(password));
  }

  change(current: string, next: string) {
    return this.run(() => api.vaultChangePassword(current, next));
  }

  reset() {
    return this.run(() => api.vaultReset());
  }
}

export const vault = new VaultStore();
