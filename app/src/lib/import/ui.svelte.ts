// UI helpers of the import wizard: pointer-based drag & drop (HTML5 DnD conflicts with Tauri's file drop),
// the keyboard "Place into…" menu and warning labels.
import type { Folder, ImportWarning } from "$lib/api";
import { LOCALES, t, type MessageKey } from "$lib/i18n/index.svelte";
import { importer } from "$lib/stores/import.svelte";
import { ws, type MenuItem } from "$lib/stores/workspace.svelte";
import { dropAs, type DestKind } from "./plan";

export interface DragState {
  id: string;
  label: string;
  x: number;
  y: number;
  /** Destination under the pointer (any node with data-drop), valid or not. */
  target: { path: string; kind: DestKind } | null;
}

/** Current drag (null when not dragging). */
export const dnd = $state<{ drag: DragState | null }>({ drag: null });

/** Start dragging a source (or ghost) node once the pointer moves a few pixels. */
export function startDrag(e: PointerEvent, id: string, label: string) {
  if (e.button !== 0 || (e.target as HTMLElement).closest("input, button, select, textarea")) return;
  const sx = e.clientX;
  const sy = e.clientY;
  let active = false;
  const move = (ev: PointerEvent) => {
    if (!active && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 5) return;
    active = true;
    const el = (document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null)?.closest<HTMLElement>("[data-drop]");
    const target = el?.dataset.drop ? { path: el.dataset.drop, kind: el.dataset.kind as DestKind } : null;
    dnd.drag = { id, label, x: ev.clientX, y: ev.clientY, target };
  };
  const cleanup = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("keydown", key, true);
  };
  const up = () => {
    cleanup();
    const target = dnd.drag?.target;
    dnd.drag = null;
    if (!active) return;
    // Swallow the click that follows the drop.
    window.addEventListener("click", (c) => c.stopPropagation(), { capture: true, once: true });
    if (target) importer.placeInto(id, target.path, target.kind);
  };
  const key = (ev: KeyboardEvent) => {
    if (ev.key !== "Escape") return;
    ev.preventDefault();
    ev.stopPropagation();
    cleanup();
    dnd.drag = null;
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("keydown", key, true);
}

/** Whether the node being dragged may be dropped on a destination of `kind`. */
export function canDropOn(kind: DestKind): boolean {
  const id = dnd.drag?.id;
  const node = id ? importer.index.get(id)?.node : null;
  return !!node && dropAs(node.kind, kind) !== null;
}

/** Every valid destination dir of the selected project, for the keyboard alternative to dragging. */
export function placeMenu(sourceId: string, x: number, y: number) {
  const node = importer.index.get(sourceId)?.node;
  const entry = ws.entries.find((e) => e.path === importer.project);
  if (!node || !entry?.project) return;
  const items: MenuItem[] = [];
  const root = entry.path;
  const rel = (p: string) => p.slice(root.length).replace(/^[\\/]+/, "").replace(/\\/g, "/");
  if (dropAs(node.kind, "project")) {
    items.push({ label: t("import.place.root", { name: entry.project.name }), action: () => importer.placeInto(sourceId, root, "project") });
  }
  if (dropAs(node.kind, "folder")) {
    const walk = (f: Folder, kind: DestKind) => {
      items.push({ label: rel(f.path) || f.name, action: () => importer.placeInto(sourceId, f.path, kind) });
      for (const sub of f.folders) walk(sub, "folder");
    };
    for (const c of entry.project.collections) walk({ ...c.root, path: c.path }, "collection");
  }
  if (importer.placements.some((p) => p.source_id === sourceId)) {
    items.push({ label: t("import.place.remove"), danger: true, action: () => importer.unplace(sourceId) });
  }
  ws.menu = { x, y, items };
}

const EN = LOCALES.find((l) => l.code === "en")!.messages as Record<string, string>;

/** Translated title of a warning code (`import.warn.<code>`), or null when unknown. */
export function warningTitle(code: string): string | null {
  const key = `import.warn.${code}`;
  return key in EN ? t(key as MessageKey) : null;
}

export function warningText(w: ImportWarning): string {
  return warningTitle(w.code) ?? w.message;
}

/** Warnings by source node id. */
export function warningsByNode(warnings: ImportWarning[]): Map<string, ImportWarning[]> {
  const map = new Map<string, ImportWarning[]>();
  for (const w of warnings) {
    if (!w.source_id) continue;
    const list = map.get(w.source_id) ?? [];
    list.push(w);
    map.set(w.source_id, list);
  }
  return map;
}

/** Select a source node: expand its ancestors and scroll it into view. */
export function revealSource(id: string) {
  importer.selected = id;
  let p = importer.index.get(id)?.parent ?? null;
  while (p) {
    importer.expanded[p] = true;
    p = importer.index.get(p)?.parent ?? null;
  }
  requestAnimationFrame(() => document.querySelector(`[data-src="${CSS.escape(id)}"]`)?.scrollIntoView({ block: "nearest" }));
}
