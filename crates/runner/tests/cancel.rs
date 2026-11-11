//! A hung request can be aborted: racing `execute_request` against a cancellation
//! token (what the app's `send_request` / `cancel_request` do) returns promptly and
//! closes the connection, instead of waiting for a server that never answers.

use std::time::{Duration, Instant};

use ilixpatch_core::{HttpMethod, Request};
use ilixpatch_engine::Engine;
use ilixpatch_runner::{CancellationToken, ExecContext, VarStore, execute_request};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

#[tokio::test]
async fn cancelling_a_hung_request_returns_immediately_and_closes_the_connection() {
    // Server that accepts, reads the request, and then never responds.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        // Read until the client hangs up (read returns 0) — that proves the drop closed it.
        loop {
            match sock.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => continue,
            }
        }
        let _ = closed_tx.send(());
    });

    let request = Request {
        name: "hangs".into(),
        method: HttpMethod::Get,
        url: format!("http://{addr}/never"),
        headers: Default::default(),
        query: Default::default(),
        body: None,
        scripts: Default::default(),
        settings: Default::default(),
    };
    let engine = Engine::new();
    let mut store = VarStore::default();
    let ctx = ExecContext::default();
    let token = CancellationToken::new();

    let canceller = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        canceller.cancel();
    });

    let start = Instant::now();
    let cancelled = tokio::select! {
        _ = execute_request(&engine, &request, &mut store, &ctx) => false,
        _ = token.cancelled() => true,
    };
    assert!(
        cancelled,
        "the request should not complete: the server never answers"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "cancel must be prompt, took {:?}",
        start.elapsed()
    );

    // The in-flight future was dropped, so the client side of the socket must be closed.
    tokio::time::timeout(Duration::from_secs(2), closed_rx)
        .await
        .expect("connection was not closed after cancelling")
        .unwrap();
}
