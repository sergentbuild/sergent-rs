//! Stage order and stop point per Intent mode and flow: the model-backed and
//! pass-through happy paths, the stop flow, the no-target end, and
//! the recipe validation rejections that stop at their owning stage.

mod harness;

use std::num::NonZeroU32;
use std::sync::Arc;

use harness::*;
use serde_json::json;
use sergent_rs_core::model::{ModelSettings, ThinkingEffort};
use sergent_rs_core::vocab::{ProgressStatus, RunStepName, Stage, TerminalStatus};
use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::SceneSource;
use sergent_rs_runtime::sergent::{ConfiguredRecipe, RunSettings, Sergent};

fn no_observers<'a>() -> [&'a dyn RunObserver<Doc>; 0] {
    []
}

fn progress_events(observer: &CapturingObserver) -> Vec<(Stage, ProgressStatus)> {
    observer
        .progress
        .lock()
        .unwrap()
        .iter()
        .map(|snapshot| (snapshot.stage, snapshot.status))
        .collect()
}

fn settings(effort: ThinkingEffort, max_output_tokens: u32, timeout_secs: u32) -> ModelSettings {
    ModelSettings {
        thinking_effort: effort,
        max_output_tokens: NonZeroU32::new(max_output_tokens).unwrap(),
        timeout_secs: NonZeroU32::new(timeout_secs).unwrap(),
    }
}

#[tokio::test]
async fn model_backed_happy_path_commits_and_orders_stages() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["hello"]));
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let observer = CapturingObserver::default();
    let observers: [&dyn RunObserver<Doc>; 1] = [&observer];
    let cancel = CancelToken::new();

    let result = sergent
        .run(
            SceneSource::plain(doc("start ", 5)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Success);
    assert_eq!(result.stage(), Stage::Commit);
    assert_eq!(result.scene().text, "start hello");
    assert_eq!(result.scene().revision, 6);
    assert_eq!(
        result.run_record().scene().revision_after(),
        Some(result.scene().revision)
    );
    assert_eq!(
        progress_events(&observer),
        [
            (Stage::Queued, ProgressStatus::Queued),
            (Stage::Started, ProgressStatus::Running),
            (Stage::IntentCall, ProgressStatus::Running),
            (Stage::Intent, ProgressStatus::Running),
            (Stage::PlanCall, ProgressStatus::Running),
            (Stage::ExecutionPlan, ProgressStatus::Running),
            (Stage::Patch, ProgressStatus::Running),
            (Stage::DryRun, ProgressStatus::Running),
            (Stage::Commit, ProgressStatus::Running),
            (Stage::Commit, ProgressStatus::Success),
        ]
    );
}

#[tokio::test]
async fn passthrough_happy_path_skips_the_intent_call() {
    let client = CannedClient::new(object(json!({})), plan_envelope(&["x"]));
    let requests = client.requests.clone();
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let observer = CapturingObserver::default();
    let observers: [&dyn RunObserver<Doc>; 1] = [&observer];
    let cancel = CancelToken::new();

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
    assert_eq!(
        progress_events(&observer),
        [
            (Stage::Queued, ProgressStatus::Queued),
            (Stage::Started, ProgressStatus::Running),
            (Stage::Intent, ProgressStatus::Running),
            (Stage::PlanCall, ProgressStatus::Running),
            (Stage::ExecutionPlan, ProgressStatus::Running),
            (Stage::Patch, ProgressStatus::Running),
            (Stage::DryRun, ProgressStatus::Running),
            (Stage::Commit, ProgressStatus::Running),
            (Stage::Commit, ProgressStatus::Success),
        ]
    );
    // Exactly one model call happened: the Plan call.
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn reached_model_phases_receive_their_distinct_settings_and_exact_model_name() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let requests = Arc::clone(&client.requests);
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let intent_settings = settings(ThinkingEffort::Low, 11, 12);
    let plan_settings = settings(ThinkingEffort::Medium, 21, 22);
    let model_name = " Provider/Mixed Model ";
    let cancel = CancelToken::new();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("", 1)),
            &DocMind,
            RunSettings::new(model_name)
                .with_intent(intent_settings)
                .with_plan(plan_settings),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Success);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].model_settings(), intent_settings);
    assert_eq!(requests[1].model_settings(), plan_settings);
    assert_eq!(requests[0].model_name(), model_name);
    assert_eq!(requests[1].model_name(), model_name);
    assert_eq!(result.run_record().model_name(), model_name);
    let intent_call = model_call_of(result.run_record(), RunStepName::Intent).expect("Intent call");
    let plan_call =
        model_call_of(result.run_record(), RunStepName::ExecutionPlan).expect("Plan call");
    assert_eq!(
        captured_request(intent_call)["model_name"],
        json!(model_name)
    );
    assert_eq!(
        captured_request(intent_call)["model_settings"],
        serde_json::to_value(intent_settings).unwrap()
    );
    assert_eq!(captured_request(plan_call)["model_name"], json!(model_name));
    assert_eq!(
        captured_request(plan_call)["model_settings"],
        serde_json::to_value(plan_settings).unwrap()
    );
}

#[tokio::test]
async fn passthrough_reaches_only_the_plan_settings() {
    let client = CannedClient::new(object(json!({})), plan_envelope(&["x"]));
    let requests = Arc::clone(&client.requests);
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let intent_settings = settings(ThinkingEffort::Low, 31, 32);
    let plan_settings = settings(ThinkingEffort::Medium, 41, 42);
    let cancel = CancelToken::new();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::plain(doc("", 1)),
            &DocMind,
            RunSettings::new("prov/model")
                .with_intent(intent_settings)
                .with_plan(plan_settings),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Success);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model_settings(), plan_settings);
}

#[tokio::test]
async fn intent_only_stop_is_terminal_success_without_a_plan_call() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let requests = client.requests.clone();
    let recipe = ModelIntentRecipe::new().with_intent(IntentKind::Stop);
    let sergent = Sergent::new(
        ConfiguredRecipe::intent_only(recipe),
        DocActions::new(),
        client,
    )
    .unwrap();
    let intent_settings = settings(ThinkingEffort::Low, 51, 52);
    let plan_settings = settings(ThinkingEffort::Medium, 61, 62);
    let cancel = CancelToken::new();
    let observer = CapturingObserver::default();
    let observers: [&dyn RunObserver<Doc>; 1] = [&observer];

    let result = sergent
        .run(
            SceneSource::plain(doc("keep", 5)),
            &DocMind,
            RunSettings::new("prov/model")
                .with_intent(intent_settings)
                .with_plan(plan_settings),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Success);
    assert_eq!(result.stage(), Stage::Intent);
    assert_eq!(result.scene().text, "keep");
    assert_eq!(result.terminal_message(), Some("nothing to do"));
    assert_eq!(result.run_record().scene().revision_after(), Some(5));
    let record = serde_json::to_value(result.run_record()).unwrap();
    assert_eq!(record["scene"]["revision_after"], 5);
    assert_eq!(record["outcome"]["status"], "success");
    assert_eq!(record["outcome"]["error"], serde_json::Value::Null);
    assert_eq!(
        record["outcome"]["terminal"]["message"]["value"],
        "nothing to do"
    );
    assert_eq!(
        record["outcome"]["terminal"]["metadata"]["value"],
        json!({ "reason": "already_satisfied" })
    );
    assert_eq!(record["steps"].as_array().unwrap().len(), 2);
    assert_eq!(record["steps"][1]["output"]["value"]["flow"], "stop");
    // The Intent call happened; no Plan call was made after stop.
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model_settings(), intent_settings);
    // The stop preserved its provider-call facts.
    assert!(model_call_of(result.run_record(), RunStepName::Intent).is_some());
    assert_eq!(
        progress_events(&observer),
        [
            (Stage::Queued, ProgressStatus::Queued),
            (Stage::Started, ProgressStatus::Running),
            (Stage::IntentCall, ProgressStatus::Running),
            (Stage::Intent, ProgressStatus::Running),
            (Stage::Intent, ProgressStatus::Success),
        ]
    );
}

#[tokio::test]
async fn no_target_ends_unchanged_with_the_recipe_fact() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let cancel = CancelToken::new();
    let observer = CapturingObserver::default();
    let observers: [&dyn RunObserver<Doc>; 1] = [&observer];
    let mut scene = doc("original", 2);
    scene.has_spot = false;

    let result = sergent
        .run(
            SceneSource::plain(scene),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::Started);
    assert_eq!(result.scene().text, "original");
    assert_eq!(result.error().unwrap().kind, "no_target");
    assert_no_commit_revision(result.run_record());
    let record = serde_json::to_value(result.run_record()).unwrap();
    assert_eq!(record["scene"]["revision_after"], serde_json::Value::Null);
    assert_eq!(record["outcome"]["status"], "failure");
    assert_eq!(record["outcome"]["terminal"], serde_json::Value::Null);
    assert_eq!(record["steps"].as_array().unwrap().len(), 1);
    assert_eq!(record["steps"][0]["status"], "failure");
    assert_eq!(
        record["steps"][0]["output"]["value"],
        json!({ "selected_target": null })
    );
    assert_eq!(
        progress_events(&observer),
        [
            (Stage::Queued, ProgressStatus::Queued),
            (Stage::Started, ProgressStatus::Running),
            (Stage::Started, ProgressStatus::Failure),
        ]
    );
}

#[tokio::test]
async fn an_intent_validation_rejection_stops_at_intent() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let mut recipe = ModelIntentRecipe::new();
    recipe.fail_validate_intent = true;
    let sergent = Sergent::new(configured_model_intent(recipe), DocActions::new(), client).unwrap();
    let cancel = CancelToken::new();
    let observer = CapturingObserver::default();
    let observers: [&dyn RunObserver<Doc>; 1] = [&observer];

    let result = sergent
        .run(
            SceneSource::plain(doc("d", 1)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.stage(), Stage::Intent);
    assert_eq!(result.error().unwrap().kind, "validation_error");
    assert_no_commit_revision(result.run_record());
    assert_eq!(
        progress_events(&observer),
        [
            (Stage::Queued, ProgressStatus::Queued),
            (Stage::Started, ProgressStatus::Running),
            (Stage::IntentCall, ProgressStatus::Running),
            (Stage::Intent, ProgressStatus::Running),
            (Stage::Intent, ProgressStatus::Failure),
        ]
    );
}

#[tokio::test]
async fn a_whole_plan_rejection_stops_at_execution_plan() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let mut recipe = ModelIntentRecipe::new();
    recipe.fail_validate_plan = true;
    let sergent = Sergent::new(configured_model_intent(recipe), DocActions::new(), client).unwrap();
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
    assert_eq!(result.error().unwrap().kind, "validation_error");
    assert_eq!(result.scene().text, "d");
    assert_no_commit_revision(result.run_record());
}
