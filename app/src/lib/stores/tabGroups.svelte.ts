// Tab folders (D50): open tabs organised in nested, collapsible folders in the tabs sidebar.
// Only the grouping lives here (tab order and the tabs themselves are in the workspace store).
// Persisted per user in localStorage, like the open tabs.

export interface TabGroup {
  id: string;
  name: string;
  /** Parent folder id (null = top level). */
  parent: string | null;
  collapsed: boolean;
}

const LS_KEY = "ilixpatch.tabGroups";

interface Saved {
  groups: TabGroup[];
  member: Record<string, string>;
}

function load(): Saved {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const s = JSON.parse(raw) as Partial<Saved>;
      return { groups: Array.isArray(s.groups) ? s.groups : [], member: s.member ?? {} };
    }
  } catch {
    // ignore
  }
  return { groups: [], member: {} };
}

let seq = 0;
const newId = () => `g${Date.now().toString(36)}${(seq++).toString(36)}`;

class TabGroups {
  groups = $state<TabGroup[]>(load().groups);
  /** Folder of each tab, by tab key (absent = top level). */
  member = $state<Record<string, string>>(load().member);
  /** Folder whose name is being edited (inline). */
  renaming = $state<string | null>(null);

  private save() {
    try {
      localStorage.setItem(LS_KEY, JSON.stringify({ groups: this.groups, member: this.member }));
    } catch {
      // ignore
    }
  }

  get(id: string | null | undefined): TabGroup | undefined {
    return id ? this.groups.find((g) => g.id === id) : undefined;
  }

  children(parent: string | null): TabGroup[] {
    return this.groups.filter((g) => g.parent === parent);
  }

  groupOf(tabKey: string): string | null {
    const g = this.member[tabKey];
    return g && this.get(g) ? g : null;
  }

  /** `id` and all its subfolders. */
  subtree(id: string): Set<string> {
    const out = new Set([id]);
    let grew = true;
    while (grew) {
      grew = false;
      for (const g of this.groups) {
        if (g.parent && out.has(g.parent) && !out.has(g.id)) {
          out.add(g.id);
          grew = true;
        }
      }
    }
    return out;
  }

  /** Path of names from the top, e.g. "API / Users". */
  path(id: string): string {
    const names: string[] = [];
    for (let g = this.get(id); g; g = this.get(g.parent)) names.unshift(g.name);
    return names.join(" / ");
  }

  /** Folders in tree order with their depth (for menus). */
  flat(): { group: TabGroup; depth: number }[] {
    const out: { group: TabGroup; depth: number }[] = [];
    const walk = (parent: string | null, depth: number) => {
      for (const g of this.children(parent)) {
        out.push({ group: g, depth });
        walk(g.id, depth + 1);
      }
    };
    walk(null, 0);
    return out;
  }

  create(name: string, parent: string | null = null, rename = true): string {
    const id = newId();
    this.groups.push({ id, name, parent, collapsed: false });
    if (parent) this.setCollapsed(parent, false);
    if (rename) this.renaming = id;
    this.save();
    return id;
  }

  rename(id: string, name: string) {
    const g = this.get(id);
    if (g && name.trim()) g.name = name.trim();
    this.renaming = null;
    this.save();
  }

  /** Delete a folder: its tabs and subfolders move up to its parent. */
  remove(id: string) {
    const g = this.get(id);
    if (!g) return;
    for (const c of this.groups) if (c.parent === id) c.parent = g.parent;
    for (const [k, v] of Object.entries(this.member)) {
      if (v !== id) continue;
      if (g.parent) this.member[k] = g.parent;
      else delete this.member[k];
    }
    this.groups = this.groups.filter((x) => x.id !== id);
    this.save();
  }

  /** Move a folder under another one (null = top level); refuses cycles. */
  moveGroup(id: string, parent: string | null) {
    const g = this.get(id);
    if (!g || (parent && this.subtree(id).has(parent))) return;
    g.parent = parent;
    this.save();
  }

  setCollapsed(id: string, collapsed: boolean) {
    const g = this.get(id);
    if (g && g.collapsed !== collapsed) {
      g.collapsed = collapsed;
      this.save();
    }
  }

  toggle(id: string) {
    const g = this.get(id);
    if (g) this.setCollapsed(id, !g.collapsed);
  }

  setAllCollapsed(collapsed: boolean) {
    for (const g of this.groups) g.collapsed = collapsed;
    this.save();
  }

  /** Put a tab in a folder (null = top level). */
  assign(tabKey: string, group: string | null) {
    if (group && this.get(group)) this.member[tabKey] = group;
    else delete this.member[tabKey];
    this.save();
  }

  /** Forget closed tabs. Folders are kept (also empty ones) until deleted. */
  prune(openKeys: Set<string>) {
    let changed = false;
    for (const k of Object.keys(this.member)) {
      if (!openKeys.has(k) || !this.get(this.member[k])) {
        delete this.member[k];
        changed = true;
      }
    }
    if (changed) this.save();
  }

  /** Number of tabs in a folder and its subfolders. */
  count(id: string, openKeys: Iterable<string>): number {
    const ids = this.subtree(id);
    let n = 0;
    for (const k of openKeys) if (ids.has(this.member[k])) n++;
    return n;
  }
}

export const tabGroups = new TabGroups();
