//! The closed normalizer: it drops only the non-authoritative emitter metadata
//! and applies the fixed conversion list to one schema node, recursing into
//! schema-bearing children. It is not a general schema repairer.
//! @sergent/docs/framework.md

use serde_json::{Map, Value, json};

// The closed set of schemars keywords the normalizer drops: only the
// non-authoritative emitter metadata title, default, and format. Every other
// keyword schemars emits is left in place, so any authored structural
// constraint the dialect cannot express fails the validator instead of being
// silently weakened. It is not a general repairer.
const DROP_KEYWORDS: &[&str] = &["title", "default", "format"];

/// Apply the closed normalizer conversions to one schema node in place, then
/// recurse into schema-bearing children only.
pub(crate) fn normalize_node(node: &mut Value) {
    let Value::Object(map) = node else {
        return;
    };

    // Drop generated metadata and out-of-dialect refinements.
    for key in DROP_KEYWORDS {
        map.remove(*key);
    }

    // const -> one-value enum.
    if let Some(constant) = map.remove("const") {
        map.insert("enum".to_owned(), Value::Array(vec![constant]));
    }

    // Nullable union: type array containing "null" -> anyOf of the non-null
    // branch (carrying its constraints) and a null branch.
    rewrite_nullable(map);

    // Described reference: a $ref node with siblings -> the ref alone inside an
    // anyOf, so the reference node carries no ordinary sibling keyword.
    rewrite_described_ref(map);

    // Require every declared property.
    strengthen_required(map);

    // Recurse only into schema-bearing positions.
    if let Some(Value::Object(props)) = map.get_mut("properties") {
        for schema in props.values_mut() {
            normalize_node(schema);
        }
    }
    if let Some(items) = map.get_mut("items") {
        normalize_node(items);
    }
    if let Some(Value::Array(members)) = map.get_mut("anyOf") {
        for member in members.iter_mut() {
            normalize_node(member);
        }
    }
    if let Some(Value::Object(defs)) = map.get_mut("$defs") {
        for schema in defs.values_mut() {
            normalize_node(schema);
        }
    }
}

/// Rewrite a two-member nullable type union into constrained non-null and null
/// `anyOf` branches while preserving its description.
fn rewrite_nullable(map: &mut Map<String, Value>) {
    let Some(Value::Array(types)) = map.get("type") else {
        return;
    };
    if types.len() != 2 || !types.iter().any(|t| t == "null") {
        return;
    }
    let Some(Value::String(non_null)) = types.iter().find(|t| *t != "null").cloned() else {
        return;
    };
    map.remove("type");
    let description = map.remove("description");
    let mut branch = Map::new();
    branch.insert("type".to_owned(), Value::String(non_null));
    let constraint_keys: Vec<String> = map.keys().cloned().collect();
    for key in constraint_keys {
        if let Some(value) = map.remove(&key) {
            branch.insert(key, value);
        }
    }
    map.insert(
        "anyOf".to_owned(),
        Value::Array(vec![Value::Object(branch), json!({ "type": "null" })]),
    );
    if let Some(description) = description {
        map.insert("description".to_owned(), description);
    }
}

/// Isolate a reference inside `anyOf` when the reference node has siblings.
fn rewrite_described_ref(map: &mut Map<String, Value>) {
    if map.len() > 1
        && let Some(reference) = map.remove("$ref")
    {
        map.insert(
            "anyOf".to_owned(),
            Value::Array(vec![json!({ "$ref": reference })]),
        );
    }
}

/// Replace an object's required list with all declared property names. Branch
/// composition reapplies it after injecting the `call` discriminator, so this
/// conversion is the single owner of the require-every-property rule.
pub(crate) fn strengthen_required(map: &mut Map<String, Value>) {
    let Some(Value::Object(props)) = map.get("properties") else {
        return;
    };
    let names: Vec<Value> = props.keys().map(|k| Value::String(k.clone())).collect();
    map.insert("required".to_owned(), Value::Array(names));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- normalizer conversions (the closed list) ---

    #[test]
    fn const_becomes_one_value_enum() {
        let mut node = json!({ "const": "place" });
        normalize_node(&mut node);
        assert_eq!(node, json!({ "enum": ["place"] }));
    }

    #[test]
    fn nullable_union_folds_constraints_into_the_non_null_branch() {
        let mut node = json!({ "type": ["integer", "null"], "minimum": 0 });
        normalize_node(&mut node);
        assert_eq!(
            node,
            json!({ "anyOf": [{ "type": "integer", "minimum": 0 }, { "type": "null" }] })
        );
    }

    #[test]
    fn described_reference_moves_the_ref_into_an_anyof() {
        let mut node = json!({ "$ref": "#/$defs/Stone", "description": "the stone" });
        normalize_node(&mut node);
        assert_eq!(
            node,
            json!({ "anyOf": [{ "$ref": "#/$defs/Stone" }], "description": "the stone" })
        );
    }

    #[test]
    fn only_emitter_metadata_is_dropped_authored_constraints_survive() {
        // title/default/format go; the authored inclusive minimum stays. Every
        // other authored keyword (multipleOf, exclusiveMinimum, ...) is left in
        // place for the dialect validator to reject, never silently dropped.
        let mut node = json!({
            "type": "integer",
            "title": "Weight",
            "default": 0,
            "format": "uint32",
            "multipleOf": 2,
            "minimum": 1
        });
        normalize_node(&mut node);
        assert_eq!(
            node,
            json!({ "type": "integer", "multipleOf": 2, "minimum": 1 })
        );
    }

    #[test]
    fn every_declared_property_becomes_required() {
        let mut node = json!({
            "type": "object",
            "properties": { "a": { "type": "integer" }, "b": { "type": "integer" } },
            "additionalProperties": false,
            "required": ["a"]
        });
        normalize_node(&mut node);
        let required = node["required"].as_array().unwrap();
        assert!(required.contains(&json!("a")));
        assert!(required.contains(&json!("b")));
        assert_eq!(required.len(), 2);
    }
}
