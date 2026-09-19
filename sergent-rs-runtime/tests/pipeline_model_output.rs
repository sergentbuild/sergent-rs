//! The model-output crossing: a malformed Intent or Plan proposal keeps a
//! complete call record with no parsed proposal, provider errors are contained,
//! and a failed call record retains the model's raw output evidence.

mod harness;

use harness::*;
use serde_json::{Value, json};
use sergent_rs_core::model::{CallUsage, ModelIdentity, TokenCounts};
use sergent_rs_core::vocab::{RunStepName, Stage, TerminalStatus};
use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::SceneSource;
use sergent_rs_runtime::sergent::Sergent;

fn no_observers<'a>() -> [&'a dyn RunObserver<Doc>; 0] {
    []
}

#[tokio::test]
async fn a_malformed_intent_proposal_keeps_a_complete_call_with_no_parsed_proposal() {
    // The parsed JSON does not decode into DocProposal (unknown field, no `go`).
    let client = CannedClient::new(object(json!({ "wrong": 1 })), plan_envelope(&["x"]));
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

    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::IntentCall);
    assert_eq!(result.error().unwrap().kind, "schema_validation_failed");
    let call = model_call_of(result.run_record(), RunStepName::Intent).unwrap();
    // The call record stays complete; only the parsed proposal is absent.
    assert!(call.payloads().parsed_json().is_some());
    assert!(call.payloads().parsed_proposal().is_none());
    assert!(call.payloads().raw_response().is_some());
    assert!(call.identity().is_some());
    assert!(call.usage().is_some());
    assert!(!call.attempts().is_empty());
    assert_eq!(captured_request(call)["model_name"], json!("prov/model"));
    assert_no_commit_revision(result.run_record());
}

#[tokio::test]
async fn a_control_heavy_intent_decode_diagnostic_is_bounded_at_the_crossing() {
    let mut hostile = serde_json::Map::new();
    hostile.insert("\u{1b}".repeat(10_000), json!(1));
    let client = CannedClient::new(
        object(Value::Object(hostile)),
        plan_envelope(&["unreached"]),
    );
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

    assert_eq!(result.status(), TerminalStatus::Failure);
    let error = result.error().unwrap();
    assert_eq!(error.kind, "schema_validation_failed");
    let diagnostic = error
        .message
        .strip_prefix("intent proposal decode failed: ")
        .unwrap();
    assert_eq!(diagnostic.chars().count(), 256);
    assert!(diagnostic.ends_with("..."));
    assert!(!diagnostic.chars().any(char::is_control));
    assert_eq!(result.stage(), Stage::IntentCall);
    assert_no_commit_revision(result.run_record());
}

#[tokio::test]
async fn a_malformed_plan_proposal_is_schema_validation_failed_at_plan_call() {
    let client = CannedClient::new(
        intent_proposal_json(),
        object(json!({ "operations": [{ "call": "teleport" }] })),
    );
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

    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::PlanCall);
    assert_eq!(result.error().unwrap().kind, "schema_validation_failed");
    let call = model_call_of(result.run_record(), RunStepName::ExecutionPlan).unwrap();
    assert!(call.payloads().parsed_json().is_some());
    assert!(call.payloads().parsed_proposal().is_none());
    assert_no_commit_revision(result.run_record());
}

#[tokio::test]
async fn a_pre_call_provider_error_has_no_completed_envelope_evidence() {
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        ErrorClient::pre_call(sergent_rs_core::error::ErrorKind::ProviderError),
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
    assert_eq!(result.stage(), Stage::PlanCall);
    assert_eq!(result.error().unwrap().kind, "provider_error");
    assert_eq!(
        result.error().unwrap().metadata,
        json!({ "retryable": false }).as_object().unwrap().clone()
    );
    let call = model_call_of(result.run_record(), RunStepName::ExecutionPlan).unwrap();
    assert!(call.payloads().raw_response().is_none());
    assert!(call.identity().is_none());
    assert!(call.usage().is_none());
    assert!(call.attempts().is_empty());
    assert!(call.payloads().parsed_proposal().is_none());
    assert_no_commit_revision(result.run_record());
}

#[tokio::test]
async fn a_failed_call_record_retains_the_model_raw_output_evidence() {
    // A completed-envelope failure (refusal/incomplete/extraction) carries the
    // semantic raw output on ModelError.raw_output; the runtime maps it into the
    // failed call record's raw_response, keeping that record complete.
    let identity = ModelIdentity {
        provider: "fake".to_owned(),
        model: "m1".to_owned(),
        sdk_package: Some("fake-sdk".to_owned()),
        sdk_version: Some("1.2.3".to_owned()),
    };
    let usage = CallUsage {
        latency_ms: 2,
        tokens: Some(TokenCounts {
            input: Some(7),
            output: Some(3),
        }),
        request_id: Some("req_failed_1".to_owned()),
    };
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        ErrorClient::completed_envelope(
            sergent_rs_core::error::ErrorKind::InvalidResponse,
            "half a plan the model never finished",
            Some(identity.clone()),
            Some(usage.clone()),
        ),
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
    assert_eq!(result.stage(), Stage::PlanCall);
    let call = model_call_of(result.run_record(), RunStepName::ExecutionPlan).unwrap();
    assert_eq!(
        call.payloads().raw_response(),
        Some("half a plan the model never finished"),
        "the failed call record retains the raw output evidence"
    );
    assert_eq!(call.identity(), Some(&identity));
    assert_eq!(call.usage(), Some(&usage));
    assert_eq!(call.attempts().len(), 1);
    assert!(!call.attempts()[0].is_success());
    assert_eq!(call.attempts()[0].error().unwrap().kind, "invalid_response");
    assert_eq!(
        call.attempts()[0].error().unwrap().metadata,
        json!({ "retryable": false }).as_object().unwrap().clone()
    );
    assert_eq!(
        result.error().unwrap().metadata,
        json!({ "retryable": false }).as_object().unwrap().clone()
    );
    assert!(call.payloads().parsed_proposal().is_none());
    assert_no_commit_revision(result.run_record());
}
