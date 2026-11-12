//! D41 end to end: large bodies are streamed to a temp file, cut at
//! `max_body_mb`, discarded on request, and a cancelled download closes the
//! connection and leaves no temp file behind.

use std::time::Duration;

use ilixpatch_core::HttpMethod;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::*;

const MB: usize = 1024 * 1024;

fn req(url: &str) -> Request {
    Request {
        name: "t".into(),
        method: HttpMethod::Get,
        url: url.into(),
        headers: Default::default(),
        query: Default::default(),
        body: None,
        scripts: Default::default(),
        settings: Default::default(),
        auth: Default::default(),
    }
}

/// Serves `total` bytes of JSON-ish lines in 64 KB writes. If `stall_after`
/// is set, stops writing after that many bytes and reports through `closed`
/// when the client hangs up.
async fn big_server(
    total: usize,
    stall_after: Option<usize>,
    closed: Option<oneshot::Sender<()>>,
) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut s, _) = l.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = s.read(&mut buf).await;
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {total}\r\n\r\n"
        );
        s.write_all(head.as_bytes()).await.unwrap();
        let line = b"{\"id\": 123456, \"name\": \"abcdefghijklmnopqrstuvwxyz\"}\n";
        let chunk: Vec<u8> = line.iter().copied().cycle().take(64 * 1024).collect();
        let limit = stall_after.unwrap_or(total);
        let mut sent = 0;
        while sent < limit {
            let n = chunk.len().min(limit - sent);
            if s.write_all(&chunk[..n]).await.is_err() {
                return;
            }
            sent += n;
        }
        if let Some(tx) = closed {
            // Client side closes ⇒ read returns 0 (or an error).
            let _ = tokio::time::timeout(Duration::from_secs(10), s.read(&mut buf)).await;
            let _ = tx.send(());
        }
    });
    format!("http://{addr}/big")
}

fn engine(dir: &std::path::Path, inline_kb: u64, max_mb: u64) -> Engine {
    Engine::with_settings(NetworkSettings {
        inline_body_kb: inline_kb,
        max_body_mb: max_mb,
        ..Default::default()
    })
    .unwrap()
    .with_spill_dir(dir)
}

fn temp_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |d| d.count())
}

#[tokio::test]
async fn fifty_mb_body_is_spilled_to_a_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let url = big_server(50 * MB, None, None).await;
    let r = engine(dir.path(), 1024, 1024)
        .send(&req(&url))
        .await
        .unwrap();
    assert!(r.body_complete);
    assert_eq!(r.size_bytes, 50 * MB);
    assert_eq!(r.body.len(), (50 * MB) as u64);
    assert!(r.body.is_file(), "a 50 MB body must not stay in memory");
    let path = r.body.path().unwrap().to_path_buf();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), (50 * MB) as u64);
    // Partial reads come from the file.
    let head = r.body.read_range(0, 7).unwrap();
    assert_eq!(&head, b"{\"id\": ");
    drop(r);
    assert!(
        !path.exists(),
        "the temp file is deleted with the last clone"
    );
}

#[tokio::test]
async fn download_stops_at_max_body_mb() {
    let dir = tempfile::tempdir().unwrap();
    let url = big_server(20 * MB, None, None).await;
    let r = engine(dir.path(), 64, 5).send(&req(&url)).await.unwrap();
    assert!(!r.body_complete);
    assert_eq!(r.body.len(), (5 * MB) as u64);
}

#[tokio::test]
async fn discarded_body_is_counted_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let url = big_server(10 * MB, None, None).await;
    let opts = SendOptions {
        discard_body: true,
        ..Default::default()
    };
    let r = engine(dir.path(), 64, 1024)
        .send_opts(&req(&url), opts, None)
        .await
        .unwrap();
    assert_eq!(r.size_bytes, 10 * MB);
    assert!(r.body.is_empty());
    assert_eq!(temp_files(dir.path()), 0);
}

#[tokio::test]
async fn cancel_mid_download_closes_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = oneshot::channel();
    // Announces 50 MB, sends 3 MB, then stalls.
    let url = big_server(50 * MB, Some(3 * MB), Some(tx)).await;
    let e = engine(dir.path(), 64, 1024);
    let r = req(&url);
    let fut = e.send(&r);
    // Dropping the future = cancel (as `tokio::select!` does in the app).
    let res = tokio::time::timeout(Duration::from_millis(800), fut).await;
    assert!(
        res.is_err(),
        "the send should still be waiting for the body"
    );
    assert_eq!(
        temp_files(dir.path()),
        0,
        "partial temp file must be deleted"
    );
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("the server should see the connection closed")
        .unwrap();
}
