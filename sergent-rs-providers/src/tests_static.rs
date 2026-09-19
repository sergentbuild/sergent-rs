//! Hermetic tests for the sanctioned `StaticLlmClient`.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::{
    Message, ModelClient, ModelError, ModelRequest, ModelRequestInput, ModelSettings,
};
use sergent_rs_core::proposal::derive_proposal_schema;
use sergent_rs_core::timing::Timestamp;

use crate::testing::{StaticLlmClient, StaticLlmOutcome};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
/// Minimal typed proposal used to exercise deterministic parsing and request capture.
struct TestProposal {
    answer: String,
}

/// Builds a request whose canonical schema crosses the deterministic client unchanged.
fn request(model_name: &str) -> ModelRequest {
    let schema = derive_proposal_schema::<TestProposal>().unwrap();
    ModelRequestInput::new(
        model_name.to_owned(),
        ModelSettings::default(),
        Arc::new(schema),
    )
    .into_request(vec![Message::user("hi")])
}

#[tokio::test]
async fn returns_canned_output_and_records_requests() {
    let client = StaticLlmClient::new([r#"{"answer":"yes"}"#.to_owned()]);
    let (response, parsed) = client.invoke(&request("openai/gpt-5.5")).await.expect("ok");

    assert_eq!(
        serde_json::Value::Object(parsed),
        json!({ "answer": "yes" })
    );
    assert_eq!(response.raw_output, r#"{"answer":"yes"}"#);
    assert_eq!(response.identity.provider, "openai");
    assert_eq!(response.identity.model, "gpt-5.5");
    assert_eq!(response.attempts.len(), 1);
    assert!(response.attempts[0].is_success());
    assert_eq!(response.usage.request_id.as_deref(), Some("static"));

    let recorded = client.recorded_requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].model_name(), "openai/gpt-5.5");
}

#[tokio::test]
async fn scripted_output_uses_the_existing_strict_success_path() {
    let client = StaticLlmClient::scripted([StaticLlmOutcome::Output(
        r#"{"answer":"scripted"}"#.to_owned(),
    )]);

    let (response, parsed) = client.invoke(&request("openai/gpt")).await.unwrap();

    assert_eq!(response.raw_output, r#"{"answer":"scripted"}"#);
    assert_eq!(
        serde_json::Value::Object(parsed),
        json!({ "answer": "scripted" })
    );
    assert!(response.attempts[0].is_success());
}

#[tokio::test]
async fn canned_evidence_comes_from_one_fixed_timing_source() {
    let client = StaticLlmClient::new([r#"{"answer":"yes"}"#.to_owned()]);
    let (response, _) = client.invoke(&request("openai/gpt-5.5")).await.expect("ok");

    let timing = response.attempts[0].timing();
    assert_eq!(timing.started_at(), Timestamp::from_unix_micros(0));
    assert_eq!(timing.finished_at(), Timestamp::from_unix_micros(1_000));
    assert_eq!(timing.duration_ms(), response.usage.latency_ms);
    assert_eq!(response.usage.latency_ms, 1);
}

#[tokio::test]
#[should_panic(expected = "static client outputs exhausted after request 1")]
async fn exhausted_queue_fails_loudly_instead_of_fabricating_a_provider_failure() {
    let client = StaticLlmClient::new(Vec::<String>::new());

    let _ = client.invoke(&request("openai/gpt-5.5")).await;
}

#[tokio::test]
async fn rejects_tagged_and_untagged_fenced_objects() {
    for raw in [
        "```json\n{\"answer\":\"yes\"}\n```",
        "```\n{\"answer\":\"yes\"}\n```",
    ] {
        let client = StaticLlmClient::new([raw.to_owned()]);
        let error = client.invoke(&request("openai/gpt-5.5")).await.unwrap_err();

        assert_eq!(
            error.kind,
            ErrorKind::InvalidResponse.as_str(),
            "for {raw:?}"
        );
        assert_eq!(error.raw_output.as_deref(), Some(raw));
        assert_eq!(error.attempts.len(), 1);
        assert!(error.attempts[0].is_success());
    }
}

#[tokio::test]
async fn reuses_real_name_resolution_and_records_before_selection() {
    let client = StaticLlmClient::new([r#"{"answer":"x"}"#.to_owned()]);
    let error = client.invoke(&request("bogus/model")).await.unwrap_err();

    assert_eq!(error.kind, ErrorKind::UnknownProvider.as_str());
    assert!(error.usage.is_none());
    let (response, _) = client.invoke(&request("openai/gpt")).await.unwrap();
    assert_eq!(response.raw_output, r#"{"answer":"x"}"#);
    assert_eq!(client.recorded_requests().len(), 2);
}

#[tokio::test]
async fn scripted_timeout_and_rate_limit_match_production_failure_evidence() {
    let client =
        StaticLlmClient::scripted([StaticLlmOutcome::Timeout, StaticLlmOutcome::RateLimited]);

    let timeout = client.invoke(&request("openai/gpt")).await.unwrap_err();
    assert_retryable_failure(&timeout, ErrorKind::Timeout, "provider request timed out");
    assert!(timeout.usage.is_none());

    let limited = client.invoke(&request("openai/gpt")).await.unwrap_err();
    assert_retryable_failure(
        &limited,
        ErrorKind::RateLimited,
        "provider returned HTTP 429",
    );
    let usage = limited.usage.as_ref().expect("rate-limit response usage");
    assert_eq!(usage.latency_ms, 2);
    assert!(usage.tokens.is_none());
    assert!(usage.request_id.is_none());
    assert_eq!(client.recorded_requests().len(), 2);
}

/// Verify one scripted transient failure against the shared production
/// identity and retry-attempt contract.
fn assert_retryable_failure(error: &ModelError, kind: ErrorKind, message: &str) {
    assert_eq!(error.kind, kind.as_str());
    assert!(error.retryable);
    assert_eq!(error.message, message);
    assert!(error.raw_output.is_none());
    assert_eq!(error.identity.as_ref().unwrap().provider, "openai");
    assert_eq!(error.identity.as_ref().unwrap().model, "gpt");
    assert_eq!(error.attempts.len(), 2);
    for (index, attempt) in error.attempts.iter().enumerate() {
        assert!(!attempt.is_success());
        assert_eq!(attempt.retryable(), Some(true));
        assert_eq!(
            attempt.error().unwrap().metadata,
            json!({ "retryable": true }).as_object().unwrap().clone()
        );
        assert_eq!(attempt.error().unwrap().kind, kind.as_str());
        assert_eq!(attempt.error().unwrap().message, message);
        assert_eq!(
            attempt.timing().started_at(),
            Timestamp::from_unix_micros(u64::try_from(index).unwrap() * 1_000)
        );
        assert_eq!(attempt.timing().duration_ms(), 1);
    }
}
