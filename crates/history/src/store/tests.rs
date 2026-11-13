use super::*;
use crate::{HistoryContext, HistorySettings};

fn request(name: &str, url: &str) -> Request {
    Request {
        name: name.into(),
        method: HttpMethod::Get,
        url: url.into(),
        headers: Default::default(),
        query: Default::default(),
        body: None,
        scripts: Default::default(),
        settings: Default::default(),
        auth: Default::default(),
        extract: Default::default(),
    }
}

fn entry(name: &str, url: &str, ts: i64, body: &str) -> NewEntry {
    NewEntry {
        timestamp_ms: ts,
        name: name.into(),
        method: HttpMethod::Get,
        url: url.into(),
        status: Some(200),
        status_text: "OK".into(),
        headers: vec![("content-type".into(), "text/plain".into())],
        body: Some(body.into()),
        elapsed_ms: 12.5,
        size_bytes: body.len() as u64,
        error: None,
        request: request(name, "{{base}}/x"),
        resolved_request: Some(request(name, url)),
        tests: serde_json::json!([{"name": "ok", "passed": true, "skipped": false}]),
        logs: vec!["hello".into()],
        context: HistoryContext {
            project_path: Some("/p".into()),
            request_path: Some("/p/c/r.toml".into()),
            environment: Some("dev".into()),
        },
    }
}

fn mem() -> HistoryStore {
    HistoryStore::open_in_memory(HistorySettings::default()).unwrap()
}

/// Pseudo-random alphanumerics so deflate can't shrink it much.
fn noise(len: usize, seed: u64) -> String {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (0..len)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            char::from(
                b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
                    [(x >> 58) as usize % 62],
            )
        })
        .collect()
}

#[test]
fn insert_list_get_delete_clear() {
    let s = mem();
    let now = now_ms();
    let a = s
        .record(entry("A", "https://a.test/1", now, "body a"))
        .unwrap()
        .unwrap();
    let b = s
        .record(entry("B", "https://b.test/2", now, "body b"))
        .unwrap()
        .unwrap();

    let list = s.list(None, None, 50).unwrap();
    assert_eq!(list.iter().map(|e| e.id).collect::<Vec<_>>(), vec![b, a]);
    assert_eq!(list[0].environment.as_deref(), Some("dev"));

    let full = s.get(a).unwrap();
    assert_eq!(full.summary.name, "A");
    assert_eq!(full.request.url, "{{base}}/x");
    assert_eq!(full.resolved_request.unwrap().url, "https://a.test/1");
    let resp = full.response.unwrap();
    assert_eq!(resp.body, "body a");
    assert!(!resp.body_truncated);
    assert_eq!(resp.headers[0].0, "content-type");
    assert_eq!(full.tests[0]["name"], "ok");
    assert_eq!(full.logs, vec!["hello"]);

    s.delete(&[a]).unwrap();
    assert!(matches!(s.get(a), Err(crate::Error::NotFound(_))));
    assert_eq!(s.stats().unwrap().entries, 1);

    s.clear().unwrap();
    let st = s.stats().unwrap();
    assert_eq!(st.entries, 0);
    assert_eq!(st.oldest, None);
    let payloads: i64 = s
        .conn()
        .query_row("SELECT COUNT(*) FROM payloads", [], |r| r.get(0))
        .unwrap();
    assert_eq!(payloads, 0);
}

#[test]
fn json_shape_matches_api_ts() {
    let s = mem();
    let id = s
        .record(entry("A", "https://a.test", 1, "x"))
        .unwrap()
        .unwrap();
    let v = serde_json::to_value(s.get(id).unwrap()).unwrap();
    for key in [
        "id",
        "timestamp",
        "name",
        "method",
        "url",
        "status",
        "elapsed_ms",
        "size_bytes",
        "error",
        "project_path",
        "request_path",
        "environment",
        "request",
        "resolved_request",
        "response",
        "tests",
        "logs",
    ] {
        assert!(v.get(key).is_some(), "missing {key}");
    }
    assert_eq!(v["method"], "GET");
    assert_eq!(v["response"]["headers"][0][1], "text/plain");
    let settings = serde_json::to_value(HistorySettings::default()).unwrap();
    assert_eq!(
        settings,
        serde_json::json!({"enabled": true, "max_days": 30, "max_size_mb": 200,
                           "save_response_body": true, "max_body_kb": 1024})
    );
}

#[test]
fn network_error_has_no_response() {
    let s = mem();
    let mut e = entry("A", "https://down.test", 1, "");
    e.status = None;
    e.body = None;
    e.error = Some("connection refused".into());
    e.tests = serde_json::Value::Null;
    let id = s.record(e).unwrap().unwrap();
    let full = s.get(id).unwrap();
    assert!(full.response.is_none());
    assert_eq!(full.summary.error.as_deref(), Some("connection refused"));
    assert_eq!(full.tests, serde_json::json!([]));
}

#[test]
fn search_is_case_insensitive_and_escapes_wildcards() {
    let s = mem();
    s.record(entry("List Users", "https://api.test/users", 1, ""))
        .unwrap();
    s.record(entry("Orders", "https://api.test/orders?x=50%", 2, ""))
        .unwrap();
    s.record(entry("Other", "https://other.test/a_b", 3, ""))
        .unwrap();

    let names = |q: &str| {
        s.list(Some(q), None, 50)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names("USERS"), vec!["List Users"]);
    assert_eq!(names("api.TEST"), vec!["Orders", "List Users"]);
    assert_eq!(names("get").len(), 3); // method
    assert_eq!(names("50%"), vec!["Orders"]);
    assert_eq!(names("a_b"), vec!["Other"]);
    assert_eq!(names("%"), vec!["Orders"]);
    assert_eq!(names("   ").len(), 3); // blank = no filter
}

#[test]
fn keyset_pagination() {
    let s = mem();
    for i in 0..25 {
        s.record(entry(&format!("r{i}"), "https://x.test", i, ""))
            .unwrap();
    }
    let mut seen = Vec::new();
    let mut before = None;
    loop {
        let page = s.list(None, before, 10).unwrap();
        if page.is_empty() {
            break;
        }
        before = Some(page.last().unwrap().id);
        seen.extend(page.into_iter().map(|e| e.id));
    }
    assert_eq!(seen.len(), 25);
    assert!(seen.windows(2).all(|w| w[0] > w[1]));
}

#[test]
fn truncates_on_char_boundary() {
    let s = HistoryStore::open_in_memory(HistorySettings {
        max_body_kb: 1,
        ..Default::default()
    })
    .unwrap();
    // 'é' is 2 bytes; 1023 ASCII + 'é' straddles the 1024-byte limit.
    let body = format!("{}é{}", "a".repeat(1023), "b".repeat(100));
    let id = s.record(entry("A", "u", 1, &body)).unwrap().unwrap();
    let resp = s.get(id).unwrap().response.unwrap();
    assert!(resp.body_truncated);
    assert_eq!(resp.body, "a".repeat(1023));
}

#[test]
fn large_bodies_are_compressed_and_round_trip() {
    let s = mem();
    let body = "hello world ".repeat(10_000);
    let id = s.record(entry("A", "u", 1, &body)).unwrap().unwrap();
    let (len, compressed): (i64, bool) = s
        .conn()
        .query_row(
            "SELECT length(body), body_compressed FROM payloads WHERE entry_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(compressed);
    assert!((len as usize) < body.len() / 10);
    assert_eq!(s.get(id).unwrap().response.unwrap().body, body);
}

#[test]
fn save_response_body_false_drops_body() {
    let s = HistoryStore::open_in_memory(HistorySettings {
        save_response_body: false,
        ..Default::default()
    })
    .unwrap();
    let id = s.record(entry("A", "u", 1, "secret")).unwrap().unwrap();
    let resp = s.get(id).unwrap().response.unwrap();
    assert_eq!(resp.body, "");
    assert!(!resp.body_truncated);
    assert_eq!(resp.status, 200);
}

#[test]
fn disabled_records_nothing() {
    let s = HistoryStore::open_in_memory(HistorySettings {
        enabled: false,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(s.record(entry("A", "u", 1, "x")).unwrap(), None);
    assert_eq!(s.stats().unwrap().entries, 0);
}

#[test]
fn prune_by_days() {
    let s = mem(); // max_days = 30
    let now = 1_000 * DAY_MS;
    s.record(entry("old", "u", now - 40 * DAY_MS, "")).unwrap();
    s.record(entry("edge", "u", now - 29 * DAY_MS, "")).unwrap();
    s.record(entry("new", "u", now - DAY_MS, "")).unwrap();
    let st = s.prune_at(now).unwrap();
    assert_eq!(st.entries, 2);
    assert_eq!(st.oldest, Some(now - 29 * DAY_MS));

    // Unlimited keeps everything.
    s.set_settings(HistorySettings {
        max_days: None,
        ..Default::default()
    })
    .unwrap();
    s.record(entry("ancient", "u", 0, "")).unwrap();
    assert_eq!(s.prune_at(now).unwrap().entries, 3);
}

/// Database file plus its WAL.
fn disk_size(db: &std::path::Path) -> u64 {
    let wal = db.with_extension("sqlite3-wal");
    [db, wal.as_path()]
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

#[test]
fn prune_by_size_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("history.sqlite3");
    let cfg = dir.path().join("history.toml");
    let s = HistoryStore::open(&db, &cfg).unwrap();
    s.set_settings(HistorySettings {
        max_size_mb: None,
        ..Default::default()
    })
    .unwrap();

    let now = now_ms();
    let mut last = 0;
    for i in 0..40 {
        last = s
            .record(entry(
                &format!("r{i}"),
                "u",
                now + i,
                &noise(100_000, i as u64),
            ))
            .unwrap()
            .unwrap();
    }
    let before = s.stats().unwrap();
    assert_eq!(before.entries, 40);
    assert!(before.size_bytes > 2 * 1024 * 1024, "{before:?}");
    let file_before = disk_size(&db);

    let after = s
        .set_settings(HistorySettings {
            max_size_mb: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert!(after.size_bytes <= 1024 * 1024, "{after:?}");
    assert!(after.entries > 0 && after.entries < 40);
    // Newest survive, oldest go.
    assert_eq!(s.list(None, None, 1).unwrap()[0].id, last);
    assert!(after.oldest.unwrap() > now);
    // incremental_vacuum gave the space back.
    assert!(disk_size(&db) < file_before / 2);

    // Inserts keep honouring the limit.
    for i in 0..60 {
        s.record(entry(
            "more",
            "u",
            now + 100 + i,
            &noise(100_000, 1000 + i as u64),
        ))
        .unwrap();
    }
    let st = s.prune().unwrap();
    assert!(st.size_bytes <= 1024 * 1024, "{st:?}");
}

#[test]
fn settings_round_trip_and_persist() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("history.sqlite3");
    let cfg = dir.path().join("history.toml");
    let s = HistoryStore::open(&db, &cfg).unwrap();
    assert_eq!(s.settings(), HistorySettings::default());

    let custom = HistorySettings {
        enabled: false,
        max_days: None,
        max_size_mb: Some(50),
        save_response_body: false,
        max_body_kb: 64,
    };
    s.set_settings(custom.clone()).unwrap();
    drop(s);

    let s = HistoryStore::open(&db, &cfg).unwrap();
    assert_eq!(s.settings(), custom);
    // Zero limits normalise to "unlimited".
    s.set_settings(HistorySettings {
        max_days: Some(0),
        max_size_mb: Some(0),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(s.settings().max_days, None);
    assert_eq!(s.settings().max_size_mb, None);

    // Missing keys fall back to defaults; garbage falls back to defaults.
    std::fs::write(&cfg, "enabled = false\n").unwrap();
    let loaded = HistorySettings::load(&cfg).unwrap();
    assert!(!loaded.enabled);
    assert_eq!(loaded.max_days, Some(30));
    std::fs::write(&cfg, "not toml [[[").unwrap();
    let s = HistoryStore::open(&db, &cfg).unwrap();
    assert_eq!(s.settings(), HistorySettings::default());
}

#[test]
fn store_is_send_sync() {
    fn assert_send_sync<T: Send + Sync + Clone + 'static>() {}
    assert_send_sync::<HistoryStore>();
}
