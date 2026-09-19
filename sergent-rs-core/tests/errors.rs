//! Exact structured merge-conflict error construction.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};
use sergent_rs_core::error::RunError;
use sergent_rs_core::ids::{SceneId, TargetId};
use sergent_rs_core::intent::Intent;
use sergent_rs_core::operation::{Operation, OperationFault};
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::run_record::PatchSummary;
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::target::Target;
use sergent_rs_core::vocab::Stage;

struct Doc;

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

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct Append {
    text: String,
}

impl Operation for Append {
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
fn scene_conflict_keeps_required_original_patch_facts_and_scene_metadata() {
    let summary = summary("original");
    let scene_metadata = Map::from_iter([
        ("reason".to_owned(), json!("human_owned")),
        ("patch".to_owned(), json!("scene_fact")),
    ]);
    let error = RunError::merge_conflict(
        "scene rejected overlap",
        3,
        4,
        &summary,
        scene_metadata.clone(),
    );

    assert_eq!(error.kind, "merge_conflict");
    assert_eq!(
        error.metadata,
        Map::from_iter([
            ("base_revision".to_owned(), json!(3)),
            ("current_live_revision".to_owned(), json!(4)),
            ("patch".to_owned(), serde_json::to_value(summary).unwrap(),),
            ("scene_metadata".to_owned(), json!(scene_metadata)),
        ])
    );
}

#[test]
fn rebased_conflict_keeps_mapping_and_complete_validation_error() {
    let summary = summary("replacement");
    let validation = RunError::new("application_fault", "replacement rejected")
        .with("domain_fact", "exact")
        .with("scene_metadata", "check_fact");
    let scene_metadata = Map::from_iter([
        ("resolution".to_owned(), json!("human_wins")),
        ("validation_error".to_owned(), json!("scene_fact")),
    ]);
    let error =
        RunError::rebased_merge_conflict(3, 4, &summary, scene_metadata.clone(), validation);

    assert_eq!(error.metadata.len(), 5);
    assert_eq!(error.metadata["base_revision"], 3);
    assert_eq!(error.metadata["current_live_revision"], 4);
    assert_eq!(error.metadata["scene_metadata"], json!(scene_metadata));
    assert_eq!(
        error.metadata["validation_error"],
        json!({
            "kind": "application_fault",
            "message": "replacement rejected",
            "metadata": {
                "domain_fact": "exact",
                "scene_metadata": "check_fact"
            }
        })
    );
    assert_eq!(
        error.metadata["patch"],
        serde_json::to_value(summary).unwrap()
    );
}

#[test]
fn reserved_validation_error_shapes_are_exact() {
    let summary = summary("operation");
    let operation_id = &summary.operation_ids()[0];
    let admissibility = RunError::admissibility("operation rejected", 2, "append", operation_id);
    assert_eq!(
        admissibility.metadata,
        Map::from_iter([
            ("index".to_owned(), json!(2)),
            ("call".to_owned(), json!("append")),
            ("operation_id".to_owned(), json!(operation_id.as_str()),),
        ])
    );

    let verification = RunError::verification("invalid Scene", ["issue".to_owned()]);
    assert_eq!(
        verification.metadata,
        Map::from_iter([("verification_issues".to_owned(), json!(["issue"]))])
    );
}

#[test]
fn reserved_patch_stale_revision_and_cancelled_shapes_are_exact() {
    let expected = identity(3);
    let actual = identity(4);
    let embedded = RunError::embedded_identity(
        "identity drift",
        ["revision mismatch".to_owned()],
        &expected,
        &actual,
    );
    assert_eq!(
        embedded.metadata,
        Map::from_iter([
            ("identity_issues".to_owned(), json!(["revision mismatch"]),),
            ("expected_identity".to_owned(), json!(expected)),
            ("actual_identity".to_owned(), json!(actual)),
        ])
    );
    assert!(RunError::patch_validation("bad Patch").metadata.is_empty());
    assert_eq!(
        RunError::stale_patch("stale", 3, 4).metadata,
        Map::from_iter([
            ("base_revision".to_owned(), json!(3)),
            ("current_revision".to_owned(), json!(4)),
        ])
    );
    let exhausted = RunError::revision_exhausted("exhausted", &expected.scene_id, u64::MAX);
    assert_eq!(exhausted.metadata["revision"], u64::MAX);
    assert!(RunError::cancelled("cancelled").metadata.is_empty());
}

#[test]
fn observer_error_shape_is_exact_and_message_is_bounded() {
    let error = RunError::observer_error(
        "x".repeat(2_049),
        "progress",
        "Observer",
        "ReturnedError",
        Stage::Commit,
    );

    assert_eq!(error.message.chars().count(), 2_048);
    assert_eq!(
        error.metadata,
        Map::from_iter([
            ("callback".to_owned(), json!("progress")),
            ("observer_type".to_owned(), json!("Observer")),
            ("exception_type".to_owned(), json!("ReturnedError")),
            ("stage".to_owned(), json!("commit")),
        ])
    );
}

fn summary(text: &str) -> PatchSummary {
    let registry = OperationRegistry::builder()
        .register::<Append>("append")
        .unwrap()
        .build(None)
        .unwrap();
    let proposal = registry
        .decode(
            json!({ "operations": [{ "call": "append", "text": text }] })
                .as_object()
                .unwrap(),
        )
        .unwrap();
    let patch = proposal
        .bind_to_scene(SceneIdentity {
            scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
            revision: 3,
        })
        .compile_isolated_patch();
    PatchSummary::from_patch(&patch)
}

fn identity(revision: u64) -> SceneIdentity {
    SceneIdentity {
        scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
        revision,
    }
}
