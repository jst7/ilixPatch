//! Mock server (D62): answers HTTP requests with the saved example responses
//! (`[[examples]]`) of a project's or collection's requests, so a frontend can
//! be developed before the backend exists.
//!
//! * A request matches a route by method and path (`{{base_url}}/users/:id`
//!   answers `GET /users/42`); `:id`, `{id}` and `{{id}}` segments match any
//!   value and the most specific route wins.
//! * The example is the first 2xx one, or the one named (or with the status)
//!   in the `X-Mock-Example` header or the `__example` query parameter.
//! * CORS is open (any origin, preflights answered), so a dev server on
//!   another port can call it from the browser.
//! * The collections are re-read at most once per second: edits show up at once.

mod routes;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::header::{HeaderName, HeaderValue};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

pub use routes::{Route, RouteInfo, Source, find, pick};

const RELOAD_EVERY: Duration = Duration::from_secs(1);
const EXAMPLE_HEADER: &str = "x-mock-example";
const EXAMPLE_QUERY: &str = "__example";

#[derive(Debug, Clone, Copy, Default)]
pub struct MockOptions {
    /// Latency added to every response (an example's own `delay_ms` wins).
    pub delay_ms: u64,
}

/// One served request (`MockLogEntry` in api.ts).
#[derive(Debug, Clone, Serialize)]
pub struct MockLogEntry {
    /// Unix milliseconds.
    pub time: u64,
    pub method: String,
    pub path: String,
    pub status: u16,
    /// The matched request (`collection/folder/Request`), if any.
    pub route: Option<String>,
    pub example: Option<String>,
    pub elapsed_ms: f64,
}

pub type OnRequest = Arc<dyn Fn(MockLogEntry) + Send + Sync>;

/// When the routes were last read, and the result.
type RouteCache = (Option<Instant>, Result<Arc<Vec<Route>>, String>);

struct Routes {
    source: Source,
    cache: Mutex<RouteCache>,
}

impl Routes {
    fn get(&self) -> Result<Arc<Vec<Route>>, String> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.0.is_none_or(|t| t.elapsed() >= RELOAD_EVERY) {
            let fresh = self.source.load().map(Arc::new);
            // A broken file while editing keeps the last good routes.
            cache.1 = match (fresh, &cache.1) {
                (Ok(r), _) => Ok(r),
                (Err(_), Ok(old)) => Ok(old.clone()),
                (Err(e), Err(_)) => Err(e),
            };
            cache.0 = Some(Instant::now());
        }
        cache.1.clone()
    }
}

/// A running mock server; stops when [`MockServer::stop`] is called or it is dropped.
pub struct MockServer {
    pub addr: SocketAddr,
    pub routes: Vec<RouteInfo>,
    cancel: CancellationToken,
}

impl MockServer {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn stop(&self) {
        self.cancel.cancel();
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Start serving `source` on `addr` (port 0 = any free port).
pub async fn start(
    source: Source,
    addr: SocketAddr,
    opts: MockOptions,
    on_request: Option<OnRequest>,
) -> Result<MockServer, String> {
    let initial = source.load()?;
    let infos = initial.iter().map(Route::info).collect();
    let routes = Arc::new(Routes {
        source,
        cache: Mutex::new((Some(Instant::now()), Ok(Arc::new(initial)))),
    });
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("cannot listen on {addr}: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        loop {
            let (sock, _) = tokio::select! {
                _ = stop.cancelled() => return,
                r = listener.accept() => match r {
                    Ok(c) => c,
                    Err(_) => continue,
                },
            };
            let routes = routes.clone();
            let on_request = on_request.clone();
            let stop = stop.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |req| {
                    let routes = routes.clone();
                    let on_request = on_request.clone();
                    async move {
                        Ok::<_, Infallible>(handle(&routes, opts, req, on_request.as_ref()).await)
                    }
                });
                let conn = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(sock), svc);
                tokio::select! {
                    _ = conn => {}
                    _ = stop.cancelled() => {}
                }
            });
        }
    });
    Ok(MockServer {
        addr,
        routes: infos,
        cancel,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn cors(resp: &mut Response<Full<Bytes>>) {
    let h = resp.headers_mut();
    h.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    h.insert(
        "access-control-expose-headers",
        HeaderValue::from_static("*"),
    );
}

fn json_response(status: StatusCode, value: serde_json::Value) -> Response<Full<Bytes>> {
    let mut resp = Response::new(Full::new(Bytes::from(value.to_string())));
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp
}

async fn handle(
    routes: &Routes,
    opts: MockOptions,
    req: Request<Incoming>,
    on_request: Option<&OnRequest>,
) -> Response<Full<Bytes>> {
    let started = Instant::now();
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
    let query_example = req.uri().query().and_then(|q| {
        q.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k == EXAMPLE_QUERY).then(|| v.replace('+', " ").replace("%20", " "))
        })
    });
    let header_example = req
        .headers()
        .get(EXAMPLE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut route_name = None;
    let mut example_name = None;

    let mut resp = if method == "OPTIONS"
        && req.headers().contains_key("access-control-request-method")
    {
        // CORS preflight: allow what is asked.
        let mut r = Response::new(Full::new(Bytes::new()));
        *r.status_mut() = StatusCode::NO_CONTENT;
        let h = r.headers_mut();
        let asked = |name: &str| req.headers().get(name).cloned();
        if let Some(m) = asked("access-control-request-method") {
            h.insert("access-control-allow-methods", m);
        }
        if let Some(hd) = asked("access-control-request-headers") {
            h.insert("access-control-allow-headers", hd);
        }
        h.insert("access-control-max-age", HeaderValue::from_static("600"));
        r
    } else {
        match routes.get() {
            Err(e) => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": format!("the mock server could not read the collections: {e}") }),
            ),
            Ok(all) => match find(&all, &method, &path) {
                None => json_response(
                    StatusCode::NOT_FOUND,
                    serde_json::json!({
                        "error": format!("no mock for {method} {path}"),
                        "hint": "Add an example to a request with this method and path (Examples tab, or \"Save as example\" on a response).",
                        "routes": all.iter().take(100).map(|r| format!("{} {}", r.method.as_str(), r.pattern)).collect::<Vec<_>>(),
                    }),
                ),
                Some(route) => {
                    let ex = pick(
                        route,
                        header_example.as_deref().or(query_example.as_deref()),
                    );
                    route_name = Some(route.name.clone());
                    example_name = Some(ex.name.clone());
                    let delay = ex.delay_ms.unwrap_or(opts.delay_ms);
                    if delay > 0 {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                    let mut r = Response::new(Full::new(Bytes::from(if method == "HEAD" {
                        String::new()
                    } else {
                        ex.body.clone()
                    })));
                    *r.status_mut() = StatusCode::from_u16(ex.status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    let h = r.headers_mut();
                    for (k, v) in &ex.headers {
                        let lk = k.to_ascii_lowercase();
                        if lk == "content-length" || lk == "transfer-encoding" || lk == "connection"
                        {
                            continue;
                        }
                        if let (Ok(name), Ok(value)) = (
                            HeaderName::from_bytes(k.as_bytes()),
                            HeaderValue::from_str(v),
                        ) {
                            h.append(name, value);
                        }
                    }
                    if !h.contains_key("content-type") && !ex.body.is_empty() {
                        let json = serde_json::from_str::<serde_json::Value>(&ex.body).is_ok();
                        h.insert(
                            "content-type",
                            HeaderValue::from_static(if json {
                                "application/json; charset=utf-8"
                            } else {
                                "text/plain; charset=utf-8"
                            }),
                        );
                    }
                    if let Ok(v) = HeaderValue::from_str(&route.name) {
                        h.insert("x-mock-route", v);
                    }
                    r
                }
            },
        }
    };
    cors(&mut resp);
    if let Some(cb) = on_request {
        cb(MockLogEntry {
            time: now_ms(),
            method,
            path,
            status: resp.status().as_u16(),
            route: route_name,
            example: example_name,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        });
    }
    resp
}

#[cfg(test)]
mod tests;
