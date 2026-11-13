// Environment icon and colour (D49): ids stored in the environment file, drawn by EnvIcon.

/** 16×16 stroke icons (paths), by id. "layers" is the default. */
export const ENV_ICONS: Record<string, string[]> = {
  layers: ["M8 2l6 3-6 3-6-3z", "M2 8l6 3 6-3", "M2 11l6 3 6-3"],
  laptop: ["M3 4h10v7H3z", "M1.5 13h13"],
  server: ["M2.5 2.5h11v4h-11z", "M2.5 9.5h11v4h-11z", "M5 4.5h.01", "M5 11.5h.01"],
  cloud: ["M4.5 12.5h7a3 3 0 0 0 .3-6 4 4 0 0 0-7.6-1A3.5 3.5 0 0 0 4.5 12.5z"],
  flask: ["M6 2h4", "M6.5 2v4L3 13a1 1 0 0 0 .9 1.5h8.2A1 1 0 0 0 13 13L9.5 6V2", "M4.5 10h7"],
  rocket: ["M9.5 2.5c2.5 0 4 1.5 4 4L8 12l-4-4z", "M4 8L2 7.5l2-2.5h3", "M8 12l.5 2 2.5-2V9", "M10 6h.01"],
  bug: ["M5.5 5.5h5v6a2.5 2.5 0 0 1-5 0z", "M6 3.5l1 2M10 3.5l-1 2", "M2.5 8h3M10.5 8h3M3 12l2.5-1M13 12l-2.5-1"],
  shield: ["M8 2l5 2v4c0 3-2.2 5.2-5 6-2.8-.8-5-3-5-6V4z"],
  globe: ["M8 2a6 6 0 1 0 0 12A6 6 0 1 0 8 2z", "M2 8h12", "M8 2c1.8 2 1.8 10 0 12M8 2c-1.8 2-1.8 10 0 12"],
  database: ["M3 4c0-1.1 2.2-2 5-2s5 .9 5 2-2.2 2-5 2-5-.9-5-2z", "M3 4v8c0 1.1 2.2 2 5 2s5-.9 5-2V4", "M3 8c0 1.1 2.2 2 5 2s5-.9 5-2"],
  flag: ["M3.5 14V2.5", "M3.5 3h8l-1.5 3 1.5 3h-8"],
  star: ["M8 2l1.8 3.8 4.2.5-3.1 2.9.8 4.1L8 11.3l-3.7 2 .8-4.1L2 6.3l4.2-.5z"],
};

export const ENV_ICON_IDS = Object.keys(ENV_ICONS);

/** Theme colours offered first (they follow light/dark themes); any #rrggbb is also allowed. */
export const ENV_COLORS = ["accent", "ok", "warn", "err", "syntax-keyword", "syntax-number", "syntax-string", "syntax-key", "muted"];

/** CSS colour of a stored value: a theme token name or a hex colour; undefined = inherit. */
export function envColor(c: string | null | undefined): string | undefined {
  if (!c) return undefined;
  if (/^#[0-9a-fA-F]{3,8}$/.test(c)) return c;
  if (/^[a-z0-9-]+$/.test(c)) return `var(--${c})`;
  return undefined;
}

export function envIconPaths(icon: string | null | undefined): string[] {
  return ENV_ICONS[icon ?? ""] ?? ENV_ICONS.layers;
}
