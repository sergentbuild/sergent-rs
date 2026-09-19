//! Adversarial byte, endpoint, evidence, and request-identity contracts at the
//! external-systems boundary.

use bytes::Bytes;
use reqwest::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use serde_json::json;

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::{ModelClient, ModelError};

use super::{
    GEMINI_BASE, OLLAMA_BASE, SENTINEL, assert_response_less_failure, parsed_object, request_for,
    response_bytes, response_chunks, scripted_client, scripted_client_with_clock,
    status_only_response,
};
use crate::client::MAX_GENERATION_BODY_BYTES;
use crate::credentials::MapEnv;
use crate::evidence::ManualClock;
use crate::http::{HttpFailure, HttpResponse, ScriptedHttpClient, ScriptedTimingStep};

#[tokio::test]
async fn non_success_status_precedes_every_unused_body_shape() {
    let bodies = [
        status_only_response(400),
        response_chunks(400, [Err(HttpFailure::BodyRead)]),
        response_bytes(400, Bytes::from_static(b"irrelevant")),
        response_chunks(
            400,
            [Ok(Bytes::from(vec![b'x'; MAX_GENERATION_BODY_BYTES + 1]))],
        ),
    ];

    for mut native in bodies {
        native.headers.insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_static("req_status_first"),
        );
        let http = ScriptedHttpClient::new([Ok(native)]);
        let client = scripted_client(http.clone(), openai_env());

        let error = client
            .invoke(&request_for("openai/x"))
            .await
            .expect_err("status is terminal");

        assert_eq!(error.kind, ErrorKind::ProviderError.as_str());
        assert_eq!(error.attempts.len(), 1);
        assert_eq!(
            error
                .usage
                .as_ref()
                .and_then(|usage| usage.request_id.as_deref()),
            Some("req_status_first")
        );
        assert_eq!(http.recorded_requests().len(), 1);
    }
}

#[tokio::test]
async fn ollama_preflight_status_never_consumes_its_body() {
    let http = ScriptedHttpClient::new([Ok(status_only_response(404))]);
    let client = scripted_client(http.clone(), ollama_env());

    let error = client
        .invoke(&request_for("ollama/missing"))
        .await
        .expect_err("preflight status is terminal");

    assert_eq!(error.kind, ErrorKind::ModelNotFound.as_str());
    assert!(error.attempts.is_empty());
    let usage = error.usage.as_ref().expect("completed preflight usage");
    assert_eq!(usage.latency_ms, 0);
    assert!(usage.tokens.is_none());
    assert!(usage.request_id.is_none());
    assert_eq!(http.recorded_requests().len(), 1);
}

#[tokio::test]
async fn accepted_generation_body_has_an_exact_byte_ceiling() {
    let base = openai_envelope_bytes("{\"answer\":\"bounded\"}");
    assert!(base.len() < MAX_GENERATION_BODY_BYTES);
    let mut exact = base;
    exact.resize(MAX_GENERATION_BODY_BYTES, b' ');
    let split = MAX_GENERATION_BODY_BYTES / 3;
    let exact_response = response_chunks(
        200,
        [
            Ok(Bytes::copy_from_slice(&exact[..split])),
            Ok(Bytes::copy_from_slice(&exact[split..])),
        ],
    );
    let exact_http = ScriptedHttpClient::new([Ok(exact_response)]);
    let exact_client = scripted_client(exact_http, openai_env());

    let (_, parsed) = exact_client
        .invoke(&request_for("openai/x"))
        .await
        .expect("the exact ceiling is admitted");
    assert_eq!(parsed_object(parsed), json!({ "answer": "bounded" }));

    let mut overflow = openai_envelope_bytes("{\"answer\":\"oversized\"}");
    overflow.resize(MAX_GENERATION_BODY_BYTES + 1, b' ');
    let mut overflow_response = response_chunks(200, [Ok(Bytes::from(overflow))]);
    overflow_response.headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("req_oversized"),
    );
    let overflow_http = ScriptedHttpClient::new([Ok(overflow_response)]);
    let overflow_client = scripted_client(overflow_http, openai_env());

    let error = overflow_client
        .invoke(&request_for("openai/x"))
        .await
        .expect_err("one byte beyond the ceiling is rejected");
    assert_eq!(error.kind, ErrorKind::InvalidResponse.as_str());
    assert!(!error.retryable);
    assert_eq!(error.attempts.len(), 1);
    assert_eq!(
        error
            .usage
            .as_ref()
            .and_then(|usage| usage.request_id.as_deref()),
        Some("req_oversized")
    );
}

#[tokio::test]
async fn native_envelopes_admit_only_exact_utf8_bytes() {
    let invalid_utf8 = response_bytes(
        200,
        Bytes::from_static(b"{\"status\":\"completed\",\"output\":\"\xff\"}"),
    );
    let invalid_http = ScriptedHttpClient::new([Ok(invalid_utf8)]);
    let invalid_client = scripted_client(invalid_http, openai_env());
    let error = invalid_client
        .invoke(&request_for("openai/x"))
        .await
        .expect_err("invalid UTF-8 is never repaired");
    assert_invalid_envelope(&error);

    let mut valid = response_bytes(200, openai_envelope_bytes("{\"answer\":\"cafe\"}"));
    valid.headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=UTF-8"),
    );
    let valid_client = scripted_client(ScriptedHttpClient::new([Ok(valid)]), openai_env());
    let (_, parsed) = valid_client
        .invoke(&request_for("openai/x"))
        .await
        .expect("valid UTF-8 provider JSON is admitted");
    assert_eq!(parsed_object(parsed), json!({ "answer": "cafe" }));
}

#[tokio::test]
async fn unsupported_or_malformed_charset_fails_without_body_repair() {
    for content_type in [
        "application/json; charset=iso-8859-1",
        "application/json; charset",
        "application/json; charset=\"utf-8",
        "application/json; charset=utf-8\"",
    ] {
        let mut native = HttpResponse::status_only(200, reqwest::header::HeaderMap::new());
        native.headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(content_type).expect("test content type"),
        );
        let client = scripted_client(ScriptedHttpClient::new([Ok(native)]), openai_env());

        let error = client
            .invoke(&request_for("openai/x"))
            .await
            .expect_err("unsupported charset is terminal");

        assert_eq!(error.kind, ErrorKind::InvalidResponse.as_str());
        assert_eq!(
            error.message,
            "provider response declared an unsupported charset"
        );
        assert_eq!(error.attempts.len(), 1);
    }
}

#[tokio::test]
async fn gemini_encodes_the_opaque_model_as_one_path_segment() {
    let model = "alpha/beta?query#fragment/../tail";
    let native = response_bytes(
        200,
        serde_json::to_vec(&json!({
            "candidates": [{
                "content": { "parts": [{ "text": "{\"answer\":\"ok\"}" }] },
                "finishReason": "STOP"
            }]
        }))
        .unwrap(),
    );
    let http = ScriptedHttpClient::new([Ok(native)]);
    let client = scripted_client(http.clone(), MapEnv::new().with("GEMINI_API_KEY", SENTINEL));

    let response = client
        .invoke(&request_for(&format!("gemini/{model}")))
        .await
        .expect("reserved model data is encoded")
        .0;

    assert_eq!(response.identity.model, model);
    let requests = http.recorded_requests();
    let url = &requests[0].url;
    assert_eq!(
        url.path(),
        "/v1beta/models/alpha%2Fbeta%3Fquery%23fragment%2F..%2Ftail:generateContent"
    );
    assert!(url.query().is_none());
    assert!(url.fragment().is_none());
    assert!(url.as_str().starts_with(GEMINI_BASE));
}

#[tokio::test]
async fn ollama_endpoint_query_and_fragment_fail_during_discovery() {
    for endpoint in [
        "http://ollama.scripted.invalid/root?query=value",
        "http://ollama.scripted.invalid/root#fragment",
    ] {
        let http = ScriptedHttpClient::new([]);
        let env = MapEnv::new().with("SERGENT_OLLAMA_BASE_URL", endpoint);
        let client = scripted_client(http.clone(), env);

        let error = client
            .invoke(&request_for("ollama/x"))
            .await
            .expect_err("ambiguous endpoint is rejected");

        assert_eq!(error.kind, ErrorKind::ProviderUnavailable.as_str());
        assert!(error.attempts.is_empty());
        assert_response_less_failure(&error);
        assert!(http.recorded_requests().is_empty());
    }
}

#[tokio::test]
async fn only_response_backed_failure_closures_retain_full_invocation_latency() {
    assert_discovery_failure_has_null_usage().await;
    assert_preflight_failure_latency().await;
    assert_exhausted_status_latency_and_id().await;
}

#[tokio::test]
async fn retry_attempts_share_the_same_serialized_request_allocation() {
    let http = ScriptedHttpClient::new([
        Ok(status_only_response(500)),
        Ok(response_bytes(
            200,
            openai_envelope_bytes("{\"answer\":\"ok\"}"),
        )),
    ]);
    let client = scripted_client(http.clone(), openai_env());

    client
        .invoke(&request_for("openai/byte-identity"))
        .await
        .expect("the second attempt succeeds");

    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(requests[0].body.as_ptr(), requests[1].body.as_ptr());
}

/// Proves elapsed discovery work cannot create response usage.
async fn assert_discovery_failure_has_null_usage() {
    let (clock, manual) = ManualClock::new(10_000);
    let env = MapEnv::new().advancing_clock_on_read(manual, 7);
    let client = scripted_client_with_clock(ScriptedHttpClient::new([]), env, clock);
    let error = client
        .invoke(&request_for("openai/x"))
        .await
        .expect_err("credential discovery fails");
    assert_response_less_failure(&error);
}

/// Proves preflight time remains in a zero-generation-attempt failure.
async fn assert_preflight_failure_latency() {
    let (clock, manual) = ManualClock::new(10_000);
    let http = ScriptedHttpClient::new_timed(
        [Ok(status_only_response(404))],
        manual.clone(),
        [ScriptedTimingStep::elapsed(11)],
    );
    let env = ollama_env().advancing_clock_on_read(manual, 2);
    let client = scripted_client_with_clock(http, env, clock);
    let error = client
        .invoke(&request_for("ollama/missing"))
        .await
        .expect_err("preflight fails");
    assert_eq!(error.usage.expect("failure usage").latency_ms, 15);
}

/// Proves exhausted status retry keeps total time and the final reached
/// provider request ID.
async fn assert_exhausted_status_latency_and_id() {
    let (clock, manual) = ManualClock::new(10_000);
    let first = status_with_openai_id(500, "req_retry_1");
    let second = status_with_openai_id(500, "req_retry_2");
    let http = ScriptedHttpClient::new_timed(
        [Ok(first), Ok(second)],
        manual.clone(),
        [
            ScriptedTimingStep::elapsed(5),
            ScriptedTimingStep::elapsed(7),
        ],
    );
    let env = openai_env().advancing_clock_on_read(manual, 3);
    let client = scripted_client_with_clock(http, env, clock);
    let error = client
        .invoke(&request_for("openai/x"))
        .await
        .expect_err("retry exhausts");
    let usage = error.usage.expect("failure usage");
    assert_eq!(usage.latency_ms, 15);
    assert_eq!(usage.request_id.as_deref(), Some("req_retry_2"));
    assert_eq!(error.attempts.len(), 2);
}

/// Builds one natural OpenAI envelope as exact UTF-8 bytes.
fn openai_envelope_bytes(text: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "status": "completed",
        "output": [{
            "type": "message",
            "content": [{ "type": "output_text", "text": text }]
        }]
    }))
    .unwrap()
}

/// Attaches one recognized OpenAI request ID to a status-only response.
fn status_with_openai_id(status: u16, request_id: &'static str) -> HttpResponse {
    let mut response = status_only_response(status);
    response.headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static(request_id),
    );
    response
}

/// Verifies an exact-byte native decode failure remains non-retryable and
/// retains full call evidence without raw semantic output.
fn assert_invalid_envelope(error: &ModelError) {
    assert_eq!(error.kind, ErrorKind::InvalidResponse.as_str());
    assert!(!error.retryable);
    assert!(error.raw_output.is_none());
    assert_eq!(error.attempts.len(), 1);
    assert!(error.usage.is_some());
}

/// Supplies the exact OpenAI credential used by boundary tests.
fn openai_env() -> MapEnv {
    MapEnv::new().with("OPENAI_API_KEY", SENTINEL)
}

/// Supplies the validated Ollama endpoint used by boundary tests.
fn ollama_env() -> MapEnv {
    MapEnv::new().with("SERGENT_OLLAMA_BASE_URL", OLLAMA_BASE)
}
