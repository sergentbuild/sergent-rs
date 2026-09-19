//! Provider-engine owner tests through the crate-private `ScriptedHttpClient`.
//! Typed outcomes, recorded requests, and deterministic side effects stay
//! entirely in memory. Raw native bodies still cross the real adapter envelope
//! classifiers and the shared strict JSON-object parser.

use std::sync::Arc;

use bytes::Bytes;
use reqwest::header::HeaderMap;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::{
    ImagePart, Message, ModelError, ModelRequest, ModelRequestInput, ModelResponse, ModelSettings,
};
use sergent_rs_core::proposal::{ProposalSchema, derive_proposal_schema};
use sergent_rs_core::run_record::OpenModelCall;

use crate::client::ScriptedLlmClient;
use crate::credentials::{Endpoints, MapEnv};
use crate::evidence::{Clock, ManualClock};
use crate::http::{HttpPostRequest, HttpResponse, ScriptedHttpClient};

mod adapters;
mod external_boundary;
mod failures;
mod fake_http;
mod ollama;
mod responses;
mod retry;

// A sentinel secret that must be present in captured test-only requests but
// absent from all returned evidence.
const SENTINEL: &str = "SCRIPTED-SECRET-2f9c";
const OPENAI_BASE: &str = "https://openai.scripted.invalid";
const ANTHROPIC_BASE: &str = "https://anthropic.scripted.invalid";
const GEMINI_BASE: &str = "https://gemini.scripted.invalid";
const OLLAMA_BASE: &str = "http://ollama.scripted.invalid";
const IMAGE_BASE64: &str = "iVBORw0KGgo=";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
/// Minimal closed proposal used to derive real canonical schemas in provider
/// engine tests.
struct TestProposal {
    answer: String,
}

/// Derives the same canonical proposal schema production requests carry.
fn schema() -> ProposalSchema {
    derive_proposal_schema::<TestProposal>().expect("test schema")
}

/// Builds a text-only request with default settings and a canonical schema.
fn request_for(model_name: &str) -> ModelRequest {
    request_with_settings_for(model_name, ModelSettings::default())
}

/// Builds a text-only request with explicit caller-owned settings.
fn request_with_settings_for(model_name: &str, settings: ModelSettings) -> ModelRequest {
    ModelRequestInput::new(model_name.to_owned(), settings, Arc::new(schema()))
        .into_request(vec![Message::system("be terse"), Message::user("hello")])
}

/// Builds a request containing one bounded PNG through the native image-part
/// representation.
fn request_with_image_for(model_name: &str) -> ModelRequest {
    ModelRequestInput::new(
        model_name.to_owned(),
        ModelSettings::default(),
        Arc::new(schema()),
    )
    .into_request(vec![
        Message::system("be terse"),
        Message::user_with_images(
            "inspect",
            [ImagePart::png(IMAGE_BASE64).expect("bounded PNG test image")],
        ),
    ])
}

/// Supplies isolated non-network cloud hosts for scripted requests.
fn endpoints() -> Endpoints {
    Endpoints::scripted(OPENAI_BASE, ANTHROPIC_BASE, GEMINI_BASE)
}

/// Creates a repeatable provider evidence clock at a fixed wall time.
fn manual_clock() -> Clock {
    ManualClock::new(10_000).0
}

/// Builds a scripted provider client using the standard fixed test clock.
fn scripted_client(http: ScriptedHttpClient, env: MapEnv) -> ScriptedLlmClient {
    scripted_client_with_clock(http, env, manual_clock())
}

/// Builds a scripted provider client with an explicitly controlled evidence
/// clock.
fn scripted_client_with_clock(
    http: ScriptedHttpClient,
    env: MapEnv,
    clock: Clock,
) -> ScriptedLlmClient {
    ScriptedLlmClient::new(http, env, endpoints(), clock)
}

/// Wraps a status and body as a headerless completed HTTP response.
fn response(status: u16, text: impl Into<String>) -> HttpResponse {
    response_bytes(status, Bytes::from(text.into()))
}

/// Wraps a status and exact bytes as a headerless scripted HTTP response.
fn response_bytes(status: u16, bytes: impl Into<Bytes>) -> HttpResponse {
    HttpResponse::scripted(status, HeaderMap::new(), [Ok(bytes.into())])
}

/// Wraps response metadata around deterministic exact byte chunks.
fn response_chunks(
    status: u16,
    chunks: impl IntoIterator<Item = Result<Bytes, crate::http::HttpFailure>>,
) -> HttpResponse {
    HttpResponse::scripted(status, HeaderMap::new(), chunks)
}

/// Builds a response whose body must remain untouched by status-only policy.
fn status_only_response(status: u16) -> HttpResponse {
    HttpResponse::status_only(status, HeaderMap::new())
}

/// Builds an OpenAI natural-completion envelope with usage and a body ID that
/// must not become request-ID evidence.
fn openai_success(text: &str) -> HttpResponse {
    response(
        200,
        json!({
            "id": "body-resource-not-request-id",
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": text
                }]
            }],
            "usage": {
                "input_tokens": 2,
                "output_tokens": 3
            }
        })
        .to_string(),
    )
}

/// Builds an Ollama envelope with exactly one natural choice and normalized
/// usage inputs.
fn ollama_success(text: &str) -> HttpResponse {
    response(
        200,
        json!({
            "choices": [{
                "finish_reason": "stop",
                "message": { "content": text }
            }],
            "usage": {
                "prompt_tokens": 2,
                "completion_tokens": 3
            }
        })
        .to_string(),
    )
}

/// Verifies a credential header has the exact wire value and remains marked
/// sensitive.
fn assert_sensitive_header(request: &HttpPostRequest, name: &str, expected: &str) {
    let value = request.headers.get(name).expect("credential header");
    assert_eq!(value, expected);
    assert!(value.is_sensitive(), "{name} must remain marked sensitive");
}

/// Verifies every success evidence carrier excludes the sentinel credential.
fn assert_evidence_excludes_secret(response: &ModelResponse) {
    assert!(!response.raw_output.contains(SENTINEL));
    for evidence in [
        serde_json::to_string(&response.identity).expect("serialize identity evidence"),
        serde_json::to_string(&response.attempts).expect("serialize attempt evidence"),
        serde_json::to_string(&response.usage).expect("serialize usage evidence"),
    ] {
        assert!(
            !evidence.contains(SENTINEL),
            "credential leaked into returned evidence"
        );
    }
}

/// Verifies every error evidence carrier excludes the sentinel credential.
fn assert_error_evidence_excludes_secret(error: &ModelError) {
    assert!(!error.message.contains(SENTINEL));
    if let Some(raw) = &error.raw_output {
        assert!(!raw.contains(SENTINEL));
    }
    assert!(
        !serde_json::to_string(&error.attempts)
            .expect("serialize attempt evidence")
            .contains(SENTINEL)
    );
    if let Some(usage) = &error.usage {
        assert!(
            !serde_json::to_string(usage)
                .expect("serialize usage evidence")
                .contains(SENTINEL)
        );
    }
    if let Some(identity) = &error.identity {
        assert!(!identity.provider.contains(SENTINEL));
        assert!(!identity.model.contains(SENTINEL));
    }
}

/// Verifies an exit without provider response metadata invents neither usage
/// nor raw response evidence.
fn assert_response_less_failure(error: &ModelError) {
    assert!(error.usage.is_none());
    assert!(error.raw_output.is_none());
}

/// Verifies response metadata created usage with only the reached whole-call
/// latency and optional request ID.
fn assert_zero_latency_response_usage(error: &ModelError, request_id: Option<&str>) {
    let usage = error.usage.as_ref().expect("response-backed call usage");
    assert_eq!(usage.latency_ms, 0);
    assert!(usage.tokens.is_none());
    assert_eq!(usage.request_id.as_deref(), request_id);
}

/// Close provider evidence through the production model-call projection and
/// prove its complete exact field presence and reached values.
fn assert_closed_failure_projection(request: &ModelRequest, error: &ModelError) {
    let value = serde_json::to_value(OpenModelCall::new(request).failed(error))
        .expect("serialize failed model call");
    let call = value.as_object().expect("model call object");
    assert_eq!(call.len(), 6);
    for key in [
        "proposal_schema",
        "model_name",
        "identity",
        "payloads",
        "usage",
        "attempts",
    ] {
        assert!(call.contains_key(key), "missing model-call field {key}");
    }
    assert_eq!(
        call["proposal_schema"],
        serde_json::to_value(request.proposal_schema().as_ref()).unwrap()
    );
    assert_eq!(call["model_name"], json!(request.model_name()));
    assert_eq!(
        call["identity"],
        serde_json::to_value(&error.identity).unwrap()
    );
    assert_eq!(call["usage"], serde_json::to_value(&error.usage).unwrap());
    assert_eq!(
        call["attempts"],
        serde_json::to_value(&error.attempts).unwrap()
    );
    let payloads = call["payloads"].as_object().expect("payload object");
    assert_eq!(payloads.len(), 4);
    assert_eq!(
        payloads["raw_response"],
        serde_json::to_value(&error.raw_output).unwrap()
    );
    assert_eq!(payloads["parsed_json"], Value::Null);
    assert_eq!(payloads["parsed_proposal"], Value::Null);
    assert_eq!(payloads["request"]["status"], "captured");
    assert_eq!(
        payloads["request"]["value"]["model_name"],
        json!(request.model_name())
    );
}

/// Verifies an invalid response closes after one non-retryable failed attempt.
fn assert_non_retryable_invalid_attempt(error: &ModelError) {
    assert_eq!(error.kind, ErrorKind::InvalidResponse.as_str());
    assert!(!error.retryable);
    assert_eq!(error.attempts.len(), 1);
    let attempt = &error.attempts[0];
    assert!(!attempt.is_success());
    assert_eq!(attempt.retryable(), Some(false));
    let attempt_error = attempt.error().expect("failed attempt error");
    assert_eq!(attempt_error.kind, ErrorKind::InvalidResponse.as_str());
    assert_eq!(
        attempt_error.metadata,
        json!({ "retryable": false }).as_object().unwrap().clone()
    );
}

/// Wraps a parsed object map for comparisons against ordinary JSON values.
fn parsed_object(parsed: serde_json::Map<String, Value>) -> Value {
    Value::Object(parsed)
}
