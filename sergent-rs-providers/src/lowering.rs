//! The Anthropic client-side schema lowering: a total function
//! over the validated canonical dialect. It moves the refinements the Anthropic
//! json_schema facility does not accept -- inclusive numeric bounds and array
//! maximum-item counts -- into the affected node's description, keeping
//! structure intact, and never mutates the stored canonical schema.
//! @sergent-rs-providers/docs/providers.md
//!
//! The dialect is a closed keyword set proven at construction, so lowering is
//! total: every construct is representable, minItems and structure are
//! preserved, and there is no reachable "cannot lower" outcome in this
//! implementation.

use serde_json::{Map, Value};

// The dialect keywords Anthropic's json_schema facility does not accept; each
// is moved into the node description instead of being sent on the wire.
const MOVED_KEYWORDS: &[&str] = &["minimum", "maximum", "maxItems"];

/// Lower one canonical schema document for Anthropic. Works on a clone; the
/// trusted canonical input is never mutated.
pub(crate) fn lower_for_anthropic(schema: &Value) -> Value {
    let mut lowered = schema.clone();
    lower_node(&mut lowered);
    lowered
}

/// Moves Anthropic-unsupported refinements into descriptions throughout every
/// schema-bearing position while preserving canonical structure.
fn lower_node(node: &mut Value) {
    let Value::Object(map) = node else {
        return;
    };

    let mut moved: Vec<String> = Vec::new();
    for key in MOVED_KEYWORDS {
        if let Some(value) = map.remove(*key) {
            moved.push(format!("{key}: {value}"));
        }
    }
    if !moved.is_empty() {
        append_description(map, &moved.join(", "));
    }

    // Recurse into the same schema-bearing positions the dialect admits.
    if let Some(Value::Object(props)) = map.get_mut("properties") {
        for child in props.values_mut() {
            lower_node(child);
        }
    }
    if let Some(items) = map.get_mut("items") {
        lower_node(items);
    }
    if let Some(Value::Array(members)) = map.get_mut("anyOf") {
        for child in members.iter_mut() {
            lower_node(child);
        }
    }
    if let Some(Value::Object(defs)) = map.get_mut("$defs") {
        for child in defs.values_mut() {
            lower_node(child);
        }
    }
}

/// Adds a lowering note parenthetically to existing prose, or installs it as
/// the description when no nonempty prose exists.
fn append_description(map: &mut Map<String, Value>, note: &str) {
    let combined = match map.get("description").and_then(Value::as_str) {
        Some(existing) if !existing.is_empty() => format!("{existing} ({note})"),
        _ => note.to_owned(),
    };
    map.insert("description".to_owned(), Value::String(combined));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numeric_bounds_move_into_description_and_structure_stays() {
        let canonical = json!({
            "type": "object",
            "properties": {
                "score": { "type": "integer", "minimum": 0, "maximum": 9, "description": "the score" }
            },
            "additionalProperties": false,
            "required": ["score"]
        });
        let lowered = lower_for_anthropic(&canonical);
        let score = &lowered["properties"]["score"];
        assert!(score.get("minimum").is_none());
        assert!(score.get("maximum").is_none());
        assert_eq!(score["type"], "integer");
        assert_eq!(score["description"], "the score (minimum: 0, maximum: 9)");
        // Structure keywords are untouched.
        assert_eq!(lowered["additionalProperties"], false);
        assert_eq!(lowered["required"], json!(["score"]));
    }

    #[test]
    fn max_items_moves_but_min_items_stays() {
        let canonical = json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 5 }
            },
            "additionalProperties": false,
            "required": ["tags"]
        });
        let lowered = lower_for_anthropic(&canonical);
        let tags = &lowered["properties"]["tags"];
        assert_eq!(tags["minItems"], 1);
        assert!(tags.get("maxItems").is_none());
        assert_eq!(tags["description"], "maxItems: 5");
    }

    #[test]
    fn enums_unions_and_local_references_are_preserved() {
        let canonical = json!({
            "type": "object",
            "properties": {
                "stone": { "$ref": "#/$defs/Stone" },
                "note": { "anyOf": [ { "type": "string" }, { "type": "null" } ] }
            },
            "additionalProperties": false,
            "required": ["stone", "note"],
            "$defs": { "Stone": { "type": "string", "enum": ["black", "white"] } }
        });
        let lowered = lower_for_anthropic(&canonical);
        assert_eq!(
            lowered, canonical,
            "no unsupported refinement, so nothing moves"
        );
    }

    #[test]
    fn refinements_inside_defs_and_arrays_are_lowered() {
        let canonical = json!({
            "type": "object",
            "properties": { "cells": { "type": "array", "items": { "$ref": "#/$defs/Cell" } } },
            "additionalProperties": false,
            "required": ["cells"],
            "$defs": {
                "Cell": {
                    "type": "object",
                    "properties": { "row": { "type": "integer", "minimum": 0, "maximum": 14 } },
                    "additionalProperties": false,
                    "required": ["row"]
                }
            }
        });
        let lowered = lower_for_anthropic(&canonical);
        let row = &lowered["$defs"]["Cell"]["properties"]["row"];
        assert!(row.get("minimum").is_none());
        assert!(row.get("maximum").is_none());
        assert_eq!(row["description"], "minimum: 0, maximum: 14");
    }

    #[test]
    fn the_canonical_input_is_never_mutated() {
        let canonical = json!({
            "type": "object",
            "properties": { "n": { "type": "integer", "minimum": 1 } },
            "additionalProperties": false,
            "required": ["n"]
        });
        let before = canonical.clone();
        let _ = lower_for_anthropic(&canonical);
        assert_eq!(canonical, before, "lowering works on a copy");
    }
}
