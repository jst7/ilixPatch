// Arrow-key navigation between visible tree rows; ArrowUp on the first row returns to the search box.
export function treeKeydown(e: KeyboardEvent) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const target = e.target as HTMLElement;
  if (target.getAttribute("role") !== "treeitem") return;
  const tree = e.currentTarget as HTMLElement;
  const rows = Array.from(tree.querySelectorAll<HTMLElement>("[role=treeitem]"));
  const i = rows.indexOf(target);
  if (i < 0) return;
  e.preventDefault();
  const next = rows[i + (e.key === "ArrowDown" ? 1 : -1)];
  if (next) next.focus();
  else if (e.key === "ArrowUp") tree.closest(".sidebar")?.querySelector<HTMLInputElement>(".tree-search input")?.focus();
}
