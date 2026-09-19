//! Admissibility facts, builder-contract enforcement, and Run Record evidence: an
//! inadmissible operation names its index/call/op_id, foreign schemas and a
//! Intent-only continue fails the builder contract before its provider call,
//! and a full run records the required facts and the patch operation trace.

mod harness;

use harness::*;
use serde_json::{Value, json};
use sergent_rs_core::run_record::OutputTokenTotal;
use sergent_rs_core::vocab::{RunStepName, Stage, TerminalStatus};
use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::SceneSource;
use sergent_rs_runtime::sergent::{ConfiguredRecipe, Sergent};

fn no_observers<'a>() -> [&'a dyn RunObserver<Doc>; 0] {
    []
}

#[tokio::test]
async fn an_inadmissible_operation_names_exact_structured_facts() {
    // An empty append text is a valid string that the admissibility check rejects.
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&[""]));
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let cancel = CancelToken::new();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("d", 1)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.stage(), Stage::ExecutionPlan);
    let error = result.error().unwrap();
    assert_eq!(error.kind, "validation_error");
    assert_eq!(error.metadata.len(), 3);
    assert_eq!(error.metadata["index"], json!(0));
    assert_eq!(error.metadata["call"], json!("append"));
    assert!(
        error.metadata["operation_id"]
            .as_str()
            .unwrap()
            .starts_with("op_")
    );
    assert!(error.message.contains("append text must not be empty"));
    // The base scene did not change.
    assert_eq!(result.scene().text, "d");
    assert_no_commit_revision(result.run_record());
}

#[tokio::test]
async fn a_continue_intent_without_a_registry_fails_before_any_plan_call() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let requests = client.requests.clone();
    let sergent = Sergent::new(
        ConfiguredRecipe::intent_only(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let cancel = CancelToken::new();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("d", 1)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::Intent);
    assert_eq!(result.error().unwrap().kind, "recipe_contract_error");
    assert!(result.error().unwrap().metadata.is_empty());
    assert_no_commit_revision(result.run_record());
    // Only the Intent call happened; the builder contract failed before Plan.
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_committed_runs_patch_step_captures_the_exact_patch_summary() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["a", "b"]));
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let cancel = CancelToken::new();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("", 1)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Success);
    let patch_step = result
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Patch)
        .expect("a committed run has a patch step");
    let summary = patch_summary(patch_step);
    assert_eq!(summary["operation_count"], json!(2));
    assert_eq!(summary["operation_call_names"], json!(["append", "append"]));
    let trace = patch_operation_trace_ids(patch_step);
    assert_eq!(trace.len(), 2, "one entry per compiled operation, in order");
    assert!(trace.iter().all(|op_id| op_id.starts_with("op_")));
    assert_ne!(trace[0], trace[1], "distinct operation ids");
    assert_eq!(summary["operation_ids"], summary["operation_trace_ids"]);
    // The exact step shape has no direct operation_trace sibling.
    for step in result.run_record().steps() {
        assert!(
            serde_json::to_value(step)
                .unwrap()
                .get("operation_trace")
                .is_none()
        );
    }
    let commit = result
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Commit)
        .unwrap()
        .output()
        .unwrap()
        .value()
        .unwrap();
    assert_eq!(commit["commit_kind"], "plain");
    assert_eq!(commit["metadata"], json!({}));
    assert!(result.terminal_message().is_none());
    assert!(result.terminal_metadata().is_empty());
}

#[tokio::test]
async fn the_run_record_facts_are_present_on_a_full_run() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["z"]));
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let cancel = CancelToken::new();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("a", 5)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    let record = result.run_record();
    assert!(record.run_id().as_str().starts_with("run_"));
    assert_eq!(record.model_name(), "prov/model");
    let step_names: Vec<RunStepName> = record.steps().iter().map(|step| step.name()).collect();
    assert_eq!(
        step_names,
        vec![
            RunStepName::ProcessInput,
            RunStepName::Intent,
            RunStepName::ExecutionPlan,
            RunStepName::Patch,
            RunStepName::Commit
        ]
    );
    assert!(model_call_of(record, RunStepName::Intent).is_some());
    assert!(model_call_of(record, RunStepName::ExecutionPlan).is_some());
    assert!(model_call_of(record, RunStepName::ProcessInput).is_none());
    assert!(model_call_of(record, RunStepName::Commit).is_none());
    let transition = record.scene();
    assert_eq!(transition.revision_before(), 5);
    assert_eq!(transition.revision_after(), Some(6));
    let values: Vec<&Value> = record
        .steps()
        .iter()
        .map(|step| {
            step.output()
                .and_then(sergent_rs_core::run_record::CapturedValue::value)
        })
        .map(|output| output.unwrap())
        .collect();
    assert!(values[0]["selected_target"].is_object());
    assert_eq!(values[1]["flow"], "continue");
    assert!(values[1]["derived_intent"].is_object());
    assert!(values[2]["derived_execution_plan"].is_object());
    assert_eq!(values[3]["compiled_patch"]["operation_count"], 1);
    assert!(values[3]["dry_run"]["after_identity"].is_object());
    assert_eq!(
        values[4],
        &json!({ "commit_kind": "plain", "metadata": {} })
    );
    let process_input = record.steps()[0]
        .input()
        .and_then(sergent_rs_core::run_record::CapturedValue::value)
        .unwrap();
    assert_eq!(
        process_input,
        &json!({ "observation": "recent human activity" })
    );
    // Two model calls, two output tokens each.
    assert_eq!(record.total_output_tokens(), OutputTokenTotal::Complete(4));
}
