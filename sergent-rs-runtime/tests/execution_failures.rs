//! Public-flow coverage for Patch compilation, envelope validation, apply
//! faults, and Scene verification rejection.

mod harness;

use std::sync::Arc;

use harness::*;
use serde_json::{Map, json};
use sergent_rs_core::operation::OperationFault;
use sergent_rs_core::vocab::{RunStepName, RunStepStatus, Stage, TerminalStatus};
use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::{SceneSource, SceneState};
use sergent_rs_runtime::sergent::Sergent;

async fn assert_plain_and_live_failure(
    recipe: PassThroughRecipe,
    actions: DocActions,
    expected_stage: Stage,
    expected_kind: &str,
    expected_trace_len: usize,
) {
    let sergent = Sergent::new(
        configured_pass_through(recipe),
        actions,
        CannedClient::new(intent_proposal_json(), plan_envelope(&[" changed"])),
    )
    .unwrap();
    let initial = doc("keep", 7);
    let cancel = CancelToken::new();
    let observers: [&dyn RunObserver<Doc>; 0] = [];

    let plain = sergent
        .run(
            SceneSource::plain(initial.clone()),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;
    assert_failed_run(&plain, expected_stage, expected_kind, expected_trace_len);

    let state = SceneState::new(initial.clone(), scene_identity(&initial));
    let live = sergent
        .run(
            SceneSource::Live(state.clone()),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;
    assert_failed_run(&live, expected_stage, expected_kind, expected_trace_len);
    let (authoritative, identity) = state.snapshot(&DocActions::new());
    assert_eq!(authoritative.text, "keep");
    assert_eq!(authoritative.revision, 7);
    assert_eq!(identity.revision, 7);
}

fn assert_failed_run(
    result: &sergent_rs_core::run_record::SergentResult<Doc>,
    expected_stage: Stage,
    expected_kind: &str,
    expected_trace_len: usize,
) {
    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), expected_stage);
    assert_eq!(result.error().unwrap().kind, expected_kind);
    assert_eq!(result.scene().text, "keep");
    assert_eq!(result.scene().revision, 7);

    let record = result.run_record();
    assert_no_commit_revision(record);
    assert_eq!(record.outcome().error().unwrap().kind, expected_kind);
    let patch = record
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Patch)
        .unwrap();
    assert!(matches!(patch.status(), RunStepStatus::Failure { .. }));
    let trace_len = if patch.output().is_some() {
        patch_operation_trace_ids(patch).len()
    } else {
        0
    };
    assert_eq!(trace_len, expected_trace_len);
    let _ = record.timing().finished_at();
    let _ = patch.timing().finished_at();
}

#[tokio::test]
async fn compile_patch_error_closes_at_patch_without_mutation() {
    assert_plain_and_live_failure(
        PassThroughRecipe::new().with_compile_error(),
        DocActions::new(),
        Stage::Patch,
        "validation_error",
        0,
    )
    .await;
}

#[tokio::test]
async fn invalid_compiled_patch_closes_at_patch_without_mutation() {
    assert_plain_and_live_failure(
        PassThroughRecipe::new().with_empty_patch(),
        DocActions::new(),
        Stage::Patch,
        "patch_validation",
        0,
    )
    .await;
}

#[tokio::test]
async fn a_patch_bound_to_a_foreign_base_records_its_complete_compiled_trace() {
    let recipe = PassThroughRecipe::new().with_wrong_base_patch();
    let compiled = Arc::clone(&recipe.compiled_op_ids);
    let sergent = Sergent::new(
        configured_pass_through(recipe),
        DocActions::new(),
        CannedClient::new(intent_proposal_json(), plan_envelope(&[" one", " two"])),
    )
    .unwrap();
    let cancel = CancelToken::new();
    let observers: [&dyn RunObserver<Doc>; 0] = [];

    let result = sergent
        .run(
            SceneSource::plain(doc("keep", 7)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_failed_run(&result, Stage::Patch, "patch_validation", 2);
    // The rejected envelope still closes its Patch step over the ordered trace
    // compilation produced, so a failed run names exactly the Operations it
    // attempted.
    let patch = result
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Patch)
        .unwrap();
    let patch_output = patch
        .output()
        .and_then(sergent_rs_core::run_record::CapturedValue::value)
        .unwrap();
    assert_eq!(
        patch_output["patch_validation"],
        json!({ "status": "failure" })
    );
    let trace: Vec<String> = patch_operation_trace_ids(patch)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let compiled = compiled.lock().unwrap();
    assert_eq!(compiled.len(), 2);
    assert_eq!(trace, *compiled);
}

#[tokio::test]
async fn operation_fault_closes_at_dry_run_without_mutation() {
    let metadata = Map::from_iter([
        ("document_section".to_owned(), json!("summary")),
        ("limit".to_owned(), json!(12)),
    ]);
    let mut actions = DocActions::new();
    actions.apply_fault = Some(OperationFault::new(
        "document_limit",
        "deterministic apply failed",
        metadata.clone(),
    ));
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        actions,
        CannedClient::new(intent_proposal_json(), plan_envelope(&[" changed"])),
    )
    .unwrap();

    let result = sergent
        .run(
            SceneSource::plain(doc("keep", 7)),
            &DocMind,
            run_settings(),
            &CancelToken::new(),
            &[],
        )
        .await;

    assert_failed_run(&result, Stage::DryRun, "document_limit", 1);
    let error = result.error().expect("fault should close the run");
    assert_eq!(error.message, "deterministic apply failed");
    assert_eq!(error.metadata, metadata);
}

#[tokio::test]
async fn verification_rejection_closes_at_dry_run_without_mutation() {
    let mut actions = DocActions::new();
    actions.verify_issues = Some((
        "after Scene is invalid".to_owned(),
        vec!["second issue".to_owned()],
    ));
    assert_plain_and_live_failure(
        PassThroughRecipe::new(),
        actions,
        Stage::DryRun,
        "validation_error",
        1,
    )
    .await;
}
