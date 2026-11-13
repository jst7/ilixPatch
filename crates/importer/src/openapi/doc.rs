//! An OpenAPI document as an order-preserving tree, loaded from JSON or YAML,
//! with `$ref` resolution inside the same document.
//!
//! `serde_json::Value` sorts object keys (the workspace doesn't enable
//! `preserve_order`), which would reorder paths, operations and example
//! bodies, so specs are read into [`Node`] instead.

use std::fmt;

use indexmap::IndexMap;
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Number;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Null,
    Bool(bool),
    Num(Number),
    Str(String),
    Arr(Vec<Node>),
    Obj(IndexMap<String, Node>),
}

pub(crate) static NULL: Node = Node::Null;

impl Node {
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Obj(m) => m.get(key),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `self[key]` as a string.
    pub fn s(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Node::str)
    }

    /// `self[key]` as a non-blank string.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.s(key).map(str::trim).filter(|s| !s.is_empty())
    }

    pub fn arr(&self) -> &[Node] {
        match self {
            Node::Arr(a) => a,
            _ => &[],
        }
    }

    pub fn obj(&self) -> Option<&IndexMap<String, Node>> {
        match self {
            Node::Obj(m) => Some(m),
            _ => None,
        }
    }

    /// Entries of an object (none for anything else).
    pub fn entries(&self) -> impl Iterator<Item = (&String, &Node)> {
        self.obj().into_iter().flatten()
    }

    pub fn is_true(&self, key: &str) -> bool {
        matches!(self.get(key), Some(Node::Bool(true)))
    }

    /// A scalar as text (for parameter values); `None` for objects/arrays/null.
    pub fn scalar_text(&self) -> Option<String> {
        match self {
            Node::Str(s) => Some(s.clone()),
            Node::Num(n) => Some(n.to_string()),
            Node::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    /// Text for a parameter value: scalars as is, arrays joined by commas
    /// (OpenAPI's default `form`/`simple` style), objects as JSON.
    pub fn param_text(&self) -> Option<String> {
        match self {
            Node::Null => None,
            Node::Arr(items) => Some(
                items
                    .iter()
                    .filter_map(Node::scalar_text)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            Node::Obj(_) => serde_json::to_string(self).ok(),
            n => n.scalar_text(),
        }
    }

    pub fn to_pretty_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

// ---------- serde ----------

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Node::Null => s.serialize_unit(),
            Node::Bool(b) => s.serialize_bool(*b),
            Node::Num(n) => n.serialize(s),
            Node::Str(v) => s.serialize_str(v),
            Node::Arr(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for i in items {
                    seq.serialize_element(i)?;
                }
                seq.end()
            }
            Node::Obj(m) => {
                let mut map = s.serialize_map(Some(m.len()))?;
                for (k, v) in m {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

/// A map key: YAML allows `200:` or `true:`; they become strings.
struct Key(String);

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a scalar map key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Key, E> {
                Ok(Key(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Key, E> {
                Ok(Key("null".into()))
            }
        }
        d.deserialize_any(V)
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Node;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON/YAML value")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Node, E> {
                Ok(Node::Bool(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Node, E> {
                Ok(Node::Num(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Node, E> {
                Ok(Node::Num(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Node, E> {
                Ok(Number::from_f64(v).map_or(Node::Null, Node::Num))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Node, E> {
                Ok(Node::Str(v.to_string()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Node, E> {
                Ok(Node::Str(v))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Node, E> {
                Ok(Node::Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<Node, E> {
                Ok(Node::Null)
            }
            fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Node, D2::Error> {
                Node::deserialize(d)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
                let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
                while let Some(i) = seq.next_element()? {
                    items.push(i);
                }
                Ok(Node::Arr(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                let mut m = IndexMap::new();
                while let Some((Key(k), v)) = map.next_entry::<Key, Node>()? {
                    m.insert(k, v);
                }
                Ok(Node::Obj(m))
            }
        }
        d.deserialize_any(V)
    }
}

// ---------- loading ----------

pub(crate) fn from_yaml(bytes: &[u8]) -> Result<Node, String> {
    // serde-saphyr's default budget suits config files: the GitHub REST spec
    // alone has more than its 250k nodes. Input is already capped at
    // `MAX_FILE`, so the size-proportional counters are raised to match it;
    // the alias-expansion guards (alias/anchor ratio, max aliases) and the
    // depth limit keep their defaults.
    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_events: 50_000_000,
            max_nodes: 25_000_000,
            max_total_scalar_bytes: crate::parse::MAX_FILE as usize,
        },
    };
    serde_saphyr::from_slice_with_options(bytes, options).map_err(|e| {
        // The error renders a code snippet; the first line is enough here.
        e.to_string().lines().next().unwrap_or("").to_string()
    })
}

pub(crate) fn from_json(bytes: &[u8]) -> Result<Node, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

/// Is this (any JSON/YAML) document an OpenAPI 3.x or Swagger 2.0 spec?
pub(crate) fn is_openapi(n: &Node) -> bool {
    n.get("openapi").is_some() || n.get("swagger").is_some()
}

// ---------- $ref ----------

/// Decode one JSON Pointer token (`~1` → `/`, `~0` → `~`, `%xx` escapes).
fn pointer_token(t: &str) -> String {
    let mut bytes = Vec::with_capacity(t.len());
    let raw = t.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && i + 2 < raw.len()
            && let Ok(b) = u8::from_str_radix(&t[i + 1..i + 3], 16)
        {
            bytes.push(b);
            i += 3;
            continue;
        }
        bytes.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes)
        .replace("~1", "/")
        .replace("~0", "~")
}

/// The node a local `$ref` (`#/components/schemas/Pet`) points to.
pub(crate) fn pointer<'a>(root: &'a Node, reference: &str) -> Option<&'a Node> {
    let path = reference.strip_prefix('#')?;
    let mut cur = root;
    for token in path.split('/').skip(1) {
        let token = pointer_token(token);
        cur = match cur {
            Node::Obj(m) => m.get(&token)?,
            Node::Arr(a) => a.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// Why a `$ref` could not be followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RefError {
    /// Points to another file or URL.
    External(String),
    /// Local pointer to nothing.
    Missing(String),
    /// `$ref` chain too long (or a loop of plain references).
    Loop(String),
}

/// Follow `$ref` chains (an object with `$ref` is replaced by its target;
/// sibling keys are ignored, as in OpenAPI 3.0).
pub(crate) fn deref<'a>(root: &'a Node, mut n: &'a Node) -> Result<&'a Node, RefError> {
    for _ in 0..32 {
        let Some(r) = n.s("$ref") else {
            return Ok(n);
        };
        if !r.starts_with('#') {
            return Err(RefError::External(r.to_string()));
        }
        n = pointer(root, r).ok_or_else(|| RefError::Missing(r.to_string()))?;
    }
    Err(RefError::Loop(n.s("$ref").unwrap_or_default().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_keeps_order_and_numeric_keys() {
        let n = from_yaml(
            b"b: 1\na: [x, 2.5, true, null]\nresponses:\n  200: {description: ok}\n  default: {}\n",
        )
        .unwrap();
        let keys: Vec<&String> = n.obj().unwrap().keys().collect();
        assert_eq!(keys, ["b", "a", "responses"]);
        let codes: Vec<&String> = n.get("responses").unwrap().obj().unwrap().keys().collect();
        assert_eq!(codes, ["200", "default"]);
        assert_eq!(
            serde_json::to_string(&n.get("a").unwrap()).unwrap(),
            r#"["x",2.5,true,null]"#
        );
    }

    #[test]
    fn json_keeps_order() {
        let n = from_json(br#"{"z":{"y":1,"x":2},"a":"s"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&n).unwrap(),
            r#"{"z":{"y":1,"x":2},"a":"s"}"#
        );
    }

    #[test]
    fn yaml_alias_bomb_is_rejected() {
        let mut y = String::from("a: &a [x,x,x,x,x,x,x,x,x,x]\n");
        for i in 0..12 {
            let prev = if i == 0 {
                "a".to_string()
            } else {
                format!("l{}", i - 1)
            };
            y.push_str(&format!(
                "l{i}: &l{i} [*{prev},*{prev},*{prev},*{prev},*{prev},*{prev},*{prev},*{prev},*{prev},*{prev}]\n"
            ));
        }
        assert!(from_yaml(y.as_bytes()).is_err());
    }

    #[test]
    fn refs() {
        let root = from_json(
            br##"{"components":{"schemas":{"A":{"$ref":"#/components/schemas/B"},"B":{"type":"string"},
                 "a/b":{"type":"integer"},"L":{"$ref":"#/components/schemas/L"}}},
                 "x":{"$ref":"#/components/schemas/A"},"e":{"$ref":"other.yaml#/X"},
                 "m":{"$ref":"#/nope"},"s":{"$ref":"#/components/schemas/a~1b"},"l":{"$ref":"#/components/schemas/L"}}"##,
        )
        .unwrap();
        let get = |k: &str| deref(&root, root.get(k).unwrap());
        assert_eq!(get("x").unwrap().s("type"), Some("string"));
        assert_eq!(get("s").unwrap().s("type"), Some("integer"));
        assert_eq!(get("e"), Err(RefError::External("other.yaml#/X".into())));
        assert_eq!(get("m"), Err(RefError::Missing("#/nope".into())));
        assert!(matches!(get("l"), Err(RefError::Loop(_))));
    }
}
