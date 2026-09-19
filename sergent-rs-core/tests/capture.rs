//! Captured-value containment and concrete Operation evidence projection.

use schemars::JsonSchema;
use serde::ser::Error as _;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value, json};

use sergent_rs_core::ids::{SceneId, TargetId};
use sergent_rs_core::intent::Intent;
use sergent_rs_core::operation::{Operation, OperationFault};
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::run_record::{CapturedValue, PatchSummary};
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::target::Target;

#[derive(Serialize)]
struct Edit;

impl Intent for Edit {}

#[derive(Serialize)]
struct Spot(TargetId);

impl Target for Spot {
    fn target_id(&self) -> &TargetId {
        &self.0
    }
}

struct Document;

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct Write {
    text: String,
}

impl Operation for Write {
    type Scene = Document;
    type Intent = Edit;
    type Target = Spot;

    fn apply(
        &self,
        _scene: &mut Document,
        _intent: &Edit,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        Ok(())
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Uncapturable {
    ignored: bool,
}

impl Serialize for Uncapturable {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        let _ = self.ignored;
        Err(S::Error::custom("projection refused"))
    }
}

impl Operation for Uncapturable {
    type Scene = Document;
    type Intent = Edit;
    type Target = Spot;

    fn apply(
        &self,
        _scene: &mut Document,
        _intent: &Edit,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        Ok(())
    }
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().expect("test object").clone()
}

fn registry() -> OperationRegistry<Document, Edit, Spot> {
    OperationRegistry::builder()
        .register::<Write>("write")
        .unwrap()
        .register::<Uncapturable>("uncapturable")
        .unwrap()
        .build(None)
        .unwrap()
}

#[test]
fn captured_value_contains_projection_failure_in_the_exact_envelope() {
    let capture = CapturedValue::capture(&Uncapturable { ignored: true });
    let serialized = serde_json::to_value(&capture).unwrap();
    let value_type = std::any::type_name::<Uncapturable>();

    assert_eq!(serialized["value"], Value::Null);
    assert_eq!(serialized["value_type"], value_type);
    assert_eq!(serialized["status"], "capture_error");
    assert_eq!(serialized["error"]["kind"], "capture_error");
    assert_eq!(
        serialized["error"]["metadata"],
        json!({ "value_type": value_type })
    );
    assert!(
        serialized["error"]["message"]
            .as_str()
            .unwrap()
            .contains("projection refused")
    );
}

#[test]
fn successful_captured_value_serializes_the_exact_envelope() {
    let value = Write {
        text: "hello".to_owned(),
    };
    let value_type = std::any::type_name::<Write>();

    assert_eq!(
        serde_json::to_value(CapturedValue::capture(&value)).unwrap(),
        json!({
            "value": { "text": "hello" },
            "value_type": value_type,
            "error": null,
            "status": "captured"
        })
    );
}

#[test]
fn operation_projections_preserve_concrete_fields_at_their_exact_owner() {
    let proposal = registry()
        .decode(&object(json!({
            "operations": [{ "call": "write", "text": "hello" }]
        })))
        .unwrap();
    let operation = serde_json::to_value(proposal.steps()[0].captured_operation()).unwrap();
    assert_eq!(operation["value"], json!({ "text": "hello" }));
    assert!(operation["value"].get("call").is_none());
    assert!(operation["value"].get("op_id").is_none());

    let proposal_capture = serde_json::to_value(proposal.captured_value()).unwrap();
    assert_eq!(
        proposal_capture["value"],
        json!({ "operations": [{ "call": "write", "text": "hello" }] })
    );

    let plan = proposal.bind_to_scene(SceneIdentity {
        scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
        revision: 7,
    });
    let plan_capture = serde_json::to_value(plan.captured_value()).unwrap();
    assert_eq!(plan_capture["value"]["steps"], json!([{ "text": "hello" }]));
    assert!(plan_capture["value"]["steps"][0].get("call").is_none());
    assert!(plan_capture["value"]["steps"][0].get("op_id").is_none());
}

#[test]
fn operation_projection_failure_stays_local_to_its_capture() {
    let proposal = registry()
        .decode(&object(json!({
            "operations": [{ "call": "uncapturable", "ignored": true }]
        })))
        .unwrap();
    let capture = serde_json::to_value(proposal.steps()[0].captured_operation()).unwrap();

    assert_eq!(capture["status"], "capture_error");
    assert_eq!(capture["value"], Value::Null);
    assert_eq!(capture["error"]["kind"], "capture_error");
}

#[test]
fn patch_summary_arrays_align_and_operation_capture_failure_stays_local() {
    let proposal = registry()
        .decode(&object(json!({
            "operations": [
                { "call": "write", "text": "hello" },
                { "call": "uncapturable", "ignored": true }
            ]
        })))
        .unwrap();
    let patch = proposal
        .bind_to_scene(SceneIdentity {
            scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
            revision: 7,
        })
        .compile_isolated_patch();
    let summary = PatchSummary::from_patch(&patch);
    let value = serde_json::to_value(&summary).unwrap();

    assert_eq!(summary.operation_count(), 2);
    assert_eq!(summary.operation_ids(), summary.operation_trace_ids());
    assert_eq!(summary.operation_call_names(), ["write", "uncapturable"]);
    assert_eq!(value["operations"][0]["status"], "captured");
    assert_eq!(value["operations"][0]["value"], json!({ "text": "hello" }));
    assert_eq!(value["operations"][1]["status"], "capture_error");
    assert_eq!(value["operations"][1]["value"], Value::Null);
    assert_eq!(value["operation_count"], 2);
    assert_eq!(value["operation_ids"], value["operation_trace_ids"]);
}

#[test]
fn serialization_contract_keeps_operation_object_safe() {
    fn assert_serialize<T: Serialize>() {}
    assert_serialize::<Edit>();
    assert_serialize::<Spot>();
    assert_serialize::<Write>();

    let proposal = registry()
        .decode(&object(json!({
            "operations": [{ "call": "write", "text": "hello" }]
        })))
        .unwrap();
    let _: &dyn Operation<Scene = Document, Intent = Edit, Target = Spot> =
        proposal.steps()[0].operation();
}
