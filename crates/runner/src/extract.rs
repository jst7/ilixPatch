//! D48: apply the `[[extract]]` rules of a request to its response.

use ilixpatch_core::{Extract, ExtractFrom, ExtractScope};
use ilixpatch_scripting::{VarScope, VarUpdate};
use serde_json::Value;

/// Rules that need the response body.
pub fn needs_body(rules: &[Extract]) -> bool {
    rules.iter().any(|r| {
        r.enabled
            && matches!(
                r.from,
                ExtractFrom::Json | ExtractFrom::Body | ExtractFrom::Regex
            )
    })
}

/// One step of a JSON path.
#[derive(Debug, PartialEq)]
enum Step {
    Key(String),
    Index(usize),
}

/// `data.items[0].id`, `$.a["b.c"]`, `[2].name` (a leading `$` is optional).
fn parse_path(path: &str) -> Result<Vec<Step>, String> {
    let p = path.trim();
    let p = p.strip_prefix('$').unwrap_or(p);
    let mut steps = Vec::new();
    let mut chars = p.chars().peekable();
    let mut key = String::new();
    let flush = |key: &mut String, steps: &mut Vec<Step>| {
        if !key.is_empty() {
            steps.push(Step::Key(std::mem::take(key)));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '.' => flush(&mut key, &mut steps),
            '[' => {
                flush(&mut key, &mut steps);
                let mut inner = String::new();
                let mut closed = false;
                let quoted = matches!(chars.peek(), Some('"' | '\''));
                if quoted {
                    let q = chars.next().unwrap_or('"');
                    for c in chars.by_ref() {
                        if c == q {
                            break;
                        }
                        inner.push(c);
                    }
                    if chars.next() == Some(']') {
                        closed = true;
                    }
                    if !closed {
                        return Err(format!("unclosed [ in path `{path}`"));
                    }
                    steps.push(Step::Key(inner));
                } else {
                    for c in chars.by_ref() {
                        if c == ']' {
                            closed = true;
                            break;
                        }
                        inner.push(c);
                    }
                    if !closed {
                        return Err(format!("unclosed [ in path `{path}`"));
                    }
                    let i = inner.trim();
                    match i.parse::<usize>() {
                        Ok(n) => steps.push(Step::Index(n)),
                        Err(_) => steps.push(Step::Key(i.to_string())),
                    }
                }
            }
            c => key.push(c),
        }
    }
    flush(&mut key, &mut steps);
    Ok(steps)
}

/// The value at `path` (strings unquoted; objects, arrays, numbers… as JSON).
pub fn json_path(doc: &Value, path: &str) -> Result<Option<String>, String> {
    let mut v = doc;
    for step in parse_path(path)? {
        let next = match (&step, v) {
            (Step::Key(k), Value::Object(m)) => m.get(k),
            (Step::Index(i), Value::Array(a)) => a.get(*i),
            (Step::Key(k), Value::Array(a)) => k.parse::<usize>().ok().and_then(|i| a.get(i)),
            _ => None,
        };
        match next {
            Some(n) => v = n,
            None => return Ok(None),
        }
    }
    Ok(Some(match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }))
}

/// The updates the rules produce, and a log line per rule.
pub fn apply(
    rules: &[Extract],
    status: u16,
    headers: &[(String, String)],
    body: Option<&str>,
) -> (Vec<VarUpdate>, Vec<String>) {
    let mut updates = Vec::new();
    let mut logs = Vec::new();
    let mut parsed: Option<Result<Value, String>> = None;
    for r in rules
        .iter()
        .filter(|r| r.enabled && !r.var.trim().is_empty())
    {
        let var = r.var.trim();
        let got: Result<Option<String>, String> = match r.from {
            ExtractFrom::Status => Ok(Some(status.to_string())),
            ExtractFrom::Header => Ok(headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(r.path.trim()))
                .map(|(_, v)| v.clone())),
            ExtractFrom::Body => Ok(body.map(str::to_string)),
            ExtractFrom::Json => {
                let doc = parsed.get_or_insert_with(|| {
                    serde_json::from_str::<Value>(body.unwrap_or(""))
                        .map_err(|e| format!("the body is not JSON ({e})"))
                });
                match doc {
                    Ok(d) => json_path(d, &r.path),
                    Err(e) => Err(e.clone()),
                }
            }
            ExtractFrom::Regex => match regex::Regex::new(&r.path) {
                Err(e) => Err(format!("invalid regex: {e}")),
                Ok(re) => Ok(body.and_then(|b| re.captures(b)).map(|c| {
                    c.get(1)
                        .or_else(|| c.get(0))
                        .map_or(String::new(), |m| m.as_str().to_string())
                })),
            },
        };
        match got {
            Ok(Some(value)) => {
                logs.push(format!("[extract] {var} ← {}", describe(r)));
                updates.push(VarUpdate {
                    scope: match r.scope {
                        ExtractScope::Environment => VarScope::Environment,
                        ExtractScope::Collection => VarScope::Collection,
                        ExtractScope::Global => VarScope::Global,
                    },
                    key: var.to_string(),
                    value: Some(value),
                });
            }
            Ok(None) => logs.push(format!("[warn] extract {var}: {} not found", describe(r))),
            Err(e) => logs.push(format!("[warn] extract {var}: {e}")),
        }
    }
    (updates, logs)
}

fn describe(r: &Extract) -> String {
    match r.from {
        ExtractFrom::Json => format!("JSON {}", r.path),
        ExtractFrom::Header => format!("header {}", r.path),
        ExtractFrom::Status => "status".into(),
        ExtractFrom::Body => "body".into(),
        ExtractFrom::Regex => format!("regex {}", r.path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(var: &str, from: ExtractFrom, path: &str) -> Extract {
        Extract {
            var: var.into(),
            from,
            path: path.into(),
            scope: ExtractScope::Environment,
            enabled: true,
        }
    }

    #[test]
    fn json_paths() {
        let d: Value = serde_json::from_str(
            r#"{"data":{"access_token":"abc","items":[{"id":7},{"id":8}],"a.b":true},"n":null}"#,
        )
        .unwrap();
        let get = |p: &str| json_path(&d, p).unwrap();
        assert_eq!(get("data.access_token").as_deref(), Some("abc"));
        assert_eq!(get("$.data.items[1].id").as_deref(), Some("8"));
        assert_eq!(get("data.items.0.id").as_deref(), Some("7"));
        assert_eq!(get(r#"data["a.b"]"#).as_deref(), Some("true"));
        assert_eq!(get("data.items[0]").as_deref(), Some(r#"{"id":7}"#));
        assert_eq!(get("n").as_deref(), Some("null"));
        assert_eq!(get("data.missing"), None);
        assert!(json_path(&d, "data[0").is_err());
    }

    #[test]
    fn rules_produce_updates_and_logs() {
        let rules = vec![
            rule("token", ExtractFrom::Json, "data.access_token"),
            rule("rid", ExtractFrom::Header, "X-Request-Id"),
            rule("code", ExtractFrom::Status, ""),
            rule("csrf", ExtractFrom::Regex, r#"csrf=(\w+)"#),
            rule("nope", ExtractFrom::Json, "data.nope"),
            Extract {
                enabled: false,
                ..rule("off", ExtractFrom::Status, "")
            },
        ];
        let body = r#"{"data":{"access_token":"abc","html":"csrf=XYZ;"}}"#;
        let headers = vec![("x-request-id".to_string(), "r-1".to_string())];
        let (u, logs) = apply(&rules, 201, &headers, Some(body));
        let got: Vec<_> = u
            .iter()
            .map(|u| (u.key.as_str(), u.value.as_deref().unwrap()))
            .collect();
        assert_eq!(
            got,
            [
                ("token", "abc"),
                ("rid", "r-1"),
                ("code", "201"),
                ("csrf", "XYZ")
            ]
        );
        assert!(u.iter().all(|u| u.scope == VarScope::Environment));
        assert!(
            logs.iter()
                .any(|l| l.contains("extract nope") && l.contains("not found"))
        );
        assert!(needs_body(&rules));
        assert!(!needs_body(&[rule("s", ExtractFrom::Status, "")]));
    }

    #[test]
    fn non_json_body_is_reported() {
        let (u, logs) = apply(
            &[rule("t", ExtractFrom::Json, "a")],
            200,
            &[],
            Some("<html>"),
        );
        assert!(u.is_empty());
        assert!(logs[0].contains("not JSON"));
    }

    #[test]
    fn toml_round_trip() {
        let toml_src = r#"
name = "login"
url = "https://x/login"

[[extract]]
var = "token"
from = "json"
path = "data.access_token"
scope = "environment"

[[extract]]
var = "rid"
from = "header"
path = "X-Request-Id"
scope = "collection"
enabled = false
"#;
        let r: ilixpatch_core::Request = toml::from_str(toml_src).unwrap();
        assert_eq!(r.extract.len(), 2);
        assert_eq!(r.extract[1].scope, ExtractScope::Collection);
        assert!(!r.extract[1].enabled);
        let back: ilixpatch_core::Request = toml::from_str(&toml::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
