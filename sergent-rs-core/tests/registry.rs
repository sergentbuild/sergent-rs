//! Operation registry composition and the two-stage typed decode.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sergent_rs_core::ids::TargetId;
use sergent_rs_core::intent::Intent;
use sergent_rs_core::operation::{Operation, OperationFault};
use sergent_rs_core::registry::{
    DecodeError, OperationRegistry, OperationRegistryBuilder, RegistryError,
};
use sergent_rs_core::target::Target;

struct Board;
#[derive(Serialize)]
struct Move;
impl Intent for Move {}
#[derive(Serialize)]
struct Cell {
    id: TargetId,
}
impl Target for Cell {
    fn target_id(&self) -> &TargetId {
        &self.id
    }
}

type Reg = OperationRegistry<Board, Move, Cell>;
type Builder = OperationRegistryBuilder<Board, Move, Cell>;
fn builder() -> Builder {
    OperationRegistry::<Board, Move, Cell>::builder()
}

// The Ok types hold trait objects and are not Debug, so unwrap_err cannot print
// them; these helpers extract the error without a Debug bound.
fn register_err(result: Result<Builder, RegistryError>) -> RegistryError {
    match result {
        Ok(_) => panic!("expected a registration error"),
        Err(error) => error,
    }
}
fn build_err(result: Result<Reg, RegistryError>) -> RegistryError {
    match result {
        Ok(_) => panic!("expected a build error"),
        Err(error) => error,
    }
}

macro_rules! op {
    ($name:ident, { $($field:tt)* }) => {
        #[derive(Clone, Deserialize, JsonSchema, Serialize)]
        #[serde(deny_unknown_fields)]
        struct $name { $($field)* }
        impl Operation for $name {
            type Scene = Board;
            type Intent = Move;
            type Target = Cell;
            fn apply(
                &self,
                _s: &mut Board,
                _intent: &Move,
                _t: &Cell,
            ) -> Result<(), OperationFault> { Ok(()) }
        }
    };
}

op!(Place, { row: u32, col: u32 });
op!(Remove, { row: u32, col: u32 });

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
enum Color {
    Black,
    White,
}
op!(Mark, { stone: Color });
op!(Paint, { stone: Color });

mod left {
    use super::*;
    #[derive(Clone, Deserialize, JsonSchema, Serialize)]
    #[serde(deny_unknown_fields)]
    pub struct Thing {
        pub x: u32,
    }
}
mod right {
    use super::*;
    #[derive(Clone, Deserialize, JsonSchema, Serialize)]
    #[serde(deny_unknown_fields)]
    pub struct Thing {
        pub y: bool,
    }
}
op!(UseLeft, { t: left::Thing });
op!(UseRight, { t: right::Thing });

// A verb that conflicts with Place on the discriminator.
op!(PlaceAgain, { row: u32, col: u32 });
op!(EmptyCall, { value: u32 });
op!(ReservedCall, { call: String });

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
struct ScalarRoot(String);

impl Operation for ScalarRoot {
    type Scene = Board;
    type Intent = Move;
    type Target = Cell;

    fn apply(&self, _s: &mut Board, _intent: &Move, _t: &Cell) -> Result<(), OperationFault> {
        Ok(())
    }
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(extend("$defs" = true))]
struct MalformedDefs {}

impl Operation for MalformedDefs {
    type Scene = Board;
    type Intent = Move;
    type Target = Cell;

    fn apply(&self, _s: &mut Board, _intent: &Move, _t: &Cell) -> Result<(), OperationFault> {
        Ok(())
    }
}

// --- composition ---

#[test]
fn the_envelope_is_named_plan_proposal_with_a_bounded_operations_array() {
    let registry: Reg = builder()
        .register::<Place>("place")
        .unwrap()
        .build(Some(3))
        .unwrap();
    let schema = registry.plan_schema();
    assert_eq!(schema.name(), "PlanProposal");
    let ops = &schema.json_schema()["properties"]["operations"];
    assert_eq!(ops["type"], "array");
    assert_eq!(ops["minItems"], 1);
    assert_eq!(ops["maxItems"], 3);
    assert_eq!(schema.json_schema()["required"], json!(["operations"]));
}

#[test]
fn one_branch_uses_a_single_ref_and_injects_the_call_discriminator() {
    let registry: Reg = builder()
        .register::<Place>("place")
        .unwrap()
        .build(None)
        .unwrap();
    let schema = registry.plan_schema();
    let doc = schema.json_schema();
    assert_eq!(
        doc["properties"]["operations"]["items"]["$ref"],
        "#/$defs/Place"
    );
    let place = &doc["$defs"]["Place"];
    assert_eq!(
        place["properties"]["call"],
        json!({ "type": "string", "enum": ["place"] })
    );
    let required = place["required"].as_array().unwrap();
    assert!(required.contains(&json!("call")));
    assert!(doc["properties"]["operations"].get("maxItems").is_none());
}

#[test]
fn multiple_branches_form_an_anyof_in_registration_order() {
    let registry: Reg = builder()
        .register::<Place>("place")
        .unwrap()
        .register::<Remove>("remove")
        .unwrap()
        .build(None)
        .unwrap();
    let schema = registry.plan_schema();
    let items = &schema.json_schema()["properties"]["operations"]["items"];
    assert_eq!(
        items["anyOf"],
        json!([{ "$ref": "#/$defs/Place" }, { "$ref": "#/$defs/Remove" }])
    );
}

#[test]
fn an_equal_shared_definition_is_lifted_once() {
    let registry: Reg = builder()
        .register::<Mark>("mark")
        .unwrap()
        .register::<Paint>("paint")
        .unwrap()
        .build(None)
        .unwrap();
    let schema = registry.plan_schema();
    let defs = schema.json_schema()["$defs"].as_object().unwrap().clone();
    assert!(defs.contains_key("Color"));
    assert!(defs.contains_key("Mark"));
    assert!(defs.contains_key("Paint"));
    // Exactly one Color definition is shared.
    assert_eq!(defs.keys().filter(|k| *k == "Color").count(), 1);
}

#[test]
fn a_duplicate_discriminator_names_both_types() {
    let error = register_err(
        builder()
            .register::<Place>("place")
            .unwrap()
            .register::<PlaceAgain>("place"),
    );
    match error {
        RegistryError::DuplicateDiscriminator {
            call,
            first,
            second,
        } => {
            assert_eq!(call, "place");
            assert!(first.ends_with("Place"));
            assert!(second.ends_with("PlaceAgain"));
        }
        other => panic!("expected duplicate discriminator, got {other:?}"),
    }
}

#[test]
fn bad_wiring_reports_exact_error_and_operation_type() {
    #[derive(Debug)]
    enum Expected {
        EmptyDiscriminator,
        BranchNotObject,
        ReservedCallField,
    }

    let cases = [
        (
            Expected::EmptyDiscriminator,
            builder().register::<EmptyCall>(""),
            std::any::type_name::<EmptyCall>(),
        ),
        (
            Expected::BranchNotObject,
            builder().register::<ScalarRoot>("scalar_root"),
            std::any::type_name::<ScalarRoot>(),
        ),
        (
            Expected::ReservedCallField,
            builder().register::<ReservedCall>("reserved_call"),
            std::any::type_name::<ReservedCall>(),
        ),
    ];

    for (expected, result, expected_type_name) in cases {
        let error = register_err(result);
        let actual_type_name = match (expected, error) {
            (Expected::EmptyDiscriminator, RegistryError::EmptyDiscriminator { type_name })
            | (Expected::BranchNotObject, RegistryError::BranchNotObject { type_name })
            | (Expected::ReservedCallField, RegistryError::ReservedCallField { type_name }) => {
                type_name
            }
            (expected, error) => panic!("expected {expected:?}, got {error:?}"),
        };
        assert_eq!(actual_type_name, expected_type_name);
    }
}

#[test]
fn a_named_definition_collision_fails_construction() {
    let error = build_err(
        builder()
            .register::<UseLeft>("use_left")
            .unwrap()
            .register::<UseRight>("use_right")
            .unwrap()
            .build(None),
    );
    assert!(matches!(
        error,
        RegistryError::DefinitionCollision { ref name, .. } if name == "Thing"
    ));
}

#[test]
fn an_empty_registry_fails_construction() {
    assert!(matches!(
        builder().build(None),
        Err(RegistryError::EmptyRegistry)
    ));
}

#[test]
fn a_zero_maximum_fails_construction() {
    let error = build_err(builder().register::<Place>("place").unwrap().build(Some(0)));
    assert!(matches!(error, RegistryError::InvalidMaximum { max: 0 }));
}

#[test]
fn a_non_object_root_defs_is_rejected_during_registration() {
    let error = register_err(builder().register::<MalformedDefs>("malformed_defs"));
    assert!(matches!(
        error,
        RegistryError::Schema(sergent_rs_core::proposal::SchemaError::Dialect {
            ref pointer,
            ..
        }) if pointer == "/$defs"
    ));
}

// --- two-stage decode ---

fn place_registry() -> Reg {
    builder()
        .register::<Place>("place")
        .unwrap()
        .register::<Remove>("remove")
        .unwrap()
        .build(None)
        .unwrap()
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(object) => object,
        _ => unreachable!("test fixture is an object"),
    }
}

#[test]
fn a_well_formed_envelope_decodes_and_mints_operation_ids() {
    let registry = place_registry();
    let envelope = object(json!({ "operations": [
        { "call": "place", "row": 1, "col": 2 },
        { "call": "remove", "row": 3, "col": 4 }
    ] }));
    let proposal = registry.decode(&envelope).unwrap();
    assert_eq!(proposal.steps().len(), 2);
    for step in proposal.steps() {
        assert!(step.op_id().as_str().starts_with("op_"));
    }
    assert_ne!(proposal.steps()[0].op_id(), proposal.steps()[1].op_id());
    // Each step carries the registered branch's fixed call discriminator.
    assert_eq!(proposal.steps()[0].call(), "place");
    assert_eq!(proposal.steps()[1].call(), "remove");
    assert_eq!(
        serde_json::to_value(proposal.steps()[0].op_id()).unwrap(),
        json!(proposal.steps()[0].op_id().as_str())
    );
}

fn decode_err(envelope: serde_json::Value) -> DecodeError {
    match place_registry().decode(&object(envelope)) {
        Ok(_) => panic!("expected a decode error"),
        Err(error) => error,
    }
}

#[test]
fn an_unknown_field_is_rejected() {
    let err =
        decode_err(json!({ "operations": [{ "call": "place", "row": 1, "col": 2, "extra": 9 }] }));
    assert!(matches!(err, DecodeError::InvalidPayload { index: 0, .. }));
}

#[test]
fn a_missing_operations_field_is_rejected() {
    assert_eq!(decode_err(json!({})), DecodeError::MissingOperations);
}

#[test]
fn a_non_array_operations_field_is_rejected() {
    assert_eq!(
        decode_err(json!({ "operations": {} })),
        DecodeError::OperationsNotArray
    );
}

#[test]
fn a_non_object_operation_names_its_index() {
    assert_eq!(
        decode_err(json!({ "operations": [1] })),
        DecodeError::MalformedOperation { index: 0 }
    );
}

#[test]
fn a_non_string_call_names_its_index() {
    assert_eq!(
        decode_err(json!({ "operations": [{ "call": 1 }] })),
        DecodeError::CallNotString { index: 0 }
    );
}

#[test]
fn invalid_payload_diagnostics_escape_controls_and_are_bounded() {
    let hostile_field = format!("\u{1b}[2J{}", "x".repeat(1024));
    let mut operation = object(json!({ "call": "place", "row": 1, "col": 2 }));
    operation.insert(hostile_field, json!(true));
    let error = decode_err(json!({ "operations": [Value::Object(operation)] }));

    match error {
        DecodeError::InvalidPayload {
            index,
            call,
            message,
        } => {
            assert_eq!(index, 0);
            assert_eq!(call, "place");
            assert!(message.contains(r"\u{1b}[2J"));
            assert!(!message.chars().any(char::is_control));
            assert!(message.chars().count() <= 256);
        }
        other => panic!("expected invalid payload, got {other:?}"),
    }
}

#[test]
fn a_missing_required_field_is_rejected() {
    let err = decode_err(json!({ "operations": [{ "call": "place", "row": 1 }] }));
    assert!(matches!(err, DecodeError::InvalidPayload { index: 0, .. }));
}

#[test]
fn a_wrong_scalar_type_is_rejected() {
    let err = decode_err(json!({ "operations": [{ "call": "place", "row": "x", "col": 2 }] }));
    assert!(matches!(err, DecodeError::InvalidPayload { index: 0, .. }));
}

#[test]
fn an_echoed_operation_id_is_rejected_as_an_unknown_field() {
    let err = decode_err(
        json!({ "operations": [{ "call": "place", "row": 1, "col": 2, "op_id": "op_0" }] }),
    );
    assert!(matches!(err, DecodeError::InvalidPayload { index: 0, .. }));
}

#[test]
fn an_echoed_top_level_run_id_is_rejected() {
    let err = decode_err(
        json!({ "operations": [{ "call": "place", "row": 1, "col": 2 }], "run_id": "run_0" }),
    );
    assert!(matches!(err, DecodeError::UnknownEnvelopeField { ref field } if field == "run_id"));
}

#[test]
fn an_unknown_call_is_rejected() {
    let err = decode_err(json!({ "operations": [{ "call": "teleport", "row": 1, "col": 2 }] }));
    assert!(matches!(err, DecodeError::UnknownCall { index: 0, ref call } if call == "teleport"));
}

#[test]
fn a_missing_call_is_rejected() {
    let err = decode_err(json!({ "operations": [{ "row": 1, "col": 2 }] }));
    assert!(matches!(err, DecodeError::MissingCall { index: 0 }));
}

#[test]
fn an_empty_operations_list_is_rejected() {
    let err = decode_err(json!({ "operations": [] }));
    assert!(matches!(err, DecodeError::EmptyOperations));
}

#[test]
fn the_maximum_is_re_enforced_at_the_crossing() {
    let registry: Reg = builder()
        .register::<Place>("place")
        .unwrap()
        .build(Some(1))
        .unwrap();
    let envelope = object(json!({ "operations": [
        { "call": "place", "row": 1, "col": 2 },
        { "call": "place", "row": 3, "col": 4 }
    ] }));
    let error = match registry.decode(&envelope) {
        Ok(_) => panic!("expected a decode error"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        DecodeError::TooManyOperations { max: 1, actual: 2 }
    ));
}
