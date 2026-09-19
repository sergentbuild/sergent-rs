//! The closed dialect validator: it proves a normalized schema conforms to the
//! language-agnostic dialect at construction, before any provider call,
//! failing with the proposal name and a JSON pointer. Reference acyclicity is
//! delegated to the `acyclic` guard. @sergent/docs/framework.md

use serde_json::{Map, Value};
use std::collections::BTreeSet;

use super::acyclic::check_acyclic;
use super::reference::{append_pointer_token, referenced_definition};
use super::schema::{SchemaError, dialect};

/// Prove that a normalized schema conforms to the closed dialect.
/// @sergent/docs/framework.md
pub(crate) fn validate_dialect(schema: &Value, proposal: &str) -> Result<(), SchemaError> {
    let root = schema
        .as_object()
        .ok_or_else(|| dialect(proposal, "", "root schema must be an object"))?;
    let defs_map = match root.get("$defs") {
        Some(Value::Object(defs)) if defs.is_empty() => {
            return Err(dialect(
                proposal,
                "/$defs",
                "$defs must be non-empty when present",
            ));
        }
        Some(Value::Object(defs)) => Some(defs),
        Some(_) => return Err(dialect(proposal, "/$defs", "$defs must be an object")),
        None => None,
    };
    let defs: BTreeSet<String> = defs_map
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();

    // The root is a top-level object, never a root anyOf or ref.
    if root.contains_key("$ref") {
        return Err(dialect(proposal, "", "root must not be a reference"));
    }
    if root.contains_key("anyOf") {
        return Err(dialect(proposal, "", "root must not be an anyOf"));
    }
    match root.get("type").and_then(Value::as_str) {
        Some("object") => {}
        _ => return Err(dialect(proposal, "", "root type must be object")),
    }
    validate_object(root, proposal, "", &defs, true)?;

    if let Some(defs_map) = defs_map {
        for (name, def) in defs_map {
            validate_node(def, proposal, &append_pointer_token("/$defs", name), &defs)?;
        }
        // Local references must be acyclic.
        check_acyclic(defs_map, proposal)?;
    }
    Ok(())
}

/// Validate one schema node as an exact reference, an `anyOf`, or a supported
/// typed node.
fn validate_node(
    node: &Value,
    proposal: &str,
    pointer: &str,
    defs: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    let Some(map) = node.as_object() else {
        return reject_node_outside_dialect(node, proposal, pointer);
    };
    if map.contains_key("$ref") {
        return validate_reference(map, proposal, pointer, defs);
    }
    if map.contains_key("anyOf") {
        return validate_any_of(map, proposal, pointer, defs);
    }
    if map.contains_key("type") {
        return validate_typed_form(map, proposal, pointer, defs);
    }
    reject_node_outside_dialect(node, proposal, pointer)
}

/// Prove that a reference is local, resolved, and carries no sibling keyword.
fn validate_reference(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
    defs: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    if map.len() != 1 {
        return Err(dialect(
            proposal,
            pointer,
            "reference node must carry no sibling keyword",
        ));
    }
    let reference = map["$ref"]
        .as_str()
        .ok_or_else(|| dialect(proposal, pointer, "$ref must be a string"))?;
    let name = referenced_definition(reference)
        .ok_or_else(|| dialect(proposal, pointer, "$ref must be a local #/$defs/ reference"))?;
    if !defs.contains(&name) {
        return Err(dialect(proposal, pointer, "$ref does not resolve"));
    }
    Ok(())
}

/// Prove that an `anyOf` has only admitted siblings and valid member schemas.
fn validate_any_of(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
    defs: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    validate_description(map, proposal, pointer)?;
    for key in map.keys() {
        if key != "anyOf" && key != "description" {
            return Err(dialect(
                proposal,
                pointer,
                format!("keyword {key:?} is not allowed beside anyOf"),
            ));
        }
    }
    let members = map["anyOf"]
        .as_array()
        .ok_or_else(|| dialect(proposal, pointer, "anyOf must be an array"))?;
    if members.is_empty() {
        return Err(dialect(proposal, pointer, "anyOf must be non-empty"));
    }
    for (index, member) in members.iter().enumerate() {
        validate_node(member, proposal, &format!("{pointer}/anyOf/{index}"), defs)?;
    }
    Ok(())
}

/// Dispatch one node carrying the canonical dialect's single string `type`.
fn validate_typed_form(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
    defs: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    let ty = map["type"]
        .as_str()
        .ok_or_else(|| dialect(proposal, pointer, "type must be a single string"))?;
    match ty {
        "object" => validate_object(map, proposal, pointer, defs, false),
        "array" => validate_array(map, proposal, pointer, defs),
        "string" | "boolean" | "integer" | "number" | "null" => {
            validate_scalar(map, proposal, pointer, ty)
        }
        other => Err(dialect(
            proposal,
            pointer,
            format!("unsupported type {other:?}"),
        )),
    }
}

/// Reject a value that is not one of the dialect's three schema node forms.
fn reject_node_outside_dialect(
    node: &Value,
    proposal: &str,
    pointer: &str,
) -> Result<(), SchemaError> {
    let message = if node.is_object() {
        "node must have type, $ref, or anyOf"
    } else {
        "schema node must be an object"
    };
    Err(dialect(proposal, pointer, message))
}

/// Prove that an object is closed, requires every property exactly once, and
/// contains only recursively valid property schemas.
fn validate_object(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
    defs: &BTreeSet<String>,
    is_root: bool,
) -> Result<(), SchemaError> {
    validate_object_keyword_shapes(map, proposal, pointer, is_root)?;
    validate_closed_object(map, proposal, pointer)?;
    let properties = validate_required_property_equivalence(map, proposal, pointer)?;
    validate_property_schemas(properties, proposal, pointer, defs)
}

/// Reject unsupported object keywords and invalid shared keyword value shapes.
fn validate_object_keyword_shapes(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
    is_root: bool,
) -> Result<(), SchemaError> {
    validate_description(map, proposal, pointer)?;
    for key in map.keys() {
        let allowed = matches!(
            key.as_str(),
            "type" | "properties" | "required" | "additionalProperties" | "description"
        ) || (is_root && key == "$defs");
        if !allowed {
            return Err(dialect(
                proposal,
                pointer,
                format!("keyword {key:?} is not allowed on an object"),
            ));
        }
    }
    Ok(())
}

/// Require the canonical closed-object declaration.
fn validate_closed_object(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
) -> Result<(), SchemaError> {
    match map.get("additionalProperties") {
        Some(Value::Bool(false)) => Ok(()),
        _ => Err(dialect(
            proposal,
            pointer,
            "object must set additionalProperties: false (derive with deny_unknown_fields)",
        )),
    }
}

/// Prove exact equivalence between required names and declared properties.
fn validate_required_property_equivalence<'a>(
    map: &'a Map<String, Value>,
    proposal: &str,
    pointer: &str,
) -> Result<&'a Map<String, Value>, SchemaError> {
    let properties = map
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| dialect(proposal, pointer, "object must declare properties"))?;
    let required = map
        .get("required")
        .and_then(Value::as_array)
        .ok_or_else(|| dialect(proposal, pointer, "object must declare required"))?;
    let mut required_names = BTreeSet::new();
    for entry in required {
        let name = entry
            .as_str()
            .ok_or_else(|| dialect(proposal, pointer, "required entries must be strings"))?;
        if !required_names.insert(name.to_owned()) {
            return Err(dialect(
                proposal,
                pointer,
                format!("property {name:?} is listed twice in required"),
            ));
        }
    }
    let property_names: BTreeSet<String> = properties.keys().cloned().collect();
    if required_names != property_names {
        return Err(dialect(
            proposal,
            pointer,
            "required must list every declared property exactly once",
        ));
    }
    Ok(properties)
}

/// Recursively prove every declared property schema.
fn validate_property_schemas(
    properties: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
    defs: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    for (name, schema) in properties {
        validate_node(
            schema,
            proposal,
            &append_pointer_token(&format!("{pointer}/properties"), name),
            defs,
        )?;
    }
    Ok(())
}

/// Prove that an array has one valid item schema, supported keywords, and
/// non-negative integer item bounds.
fn validate_array(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
    defs: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    validate_description(map, proposal, pointer)?;
    for key in map.keys() {
        if !matches!(
            key.as_str(),
            "type" | "items" | "minItems" | "maxItems" | "description"
        ) {
            return Err(dialect(
                proposal,
                pointer,
                format!("keyword {key:?} is not allowed on an array"),
            ));
        }
    }
    // items must be present on every array.
    let items = map
        .get("items")
        .ok_or_else(|| dialect(proposal, pointer, "array must declare items"))?;
    for bound in ["minItems", "maxItems"] {
        if let Some(value) = map.get(bound)
            && !value.is_u64()
        {
            return Err(dialect(
                proposal,
                pointer,
                format!("{bound} must be a non-negative integer"),
            ));
        }
    }
    validate_node(items, proposal, &format!("{pointer}/items"), defs)
}

/// Prove that a scalar uses only supported keywords, type-matching enum values,
/// and numeric bounds where applicable.
fn validate_scalar(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
    ty: &str,
) -> Result<(), SchemaError> {
    validate_description(map, proposal, pointer)?;
    let numeric = matches!(ty, "integer" | "number");
    for key in map.keys() {
        let allowed = match key.as_str() {
            "type" | "description" | "enum" => true,
            // Inclusive minimum and maximum only.
            "minimum" | "maximum" => numeric,
            _ => false,
        };
        if !allowed {
            return Err(dialect(
                proposal,
                pointer,
                format!("keyword {key:?} is not allowed on a scalar"),
            ));
        }
    }
    if let Some(values) = map.get("enum") {
        let values = values
            .as_array()
            .ok_or_else(|| dialect(proposal, pointer, "enum must be an array"))?;
        if values.is_empty() {
            return Err(dialect(proposal, pointer, "enum must be non-empty"));
        }
        // Scalar enum values matching the declared type only.
        for value in values {
            let matches_type = match ty {
                "string" => value.is_string(),
                "boolean" => value.is_boolean(),
                "integer" => value.is_i64() || value.is_u64(),
                "number" => value.is_number(),
                "null" => value.is_null(),
                _ => false,
            };
            if !matches_type {
                return Err(dialect(
                    proposal,
                    pointer,
                    "enum values must match the declared scalar type",
                ));
            }
        }
    }
    if numeric {
        for bound in ["minimum", "maximum"] {
            if let Some(value) = map.get(bound)
                && !value.is_number()
            {
                return Err(dialect(
                    proposal,
                    pointer,
                    format!("{bound} must be a number"),
                ));
            }
        }
    }
    Ok(())
}

/// Require an optional schema description to be a string.
fn validate_description(
    map: &Map<String, Value>,
    proposal: &str,
    pointer: &str,
) -> Result<(), SchemaError> {
    if let Some(description) = map.get("description")
        && !description.is_string()
    {
        return Err(dialect(proposal, pointer, "description must be a string"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
