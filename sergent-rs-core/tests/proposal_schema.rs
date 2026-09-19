//! End-to-end proposal schema derivation: the canonical output for in-profile
//! types, and construction failure for rejected native forms.
//!
//! Fixture fields exist to shape the derived schema, not to be read in Rust.
#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};
use std::num::{
    NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI128, NonZeroIsize, NonZeroU32,
};

use schemars::JsonSchema;
use serde::Deserialize;
use sergent_rs_core::proposal::{SchemaError, derive_proposal_schema};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlaceStone {
    /// Row index on the board.
    row: u32,
    #[schemars(range(min = 0, max = 14))]
    col: i32,
    note: Option<String>,
    stone: Stone,
}

#[derive(Deserialize, JsonSchema)]
enum Stone {
    Black,
    White,
}

#[test]
fn derivation_produces_a_canonical_dialect_schema() {
    let schema = derive_proposal_schema::<PlaceStone>().unwrap();
    assert_eq!(schema.name(), "PlaceStone");
    let doc = schema.json_schema();

    // No metadata leakage.
    assert!(doc.get("$schema").is_none());
    assert!(doc.get("title").is_none());

    // Closed object with every property required.
    assert_eq!(doc["type"], "object");
    assert_eq!(doc["additionalProperties"], false);
    let required: Vec<&str> = doc["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for name in ["row", "col", "note", "stone"] {
        assert!(required.contains(&name), "{name} must be required");
    }

    let props = &doc["properties"];
    // format dropped; unsigned minimum kept; description kept.
    assert_eq!(props["row"]["type"], "integer");
    assert_eq!(props["row"]["minimum"], 0);
    assert!(props["row"].get("format").is_none());
    assert_eq!(props["row"]["description"], "Row index on the board.");

    // Inclusive numeric bounds kept.
    assert_eq!(props["col"]["minimum"], 0);
    assert_eq!(props["col"]["maximum"], 14);

    // Nullable union folded to an anyOf.
    assert_eq!(
        props["note"]["anyOf"],
        serde_json::json!([{ "type": "string" }, { "type": "null" }])
    );

    // The enum lives in $defs and is a string enum.
    assert_eq!(props["stone"]["$ref"], "#/$defs/Stone");
    assert_eq!(doc["$defs"]["Stone"]["type"], "string");
    assert_eq!(
        doc["$defs"]["Stone"]["enum"],
        serde_json::json!(["Black", "White"])
    );
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OpenMap {
    entries: HashMap<String, u32>,
}

#[test]
fn an_open_map_field_fails_construction() {
    assert!(matches!(
        derive_proposal_schema::<OpenMap>(),
        Err(SchemaError::Dialect { .. })
    ));
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ArbitraryJson {
    payload: serde_json::Value,
}

#[test]
fn an_arbitrary_json_field_fails_construction() {
    assert!(matches!(
        derive_proposal_schema::<ArbitraryJson>(),
        Err(SchemaError::Dialect { .. })
    ));
}

#[derive(Deserialize, JsonSchema)]
struct NotClosed {
    n: u32,
}

#[test]
fn a_type_without_deny_unknown_fields_fails_construction() {
    // schemars omits additionalProperties, so the object is not closed.
    assert!(matches!(
        derive_proposal_schema::<NotClosed>(),
        Err(SchemaError::Dialect { .. })
    ));
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Tupled {
    pair: (u32, u32),
}

#[test]
fn a_tuple_shaped_field_fails_construction() {
    // schemars emits prefixItems, which the array dialect does not admit.
    assert!(matches!(
        derive_proposal_schema::<Tupled>(),
        Err(SchemaError::Dialect { .. })
    ));
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Node {
    next: Option<Box<Node>>,
}

#[test]
fn a_recursive_type_fails_construction() {
    assert!(matches!(
        derive_proposal_schema::<Node>(),
        Err(SchemaError::Dialect { .. })
    ));
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AtLeastOne {
    /// A positive count; schemars emits an inclusive minimum of 1 for NonZero.
    count: NonZeroU32,
}

#[test]
fn a_non_zero_integer_stays_in_profile_as_an_inclusive_minimum_of_one() {
    // schemars =1.2.1 emits inclusive "minimum": 1 for NonZero integers, which
    // is in-dialect. This pins that assumption: were a future schemars to emit
    // "exclusiveMinimum" instead, the drop of exclusive bounds is gone, so this
    // construction would fail loudly rather than silently weaken the bound.
    let schema = derive_proposal_schema::<AtLeastOne>().unwrap();
    let count = &schema.json_schema()["properties"]["count"];
    assert_eq!(count["type"], "integer");
    assert_eq!(count["minimum"], 1);
    assert!(count.get("exclusiveMinimum").is_none());
}

macro_rules! signed_non_zero_proposal {
    ($name:ident, $integer:ty) => {
        #[derive(Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct $name {
            count: $integer,
        }
    };
}

signed_non_zero_proposal!(SignedI8, NonZeroI8);
signed_non_zero_proposal!(SignedI16, NonZeroI16);
signed_non_zero_proposal!(SignedI32, NonZeroI32);
signed_non_zero_proposal!(SignedI64, NonZeroI64);
signed_non_zero_proposal!(SignedI128, NonZeroI128);
signed_non_zero_proposal!(SignedIsize, NonZeroIsize);

#[test]
fn every_signed_non_zero_integer_fails_the_existing_dialect() {
    assert!(matches!(
        derive_proposal_schema::<SignedI8>(),
        Err(SchemaError::Dialect { .. })
    ));
    assert!(matches!(
        derive_proposal_schema::<SignedI16>(),
        Err(SchemaError::Dialect { .. })
    ));
    assert!(matches!(
        derive_proposal_schema::<SignedI32>(),
        Err(SchemaError::Dialect { .. })
    ));
    assert!(matches!(
        derive_proposal_schema::<SignedI64>(),
        Err(SchemaError::Dialect { .. })
    ));
    assert!(matches!(
        derive_proposal_schema::<SignedI128>(),
        Err(SchemaError::Dialect { .. })
    ));
    assert!(matches!(
        derive_proposal_schema::<SignedIsize>(),
        Err(SchemaError::Dialect { .. })
    ));
}

// An authored structural constraint the dialect cannot express must now fail
// construction with a located dialect error, never be silently dropped. Each
// authored type below carries one such keyword.

fn assert_rejected_at(error: SchemaError, pointer: &str, keyword: &str) {
    match error {
        SchemaError::Dialect {
            pointer: at,
            message,
            ..
        } => {
            assert_eq!(at, pointer, "rejection pointer");
            assert!(
                message.contains(keyword),
                "message {message:?} must name {keyword:?}"
            );
        }
        other => panic!("expected a dialect rejection, got {other:?}"),
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Patterned {
    #[schemars(regex(pattern = r"^[a-z]+$"))]
    code: String,
}

#[test]
fn an_authored_string_pattern_fails_construction_with_a_pointer() {
    assert_rejected_at(
        derive_proposal_schema::<Patterned>().unwrap_err(),
        "/properties/code",
        "pattern",
    );
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MinLength {
    #[schemars(length(min = 1))]
    label: String,
}

#[test]
fn an_authored_min_length_fails_construction_with_a_pointer() {
    assert_rejected_at(
        derive_proposal_schema::<MinLength>().unwrap_err(),
        "/properties/label",
        "minLength",
    );
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MultipleOf {
    #[schemars(extend("multipleOf" = 2))]
    step: u32,
}

#[test]
fn an_authored_multiple_of_fails_construction_with_a_pointer() {
    assert_rejected_at(
        derive_proposal_schema::<MultipleOf>().unwrap_err(),
        "/properties/step",
        "multipleOf",
    );
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UniqueTags {
    tags: BTreeSet<u32>,
}

#[test]
fn an_authored_unique_items_fails_construction_with_a_pointer() {
    assert_rejected_at(
        derive_proposal_schema::<UniqueTags>().unwrap_err(),
        "/properties/tags",
        "uniqueItems",
    );
}
