// Ready-made script snippets (like Postman's side list). Labels are i18n keys; the
// translated label is also used as the test name inside the inserted code.
import type { MessageKey } from "$lib/i18n/index.svelte";

export type ScriptKind = "pre" | "tests";

export interface Snippet {
  id: string;
  label: MessageKey;
  /** Which script tabs list it. */
  where: ScriptKind[];
  code: (label: string) => string;
}

const q = (s: string) => JSON.stringify(s);

export const SNIPPETS: Snippet[] = [
  {
    id: "status200",
    label: "snippet.status200",
    where: ["tests"],
    code: (l) => `pm.test(${q(l)}, function () {\n  pm.response.to.have.status(200);\n});`,
  },
  {
    id: "responseTime",
    label: "snippet.responseTime",
    where: ["tests"],
    code: (l) => `pm.test(${q(l)}, function () {\n  pm.expect(pm.response.responseTime).to.be.below(200);\n});`,
  },
  {
    id: "bodyContains",
    label: "snippet.bodyContains",
    where: ["tests"],
    code: (l) => `pm.test(${q(l)}, function () {\n  pm.expect(pm.response.text()).to.include("string_you_want_to_search");\n});`,
  },
  {
    id: "jsonValue",
    label: "snippet.jsonValue",
    where: ["tests"],
    code: (l) => `pm.test(${q(l)}, function () {\n  const jsonData = pm.response.json();\n  pm.expect(jsonData.value).to.eql(100);\n});`,
  },
  {
    id: "headerPresent",
    label: "snippet.headerPresent",
    where: ["tests"],
    code: (l) => `pm.test(${q(l)}, function () {\n  pm.response.to.have.header("Content-Type");\n});`,
  },
  {
    id: "setEnvFromResponse",
    label: "snippet.setEnvFromResponse",
    where: ["tests"],
    code: () => `const jsonData = pm.response.json();\npm.environment.set("token", jsonData.token);`,
  },
  {
    id: "setEnvVar",
    label: "snippet.setEnvVar",
    where: ["pre"],
    code: () => `pm.environment.set("variable_key", "variable_value");`,
  },
  {
    id: "setCollectionVar",
    label: "snippet.setCollectionVar",
    where: ["pre", "tests"],
    code: () => `pm.collectionVariables.set("variable_key", "variable_value");`,
  },
  {
    id: "getVariable",
    label: "snippet.getVariable",
    where: ["pre", "tests"],
    code: () => `const value = pm.variables.get("variable_key");`,
  },
  {
    id: "useIterationData",
    label: "snippet.useIterationData",
    where: ["pre", "tests"],
    code: () => `const value = pm.iterationData.get("column_name");`,
  },
  {
    id: "log",
    label: "snippet.log",
    where: ["pre", "tests"],
    code: () => `console.log(pm.variables.get("variable_key"));`,
  },
];
