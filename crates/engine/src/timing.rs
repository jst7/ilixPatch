//! D46 phase 2: timings of one exchange for the network console — DNS lookup,
//! connection setup (TCP + TLS) and whether a pooled connection was reused.
//!
//! reqwest does not report them, so the engine's clients use a resolver and a
//! connector layer that write into a slot set (task-local) around each send.
//! TCP and TLS cannot be told apart from outside reqwest's connector, so they
//! are measured together.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;

#[derive(Debug, Default, Clone, Copy)]
pub struct ConnTiming {
    /// DNS lookup (new connection to a host name).
    pub dns_ms: Option<f64>,
    /// Whole connector call: DNS + TCP + TLS (+ proxy CONNECT).
    pub connector_ms: Option<f64>,
}

type Slot = Arc<Mutex<ConnTiming>>;

tokio::task_local! {
    static SLOT: Slot;
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn update(slot: &Slot, f: impl FnOnce(&mut ConnTiming)) {
    f(&mut slot.lock().unwrap_or_else(|e| e.into_inner()));
}

/// Run `fut` (a send) collecting the connection timings it causes.
pub async fn timed<F: Future>(fut: F) -> (F::Output, ConnTiming) {
    let slot: Slot = Arc::default();
    let out = SLOT.scope(slot.clone(), fut).await;
    let t = *slot.lock().unwrap_or_else(|e| e.into_inner());
    (out, t)
}

/// System resolver (getaddrinfo, like reqwest's default) that records the
/// lookup time.
pub(crate) struct TimedResolver;

impl reqwest::dns::Resolve for TimedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        // Called while the connector future is polled inside the send's scope.
        let slot = SLOT.try_with(Clone::clone).ok();
        Box::pin(async move {
            let start = Instant::now();
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if let Some(s) = slot {
                update(&s, |t| t.dns_ms = Some(ms(start)));
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Connector layer: times the setup of each new connection.
#[derive(Clone, Copy)]
pub(crate) struct TimingLayer;

impl<S> tower::Layer<S> for TimingLayer {
    type Service = TimingService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        TimingService(inner)
    }
}

#[derive(Clone)]
pub(crate) struct TimingService<S>(S);

impl<S, R> tower::Service<R> for TimingService<S>
where
    S: tower::Service<R>,
    S::Future: Send + 'static,
    S::Response: 'static,
    S::Error: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        let fut = self.0.call(req);
        match SLOT.try_with(Clone::clone) {
            // Keep the slot visible to the resolver while the connection is made,
            // even if the pool finishes it in another task.
            Ok(slot) => Box::pin(async move {
                let start = Instant::now();
                let out = SLOT.scope(slot.clone(), fut).await;
                update(&slot, |t| t.connector_ms = Some(ms(start)));
                out
            }),
            Err(_) => Box::pin(fut),
        }
    }
}
