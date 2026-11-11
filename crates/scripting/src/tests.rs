use std::time::{Duration, Instant};

use ilixpatch_core::{Body, HttpMethod, Request, Scripts};
use indexmap::IndexMap;

use super::*;

fn response(status: u16, body: &str) -> ResponseView {
    ResponseView {
        status,
        status_text: match status {
            200 => "OK",
            201 => "Created",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
        .into(),
        headers: vec![
            (
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ),
            ("X-Request-Id".into(), "abc123".into()),
        ],
        body: body.into(),
        elapsed_ms: 42.5,
        size_bytes: body.len() + 60,
    }
}

fn request() -> Request {
    let mut headers = IndexMap::new();
    headers.insert("Accept".to_string(), "application/json".to_string());
    Request {
        name: "Get user".into(),
        method: HttpMethod::Get,
        url: "https://api.example.com/users/{{id}}".into(),
        headers,
        query: IndexMap::new(),
        body: None,
        scripts: Scripts::default(),
        settings: Default::default(),
    }
}

fn test_script(source: &str, resp: &ResponseView) -> ScriptOutput {
    let input = ScriptInput {
        response: Some(resp),
        ..Default::default()
    };
    run_script(ScriptKind::Test, source, &input)
}

fn ok_json() -> ResponseView {
    response(
        200,
        r#"{"id":7,"name":"Ada","tags":["a","b"],"nested":{"items":[{"v":1},{"v":2}]}}"#,
    )
}

/// Assert there is no script error and every test passed; returns the output.
#[track_caller]
fn all_pass(out: ScriptOutput) -> ScriptOutput {
    assert_eq!(out.error, None, "{out:#?}");
    assert!(!out.tests.is_empty(), "no tests recorded: {out:#?}");
    for t in &out.tests {
        assert!(t.passed, "test {:?} failed: {:?}", t.name, t.error);
    }
    out
}

// ------------------------------------------------------------------ pm.test

#[test]
fn passing_and_failing_tests() {
    let out = test_script(
        r#"
        pm.test("passes", function () { pm.expect(1 + 1).to.equal(2); });
        pm.test("fails", function () { pm.expect(1).to.equal(2); });
        pm.test("throws", function () { null.x; });
        pm.test.skip("skipped", function () { throw new Error("never"); });
        "#,
        &ok_json(),
    );
    assert_eq!(out.error, None);
    assert_eq!(out.tests.len(), 4);
    assert!(out.tests[0].passed);
    assert_eq!(out.tests[0].error, None);
    assert!(!out.tests[1].passed);
    assert_eq!(
        out.tests[1].error.as_deref(),
        Some("AssertionError: expected 1 to equal 2")
    );
    assert!(!out.tests[2].passed);
    assert!(
        out.tests[2]
            .error
            .as_deref()
            .unwrap()
            .starts_with("TypeError"),
        "{:?}",
        out.tests[2]
    );
    assert!(out.tests[3].skipped);
    assert!(out.tests[3].passed);
}

#[test]
fn chai_bdd_api() {
    all_pass(test_script(
        r#"
        pm.test("chai", function () {
            pm.expect(5).to.be.above(3).and.below(10);
            pm.expect("hello world").to.include("world");
            pm.expect([1, 2, 3]).to.have.lengthOf(3);
            pm.expect([1, 2, 3]).to.include(2);
            pm.expect({ a: 1, b: { c: 2 } }).to.eql({ a: 1, b: { c: 2 } });
            pm.expect({ a: 1 }).to.have.property("a", 1);
            pm.expect("x").to.be.a("string");
            pm.expect([]).to.be.an("array").that.is.empty;
            pm.expect(2).to.be.oneOf([1, 2, 3]);
            pm.expect("abc123").to.match(/\d+$/);
            pm.expect(null).to.be.null;
            pm.expect(undefined).to.be.undefined;
            pm.expect(true).to.be.true;
            pm.expect(1).to.not.equal(2);
            pm.expect({ a: 1, b: 2 }).to.have.all.keys("a", "b");
            pm.expect(function () { throw new Error("boom"); }).to.throw("boom");
            pm.expect(1).to.be.ok;
            pm.expect(0).to.not.be.ok;
        });
        pm.test("require chai", function () {
            var chai = require("chai");
            chai.expect(1).to.equal(1);
            chai.assert.strictEqual(2, 2);
        });
        "#,
        &ok_json(),
    ));
}

#[test]
fn chai_failure_messages() {
    let out = test_script(
        r#"
        pm.test("eql", function () { pm.expect({a: 1}).to.eql({a: 2}); });
        pm.test("prop", function () { pm.expect({a: 1}).to.have.property("b"); });
        pm.test("custom", function () { pm.expect(1, "my message").to.equal(2); });
        "#,
        &ok_json(),
    );
    assert_eq!(
        out.tests[0].error.as_deref(),
        Some("AssertionError: expected { a: 1 } to deeply equal { a: 2 }")
    );
    assert_eq!(
        out.tests[1].error.as_deref(),
        Some("AssertionError: expected { a: 1 } to have property 'b'")
    );
    assert_eq!(
        out.tests[2].error.as_deref(),
        Some("AssertionError: my message: expected 1 to equal 2")
    );
}

// --------------------------------------------------------------- pm.response

#[test]
fn response_fields() {
    all_pass(test_script(
        r#"
        pm.test("fields", function () {
            pm.expect(pm.response.code).to.equal(200);
            pm.expect(pm.response.status).to.equal("OK");
            pm.expect(pm.response.responseTime).to.equal(42.5);
            pm.expect(pm.response.responseSize).to.be.above(60);
            pm.expect(pm.response.text()).to.include("Ada");
            var json = pm.response.json();
            pm.expect(json.name).to.equal("Ada");
            pm.expect(json.tags).to.eql(["a", "b"]);
        });
        "#,
        &ok_json(),
    ));
}

#[test]
fn response_headers_case_insensitive() {
    all_pass(test_script(
        r#"
        pm.test("headers", function () {
            pm.expect(pm.response.headers.get("content-type")).to.include("application/json");
            pm.expect(pm.response.headers.get("CONTENT-TYPE")).to.include("application/json");
            pm.expect(pm.response.headers.has("x-request-id")).to.be.true;
            pm.expect(pm.response.headers.has("X-Request-Id", "abc123")).to.be.true;
            pm.expect(pm.response.headers.has("x-missing")).to.be.false;
            pm.expect(pm.response.headers.get("x-missing")).to.be.undefined;
            pm.response.to.have.header("x-request-id");
            pm.response.to.have.header("X-REQUEST-ID", "abc123");
            pm.response.to.not.have.header("X-Nope");
        });
        "#,
        &ok_json(),
    ));
}

#[test]
fn response_assertion_helpers() {
    all_pass(test_script(
        r#"
        pm.test("helpers", function () {
            pm.response.to.have.status(200);
            pm.response.to.have.status("OK");
            pm.response.to.be.ok;
            pm.response.to.be.success;
            pm.response.to.be.json;
            pm.response.to.be.withBody;
            pm.response.to.not.be.error;
            pm.response.to.not.be.clientError;
            pm.response.to.have.jsonBody();
            pm.response.to.have.jsonBody("name");
            pm.response.to.have.jsonBody("name", "Ada");
            pm.response.to.have.jsonBody("nested.items[1].v", 2);
            pm.response.to.not.have.jsonBody("missing");
            pm.response.to.have.body(/Ada/);
            pm.response.to.not.have.status(404);
            pm.expect(pm.response).to.have.status(200);
            pm.expect(pm.response).to.be.ok;
            pm.expect(pm.response).to.have.header("Content-Type");
        });
        "#,
        &ok_json(),
    ));
}

#[test]
fn response_assertion_failures() {
    let out = test_script(
        r#"
        pm.test("status", function () { pm.response.to.have.status(200); });
        pm.test("ok", function () { pm.response.to.be.ok; });
        pm.test("error", function () { pm.response.to.not.be.error; });
        pm.test("header", function () { pm.response.to.have.header("X-Nope"); });
        pm.test("body", function () { pm.response.to.have.body("nope"); });
        pm.test("notFound", function () { pm.response.to.be.notFound; });
        "#,
        &response(404, "missing"),
    );
    assert_eq!(out.error, None);
    let errs: Vec<_> = out
        .tests
        .iter()
        .map(|t| (t.passed, t.error.clone().unwrap_or_default()))
        .collect();
    assert_eq!(
        errs[0],
        (
            false,
            "AssertionError: expected response to have status code 200 but got 404".into()
        )
    );
    assert!(!errs[1].0 && errs[1].1.contains("200"), "{errs:?}");
    assert!(
        !errs[2].0 && errs[2].1.contains("not be 4XX or 5XX"),
        "{errs:?}"
    );
    assert!(!errs[3].0 && errs[3].1.contains("X-Nope"), "{errs:?}");
    assert!(!errs[4].0, "{errs:?}");
    assert!(errs[5].0, "{errs:?}");
}

#[test]
fn response_text_body() {
    all_pass(test_script(
        r#"pm.test("body", function () { pm.response.to.have.body("plain text"); });"#,
        &response(200, "plain text"),
    ));
}

#[test]
fn json_parse_failure() {
    let out = test_script(
        r#"
        pm.test("json", function () { pm.response.json(); });
        pm.test("is json", function () { pm.response.to.be.json; });
        "#,
        &response(200, "<html>not json</html>"),
    );
    assert_eq!(out.error, None);
    assert!(!out.tests[0].passed);
    assert!(
        out.tests[0]
            .error
            .as_deref()
            .unwrap()
            .starts_with("JSONError: "),
        "{:?}",
        out.tests[0]
    );
    assert!(!out.tests[1].passed);

    // Outside pm.test it's an uncaught error.
    let out = test_script("var x = pm.response.json();", &response(200, "nope"));
    assert!(
        out.error.as_deref().unwrap().starts_with("JSONError"),
        "{out:?}"
    );
}

// ---------------------------------------------------------------- variables

fn map(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn variable_precedence_and_updates() {
    let globals = map(&[
        ("a", "global"),
        ("b", "global"),
        ("c", "global"),
        ("d", "global"),
        ("g", "g"),
    ]);
    let collection = map(&[("a", "coll"), ("b", "coll"), ("c", "coll")]);
    let env = map(&[("a", "env"), ("b", "env")]);
    let data: IndexMap<String, serde_json::Value> = [("a".to_string(), serde_json::json!("data"))]
        .into_iter()
        .collect();
    let input = ScriptInput {
        globals: Some(&globals),
        collection_variables: Some(&collection),
        environment: Some(&env),
        environment_name: Some("dev"),
        iteration_data: Some(&data),
        ..Default::default()
    };
    let out = run_script(
        ScriptKind::PreRequest,
        r#"
        pm.test("precedence", function () {
            pm.expect(pm.variables.get("a")).to.equal("data");
            pm.expect(pm.variables.get("b")).to.equal("env");
            pm.expect(pm.variables.get("c")).to.equal("coll");
            pm.expect(pm.variables.get("d")).to.equal("global");
            pm.expect(pm.variables.get("zzz")).to.be.undefined;
            pm.expect(pm.variables.has("g")).to.be.true;
            pm.expect(pm.environment.name).to.equal("dev");
            pm.expect(pm.iterationData.get("a")).to.equal("data");
            pm.expect(pm.iterationData.toObject()).to.eql({ a: "data" });
        });
        pm.environment.set("token", "t1");
        pm.collectionVariables.set("n", 5);
        pm.globals.set("obj", { x: 1 });
        pm.variables.set("a", "local");
        pm.environment.unset("b");
        pm.test("reads see writes", function () {
            pm.expect(pm.environment.get("token")).to.equal("t1");
            pm.expect(pm.environment.has("token")).to.be.true;
            pm.expect(pm.collectionVariables.get("n")).to.equal("5");
            pm.expect(pm.globals.get("obj")).to.equal('{"x":1}');
            pm.expect(pm.variables.get("a")).to.equal("local");
            pm.expect(pm.variables.get("b")).to.equal("coll");
            pm.expect(pm.environment.has("b")).to.be.false;
            pm.expect(pm.environment.toObject()).to.eql({ a: "env", token: "t1" });
            pm.expect(pm.variables.replaceIn("{{a}}-{{c}}-{{token}}-{{missing}}")).to.equal("local-coll-t1-{{missing}}");
            pm.expect(pm.environment.replaceIn("{{a}}")).to.equal("env");
            pm.expect(pm.variables.replaceIn("{{$guid}}")).to.match(/^[0-9a-f-]{36}$/);
        });
        "#,
        &input,
    );
    let out = all_pass(out);
    let s = |scope, k: &str, v: Option<&str>| VarUpdate {
        scope,
        key: k.into(),
        value: v.map(Into::into),
    };
    assert_eq!(
        out.variable_updates,
        vec![
            s(VarScope::Environment, "token", Some("t1")),
            s(VarScope::Collection, "n", Some("5")),
            s(VarScope::Global, "obj", Some(r#"{"x":1}"#)),
            s(VarScope::Local, "a", Some("local")),
            s(VarScope::Environment, "b", None),
        ]
    );
}

#[test]
fn variables_dont_leak_between_runs() {
    let out = run_script(
        ScriptKind::Test,
        "leaked = 1; var notLeaked = 2; pm.environment.set('x', '1');",
        &ScriptInput::default(),
    );
    assert_eq!(out.error, None);
    let out = run_script(
        ScriptKind::Test,
        r#"pm.test("clean", function () {
            pm.expect(typeof leaked).to.equal("undefined");
            pm.expect(pm.environment.get("x")).to.be.undefined;
        });"#,
        &ScriptInput::default(),
    );
    all_pass(out);
}

// ----------------------------------------------------------- request editing

#[test]
fn pre_request_modifies_request() {
    let req = request();
    let input = ScriptInput {
        request: Some(&req),
        ..Default::default()
    };
    let out = run_script(
        ScriptKind::PreRequest,
        r#"
        pm.test("read", function () {
            pm.expect(pm.request.url.toString()).to.equal("https://api.example.com/users/{{id}}");
            pm.expect(String(pm.request.url)).to.equal("https://api.example.com/users/{{id}}");
            pm.expect(pm.request.method).to.equal("GET");
            pm.expect(pm.request.headers.get("accept")).to.equal("application/json");
        });
        pm.request.headers.add({ key: "X-Trace", value: "1" });
        pm.request.headers.upsert({ key: "accept", value: "text/plain" });
        pm.request.headers.remove("X-Nothing");
        pm.request.url.update("https://api.example.com/v2/users/{{id}}");
        pm.request.method = "POST";
        pm.request.body.update('{"a":1}');
        "#,
        &input,
    );
    let out = all_pass(out);
    let new = out.request.expect("request changed");
    assert_eq!(new.url, "https://api.example.com/v2/users/{{id}}");
    assert_eq!(new.method, HttpMethod::Post);
    assert_eq!(
        new.headers,
        map(&[("Accept", "text/plain"), ("X-Trace", "1")])
    );
    assert_eq!(
        new.body,
        Some(Body::Json {
            content: r#"{"a":1}"#.into()
        })
    );
    assert_eq!(new.name, "Get user");
}

#[test]
fn pre_request_url_assignment_and_query() {
    let mut req = request();
    req.query.insert("page".into(), "1".into());
    let input = ScriptInput {
        request: Some(&req),
        ..Default::default()
    };

    let out = run_script(
        ScriptKind::PreRequest,
        r#"pm.request.url = "http://other/x";"#,
        &input,
    );
    assert_eq!(out.error, None);
    let new = out.request.unwrap();
    assert_eq!(new.url, "http://other/x");
    assert!(new.query.is_empty());

    let out = run_script(
        ScriptKind::PreRequest,
        r#"
        pm.expect(pm.request.url.toString()).to.equal("https://api.example.com/users/{{id}}?page=1");
        pm.request.url.query.add({ key: "limit", value: "10" });
        "#,
        &input,
    );
    assert_eq!(out.error, None, "{out:?}");
    assert_eq!(
        out.request.unwrap().url,
        "https://api.example.com/users/{{id}}?page=1&limit=10"
    );
}

#[test]
fn unchanged_request_is_none() {
    let req = request();
    let input = ScriptInput {
        request: Some(&req),
        ..Default::default()
    };
    let out = run_script(
        ScriptKind::PreRequest,
        "var u = pm.request.url.toString(); pm.request.headers.get('a');",
        &input,
    );
    assert_eq!(out.error, None);
    assert_eq!(out.request, None);
    // Changes in test scripts are never reported.
    let out = run_script(
        ScriptKind::Test,
        "pm.request.headers.add({key:'A', value:'b'});",
        &input,
    );
    assert_eq!(out.request, None);
}

#[test]
fn form_body_editing() {
    let mut req = request();
    req.body = Some(Body::Form {
        fields: map(&[("a", "1")]),
    });
    let input = ScriptInput {
        request: Some(&req),
        ..Default::default()
    };
    let out = run_script(
        ScriptKind::PreRequest,
        r#"
        pm.expect(pm.request.body.mode).to.equal("urlencoded");
        pm.request.body.urlencoded.add({ key: "b", value: "2" });
        "#,
        &input,
    );
    assert_eq!(out.error, None, "{out:?}");
    assert_eq!(
        out.request.unwrap().body,
        Some(Body::Form {
            fields: map(&[("a", "1"), ("b", "2")])
        })
    );
}

// ------------------------------------------------------------- control flow

#[test]
fn set_next_request_and_skip() {
    let out = run_script(
        ScriptKind::Test,
        "postman.setNextRequest('Login');",
        &ScriptInput::default(),
    );
    assert_eq!(out.next_request, Some(Some("Login".into())));
    let out = run_script(
        ScriptKind::Test,
        "pm.setNextRequest(null);",
        &ScriptInput::default(),
    );
    assert_eq!(out.next_request, Some(None));
    let out = run_script(
        ScriptKind::Test,
        "pm.execution.setNextRequest('X');",
        &ScriptInput::default(),
    );
    assert_eq!(out.next_request, Some(Some("X".into())));
    let out = run_script(ScriptKind::Test, "1;", &ScriptInput::default());
    assert_eq!(out.next_request, None);

    let out = run_script(
        ScriptKind::PreRequest,
        "pm.execution.skipRequest();",
        &ScriptInput::default(),
    );
    assert!(out.skip_request);
    let out = run_script(
        ScriptKind::Test,
        "pm.execution.skipRequest();",
        &ScriptInput::default(),
    );
    assert!(!out.skip_request);
}

#[test]
fn info_object() {
    let input = ScriptInput {
        iteration: 2,
        iteration_count: 5,
        request_name: Some("Get user"),
        ..Default::default()
    };
    let out = run_script(
        ScriptKind::PreRequest,
        r#"pm.test("info", function () {
            pm.expect(pm.info.eventName).to.equal("prerequest");
            pm.expect(pm.info.iteration).to.equal(2);
            pm.expect(pm.info.iterationCount).to.equal(5);
            pm.expect(pm.info.requestName).to.equal("Get user");
        });"#,
        &input,
    );
    all_pass(out);
    let out = run_script(
        ScriptKind::Test,
        "pm.test('e', () => pm.expect(pm.info.eventName).to.equal('test'));",
        &input,
    );
    all_pass(out);
}

#[test]
fn console_logs() {
    let out = run_script(
        ScriptKind::Test,
        r#"
        console.log("hello", 1, true, { a: [1, 2] }, null, undefined);
        console.info("info");
        console.warn("careful");
        console.error(new Error("bad"));
        console.debug("dbg");
        "#,
        &ScriptInput::default(),
    );
    assert_eq!(out.error, None);
    assert_eq!(
        out.logs,
        vec![
            r#"hello 1 true {"a":[1,2]} null undefined"#,
            "[info] info",
            "[warn] careful",
            "[error] Error: bad",
            "[debug] dbg",
        ]
    );
}

// ------------------------------------------------------------------- errors

#[test]
fn syntax_error() {
    let out = run_script(
        ScriptKind::Test,
        "pm.test('x', function () {",
        &ScriptInput::default(),
    );
    assert!(
        out.error.as_deref().unwrap().starts_with("SyntaxError"),
        "{out:?}"
    );
    assert!(out.tests.is_empty());
}

#[test]
fn runtime_error_outside_test_keeps_earlier_results() {
    let out = run_script(
        ScriptKind::Test,
        r#"
        console.log("before");
        pm.environment.set("x", "1");
        pm.test("ok", function () {});
        undefinedFunction();
        pm.test("never", function () {});
        "#,
        &ScriptInput::default(),
    );
    assert_eq!(
        out.error.as_deref(),
        Some("ReferenceError: undefinedFunction is not defined"),
        "{out:?}"
    );
    assert_eq!(out.tests.len(), 1);
    assert_eq!(out.logs, vec!["before"]);
    assert_eq!(out.variable_updates.len(), 1);

    let out = run_script(
        ScriptKind::Test,
        "throw 'plain string';",
        &ScriptInput::default(),
    );
    assert_eq!(out.error.as_deref(), Some("plain string"));
}

#[test]
fn timeout_on_infinite_loop() {
    let start = Instant::now();
    let out = run_script_with_timeout(
        ScriptKind::Test,
        "console.log('start'); pm.test('t', function(){}); while (true) {}",
        &ScriptInput::default(),
        Duration::from_millis(200),
    );
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "took {:?}",
        start.elapsed()
    );
    assert_eq!(out.error.as_deref(), Some("script timed out"));
    assert_eq!(out.logs, vec!["start"]);
    // Infinite loop inside pm.test cannot be caught by the test wrapper either.
    let out = run_script_with_timeout(
        ScriptKind::Test,
        "pm.test('t', function(){ try { for(;;){} } catch (e) {} });",
        &ScriptInput::default(),
        Duration::from_millis(100),
    );
    assert_eq!(out.error.as_deref(), Some("script timed out"));
    // The engine recovers afterwards.
    all_pass(run_script(
        ScriptKind::Test,
        "pm.test('after', function(){});",
        &ScriptInput::default(),
    ));
}

#[test]
fn memory_limit() {
    let out = run_script(
        ScriptKind::Test,
        "var a = []; while (true) { a.push(new Array(100000).fill('xxxxxxxx')); }",
        &ScriptInput::default(),
    );
    assert!(out.error.is_some(), "{out:?}");
    all_pass(run_script(
        ScriptKind::Test,
        "pm.test('after', function(){});",
        &ScriptInput::default(),
    ));
}

#[test]
fn unsupported_features() {
    let out = run_script(
        ScriptKind::Test,
        r#"
        pm.test("sendRequest", function () { pm.sendRequest("http://x", function () {}); });
        pm.test("lodash", function () { require("lodash"); });
        "#,
        &ScriptInput::default(),
    );
    assert_eq!(
        out.tests[0].error.as_deref(),
        Some("Error: pm.sendRequest is not supported yet")
    );
    assert_eq!(
        out.tests[1].error.as_deref(),
        Some("Error: module 'lodash' is not available yet")
    );
}

// ------------------------------------------------------------------- legacy

#[test]
fn legacy_globals() {
    let env = map(&[("host", "example.com")]);
    let globals = map(&[("g", "1")]);
    let data: IndexMap<String, serde_json::Value> =
        [("user".to_string(), serde_json::json!("bob"))]
            .into_iter()
            .collect();
    let resp = ok_json();
    let input = ScriptInput {
        response: Some(&resp),
        environment: Some(&env),
        globals: Some(&globals),
        iteration_data: Some(&data),
        ..Default::default()
    };
    let out = run_script(
        ScriptKind::Test,
        r#"
        tests["status is 200"] = responseCode.code === 200;
        tests["body has Ada"] = responseBody.has ? true : responseBody.indexOf("Ada") >= 0;
        tests["time"] = responseTime === 42.5;
        tests["header"] = responseHeaders["Content-Type"].indexOf("json") >= 0;
        tests["env"] = environment.host === "example.com" && postman.getEnvironmentVariable("host") === "example.com";
        tests["globals"] = globals.g === "1";
        tests["data"] = data.user === "bob";
        tests["failing"] = false;
        postman.setEnvironmentVariable("a", "1");
        postman.setGlobalVariable("b", "2");
        postman.clearEnvironmentVariable("host");
        var body = JSON.parse(responseBody);
        tests["json"] = body.id === 7;
        "#,
        &input,
    );
    assert_eq!(out.error, None, "{out:?}");
    let names: Vec<_> = out
        .tests
        .iter()
        .map(|t| (t.name.as_str(), t.passed))
        .collect();
    assert_eq!(
        names,
        vec![
            ("status is 200", true),
            ("body has Ada", true),
            ("time", true),
            ("header", true),
            ("env", true),
            ("globals", true),
            ("data", true),
            ("failing", false),
            ("json", true),
        ]
    );
    assert_eq!(out.variable_updates.len(), 3);
    assert_eq!(out.variable_updates[1].scope, VarScope::Global);
    assert_eq!(out.variable_updates[2].value, None);
}

#[test]
fn base64_and_timers_and_async_tests() {
    all_pass(run_script(
        ScriptKind::Test,
        r#"
        pm.test("b64", function () {
            pm.expect(btoa("hello:world")).to.equal("aGVsbG86d29ybGQ=");
            pm.expect(atob("aGVsbG86d29ybGQ=")).to.equal("hello:world");
            pm.expect(btoa("ab")).to.equal("YWI=");
            pm.expect(atob(btoa("a"))).to.equal("a");
        });
        var later = false;
        setTimeout(function () { later = true; }, 10);
        pm.test("callback", function (done) {
            setTimeout(function () { pm.expect(later).to.be.true; done(); }, 20);
        });
        pm.test("promise", function () {
            return Promise.resolve(1).then(function (v) { pm.expect(v).to.equal(1); });
        });
        "#,
        &ScriptInput::default(),
    ));
    let out = run_script(
        ScriptKind::Test,
        r#"
        pm.test("never done", function (done) {});
        pm.test("rejects", async function () { throw new Error("nope"); });
        "#,
        &ScriptInput::default(),
    );
    assert!(!out.tests[0].passed);
    assert!(
        out.tests[0]
            .error
            .as_deref()
            .unwrap()
            .contains("did not complete")
    );
    assert_eq!(out.tests[1].error.as_deref(), Some("Error: nope"));
}

#[test]
fn runs_on_many_threads() {
    let handles: Vec<_> = (0..4)
        .map(|i| {
            std::thread::spawn(move || {
                let out = run_script(
                    ScriptKind::Test,
                    &format!("pm.test('t{i}', function () {{ pm.expect({i}).to.equal({i}); }});"),
                    &ScriptInput::default(),
                );
                out.tests[0].passed
            })
        })
        .collect();
    for h in handles {
        assert!(h.join().unwrap());
    }
}

/// `cargo test -p ilixpatch-scripting --release -- --ignored --nocapture bench`
#[test]
#[ignore]
fn bench_trivial_test_script() {
    let resp = ok_json();
    let req = request();
    let env = map(&[("a", "1"), ("b", "2")]);
    let input = ScriptInput {
        response: Some(&resp),
        request: Some(&req),
        environment: Some(&env),
        ..Default::default()
    };
    let src = r#"
        pm.test("status", function () { pm.response.to.have.status(200); });
        pm.test("body", function () { pm.expect(pm.response.json().name).to.equal("Ada"); });
        pm.environment.set("last", "x");
    "#;
    let cold = Instant::now();
    all_pass(run_script(ScriptKind::Test, src, &input));
    let cold = cold.elapsed();
    let n = 2000;
    let start = Instant::now();
    for _ in 0..n {
        let out = run_script(ScriptKind::Test, src, &input);
        assert!(out.tests[0].passed);
    }
    let per = start.elapsed() / n;
    println!("first call (engine init): {cold:?}; per call after warm-up: {per:?}");
}

#[test]
fn local_variables_input_has_highest_precedence() {
    let env = map(&[("a", "env"), ("b", "env")]);
    let local = map(&[("a", "local"), ("only", "l")]);
    let data: IndexMap<String, serde_json::Value> = [("a".to_string(), serde_json::json!("data"))]
        .into_iter()
        .collect();
    let input = ScriptInput {
        environment: Some(&env),
        iteration_data: Some(&data),
        local: Some(&local),
        ..Default::default()
    };
    let out = all_pass(run_script(
        ScriptKind::Test,
        r#"pm.test("local", function () {
            pm.expect(pm.variables.get("a")).to.equal("local");
            pm.expect(pm.variables.get("b")).to.equal("env");
            pm.expect(pm.variables.has("only")).to.be.true;
            pm.expect(pm.variables.toObject()).to.eql({ a: "local", b: "env", only: "l" });
            pm.expect(pm.variables.replaceIn("{{a}}/{{only}}")).to.equal("local/l");
            pm.variables.unset("only");
            pm.expect(pm.variables.has("only")).to.be.false;
        });"#,
        &input,
    ));
    assert_eq!(
        out.variable_updates,
        vec![VarUpdate {
            scope: VarScope::Local,
            key: "only".into(),
            value: None
        }]
    );
}
