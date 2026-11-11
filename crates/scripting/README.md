# ilixpatch-scripting

A Postman/Newman-compatible JavaScript sandbox for pre-request and test scripts,
so existing Postman collections keep working unchanged.

- Engine: [QuickJS](https://github.com/quickjs-ng/quickjs) through `rquickjs`.
- Assertions: the real [chai](https://www.chaijs.com/) 4.5.0 (vendored in
  `vendor/chai.js`, MIT, see `vendor/LICENSE-chai`), plus Postman's response
  assertions as a chai plugin.
- The `pm` object is built by `src/prelude.js`. Rust sends the inputs as JSON
  and reads a JSON result back (`src/bridge.rs`).

## Runtime model

- Each thread keeps one QuickJS runtime with chai and the prelude already loaded,
  so a call only builds a fresh `pm` object. A trivial test script takes about
  0.1 ms in release builds and 0.6 ms in debug builds. Starting the engine on a
  thread takes about 16 ms, once.
- User code runs inside a function (`new Function`), so top-level `var`s are
  local. Globals that a script creates without declaring them are deleted after
  the run, so runs do not affect each other.
- Limits: 64 MiB heap, 512 KiB stack, 5 s wall clock (`run_script_with_timeout`
  sets a different timeout). The infinite-loop check cannot be caught by a
  `try/catch` in the script. A timeout reports `error = "script timed out"` and
  keeps the tests, logs and variable updates recorded before it. After a timeout
  or out-of-memory error the thread's engine is rebuilt.
- After the script body runs, `setTimeout` callbacks (all of them, ordered by
  delay, without actually waiting) and promise jobs run until nothing is left.

## Supported API

| Area | API |
|---|---|
| Tests | `pm.test(name, fn)` (sync, `function (done)` callback, or returns a promise / `async`), `pm.test.skip`, `pm.expect` (chai `expect`), `pm.expect.fail` |
| Response | `pm.response.code`, `.status`, `.responseTime`, `.responseSize`, `.headers.get/has/one/all/toObject/each` (case-insensitive), `.text()`, `.json()` (throws `JSONError`), `.reason()` |
| Response assertions (`pm.response.to…` and `pm.expect(pm.response).to…`) | `.have.status(code \| reason)`, `.have.header(name[, value])`, `.have.body([text \| RegExp \| object])`, `.have.jsonBody([path[, value]] \| object)`, `.be.ok/success/info/redirection/clientError/serverError/error/accepted/badRequest/unauthorized/forbidden/notFound/rateLimited/json/withBody`, all work with `.not` |
| Request | `pm.request.url` (`toString()`, `update()`, assignment, `getHost/getPath/getQueryString`, `query.add/upsert/remove/get`), `.method` (settable), `.headers.get/has/add/upsert/remove/toObject`, `.body` (`mode`, `raw`, `urlencoded`, `update()`, `toString()`) |
| Variables | `pm.environment`, `pm.collectionVariables`, `pm.globals`: `get/set/unset/has/clear/toObject/replaceIn` (`pm.environment.name` too). `pm.variables`: `get/set/unset/has/toObject/replaceIn` (lookup order: local > data > environment > collection > globals; `set` writes local). `pm.iterationData`: `get/has/toObject`. `replaceIn` also fills `{{$guid}}`, `{{$randomUUID}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`, `{{$randomInt}}` |
| Info and flow | `pm.info.eventName/iteration/iterationCount/requestName`, `pm.execution.skipRequest()` (pre-request only), `pm.execution.setNextRequest`, `pm.setNextRequest`, `postman.setNextRequest(name \| null)` |
| Console | `console.log/info/warn/error/debug` → `logs`. Every level except `log` gets a prefix such as `[warn] `, and objects are written as JSON |
| Legacy (Newman) | `tests["name"] = bool`, `responseBody`, `responseCode.code`, `responseTime`, `responseHeaders`, `environment`, `globals`, `data`, `iteration`, `request`, `postman.set/get/clearEnvironmentVariable`, `postman.set/get/clearGlobalVariable`, `postman.getResponseHeader` |
| Utilities | `JSON`, `atob`, `btoa`, `setTimeout/clearTimeout/setImmediate`, `require("chai")` |

Variable values are strings, as `VarUpdate` requires. Objects passed to `set`
are stored as JSON, `null` and `undefined` are stored as `""`, and everything
else goes through `String(v)`. Every `set` and `unset` produces a `VarUpdate`,
in call order, and later reads in the same script see the write.

In pre-request scripts, `ScriptOutput.request` holds the modified request, or
`None` if the script changed nothing. The script sees `url` and `query` as a
single string. If that string changes, the new `url` contains the whole string
and `query` is cleared. Changes made in test scripts are ignored.

## Known gaps

- `pm.sendRequest`: throws `"pm.sendRequest is not supported yet"`.
- `require()` only knows `chai`. Every other module (lodash, moment,
  crypto-js, cheerio, tv4/ajv, uuid, xml2js, …) throws `module 'x' is not available yet`.
  The legacy `_`, `CryptoJS`, `xml2Json` and `tv4` globals are missing.
- `pm.response.to.have.jsonSchema` throws "not supported yet". `pm.cookies`,
  `pm.vault`, `pm.visualizer` and `pm.response.to.have.responseTime` are missing.
- The `pm.request.auth` and `pm.request.body` modes `formdata`, `file` and
  `graphql` are not supported.
- Timers do not wait in real time: they run in delay order once the script
  body has finished.
- Error messages contain only the error name and message, with no line or
  column numbers.
- Only top-level globals are reset between runs. A script that changes shared
  objects, such as chai prototypes through `require("chai").use(...)`, affects
  later runs on the same thread.
