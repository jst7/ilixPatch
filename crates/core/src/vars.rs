//! `{{variable}}` interpolation with layered scopes.
//!
//! Scopes are pushed from lowest to highest priority, e.g.
//! collection -> environment -> local secrets -> runtime.

use indexmap::IndexMap;
use thiserror::Error;

use crate::auth::Auth;
use crate::model::{Body, Request};

#[derive(Debug, Error, PartialEq)]
pub enum VarError {
    #[error("undefined variable(s): {}", .0.join(", "))]
    Undefined(Vec<String>),
    #[error("unclosed '{{{{' in: {0}")]
    Unclosed(String),
}

#[derive(Debug, Clone, Default)]
pub struct Scope {
    vars: IndexMap<String, String>,
}

impl Scope {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a layer of variables. Later layers override earlier ones.
    pub fn push<'a>(mut self, layer: impl IntoIterator<Item = (&'a String, &'a String)>) -> Self {
        for (k, v) in layer {
            self.vars.insert(k.clone(), v.clone());
        }
        self
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    /// Replace every `{{name}}` in `input`.
    pub fn interpolate(&self, input: &str) -> Result<String, VarError> {
        let mut out = String::with_capacity(input.len());
        let mut missing = Vec::new();
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else {
                return Err(VarError::Unclosed(input.to_string()));
            };
            let name = after[..end].trim();
            match self.get(name) {
                Some(v) => out.push_str(v),
                None => match dynamic_variable(name) {
                    Some(v) => out.push_str(&v),
                    None => missing.push(name.to_string()),
                },
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        if missing.is_empty() {
            Ok(out)
        } else {
            Err(VarError::Undefined(missing))
        }
    }

    /// Return a copy of `req` with all variables resolved.
    pub fn resolve_request(&self, req: &Request) -> Result<Request, VarError> {
        let map = |m: &IndexMap<String, String>| -> Result<IndexMap<String, String>, VarError> {
            m.iter()
                .map(|(k, v)| Ok((self.interpolate(k)?, self.interpolate(v)?)))
                .collect()
        };
        Ok(Request {
            name: req.name.clone(),
            method: req.method,
            url: self.interpolate(&req.url)?,
            headers: map(&req.headers)?,
            query: map(&req.query)?,
            body: match &req.body {
                None => None,
                Some(Body::Json { content }) => Some(Body::Json {
                    content: self.interpolate(content)?,
                }),
                Some(Body::Text { content }) => Some(Body::Text {
                    content: self.interpolate(content)?,
                }),
                Some(Body::Form { fields }) => Some(Body::Form {
                    fields: map(fields)?,
                }),
            },
            // Scripts are code, never interpolated.
            scripts: req.scripts.clone(),
            settings: req.settings,
            auth: self.resolve_auth(&req.auth)?,
            // Variable names and paths are not templated.
            extract: req.extract.clone(),
        })
    }

    /// Resolve `{{variables}}` in every string field of an [`Auth`] (D39).
    ///
    /// Walks the serialized form, so new auth types and fields are covered
    /// without changes here. All undefined names are reported together.
    pub fn resolve_auth(&self, auth: &Auth) -> Result<Auth, VarError> {
        if matches!(auth, Auth::Inherit | Auth::NoAuth) {
            return Ok(auth.clone());
        }
        let Ok(mut value) = toml::Value::try_from(auth) else {
            return Ok(auth.clone());
        };
        let mut missing = Vec::new();
        self.interpolate_value(&mut value, &mut missing)?;
        if !missing.is_empty() {
            return Err(VarError::Undefined(missing));
        }
        Ok(value.try_into().unwrap_or_else(|_| auth.clone()))
    }

    fn interpolate_value(
        &self,
        value: &mut toml::Value,
        missing: &mut Vec<String>,
    ) -> Result<(), VarError> {
        match value {
            toml::Value::String(s) if s.contains("{{") => match self.interpolate(s) {
                Ok(v) => *s = v,
                Err(VarError::Undefined(names)) => {
                    for n in names {
                        if !missing.contains(&n) {
                            missing.push(n);
                        }
                    }
                }
                Err(e) => return Err(e),
            },
            toml::Value::Array(items) => {
                for v in items {
                    self.interpolate_value(v, missing)?;
                }
            }
            toml::Value::Table(t) => {
                for (_, v) in t.iter_mut() {
                    self.interpolate_value(v, missing)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Postman dynamic variables (`{{$guid}}`, `{{$timestamp}}`…), generated fresh
/// on every use. A variable of the same name defined in a scope wins. Unknown
/// `$names` stay undefined.
pub fn dynamic_variable(name: &str) -> Option<String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Some(match name {
        "$guid" | "$randomUUID" => uuid_v4(),
        "$timestamp" => now.as_secs().to_string(),
        "$isoTimestamp" => iso_timestamp(now.as_secs(), now.subsec_millis()),
        "$randomInt" => (random_u64() % 1001).to_string(),
        _ => return None,
    })
}

/// Random 64 bits from std's randomly seeded hasher (no extra dependency).
fn random_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    h.finish()
}

fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&random_u64().to_le_bytes());
    b[8..].copy_from_slice(&random_u64().to_le_bytes());
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// `2026-11-11T09:30:00.123Z` from Unix seconds (UTC, proleptic Gregorian).
fn iso_timestamp(secs: u64, millis: u32) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn interpolates_and_trims() {
        let s = Scope::new().push(&scope(&[("host", "api.dev")]));
        assert_eq!(
            s.interpolate("https://{{ host }}/x").unwrap(),
            "https://api.dev/x"
        );
    }

    #[test]
    fn later_layers_override() {
        let s = Scope::new()
            .push(&scope(&[("host", "collection")]))
            .push(&scope(&[("host", "env")]));
        assert_eq!(s.interpolate("{{host}}").unwrap(), "env");
    }

    #[test]
    fn reports_all_missing() {
        let s = Scope::new();
        assert_eq!(
            s.interpolate("{{a}}/{{b}}"),
            Err(VarError::Undefined(vec!["a".into(), "b".into()]))
        );
    }

    #[test]
    fn unclosed_is_error() {
        assert!(matches!(
            Scope::new().interpolate("{{oops"),
            Err(VarError::Unclosed(_))
        ));
    }

    #[test]
    fn postman_dynamic_variables() {
        let s = Scope::new();
        let guid = s.interpolate("{{$guid}}").unwrap();
        assert_eq!(guid.len(), 36);
        assert_eq!(&guid[14..15], "4");
        assert_ne!(guid, s.interpolate("{{$randomUUID}}").unwrap());
        let ts: u64 = s.interpolate("{{$timestamp}}").unwrap().parse().unwrap();
        assert!(ts > 1_700_000_000);
        let n: u32 = s.interpolate("{{ $randomInt }}").unwrap().parse().unwrap();
        assert!(n <= 1000);
        assert!(s.interpolate("{{$isoTimestamp}}").unwrap().ends_with('Z'));
        assert_eq!(iso_timestamp(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_timestamp(1_762_853_400, 7), "2025-11-11T09:30:00.007Z");
        // Unknown `$x` is still an error; a user-defined variable wins.
        assert!(s.interpolate("{{$nope}}").is_err());
        let v = scope(&[("$guid", "fixed")]);
        assert_eq!(
            Scope::new().push(&v).interpolate("{{$guid}}").unwrap(),
            "fixed"
        );
    }
}
