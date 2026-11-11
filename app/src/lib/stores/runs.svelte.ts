// Collection-runner and load-test sessions, keyed by target path. They live outside the
// components so a run keeps streaming (and its results survive) while its tab is hidden.
//
// Large runs: results are kept in plain (non-reactive) arrays; a `version` counter is bumped
// at most once per animation frame so the UI re-renders O(visible rows), not O(n) per event.
import {
  api,
  type LoadOptions,
  type LoadProfile,
  type LoadSnapshot,
  type RequestRunResult,
  type RunEvent,
  type RunSummary,
} from "$lib/api";
import type { EnvChoice } from "./environments.svelte";

/** Runner/load options for an environment choice (legacy collection envs: no scope → backend fallback). */
function envOptions(env: EnvChoice | null): { environment: string | null; environment_scope: "project" | "global" | null } {
  if (!env) return { environment: null, environment_scope: null };
  return { environment: env.name, environment_scope: env.scope === "collection" ? null : env.scope };
}

function frame(cb: () => void): void {
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(cb);
  else setTimeout(cb, 16);
}

export function resultFailed(r: RequestRunResult): boolean {
  return r.error !== null || r.tests.some((x) => !x.passed && !x.skipped);
}

// ---------- Collection runner ----------

export interface RunnerConfig {
  /** Environment to run with (D28). */
  env: EnvChoice | null;
  iterations: number;
  delay_ms: number;
  timeout_ms: number;
  data_file: string | null;
  bail: boolean;
  persist_variables: boolean;
}

export class RunnerSession {
  readonly target: string;
  config = $state<RunnerConfig>({
    env: null,
    iterations: 1,
    delay_ms: 0,
    timeout_ms: 0,
    data_file: null,
    bail: false,
    persist_variables: true,
  });
  /** Whether the environment was picked by the user (else follow the request editor's choice). */
  envTouched = $state(false);
  running = $state(false);
  started = $state(false);
  total = $state(0);
  iterations = $state(0);
  done = $state(0);
  failedCount = $state(0);
  summary = $state.raw<RunSummary | null>(null);
  error = $state<string | null>(null);
  /** Bumped (throttled) whenever the plain arrays below change. */
  version = $state(0);

  results: RequestRunResult[] = [];
  /** Indices into `results`. */
  passedIdx: number[] = [];
  failedIdx: number[] = [];

  private pending = false;

  constructor(target: string) {
    this.target = target;
  }

  private bump() {
    if (this.pending) return;
    this.pending = true;
    frame(() => {
      this.pending = false;
      this.done = this.results.length;
      this.failedCount = this.failedIdx.length;
      this.version++;
    });
  }

  private onEvent = (e: RunEvent) => {
    switch (e.type) {
      case "started":
        this.total = e.total_requests;
        this.iterations = e.iterations;
        break;
      case "iteration":
        break;
      case "request": {
        const i = this.results.length;
        this.results.push(e.result);
        (resultFailed(e.result) ? this.failedIdx : this.passedIdx).push(i);
        this.bump();
        break;
      }
      case "finished":
        this.summary = e.summary;
        break;
    }
  };

  async run() {
    if (this.running) return;
    this.results = [];
    this.passedIdx = [];
    this.failedIdx = [];
    this.done = 0;
    this.failedCount = 0;
    this.total = 0;
    this.iterations = 0;
    this.summary = null;
    this.error = null;
    this.version++;
    this.running = true;
    this.started = true;
    const c = this.config;
    try {
      const summary = await api.runCollection(
        {
          target: this.target,
          ...envOptions(c.env),
          iterations: Math.max(1, Math.floor(c.iterations || 1)),
          data_file: c.data_file,
          delay_ms: Math.max(0, Math.floor(c.delay_ms || 0)),
          timeout_ms: Math.max(0, Math.floor(c.timeout_ms || 0)),
          bail: c.bail,
          persist_variables: c.persist_variables,
        },
        this.onEvent,
      );
      this.summary = summary;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.running = false;
      this.done = this.results.length;
      this.failedCount = this.failedIdx.length;
      this.version++;
    }
  }

  async cancel() {
    try {
      await api.cancelRun();
    } catch (e) {
      this.error = String(e);
    }
  }
}

// ---------- Load test ----------

export interface LoadConfig {
  /** Environment to run with (D28). */
  env: EnvChoice | null;
  virtual_users: number;
  duration_secs: number;
  ramp_up_secs: number;
  profile: LoadProfile;
  think_time_ms: number;
  run_scripts: boolean;
}

export class LoadSession {
  readonly target: string;
  config = $state<LoadConfig>({
    env: null,
    virtual_users: 10,
    duration_secs: 60,
    ramp_up_secs: 10,
    profile: "fixed",
    think_time_ms: 0,
    run_scripts: true,
  });
  envTouched = $state(false);
  running = $state(false);
  started = $state(false);
  latest = $state.raw<LoadSnapshot | null>(null);
  final = $state.raw<LoadSnapshot | null>(null);
  error = $state<string | null>(null);
  /** Duration of the current/last run (for the chart's x range). */
  runDuration = $state(0);
  version = $state(0);
  /** All snapshots of the current run (plain array, ~1 per second). */
  snapshots: LoadSnapshot[] = [];

  constructor(target: string) {
    this.target = target;
  }

  private onSnapshot = (s: LoadSnapshot) => {
    this.snapshots.push(s);
    this.latest = s;
    if (s.final) this.final = s;
    this.version++;
  };

  async start() {
    if (this.running) return;
    const c = this.config;
    this.snapshots = [];
    this.latest = null;
    this.final = null;
    this.error = null;
    this.running = true;
    this.started = true;
    this.runDuration = Math.max(1, Math.floor(c.duration_secs || 1));
    this.version++;
    const options: LoadOptions = {
      target: this.target,
      ...envOptions(c.env),
      virtual_users: Math.max(1, Math.floor(c.virtual_users || 1)),
      duration_secs: this.runDuration,
      ramp_up_secs: Math.max(0, Math.floor(c.ramp_up_secs || 0)),
      profile: c.profile,
      think_time_ms: Math.max(0, Math.floor(c.think_time_ms || 0)),
      run_scripts: c.run_scripts,
    };
    try {
      const last = await api.startLoadTest(options, this.onSnapshot);
      if (!this.final) {
        this.final = last;
        this.latest = last;
        if (this.snapshots[this.snapshots.length - 1] !== last) this.snapshots.push(last);
        this.version++;
      }
    } catch (e) {
      this.error = String(e);
    } finally {
      this.running = false;
    }
  }

  async stop() {
    try {
      await api.stopLoadTest();
    } catch (e) {
      this.error = String(e);
    }
  }
}

/** Target VU count at time `t` for a profile (mirrors the backend's intent; used for the preview). */
export function vuAt(profile: LoadProfile, t: number, vus: number, duration: number, ramp: number): number {
  const r = Math.max(0, Math.min(ramp, duration));
  switch (profile) {
    case "fixed":
      return vus;
    case "ramp_up":
      return r > 0 && t < r ? Math.max(1, (vus * t) / r) : vus;
    case "spike": {
      // Baseline load with a short burst to full VUs in the middle.
      const base = Math.max(1, Math.round(vus * 0.2));
      const width = Math.max(duration * 0.2, 1);
      const mid = duration / 2;
      return Math.abs(t - mid) <= width / 2 ? vus : base;
    }
    case "peak": {
      // Ramp up, hold, ramp down.
      if (r > 0 && t < r) return Math.max(1, (vus * t) / r);
      if (r > 0 && t > duration - r) return Math.max(1, (vus * (duration - t)) / r);
      return vus;
    }
  }
}

const runners = new Map<string, RunnerSession>();
const loads = new Map<string, LoadSession>();

export function runnerSession(target: string): RunnerSession {
  let s = runners.get(target);
  if (!s) runners.set(target, (s = new RunnerSession(target)));
  return s;
}

export function loadSession(target: string): LoadSession {
  let s = loads.get(target);
  if (!s) loads.set(target, (s = new LoadSession(target)));
  return s;
}
