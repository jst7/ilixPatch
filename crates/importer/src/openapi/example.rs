//! Example values from JSON Schemas (OpenAPI 3.0/3.1 and Swagger 2.0).
//!
//! Explicit `example` / `examples` / `default` / `const` / `enum` win; otherwise
//! a placeholder of the right type is generated. Recursive schemas stop at
//! the first repetition, and depth is capped.

use indexmap::IndexMap;

use super::doc::{Node, deref};

const MAX_DEPTH: usize = 12;

/// Examples for request bodies and parameters (`readOnly` properties are left out).
pub(crate) struct Gen<'a> {
    root: &'a Node,
    /// `$ref`s being expanded (cycle guard).
    stack: Vec<String>,
}

impl<'a> Gen<'a> {
    pub fn new(root: &'a Node) -> Self {
        Self {
            root,
            stack: Vec::new(),
        }
    }

    /// An example for `schema`, or `None` when nothing sensible can be produced
    /// (empty schema, unresolved reference, recursion).
    pub fn example(&mut self, schema: &Node) -> Option<Node> {
        self.gen_schema(schema, 0)
    }

    fn gen_schema(&mut self, schema: &Node, depth: usize) -> Option<Node> {
        if depth > MAX_DEPTH {
            return None;
        }
        let reference = schema.s("$ref").map(str::to_string);
        if let Some(r) = &reference {
            if self.stack.contains(r) {
                return None;
            }
            self.stack.push(r.clone());
        }
        let out = match deref(self.root, schema) {
            Ok(s) => self.gen_resolved(s, depth),
            Err(_) => None,
        };
        if reference.is_some() {
            self.stack.pop();
        }
        out
    }

    fn gen_resolved(&mut self, s: &Node, depth: usize) -> Option<Node> {
        if let Some(e) = explicit(s) {
            return Some(e);
        }
        // Compositions.
        let all = s.get("allOf").map(Node::arr).unwrap_or_default();
        if !all.is_empty() {
            let mut merged = IndexMap::new();
            let mut last = None;
            for part in all {
                match self.gen_schema(part, depth + 1) {
                    Some(Node::Obj(m)) => merged.extend(m),
                    Some(other) => last = Some(other),
                    None => {}
                }
            }
            if let Some(Node::Obj(own)) = self.object(s, depth) {
                merged.extend(own);
            }
            return if merged.is_empty() {
                last
            } else {
                Some(Node::Obj(merged))
            };
        }
        for key in ["oneOf", "anyOf"] {
            let first = s
                .get(key)
                .map(Node::arr)
                .unwrap_or_default()
                .iter()
                .find(|alt| !is_null_type(alt));
            if let Some(alt) = first {
                return self.gen_schema(alt, depth + 1);
            }
        }

        match schema_type(s).as_deref() {
            Some("object") => self.object(s, depth),
            Some("array") => {
                let item = s
                    .get("items")
                    .and_then(|i| self.gen_schema(i, depth + 1))
                    .map(|i| vec![i])
                    .unwrap_or_default();
                Some(Node::Arr(item))
            }
            Some("string") => Some(Node::Str(string_example(s))),
            Some("integer") => Some(Node::Num(0.into())),
            Some("number") => Some(Node::Num(0.into())),
            Some("boolean") => Some(Node::Bool(true)),
            Some("null") => Some(Node::Null),
            _ => None,
        }
    }

    fn object(&mut self, s: &Node, depth: usize) -> Option<Node> {
        let props = s.get("properties")?.obj()?;
        let mut out = IndexMap::new();
        for (name, prop) in props {
            let resolved = deref(self.root, prop).unwrap_or(prop);
            if resolved.is_true("readOnly") {
                continue;
            }
            if let Some(v) = self.gen_schema(prop, depth + 1) {
                out.insert(name.clone(), v);
            }
        }
        Some(Node::Obj(out))
    }
}

/// `example`, `examples[0]` (JSON Schema 2020-12), `default`, `const` or `enum[0]`.
pub(crate) fn explicit(s: &Node) -> Option<Node> {
    if let Some(e) = s.get("example") {
        return Some(e.clone());
    }
    if let Some(Node::Arr(list)) = s.get("examples")
        && let Some(first) = list.first()
    {
        return Some(first.clone());
    }
    for key in ["default", "const"] {
        if let Some(v) = s.get(key) {
            return Some(v.clone());
        }
    }
    s.get("enum")
        .map(Node::arr)
        .and_then(|e| e.iter().find(|v| **v != Node::Null))
        .cloned()
}

fn is_null_type(s: &Node) -> bool {
    s.s("type") == Some("null")
}

/// `type`, also as a 3.1 list (`["string", "null"]` → string), or inferred.
fn schema_type(s: &Node) -> Option<String> {
    match s.get("type") {
        Some(Node::Str(t)) => Some(t.clone()),
        Some(Node::Arr(ts)) => ts
            .iter()
            .filter_map(Node::str)
            .find(|t| *t != "null")
            .or_else(|| ts.first().and_then(Node::str))
            .map(str::to_string),
        _ if s.get("properties").is_some() => Some("object".into()),
        _ if s.get("items").is_some() => Some("array".into()),
        _ => None,
    }
}

fn string_example(s: &Node) -> String {
    match s.s("format").unwrap_or_default() {
        "date-time" => "2024-01-01T00:00:00Z",
        "date" => "2024-01-01",
        "time" => "00:00:00",
        "email" => "user@example.com",
        "uuid" => "3fa85f64-5717-4562-b3fc-2c963f66afa6",
        "uri" | "url" => "https://example.com",
        "hostname" => "example.com",
        "ipv4" => "192.168.0.1",
        "ipv6" => "::1",
        "byte" => "",
        "binary" => "",
        "password" => "",
        _ => "string",
    }
    .to_string()
}

/// An example for a parameter / media type object: `example`, the first of
/// `examples` (`{name: {value}}`), then its schema.
pub(crate) fn of_holder(root: &Node, holder: &Node) -> Option<Node> {
    if let Some(e) = holder.get("example") {
        return Some(e.clone());
    }
    if let Some((_, first)) = holder
        .get("examples")
        .into_iter()
        .flat_map(Node::entries)
        .next()
    {
        let first = deref(root, first).unwrap_or(first);
        if let Some(v) = first.get("value") {
            return Some(v.clone());
        }
    }
    let schema = holder.get("schema")?;
    Gen::new(root).example(schema)
}

#[cfg(test)]
mod tests {
    use super::super::doc::from_json;
    use super::*;

    fn ex(doc: &str, schema: &str) -> String {
        let root = from_json(doc.as_bytes()).unwrap();
        let schema = from_json(schema.as_bytes()).unwrap();
        let v = Gen::new(&root).example(&schema);
        serde_json::to_string(&v).unwrap()
    }

    #[test]
    fn types_formats_and_explicit_values() {
        let s = r#"{"type":"object","properties":{
            "id":{"type":"integer","readOnly":true},
            "name":{"type":"string","example":"doggie"},
            "when":{"type":"string","format":"date-time"},
            "status":{"type":"string","enum":["available","sold"]},
            "tags":{"type":"array","items":{"type":"string"}},
            "ratio":{"type":["number","null"]},
            "ok":{"type":"boolean","default":false}}}"#;
        assert_eq!(
            ex("{}", s),
            r#"{"name":"doggie","when":"2024-01-01T00:00:00Z","status":"available","tags":["string"],"ratio":0,"ok":false}"#
        );
    }

    #[test]
    fn refs_compositions_and_recursion() {
        let doc = r##"{"components":{"schemas":{
            "Node":{"type":"object","properties":{"name":{"type":"string"},"child":{"$ref":"#/components/schemas/Node"}}},
            "Base":{"type":"object","properties":{"id":{"type":"integer"}}},
            "Cat":{"allOf":[{"$ref":"#/components/schemas/Base"},{"type":"object","properties":{"meow":{"type":"boolean"}}}]},
            "Pet":{"oneOf":[{"$ref":"#/components/schemas/Cat"},{"type":"string"}]}}}}"##;
        // The recursive property is left out.
        assert_eq!(
            ex(doc, r##"{"$ref":"#/components/schemas/Node"}"##),
            r#"{"name":"string"}"#
        );
        assert_eq!(
            ex(doc, r##"{"$ref":"#/components/schemas/Pet"}"##),
            r#"{"id":0,"meow":true}"#
        );
        assert_eq!(ex(doc, r##"{"$ref":"other.json#/X"}"##), "null");
    }
}
