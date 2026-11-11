// Runtime variable layer (Postman's "current value"): values written by scripts via
// pm.environment.set / pm.collectionVariables.set / pm.globals.set during this session.
// They live only in memory, override the values stored in files, and can be reset.
// `null` records an explicit unset, which hides the file value too.
import type { SendVariables, VarUpdate } from "$lib/api";

export type Overrides = Record<string, string | null>;

// Environment overrides are keyed by an environment id (scope + owner + name, see
// environments.svelte.ts `envId`), so project/global environments share their runtime values
// across all collections that use them.

/** Apply overrides on top of file values (null removes the key). */
function merge(base: Record<string, string>, overrides: Overrides | undefined): Record<string, string> {
  if (!overrides) return { ...base };
  const out = { ...base };
  for (const [k, v] of Object.entries(overrides)) {
    if (v === null) delete out[k];
    else out[k] = v;
  }
  return out;
}

class RuntimeVars {
  globals = $state<Overrides>({});
  /** By collection path. */
  collection = $state<Record<string, Overrides>>({});
  /** By environment id. */
  environment = $state<Record<string, Overrides>>({});

  /** Store the variable writes of one send. `local` writes only live for that request and are ignored. */
  apply(collectionPath: string, envId: string | null, updates: VarUpdate[]) {
    for (const u of updates) {
      if (u.scope === "global") this.globals[u.key] = u.value;
      else if (u.scope === "collection") (this.collection[collectionPath] ??= {})[u.key] = u.value;
      else if (u.scope === "environment" && envId) {
        (this.environment[envId] ??= {})[u.key] = u.value;
      }
    }
  }

  /** Variables for a send/run: file values with runtime overrides applied, by scope. */
  sendVariables(
    collectionPath: string,
    collectionVars: Record<string, string>,
    envId: string | null,
    envName: string | null,
    envVars: Record<string, string>,
    globalVars: Record<string, string> = {},
  ): SendVariables {
    return {
      globals: merge(globalVars, this.globals),
      collection: merge(collectionVars, this.collection[collectionPath]),
      environment: envId ? merge(envVars, this.environment[envId]) : {},
      environment_name: envId ? envName : null,
    };
  }

  collectionOverrides(collectionPath: string): Overrides {
    return this.collection[collectionPath] ?? {};
  }

  environmentOverrides(envId: string | null): Overrides {
    return envId ? (this.environment[envId] ?? {}) : {};
  }

  /** Number of runtime values visible to a request of this collection/environment. */
  count(collectionPath: string, envId: string | null): number {
    return (
      Object.keys(this.globals).length +
      Object.keys(this.collectionOverrides(collectionPath)).length +
      Object.keys(this.environmentOverrides(envId)).length
    );
  }

  resetGlobals() {
    this.globals = {};
  }

  resetCollection(collectionPath: string) {
    delete this.collection[collectionPath];
  }

  resetEnvironment(envId: string) {
    delete this.environment[envId];
  }

  /** Remove one runtime value (the file value applies again). */
  resetKey(scope: "global" | "collection" | "environment", key: string, collectionPath: string, envId: string | null) {
    if (scope === "global") delete this.globals[key];
    else if (scope === "collection") delete this.collection[collectionPath]?.[key];
    else if (envId) delete this.environment[envId]?.[key];
  }

  resetAll() {
    this.globals = {};
    this.collection = {};
    this.environment = {};
  }
}

export const runtimeVars = new RuntimeVars();
