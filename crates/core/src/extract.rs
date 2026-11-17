//! Variables from a response (D48): declarative rules of a request, e.g. "save
//! `data.access_token` of the JSON body as the environment variable `token`".
//! Stored as `[[extract]]` tables in the request file; applied by the runner
//! after each send (app, Runner and CLI), like `pm.environment.set` in a script.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extract {
    /// Variable name to set.
    pub var: String,
    #[serde(default)]
    pub from: ExtractFrom,
    /// JSON path (`data.items[0].id`), header name or regex (first group),
    /// depending on `from`; unused for `status` and `body`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default)]
    pub scope: ExtractScope,
    /// Disabled rules are kept but not applied.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractFrom {
    /// A value of the JSON body (strings unquoted, other values as JSON).
    #[default]
    Json,
    /// A response header (case-insensitive).
    Header,
    /// The HTTP status code.
    Status,
    /// The whole body as text.
    Body,
    /// First capture group of a regex over the body (the whole match if it has none).
    Regex,
}

/// Where the value goes (a runtime value of that scope, like the scripts' `pm.*.set`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractScope {
    #[default]
    Environment,
    Collection,
    Global,
}

/// One step of a JSON path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStep {
    Key(String),
    Index(usize),
}

/// `data.items[0].id`, `$.a["b.c"]`, `[2].name` (a leading `$` is optional).
pub fn parse_json_path(path: &str) -> Result<Vec<PathStep>, String> {
    let p = path.trim();
    let p = p.strip_prefix('$').unwrap_or(p);
    let mut steps = Vec::new();
    let mut chars = p.chars().peekable();
    let mut key = String::new();
    let flush = |key: &mut String, steps: &mut Vec<PathStep>| {
        if !key.is_empty() {
            steps.push(PathStep::Key(std::mem::take(key)));
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
                    steps.push(PathStep::Key(inner));
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
                        Ok(n) => steps.push(PathStep::Index(n)),
                        Err(_) => steps.push(PathStep::Key(i.to_string())),
                    }
                }
            }
            c => key.push(c),
        }
    }
    flush(&mut key, &mut steps);
    Ok(steps)
}
