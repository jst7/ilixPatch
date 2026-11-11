//! In-memory variable store with Postman scopes.

use ilixpatch_core::Scope;
use ilixpatch_scripting::{VarScope, VarUpdate};
use indexmap::IndexMap;
use serde_json::Value;

/// Current values of every variable scope during a send or a run.
///
/// Resolution precedence (highest first): local > iteration data >
/// environment > collection > globals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VarStore {
    pub globals: IndexMap<String, String>,
    pub collection: IndexMap<String, String>,
    pub environment: IndexMap<String, String>,
    pub environment_name: Option<String>,
    /// `pm.variables.set(...)`: lives for the current run only.
    pub local: IndexMap<String, String>,
}

impl VarStore {
    /// Apply script variable writes (`None` value = unset).
    pub fn apply(&mut self, updates: &[VarUpdate]) {
        for u in updates {
            let map = match u.scope {
                VarScope::Global => &mut self.globals,
                VarScope::Collection => &mut self.collection,
                VarScope::Environment => &mut self.environment,
                VarScope::Local => &mut self.local,
            };
            match &u.value {
                Some(v) => {
                    map.insert(u.key.clone(), v.clone());
                }
                None => {
                    map.shift_remove(&u.key);
                }
            }
        }
    }

    /// Build the interpolation scope, optionally layering a data-file row.
    pub fn scope(&self, data: Option<&IndexMap<String, Value>>) -> Scope {
        let data: IndexMap<String, String> = data
            .map(|d| {
                d.iter()
                    .map(|(k, v)| (k.clone(), value_to_string(v)))
                    .collect()
            })
            .unwrap_or_default();
        Scope::new()
            .push(&self.globals)
            .push(&self.collection)
            .push(&self.environment)
            .push(&data)
            .push(&self.local)
    }
}

/// Strings are used verbatim; other JSON values use their JSON text.
pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn precedence() {
        let store = VarStore {
            globals: m(&[("a", "g"), ("b", "g"), ("c", "g"), ("d", "g"), ("e", "g")]),
            collection: m(&[("b", "c"), ("c", "c"), ("d", "c"), ("e", "c")]),
            environment: m(&[("c", "e"), ("d", "e"), ("e", "e")]),
            environment_name: None,
            local: m(&[("e", "l")]),
        };
        let data: IndexMap<String, Value> =
            [("d".to_string(), Value::from(7))].into_iter().collect();
        let s = store.scope(Some(&data));
        assert_eq!(s.interpolate("{{a}}{{b}}{{c}}{{d}}{{e}}").unwrap(), "gce7l");
    }

    #[test]
    fn apply_updates() {
        let mut store = VarStore {
            environment: m(&[("x", "1")]),
            ..Default::default()
        };
        store.apply(&[
            VarUpdate {
                scope: VarScope::Environment,
                key: "x".into(),
                value: None,
            },
            VarUpdate {
                scope: VarScope::Global,
                key: "g".into(),
                value: Some("2".into()),
            },
        ]);
        assert!(store.environment.is_empty());
        assert_eq!(store.globals["g"], "2");
    }
}
