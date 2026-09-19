//! The three interfaces are implementable, and the mechanical recipe defaults
//! hold their contracts (op-id preservation, the ExecutionPlan bind).
#![allow(dead_code)]

use std::sync::Mutex;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::ids::{SceneId, TargetId};
use sergent_rs_core::intent::Intent;
use sergent_rs_core::mindbuf::MindBuf;
use sergent_rs_core::model::{
    ImageError, ImagePart, Message, MessageRole, ModelClient, ModelError, ModelRequest,
    ModelResponse, ModelSettings, ParsedJsonObject, ThinkingEffort,
};
use sergent_rs_core::operation::{Operation, OperationFault};
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::scene::{SceneActions, SceneIdentity, VerificationReport};
use sergent_rs_core::target::Target;

struct Doc;
struct DocMind;
impl MindBuf for DocMind {
    fn export(&self) -> String {
        String::new()
    }
}
#[derive(Serialize)]
struct Edit;
impl Intent for Edit {}
#[derive(Serialize)]
struct Spot {
    id: TargetId,
}
impl Target for Spot {
    fn target_id(&self) -> &TargetId {
        &self.id
    }
}

#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct EditProposal {
    go: bool,
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct Write {
    n: u32,
}
impl Operation for Write {
    type Scene = Doc;
    type Intent = Edit;
    type Target = Spot;
    fn apply(
        &self,
        _scene: &mut Doc,
        _intent: &Edit,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        Ok(())
    }
}

struct DocRecipe;
impl SergentRecipe for DocRecipe {
    type Scene = Doc;
    type MindBuf = DocMind;
    type IntentProposal = EditProposal;
    type Intent = Edit;
    type Target = Spot;
    fn no_target_error(&self) -> RunError {
        RunError::of(ErrorKind::ValidationError, "no target")
    }
    fn derive_intent(
        &self,
        _scene: &Doc,
        _identity: &SceneIdentity,
        _target: &Spot,
        _proposal: &EditProposal,
    ) -> Result<Edit, RunError> {
        Ok(Edit)
    }
}

fn base() -> SceneIdentity {
    SceneIdentity {
        scene_id: SceneId::mint("doc").unwrap(),
        revision: 3,
    }
}

fn spot() -> Spot {
    Spot {
        id: TargetId::mint("spot").unwrap(),
    }
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(object) => object,
        _ => unreachable!("test fixture is an object"),
    }
}

fn proposal(values: &[u32]) -> sergent_rs_core::plan::PlanProposal<Doc, Edit, Spot> {
    let registry = OperationRegistry::builder()
        .register::<Write>("write")
        .unwrap()
        .build(None)
        .unwrap();
    let operations: Vec<Value> = values
        .iter()
        .map(|n| json!({ "call": "write", "n": n }))
        .collect();
    registry
        .decode(&object(json!({ "operations": operations })))
        .unwrap()
}

#[test]
fn compile_patch_preserves_operation_ids_calls_and_count() {
    let plan = proposal(&[1, 2]).bind_to_scene(base());
    let plan_ids: Vec<_> = plan
        .steps()
        .iter()
        .map(|step| step.op_id().clone())
        .collect();
    let plan_calls: Vec<_> = plan.steps().iter().map(|step| step.call()).collect();
    let patch = DocRecipe.compile_patch(&plan).unwrap();
    assert_eq!(patch.base(), plan.base());
    let patch_ids: Vec<_> = patch
        .steps()
        .iter()
        .map(|step| step.op_id().clone())
        .collect();
    assert_eq!(plan_ids, patch_ids);
    let patch_calls: Vec<_> = patch.steps().iter().map(|step| step.call()).collect();
    assert_eq!(plan_calls, patch_calls);
}

#[test]
fn derive_plan_default_binds_the_observed_identity() {
    let proposal = proposal(&[9]);
    let ids: Vec<_> = proposal
        .steps()
        .iter()
        .map(|step| step.op_id().clone())
        .collect();
    let identity = base();
    let plan = DocRecipe
        .derive_plan(&Doc, &identity, &spot(), &Edit, proposal)
        .unwrap();
    assert_eq!(plan.base(), &identity);
    let plan_ids: Vec<_> = plan
        .steps()
        .iter()
        .map(|step| step.op_id().clone())
        .collect();
    assert_eq!(plan_ids, ids);
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct Erase;

impl Operation for Erase {
    type Scene = Doc;
    type Intent = Edit;
    type Target = Spot;

    fn apply(
        &self,
        _scene: &mut Doc,
        _intent: &Edit,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        Ok(())
    }
}

#[test]
fn different_replacement_behavior_inherits_identity_and_fixed_call() {
    let proposal = proposal(&[1]);
    let step = &proposal.steps()[0];
    let replaced = step.with_operation(Erase);

    assert_eq!(step.call(), "write");
    assert_eq!(replaced.op_id(), step.op_id());
    assert_eq!(replaced.call(), step.call());
}

#[derive(Clone, Default)]
struct ObservedDoc {
    nested_lengths: Vec<usize>,
    intent_addresses: Vec<usize>,
}

#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct NestedMutable {
    values: Mutex<Vec<u32>>,
}

impl Operation for NestedMutable {
    type Scene = ObservedDoc;
    type Intent = Edit;
    type Target = Spot;

    fn apply(
        &self,
        scene: &mut ObservedDoc,
        intent: &Edit,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        let mut values = self.values.lock().unwrap();
        scene.nested_lengths.push(values.len());
        scene.intent_addresses.push(intent as *const Edit as usize);
        values.push(99);
        Ok(())
    }
}

struct ObservedActions;

impl SceneActions for ObservedActions {
    type Scene = ObservedDoc;
    type Intent = Edit;
    type Target = Spot;

    fn identity(&self, _scene: &ObservedDoc) -> SceneIdentity {
        base()
    }

    fn clone_scene(&self, scene: &ObservedDoc) -> ObservedDoc {
        scene.clone()
    }

    fn select_target(&self, _scene: &ObservedDoc) -> Option<Spot> {
        Some(spot())
    }

    fn has_target(&self, _scene: &ObservedDoc, _target: &Spot) -> bool {
        true
    }

    fn apply(
        &self,
        scene: &mut ObservedDoc,
        intent: &Edit,
        target: &Spot,
        operations: &[sergent_rs_core::operation::PlanStep<ObservedDoc, Edit, Spot>],
    ) -> Result<(), OperationFault> {
        scene.intent_addresses.push(intent as *const Edit as usize);
        for step in operations {
            step.operation().apply(scene, intent, target)?;
        }
        Ok(())
    }

    fn verify(
        &self,
        _before: &ObservedDoc,
        _after: &ObservedDoc,
        _target: &Spot,
        _operations: &[sergent_rs_core::operation::PlanStep<ObservedDoc, Edit, Spot>],
    ) -> VerificationReport {
        VerificationReport::accepted()
    }
}

#[test]
fn scene_actions_and_every_operation_receive_the_exact_intent_reference() {
    let registry = OperationRegistry::builder()
        .register::<NestedMutable>("nested_mutable")
        .unwrap()
        .build(None)
        .unwrap();
    let proposal = registry
        .decode(&object(json!({
            "operations": [
                { "call": "nested_mutable", "values": [1] },
                { "call": "nested_mutable", "values": [2] }
            ]
        })))
        .unwrap();
    let patch = proposal.bind_to_scene(base()).compile_isolated_patch();
    let intent = Edit;
    let expected = &intent as *const Edit as usize;
    let mut scene = ObservedDoc::default();

    ObservedActions
        .apply(&mut scene, &intent, &spot(), patch.steps())
        .unwrap();

    assert_eq!(scene.intent_addresses, vec![expected; 3]);
}

impl Clone for NestedMutable {
    fn clone(&self) -> Self {
        Self {
            values: Mutex::new(self.values.lock().unwrap().clone()),
        }
    }
}

#[test]
fn mutating_a_patch_operations_nested_state_cannot_affect_the_plan_copy() {
    let registry = OperationRegistry::builder()
        .register::<NestedMutable>("nested_mutable")
        .unwrap()
        .build(None)
        .unwrap();
    let proposal = registry
        .decode(&object(json!({
            "operations": [{ "call": "nested_mutable", "values": [1] }]
        })))
        .unwrap();
    let plan = proposal.bind_to_scene(base());
    let patch = plan.compile_isolated_patch();
    let target = spot();

    let mut patch_scene = ObservedDoc::default();
    patch.steps()[0]
        .operation()
        .apply(&mut patch_scene, &Edit, &target)
        .unwrap();

    let mut plan_scene = ObservedDoc::default();
    plan.steps()[0]
        .operation()
        .apply(&mut plan_scene, &Edit, &target)
        .unwrap();

    assert_eq!(patch_scene.nested_lengths, vec![1]);
    assert_eq!(plan_scene.nested_lengths, vec![1]);
}

#[test]
fn a_recipe_defaults_to_model_backed_intent() {
    assert!(DocRecipe.passthrough_proposal().is_none());
}

// --- ModelClient is implementable with a Send future ---

struct FakeClient;
impl ModelClient for FakeClient {
    async fn invoke(
        &self,
        _request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        Err(ModelError {
            kind: ErrorKind::ProviderError.as_str().to_owned(),
            retryable: false,
            message: "fake".to_owned(),
            raw_output: None,
            identity: None,
            attempts: Vec::new(),
            usage: None,
        })
    }
}

fn assert_is_model_client<M: ModelClient>(_client: &M) {}

#[test]
fn model_client_is_implementable() {
    assert_is_model_client(&FakeClient);
}

// --- model-call value types ---

#[test]
fn model_settings_defaults_match_the_reference_profile() {
    let settings = ModelSettings::default();
    assert_eq!(settings.thinking_effort, ThinkingEffort::High);
    assert_eq!(settings.max_output_tokens.get(), 4096);
    assert_eq!(settings.timeout_secs.get(), 60);
}

#[test]
fn image_part_accepts_a_bounded_png_and_captures_only_metadata() {
    // Base64 of the 8-byte PNG signature.
    let part = ImagePart::png("iVBORw0KGgo=").unwrap();
    assert_eq!(part.media_type(), "image/png");
    assert_eq!(part.decoded_byte_count(), 8);
    assert_eq!(
        serde_json::to_value(&part).unwrap(),
        json!({ "media_type": "image/png", "bytes": 8 })
    );
}

#[test]
fn image_part_rejects_non_base64_and_non_png() {
    assert!(matches!(
        ImagePart::png("not base64 !!"),
        Err(ImageError::NotBase64)
    ));
    // Valid base64 of "hello", which is not PNG.
    assert!(matches!(
        ImagePart::png("aGVsbG8="),
        Err(ImageError::NotPng)
    ));
}

#[test]
fn image_part_rejects_oversize() {
    use base64::Engine;
    let mut bytes = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    bytes.resize(1_000_001, 0);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    assert!(matches!(ImagePart::png(encoded), Err(ImageError::TooLarge)));
}

#[test]
fn message_projections_are_exact_and_images_are_conditional() {
    let text_only = Message::system("hi");
    assert_eq!(text_only.role(), MessageRole::System);
    assert_eq!(text_only.content(), "hi");
    assert!(text_only.images().is_empty());
    assert_eq!(
        serde_json::to_value(&text_only).unwrap(),
        json!({ "role": "system", "content": "hi" })
    );

    let part = ImagePart::png("iVBORw0KGgo=").unwrap();
    let message = Message::user_with_images("look", [part]);
    assert_eq!(message.role(), MessageRole::User);
    assert_eq!(message.content(), "look");
    assert_eq!(message.images().len(), 1);
    assert_eq!(
        serde_json::to_value(&message).unwrap(),
        json!({
            "role": "user",
            "content": "look",
            "images": [{ "media_type": "image/png", "bytes": 8 }]
        })
    );
}
