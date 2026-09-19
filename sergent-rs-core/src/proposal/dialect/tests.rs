use super::*;
use serde_json::json;

/// Construct the smallest representative closed object used by dialect tests.
fn valid_object() -> Value {
    json!({
        "type": "object",
        "properties": { "n": { "type": "integer", "minimum": 0, "maximum": 9 } },
        "additionalProperties": false,
        "required": ["n"]
    })
}

/// Assert that the closed dialect rejects a schema.
fn assert_rejected(schema: Value) {
    assert!(matches!(
        validate_dialect(&schema, "P"),
        Err(SchemaError::Dialect { .. })
    ));
}

#[test]
fn a_conforming_schema_passes() {
    assert!(validate_dialect(&valid_object(), "P").is_ok());
}

#[test]
fn a_non_object_root_is_rejected() {
    assert_rejected(json!(["not", "a", "schema"]));
}

#[test]
fn a_nested_reference_that_resolves_passes() {
    let schema = json!({
        "type": "object",
        "properties": { "stone": { "$ref": "#/$defs/Stone" } },
        "additionalProperties": false,
        "required": ["stone"],
        "$defs": { "Stone": { "type": "string", "enum": ["black", "white"] } }
    });
    assert!(validate_dialect(&schema, "P").is_ok());
}

#[test]
fn nested_any_of_and_object_properties_pass() {
    let schema = json!({
        "type": "object",
        "properties": {
            "choice": {
                "anyOf": [
                    { "type": "null" },
                    {
                        "type": "object",
                        "properties": {
                            "value": {
                                "anyOf": [
                                    { "type": "string" },
                                    { "type": "integer" }
                                ]
                            }
                        },
                        "additionalProperties": false,
                        "required": ["value"]
                    }
                ]
            }
        },
        "additionalProperties": false,
        "required": ["choice"]
    });
    assert!(validate_dialect(&schema, "P").is_ok());
}

#[test]
fn a_root_reference_is_rejected() {
    assert_rejected(json!({ "$ref": "#/$defs/X", "$defs": { "X": valid_object() } }));
}

#[test]
fn a_root_anyof_is_rejected() {
    assert_rejected(json!({ "anyOf": [valid_object()] }));
}

#[test]
fn root_definitions_must_be_a_non_empty_object() {
    let mut non_object = valid_object();
    non_object["$defs"] = json!(["bad"]);
    assert_rejected(non_object);

    let mut empty = valid_object();
    empty["$defs"] = json!({});
    assert_rejected(empty);
}

#[test]
fn an_object_without_additional_properties_false_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "n": { "type": "integer" } },
        "required": ["n"]
    }));
}

#[test]
fn an_open_object_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "n": { "type": "integer" } },
        "additionalProperties": true,
        "required": ["n"]
    }));
}

#[test]
fn required_must_list_every_property() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "a": { "type": "integer" }, "b": { "type": "integer" } },
        "additionalProperties": false,
        "required": ["a"]
    }));
}

#[test]
fn a_reference_node_with_a_sibling_keyword_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "s": { "$ref": "#/$defs/S", "description": "d" } },
        "additionalProperties": false,
        "required": ["s"],
        "$defs": { "S": { "type": "string" } }
    }));
}

#[test]
fn an_unresolved_reference_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "s": { "$ref": "#/$defs/Missing" } },
        "additionalProperties": false,
        "required": ["s"]
    }));
}

#[test]
fn an_unknown_keyword_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "n": { "type": "integer" } },
        "additionalProperties": false,
        "required": ["n"],
        "patternProperties": {}
    }));
}

#[test]
fn admitted_keywords_reject_wrong_json_value_types() {
    let mut bad_description = valid_object();
    bad_description["description"] = json!(7);
    assert_rejected(bad_description);

    let mut bad_properties = valid_object();
    bad_properties["properties"] = json!([]);
    assert_rejected(bad_properties);

    let mut bad_required = valid_object();
    bad_required["required"] = json!("n");
    assert_rejected(bad_required);

    let mut bad_required_member = valid_object();
    bad_required_member["required"] = json!([1]);
    assert_rejected(bad_required_member);

    let mut bad_definition_member = valid_object();
    bad_definition_member["$defs"] = json!({ "N": 1 });
    assert_rejected(bad_definition_member);

    let mut bad_items = valid_object();
    bad_items["properties"]["n"] = json!({ "type": "array", "items": 1 });
    assert_rejected(bad_items);

    let mut bad_array_bound = valid_object();
    bad_array_bound["properties"]["n"] =
        json!({ "type": "array", "items": { "type": "integer" }, "minItems": "1" });
    assert_rejected(bad_array_bound);

    let mut bad_numeric_bound = valid_object();
    bad_numeric_bound["properties"]["n"]["minimum"] = json!("zero");
    assert_rejected(bad_numeric_bound);

    let mut bad_enum = valid_object();
    bad_enum["properties"]["n"] = json!({ "type": "integer", "enum": "one" });
    assert_rejected(bad_enum);

    let mut bad_enum_member = valid_object();
    bad_enum_member["properties"]["n"] = json!({ "type": "integer", "enum": [[1]] });
    assert_rejected(bad_enum_member);

    let mut mismatched_enum_member = valid_object();
    mismatched_enum_member["properties"]["n"] = json!({ "type": "integer", "enum": ["one"] });
    assert_rejected(mismatched_enum_member);

    let mut bad_any_of = valid_object();
    bad_any_of["properties"]["n"] = json!({ "anyOf": "integer" });
    assert_rejected(bad_any_of);

    let mut bad_any_of_member = valid_object();
    bad_any_of_member["properties"]["n"] = json!({ "anyOf": [1] });
    assert_rejected(bad_any_of_member);

    let mut bad_reference = valid_object();
    bad_reference["properties"]["n"] = json!({ "$ref": 1 });
    assert_rejected(bad_reference);
}

#[test]
fn exclusive_bounds_are_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "n": { "type": "integer", "exclusiveMinimum": 0 } },
        "additionalProperties": false,
        "required": ["n"]
    }));
}

#[test]
fn a_type_array_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "n": { "type": ["integer", "null"] } },
        "additionalProperties": false,
        "required": ["n"]
    }));
}

#[test]
fn an_object_valued_enum_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "n": { "type": "string", "enum": [{ "k": 1 }] } },
        "additionalProperties": false,
        "required": ["n"]
    }));
}

#[test]
fn an_array_without_items_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "xs": { "type": "array", "minItems": 1 } },
        "additionalProperties": false,
        "required": ["xs"]
    }));
}

#[test]
fn a_recursive_reference_is_rejected() {
    assert_rejected(json!({
        "type": "object",
        "properties": { "child": { "$ref": "#/$defs/Node" } },
        "additionalProperties": false,
        "required": ["child"],
        "$defs": {
            "Node": {
                "type": "object",
                "properties": { "next": { "$ref": "#/$defs/Node" } },
                "additionalProperties": false,
                "required": ["next"]
            }
        }
    }));
}
