//! The reference-graph acyclicity guard for the dialect validator: it proves
//! the root `$defs` local `#/$defs/...` references are acyclic by depth-first
//! cycle detection over the reference graph.
//! @sergent/docs/framework.md

use serde_json::{Map, Value};

use super::reference::{append_pointer_token, referenced_definition};
use super::schema::{SchemaError, dialect};

/// Recursively collect the definition names used by local `#/$defs/...`
/// references.
fn collect_refs(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && let Some(name) = referenced_definition(reference)
            {
                out.push(name);
            }
            for value in map.values() {
                collect_refs(value, out);
            }
        }
        Value::Array(items) => {
            for value in items {
                collect_refs(value, out);
            }
        }
        _ => {}
    }
}

/// Build the local definition reference graph and reject the first detected
/// recursive definition.
pub(super) fn check_acyclic(defs: &Map<String, Value>, proposal: &str) -> Result<(), SchemaError> {
    let mut edges: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (name, body) in defs {
        let mut refs = Vec::new();
        collect_refs(body, &mut refs);
        edges.insert(name.clone(), refs);
    }
    // 0 = unvisited, 1 = on the current DFS stack, 2 = fully explored.
    let mut state: std::collections::BTreeMap<String, u8> = std::collections::BTreeMap::new();
    for name in defs.keys() {
        if has_cycle(name, &edges, &mut state) {
            return Err(dialect(
                proposal,
                &append_pointer_token("/$defs", name),
                "recursive references are not allowed",
            ));
        }
    }
    Ok(())
}

/// Detect a cycle from one definition using three-state depth-first traversal.
fn has_cycle(
    node: &str,
    edges: &std::collections::BTreeMap<String, Vec<String>>,
    state: &mut std::collections::BTreeMap<String, u8>,
) -> bool {
    match state.get(node) {
        Some(1) => return true,
        Some(2) => return false,
        _ => {}
    }
    state.insert(node.to_owned(), 1);
    if let Some(neighbors) = edges.get(node) {
        for next in neighbors {
            if has_cycle(next, edges, state) {
                return true;
            }
        }
    }
    state.insert(node.to_owned(), 2);
    false
}
