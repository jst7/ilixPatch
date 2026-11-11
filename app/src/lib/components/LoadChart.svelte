<script lang="ts">
  // One single-series time chart (small multiple). Hand-drawn SVG, colours from theme vars.
  interface Props {
    title: string;
    /** [seconds, value] points, x ascending. */
    points: [number, number][];
    /** X range end (seconds). */
    xMax: number;
    color: string;
    format: (v: number) => string;
    /** Fill the area under the line. */
    area?: boolean;
    /** Hovered x (seconds), shared between charts. */
    hoverX?: number | null;
    onhover?: (x: number | null) => void;
  }

  let { title, points, xMax, color, format, area = false, hoverX = null, onhover }: Props = $props();

  const W = 600;
  const H = 120;
  const PAD_L = 44;
  const PAD_R = 8;
  const PAD_T = 8;
  const PAD_B = 18;

  function niceMax(v: number): number {
    if (v <= 0) return 1;
    const p = Math.pow(10, Math.floor(Math.log10(v)));
    for (const m of [1, 2, 2.5, 5, 10]) if (m * p >= v) return m * p;
    return 10 * p;
  }

  const yMax = $derived(niceMax(points.reduce((m, [, v]) => Math.max(m, v), 0)));
  const xs = (x: number) => PAD_L + (x / Math.max(xMax, 1)) * (W - PAD_L - PAD_R);
  const ys = (y: number) => PAD_T + (1 - y / yMax) * (H - PAD_T - PAD_B);

  const line = $derived(
    points.map(([x, y], i) => `${i ? "L" : "M"}${xs(x).toFixed(1)},${ys(y).toFixed(1)}`).join(""),
  );
  const fill = $derived(
    points.length
      ? `${line}L${xs(points[points.length - 1][0]).toFixed(1)},${ys(0)}L${xs(points[0][0]).toFixed(1)},${ys(0)}Z`
      : "",
  );
  const ticks = $derived([0, yMax / 2, yMax]);
  const xTicks = $derived.by(() => {
    const step = niceMax(xMax / 5);
    const out: number[] = [];
    for (let x = 0; x <= xMax + 1e-9; x += step) out.push(x);
    return out;
  });

  const hovered = $derived.by(() => {
    if (hoverX === null || points.length === 0) return null;
    let best = points[0];
    for (const p of points) if (Math.abs(p[0] - hoverX) < Math.abs(best[0] - hoverX)) best = p;
    return best;
  });

  let svg: SVGSVGElement | undefined = $state();

  function move(e: PointerEvent) {
    if (!svg || !onhover) return;
    const r = svg.getBoundingClientRect();
    const px = ((e.clientX - r.left) / r.width) * W;
    const x = ((px - PAD_L) / (W - PAD_L - PAD_R)) * Math.max(xMax, 1);
    onhover(x < 0 || x > xMax ? null : x);
  }
</script>

<figure class="chart">
  <figcaption>
    <span class="swatch" style:background={color}></span>
    <span class="t">{title}</span>
    {#if hovered}
      <span class="hv">{Math.round(hovered[0])}s · <strong>{format(hovered[1])}</strong></span>
    {:else if points.length}
      <span class="hv"><strong>{format(points[points.length - 1][1])}</strong></span>
    {/if}
  </figcaption>
  <svg
    bind:this={svg}
    viewBox="0 0 {W} {H}"
    role="img"
    aria-label={title}
    onpointermove={move}
    onpointerleave={() => onhover?.(null)}
  >
    {#each ticks as tk (tk)}
      <line class="grid" x1={PAD_L} x2={W - PAD_R} y1={ys(tk)} y2={ys(tk)} />
      <text class="axis" x={PAD_L - 6} y={ys(tk) + 3} text-anchor="end">{format(tk)}</text>
    {/each}
    {#each xTicks as x (x)}
      <text class="axis" x={xs(x)} y={H - 4} text-anchor="middle">{Math.round(x)}s</text>
    {/each}
    {#if area && fill}<path d={fill} class="area" style:fill={color} />{/if}
    {#if points.length > 1}
      <path d={line} class="line" style:stroke={color} />
    {/if}
    {#if hovered}
      <line class="cross" x1={xs(hovered[0])} x2={xs(hovered[0])} y1={PAD_T} y2={H - PAD_B} />
      <circle cx={xs(hovered[0])} cy={ys(hovered[1])} r="4" class="dot" style:fill={color} />
    {/if}
  </svg>
</figure>

<style>
  .chart {
    margin: 0;
    min-width: 0;
  }
  figcaption {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--muted);
    margin-bottom: 2px;
  }
  .swatch {
    width: 10px;
    height: 3px;
    border-radius: 2px;
  }
  .t {
    color: var(--text);
    font-weight: 500;
  }
  .hv {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }
  .hv strong {
    color: var(--text);
  }
  svg {
    width: 100%;
    height: auto;
    display: block;
    touch-action: none;
  }
  .grid {
    stroke: var(--border);
    stroke-width: 1;
  }
  .axis {
    fill: var(--muted);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .line {
    fill: none;
    stroke-width: 2;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .area {
    opacity: 0.15;
  }
  .cross {
    stroke: var(--muted);
    stroke-width: 1;
    stroke-dasharray: 3 3;
  }
  .dot {
    stroke: var(--surface);
    stroke-width: 2;
  }
</style>
