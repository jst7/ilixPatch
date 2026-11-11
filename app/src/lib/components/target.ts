// Resolve a runner / load-test target (collection dir, folder dir or request .toml) in the loaded tree.
import type { Collection, Folder, ProjectEntry, RequestFile } from "$lib/api";
import { isWithin } from "$lib/stores/workspace.svelte";

export interface TargetInfo {
  projectName: string;
  collection: Collection;
  kind: "collection" | "folder" | "request";
  /** Display chain: collection / folders / item. */
  crumbs: string[];
  /** Requests the target contains, in tree order. */
  requests: RequestFile[];
}

function collect(folder: Folder, out: RequestFile[]) {
  out.push(...folder.requests);
  for (const f of folder.folders) collect(f, out);
}

export function resolveTarget(entries: ProjectEntry[], target: string): TargetInfo | null {
  for (const entry of entries) {
    const project = entry.project;
    if (!project) continue;
    for (const collection of project.collections) {
      if (!isWithin(target, collection.path)) continue;
      const base = { projectName: project.name, collection };
      const walk = (folder: Folder, chain: string[]): TargetInfo | null => {
        if (folder.path === target) {
          const requests: RequestFile[] = [];
          collect(folder, requests);
          return { ...base, kind: folder === collection.root ? "collection" : "folder", crumbs: chain, requests };
        }
        const file = folder.requests.find((r) => r.path === target);
        if (file) return { ...base, kind: "request", crumbs: [...chain, file.request.name], requests: [file] };
        for (const sub of folder.folders) {
          if (!isWithin(target, sub.path)) continue;
          const found = walk(sub, [...chain, sub.name]);
          if (found) return found;
        }
        return null;
      };
      const found = walk(collection.root, [collection.name]);
      if (found) return found;
    }
  }
  return null;
}

export function formatSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

/** Milliseconds → "123 ms" / "4.56 s" / "2m 05s". */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(2)} s`;
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
}
