use std::fs;
use std::sync::Mutex;

use ilixpatch_core::storage::save_request;
use ilixpatch_core::{Example, HttpMethod, Request, project};

use super::*;

fn example(name: &str, status: u16, body: &str) -> Example {
    Example {
        name: name.into(),
        status,
        headers: Default::default(),
        body: body.into(),
        delay_ms: None,
    }
}

fn request(name: &str, method: HttpMethod, url: &str, examples: Vec<Example>) -> Request {
    Request {
        name: name.into(),
        method,
        url: url.into(),
        headers: Default::default(),
        query: Default::default(),
        body: None,
        scripts: Default::default(),
        settings: Default::default(),
        auth: Default::default(),
        extract: Vec::new(),
        examples,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serves_examples_with_cors_and_reloads() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("proj");
    fs::create_dir_all(&root).unwrap();
    project::init_project(&root).unwrap();
    let col = project::create_collection(&root, "Shop").unwrap();
    let mut created = example("Created", 201, r#"{"id": 9}"#);
    created.headers.insert("X-Custom".into(), "yes".into());
    save_request(
        col.join("get-user.toml"),
        &request(
            "Get user",
            HttpMethod::Get,
            "{{base_url}}/users/:id",
            vec![
                example("Missing", 404, r#"{"error":"nope"}"#),
                example("OK", 200, r#"{"id": 1, "name": "Ann"}"#),
            ],
        ),
    )
    .unwrap();
    save_request(
        col.join("me.toml"),
        &request(
            "Me",
            HttpMethod::Get,
            "{{base_url}}/users/me",
            vec![example("Me", 200, "me")],
        ),
    )
    .unwrap();
    save_request(
        col.join("create.toml"),
        &request(
            "Create",
            HttpMethod::Post,
            "https://api.example.com/users",
            vec![created],
        ),
    )
    .unwrap();
    save_request(
        col.join("no-examples.toml"),
        &request("Plain", HttpMethod::Get, "{{base_url}}/plain", vec![]),
    )
    .unwrap();

    let log: Arc<Mutex<Vec<MockLogEntry>>> = Arc::default();
    let sink = log.clone();
    let server = start(
        Source::new(&root),
        "127.0.0.1:0".parse().unwrap(),
        MockOptions::default(),
        Some(Arc::new(move |e| sink.lock().unwrap().push(e))),
    )
    .await
    .unwrap();
    assert_eq!(
        server.routes.len(),
        3,
        "requests without examples are not routes"
    );
    let base = server.url();
    let http = reqwest::Client::new();

    let r = http.get(format!("{base}/users/42")).send().await.unwrap();
    assert_eq!(r.status(), 200, "first 2xx example by default");
    assert_eq!(r.headers()["access-control-allow-origin"], "*");
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert_eq!(r.text().await.unwrap(), r#"{"id": 1, "name": "Ann"}"#);

    let r = http
        .get(format!("{base}/users/42"))
        .header("X-Mock-Example", "404")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    let r = http
        .get(format!("{base}/users/42?__example=Missing"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);

    let r = http.get(format!("{base}/users/me")).send().await.unwrap();
    assert_eq!(r.text().await.unwrap(), "me", "literal segments win");

    let r = http
        .post(format!("{base}/users"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    assert_eq!(r.headers()["x-custom"], "yes");

    let r = http.get(format!("{base}/nowhere")).send().await.unwrap();
    assert_eq!(r.status(), 404);
    let body: serde_json::Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
    assert_eq!(body["error"], "no mock for GET /nowhere");
    assert!(
        body["routes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "GET /users/:id")
    );

    let r = http
        .request(reqwest::Method::OPTIONS, format!("{base}/users"))
        .header("Origin", "http://localhost:5173")
        .header("Access-Control-Request-Method", "POST")
        .header(
            "Access-Control-Request-Headers",
            "content-type,authorization",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    assert_eq!(r.headers()["access-control-allow-methods"], "POST");
    assert_eq!(
        r.headers()["access-control-allow-headers"],
        "content-type,authorization"
    );

    // Edits are picked up without restarting.
    save_request(
        col.join("me.toml"),
        &request(
            "Me",
            HttpMethod::Get,
            "{{base_url}}/users/me",
            vec![example("Me", 200, "me v2")],
        ),
    )
    .unwrap();
    tokio::time::sleep(RELOAD_EVERY + Duration::from_millis(100)).await;
    let r = http.get(format!("{base}/users/me")).send().await.unwrap();
    assert_eq!(r.text().await.unwrap(), "me v2");

    {
        let log = log.lock().unwrap();
        assert!(log.iter().any(|e| e.path == "/users/42"
            && e.status == 200
            && e.route.as_deref() == Some("Get user")
            && e.example.as_deref() == Some("OK")));
        assert!(
            log.iter()
                .any(|e| e.path == "/nowhere" && e.status == 404 && e.route.is_none())
        );
    }

    server.stop();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        http.get(format!("{base}/users/me")).send().await.is_err(),
        "stopped"
    );
}

#[tokio::test]
async fn delay_and_unknown_source() {
    let t = tempfile::tempdir().unwrap();
    assert!(
        start(
            Source::new(t.path().join("missing")),
            "127.0.0.1:0".parse().unwrap(),
            MockOptions::default(),
            None
        )
        .await
        .is_err()
    );

    let root = t.path().join("p");
    fs::create_dir_all(&root).unwrap();
    project::init_project(&root).unwrap();
    let col = project::create_collection(&root, "C").unwrap();
    let mut slow = example("Slow", 200, "ok");
    slow.delay_ms = Some(300);
    save_request(
        col.join("slow.toml"),
        &request("Slow", HttpMethod::Get, "/slow", vec![slow]),
    )
    .unwrap();
    let server = start(
        Source::new(&col),
        "127.0.0.1:0".parse().unwrap(),
        MockOptions { delay_ms: 0 },
        None,
    )
    .await
    .unwrap();
    let started = Instant::now();
    let body = reqwest::get(format!("{}/slow", server.url()))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "ok");
    assert!(started.elapsed() >= Duration::from_millis(300));
}
