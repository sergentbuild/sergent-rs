//! Cancellation at each reference checkpoint and during a provider await leaves
//! the scene unchanged and produces cancelled audit data. Provider awaits are
//! parked on tokio notify gates, never on sleeps.

mod harness;

use std::sync::Arc;

use harness::*;
use serde_json::Value;
use sergent_rs_core::run_record::CancellationCheckpoint;
use sergent_rs_core::vocab::{ProgressStatus, RunStepName, RunStepStatus, Stage, TerminalStatus};
use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::SceneSource;
use sergent_rs_runtime::sergent::Sergent;

fn no_observers<'a>() -> [&'a dyn RunObserver<Doc>; 0] {
    []
}

fn assert_cancelled(
    result: &sergent_rs_core::run_record::SergentResult<Doc>,
    stage: Stage,
    checkpoint: CancellationCheckpoint,
) {
    assert_eq!(result.status(), TerminalStatus::Cancelled);
    assert_eq!(result.stage(), stage);
    assert_eq!(result.error().unwrap().kind, "cancelled");
    assert!(result.run_record().cancellation().is_some());
    assert_eq!(
        result.run_record().cancellation().unwrap().checkpoint(),
        Some(checkpoint)
    );
    // The scene is unchanged.
    assert_eq!(result.scene().text, "keep");
    assert_eq!(result.scene().revision, 7);
    assert_no_commit_revision(result.run_record());
    let final_step = result.run_record().steps().last().unwrap();
    if stage == Stage::Started {
        assert!(matches!(final_step.status(), RunStepStatus::Success));
        assert!(final_step.error().is_none());
    } else {
        assert!(matches!(
            final_step.status(),
            RunStepStatus::Cancelled { .. }
        ));
        assert_eq!(final_step.error(), result.error());
    }
}

fn assert_request_only_model_call(result: &sergent_rs_core::run_record::SergentResult<Doc>) {
    let final_step = result.run_record().steps().last().unwrap();
    let call = final_step
        .model_call()
        .expect("provider-await cancellation must retain the opened request");
    let value = serde_json::to_value(call).unwrap();

    assert_eq!(value["identity"], Value::Null);
    assert_eq!(value["payloads"]["request"]["status"], "captured");
    assert_eq!(value["payloads"]["raw_response"], Value::Null);
    assert_eq!(value["payloads"]["parsed_json"], Value::Null);
    assert_eq!(value["payloads"]["parsed_proposal"], Value::Null);
    assert_eq!(value["usage"], Value::Null);
    assert_eq!(value["attempts"], serde_json::json!([]));
}

#[tokio::test]
async fn cancel_before_the_intent_call_stops_at_started() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let requests = client.requests.clone();
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let observer = CapturingObserver::default();
    let observers: [&dyn RunObserver<Doc>; 1] = [&observer];

    let result = sergent
        .run(
            SceneSource::plain(doc("keep", 7)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_cancelled(
        &result,
        Stage::Started,
        CancellationCheckpoint::BeforeIntent,
    );
    let progress = observer.progress.lock().unwrap();
    assert_eq!(
        progress
            .iter()
            .map(|snapshot| (snapshot.stage, snapshot.status))
            .collect::<Vec<_>>(),
        [
            (Stage::Queued, ProgressStatus::Queued),
            (Stage::Started, ProgressStatus::Running),
            (Stage::Started, ProgressStatus::Cancelled),
        ]
    );
    // No provider call happened.
    assert_eq!(requests.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn cancel_after_intent_validation_stops_at_intent() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let cancel = CancelToken::new();
    let mut recipe = ModelIntentRecipe::new();
    recipe.trip_in_validate_intent = Some(cancel.clone());
    let sergent = Sergent::new(configured_model_intent(recipe), DocActions::new(), client).unwrap();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("keep", 7)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_cancelled(
        &result,
        Stage::Intent,
        CancellationCheckpoint::AfterIntentValidation,
    );
}

#[tokio::test]
async fn cancel_before_dry_run_stops_at_dry_run() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let cancel = CancelToken::new();
    let mut recipe = ModelIntentRecipe::new();
    recipe.trip_in_validate_plan = Some(cancel.clone());
    let sergent = Sergent::new(configured_model_intent(recipe), DocActions::new(), client).unwrap();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("keep", 7)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_cancelled(&result, Stage::DryRun, CancellationCheckpoint::BeforeDryRun);
    let patch = result
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Patch)
        .expect("Patch started before the dry-run cancellation checkpoint");
    assert_eq!(patch_operation_trace_ids(patch).len(), 1);
}

#[tokio::test]
async fn cancel_before_commit_stops_at_commit() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let cancel = CancelToken::new();
    let actions = DocActions {
        trip_in_verify: Some(cancel.clone()),
        ..DocActions::new()
    };
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        actions,
        client,
    )
    .unwrap();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("keep", 7)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_cancelled(&result, Stage::Commit, CancellationCheckpoint::BeforeCommit);
}

#[tokio::test]
async fn cancel_during_the_intent_call_stops_at_intent_call() {
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let entered = client.entered();
    let sergent = Arc::new(
        Sergent::new(
            configured_model_intent(ModelIntentRecipe::new()),
            DocActions::new(),
            client,
        )
        .unwrap(),
    );

    let handle = sergent.start(
        SceneSource::plain(doc("keep", 7)),
        DocMind,
        run_settings(),
        Vec::new(),
    );
    entered.notified().await;
    handle.cancel();
    let result = handle.result().await;

    assert_cancelled(
        &result,
        Stage::IntentCall,
        CancellationCheckpoint::TaskCancelled,
    );
    assert_request_only_model_call(&result);
}

#[tokio::test]
async fn cancel_during_the_plan_call_stops_at_plan_call() {
    // A pass-through recipe reaches the Plan call as its first and only await.
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let entered = client.entered();
    let sergent = Arc::new(
        Sergent::new(
            configured_pass_through(PassThroughRecipe::new()),
            DocActions::new(),
            client,
        )
        .unwrap(),
    );

    let handle = sergent.start(
        SceneSource::plain(doc("keep", 7)),
        DocMind,
        run_settings(),
        Vec::new(),
    );
    entered.notified().await;
    handle.cancel();
    let result = handle.result().await;

    assert_cancelled(
        &result,
        Stage::PlanCall,
        CancellationCheckpoint::TaskCancelled,
    );
    assert_request_only_model_call(&result);
}
