//! Requests of a project / collection → mock routes, and request matching.

use std::path::{Path, PathBuf};

use ilixpatch_core::project::{is_project, load_project};
use ilixpatch_core::storage::load_collection;
use ilixpatch_core::{Collection, Example, Folder, HttpMethod};
use serde::Serialize;

/// One path segment of a route.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Literal(String),
    /// `:id`, `{id}` or `{{id}}`: any value.
    Param(String),
}

/// A request that has examples, as a route.
#[derive(Debug, Clone)]
pub struct Route {
    pub method: HttpMethod,
    segs: Vec<Seg>,
    /// `/users/:id` for display.
    pub pattern: String,
    /// `Collection/folder/Request`.
    pub name: String,
    pub examples: Vec<Example>,
}

/// `MockRoute` in api.ts: what the UI / CLI lists.
#[derive(Debug, Clone, Serialize)]
pub struct RouteInfo {
    pub method: String,
    pub pattern: String,
    pub name: String,
    pub examples: Vec<String>,
}

impl Route {
    pub fn info(&self) -> RouteInfo {
        RouteInfo {
            method: self.method.as_str().into(),
            pattern: self.pattern.clone(),
            name: self.name.clone(),
            examples: self
                .examples
                .iter()
                .map(|e| format!("{} {}", e.status, e.name))
                .collect(),
        }
    }
}

/// The path of a request URL, without scheme, host or a leading `{{base}}`
/// variable, and without the query: `{{base_url}}/users/:id?x=1` → `/users/:id`.
pub(crate) fn url_path(url: &str) -> String {
    let url = url.trim();
    let no_query = url.split(['?', '#']).next().unwrap_or("");
    let rest = if let Some((_, after)) = no_query.split_once("://") {
        after.find('/').map_or("", |i| &after[i..])
    } else if no_query.starts_with("{{") {
        // `{{base_url}}/users` → `/users` (the base is the host part).
        match no_query.find("}}") {
            Some(end) => &no_query[end + 2..],
            None => no_query,
        }
    } else {
        no_query
    };
    let trimmed = rest.trim_matches('/');
    format!("/{trimmed}")
}

fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

fn parse_seg(s: &str) -> Seg {
    let param = s
        .strip_prefix(':')
        .or_else(|| s.strip_prefix("{{").and_then(|x| x.strip_suffix("}}")))
        .or_else(|| s.strip_prefix('{').and_then(|x| x.strip_suffix('}')));
    match param {
        Some(name) if !name.is_empty() => Seg::Param(name.trim().to_string()),
        _ => Seg::Literal(s.to_string()),
    }
}

fn collect(folder: &Folder, prefix: &str, out: &mut Vec<Route>) {
    for r in &folder.requests {
        let req = &r.request;
        if req.examples.is_empty() {
            continue;
        }
        let path = url_path(&req.url);
        out.push(Route {
            method: req.method,
            segs: segments(&path).into_iter().map(parse_seg).collect(),
            pattern: path,
            name: format!("{prefix}{}", req.name),
            examples: req.examples.clone(),
        });
    }
    for f in &folder.folders {
        collect(f, &format!("{prefix}{}/", f.name), out);
    }
}

/// What the mock server serves: a project (every collection) or a collection.
#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
}

impl Source {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Load the routes (requests with examples), most specific first.
    pub fn load(&self) -> Result<Vec<Route>, String> {
        let collections: Vec<Collection> = if is_project(&self.path) {
            load_project(&self.path)
                .map_err(|e| e.to_string())?
                .collections
        } else {
            vec![load_collection(&self.path).map_err(|e| e.to_string())?]
        };
        let multi = collections.len() > 1;
        let mut routes = Vec::new();
        for c in &collections {
            let prefix = if multi {
                format!("{}/", c.name)
            } else {
                String::new()
            };
            collect(&c.root, &prefix, &mut routes);
        }
        // More literal segments first, so `/users/me` wins over `/users/:id`.
        routes.sort_by_key(|r| {
            std::cmp::Reverse(
                r.segs
                    .iter()
                    .filter(|s| matches!(s, Seg::Literal(_)))
                    .count(),
            )
        });
        Ok(routes)
    }
}

/// The route for `method` + `path`. HEAD falls back to GET routes.
pub fn find<'a>(routes: &'a [Route], method: &str, path: &str) -> Option<&'a Route> {
    let segs = segments(path);
    let matches = |r: &&Route| {
        r.segs.len() == segs.len()
            && r.segs.iter().zip(&segs).all(|(p, s)| match p {
                Seg::Literal(l) => l == s,
                Seg::Param(_) => true,
            })
    };
    let m = method.to_ascii_uppercase();
    routes
        .iter()
        .filter(matches)
        .find(|r| r.method.as_str() == m)
        .or_else(|| {
            (m == "HEAD")
                .then(|| {
                    routes
                        .iter()
                        .filter(matches)
                        .find(|r| r.method == HttpMethod::Get)
                })
                .flatten()
        })
}

/// The example to answer with: the one named (or with the status) in
/// `wanted`, else the first 2xx, else the first.
pub fn pick<'a>(route: &'a Route, wanted: Option<&str>) -> &'a Example {
    let wanted = wanted.map(str::trim).filter(|w| !w.is_empty());
    if let Some(w) = wanted
        && let Some(e) = route
            .examples
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(w) || e.status.to_string() == w)
    {
        return e;
    }
    route
        .examples
        .iter()
        .find(|e| (200..300).contains(&e.status))
        .unwrap_or(&route.examples[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_paths() {
        assert_eq!(url_path("{{base_url}}/users/:id?x=1"), "/users/:id");
        assert_eq!(
            url_path("https://api.example.com:8443/v1/items/"),
            "/v1/items"
        );
        assert_eq!(url_path("http://localhost:3000"), "/");
        assert_eq!(url_path("/health"), "/health");
        assert_eq!(url_path("{{host}}"), "/");
    }

    fn route(method: HttpMethod, path: &str) -> Route {
        Route {
            method,
            segs: segments(path).into_iter().map(parse_seg).collect(),
            pattern: path.into(),
            name: path.into(),
            examples: vec![Example {
                name: "ok".into(),
                status: 200,
                headers: Default::default(),
                body: String::new(),
                delay_ms: None,
            }],
        }
    }

    #[test]
    fn matching_prefers_literals_and_methods() {
        let mut routes = vec![
            route(HttpMethod::Get, "/users/:id"),
            route(HttpMethod::Get, "/users/me"),
            route(HttpMethod::Post, "/users"),
            route(HttpMethod::Get, "/orders/{orderId}/items/{{item}}"),
        ];
        routes.sort_by_key(|r| {
            std::cmp::Reverse(
                r.segs
                    .iter()
                    .filter(|s| matches!(s, Seg::Literal(_)))
                    .count(),
            )
        });
        assert_eq!(
            find(&routes, "GET", "/users/me").unwrap().pattern,
            "/users/me"
        );
        assert_eq!(
            find(&routes, "GET", "/users/42").unwrap().pattern,
            "/users/:id"
        );
        assert_eq!(find(&routes, "post", "/users").unwrap().pattern, "/users");
        assert!(find(&routes, "GET", "/users").is_none());
        assert_eq!(
            find(&routes, "HEAD", "/users/7").unwrap().pattern,
            "/users/:id"
        );
        assert!(find(&routes, "GET", "/orders/1/items/2").is_some());
        assert!(find(&routes, "DELETE", "/users/1").is_none());
    }

    #[test]
    fn example_choice() {
        let mut r = route(HttpMethod::Get, "/x");
        r.examples.insert(
            0,
            Example {
                name: "Not found".into(),
                status: 404,
                headers: Default::default(),
                body: String::new(),
                delay_ms: None,
            },
        );
        assert_eq!(pick(&r, None).status, 200, "first 2xx by default");
        assert_eq!(pick(&r, Some("404")).status, 404);
        assert_eq!(pick(&r, Some("not found")).status, 404);
        assert_eq!(pick(&r, Some("nope")).status, 200);
    }
}
