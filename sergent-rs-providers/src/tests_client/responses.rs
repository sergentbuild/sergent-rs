//! Invalid native envelopes, non-natural completion states, and strict
//! semantic extraction through the real provider engine.

use reqwest::header::{HeaderName, HeaderValue};
use serde_json::json;

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::ModelClient;

use super::{
    OLLAMA_BASE, SENTINEL, assert_closed_failure_projection, assert_error_evidence_excludes_secret,
    assert_non_retryable_invalid_attempt, assert_zero_latency_response_usage, openai_success,
    request_for, response, scripted_client, scripted_client_with_clock,
};
use crate::credentials::MapEnv;
use crate::evidence::ManualClock;
use crate::http::{ScriptedHttpClient, ScriptedTimingStep};

#[tokio::test]
async fn completed_native_envelope_applies_the_strict_json_object_parser() {
    for raw in [
        "this is not json",
        "{\"answer\":\"partial\"",
        "```json\n{\"answer\":\"yes\"}\n```",
        "```\n{\"answer\":\"yes\"}\n```",
        "[{\"answer\":\"yes\"}]",
        "\"a scalar\"",
        "before {\"answer\":\"yes\"}",
        "{\"answer\":\"yes\"} after",
        "{\"answer\":\"yes\"}{\"answer\":\"second\"}",
    ] {
        let mut native = openai_success(raw);
        native.headers.insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_static("req_strict_parser"),
        );
        let http = ScriptedHttpClient::new([Ok(native)]);
        let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));
        let request = request_for("openai/x");

        let error = client
            .invoke(&request)
            .await
            .expect_err("strict parser rejects non-object output");
        assert_closed_failure_projection(&request, &error);

        assert_eq!(
            error.kind,
            ErrorKind::InvalidResponse.as_str(),
            "for {raw:?}"
        );
        assert!(!error.retryable, "for {raw:?}");
        assert_eq!(error.message, "provider output was not one JSON object");
        assert!(!error.message.contains(raw), "for {raw:?}");
        assert_eq!(error.raw_output.as_deref(), Some(raw), "for {raw:?}");
        assert_eq!(error.attempts.len(), 1, "for {raw:?}");
        assert!(
            error.attempts[0].is_success(),
            "native generation completed for {raw:?}"
        );
        let usage = error.usage.as_ref().expect("completed usage evidence");
        assert_eq!(usage.request_id.as_deref(), Some("req_strict_parser"));
        assert!(usage.tokens.is_some());
        assert_eq!(http.recorded_requests().len(), 1);
        assert_error_evidence_excludes_secret(&error);
    }
}

#[tokio::test]
async fn openai_invalid_native_envelopes_keep_exact_admitted_response_text() {
    let missing_status = json!({
        "output": [{
            "type": "message",
            "content": [{
                "type": "output_text",
                "text": "{\"answer\":\"valid\"}"
            }]
        }]
    })
    .to_string();
    for native_body in ["{".to_owned(), missing_status] {
        let expected_raw = native_body.clone();
        let http = ScriptedHttpClient::new([Ok(response(200, native_body))]);
        let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));
        let request = request_for("openai/x");

        let error = client
            .invoke(&request)
            .await
            .expect_err("invalid native envelope");
        assert_closed_failure_projection(&request, &error);

        assert_non_retryable_invalid_attempt(&error);
        assert_eq!(
            error.message,
            "provider returned an invalid success envelope"
        );
        assert_eq!(error.raw_output.as_deref(), Some(expected_raw.as_str()));
        assert_zero_latency_response_usage(&error, None);
        assert_eq!(http.recorded_requests().len(), 1);
    }
}

#[tokio::test]
async fn anthropic_missing_stop_reason_rejects_semantic_text_before_parsing() {
    let native_body = json!({
        "content": [{ "type": "text", "text": "{\"answer\":\"valid\"}" }]
    })
    .to_string();
    let http = ScriptedHttpClient::new([Ok(response(200, native_body.clone()))]);
    let client = scripted_client(
        http.clone(),
        MapEnv::new().with("ANTHROPIC_API_KEY", SENTINEL),
    );

    let error = client
        .invoke(&request_for("anthropic/claude-opus-4.8"))
        .await
        .expect_err("missing stop reason");

    assert_non_retryable_invalid_attempt(&error);
    assert_eq!(error.raw_output.as_deref(), Some(native_body.as_str()));
    assert_zero_latency_response_usage(&error, None);
    assert_eq!(http.recorded_requests().len(), 1);
}

#[tokio::test]
async fn gemini_missing_finish_reason_rejects_semantic_text_before_parsing() {
    let native_body = json!({
        "candidates": [{
            "content": { "parts": [{ "text": "{\"answer\":\"valid\"}" }] }
        }]
    })
    .to_string();
    let http = ScriptedHttpClient::new([Ok(response(200, native_body.clone()))]);
    let client = scripted_client(http.clone(), MapEnv::new().with("GEMINI_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("gemini/x"))
        .await
        .expect_err("missing finish reason");

    assert_non_retryable_invalid_attempt(&error);
    assert_eq!(error.raw_output.as_deref(), Some(native_body.as_str()));
    assert_zero_latency_response_usage(&error, None);
    assert_eq!(http.recorded_requests().len(), 1);
}

#[tokio::test]
async fn ollama_missing_finish_reason_rejects_semantic_text_before_parsing() {
    let native_body = json!({
        "choices": [{
            "message": { "content": "{\"answer\":\"valid\"}" }
        }]
    })
    .to_string();
    let http = ScriptedHttpClient::new([
        Ok(response(200, "{}")),
        Ok(response(200, native_body.clone())),
    ]);
    let client = scripted_client(
        http.clone(),
        MapEnv::new().with("SERGENT_OLLAMA_BASE_URL", OLLAMA_BASE),
    );

    let error = client
        .invoke(&request_for("ollama/x"))
        .await
        .expect_err("missing finish reason");

    assert_non_retryable_invalid_attempt(&error);
    assert_eq!(error.raw_output.as_deref(), Some(native_body.as_str()));
    assert_zero_latency_response_usage(&error, None);
    assert_eq!(http.recorded_requests().len(), 2);
}

#[tokio::test]
async fn openai_incomplete_precedes_parsing_and_preserves_timed_evidence() {
    let (clock, manual) = ManualClock::new(4_000);
    let mut native = response(
        200,
        json!({
            "status": "incomplete",
            "incomplete_details": { "reason": "max_output_tokens" },
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "{\"answer\":\"valid\"}"
                }]
            }],
            "usage": { "input_tokens": 1, "output_tokens": 3 }
        })
        .to_string(),
    );
    native.headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("req_openai_incomplete"),
    );
    let http = ScriptedHttpClient::new_timed(
        [Ok(native)],
        manual.clone(),
        [ScriptedTimingStep::elapsed(9)],
    );
    let env = MapEnv::new()
        .with("OPENAI_API_KEY", SENTINEL)
        .advancing_clock_on_read(manual, 4);
    let client = scripted_client_with_clock(http.clone(), env, clock);
    let request = request_for("openai/x");

    let error = client
        .invoke(&request)
        .await
        .expect_err("incomplete response");
    assert_closed_failure_projection(&request, &error);

    assert_non_retryable_invalid_attempt(&error);
    assert_eq!(error.raw_output.as_deref(), Some("{\"answer\":\"valid\"}"));
    assert!(error.message.contains("max_output_tokens"));
    assert!(!error.message.contains("valid"));
    let usage = error.usage.as_ref().expect("non-natural usage");
    assert_eq!(usage.latency_ms, 13);
    assert_eq!(usage.request_id.as_deref(), Some("req_openai_incomplete"));
    assert_eq!(usage.tokens.as_ref().expect("tokens").output, Some(3));
    assert_eq!(error.attempts[0].timing().duration_ms(), 9);
    assert_eq!(http.recorded_requests().len(), 1);
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn openai_refusal_overrides_completed_status_and_preserves_evidence() {
    let mut native = response(
        200,
        json!({
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [
                    { "type": "output_text", "text": "{\"answer\":\"partial\"}" },
                    { "type": "refusal", "refusal": "policy" }
                ]
            }],
            "usage": { "input_tokens": 4, "output_tokens": 2 }
        })
        .to_string(),
    );
    native.headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("req_openai_refusal"),
    );
    let http = ScriptedHttpClient::new([Ok(native)]);
    let client = scripted_client(http, MapEnv::new().with("OPENAI_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("openai/x"))
        .await
        .expect_err("refusal is non-natural");

    assert_non_retryable_invalid_attempt(&error);
    assert_eq!(
        error.raw_output.as_deref(),
        Some("{\"answer\":\"partial\"}")
    );
    assert!(error.message.contains("policy"));
    assert!(!error.message.contains("partial"));
    let usage = error.usage.as_ref().expect("refusal usage");
    assert_eq!(usage.request_id.as_deref(), Some("req_openai_refusal"));
    assert_eq!(usage.tokens.as_ref().expect("tokens").output, Some(2));
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn provider_native_reason_is_bounded_human_prose_not_raw_response() {
    let reason = "\u{1b}".repeat(10_000);
    let http = ScriptedHttpClient::new([Ok(response(
        200,
        json!({
            "status": "incomplete",
            "incomplete_details": { "reason": reason },
            "output": []
        })
        .to_string(),
    ))]);
    let client = scripted_client(http, MapEnv::new().with("OPENAI_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("openai/x"))
        .await
        .expect_err("incomplete response");

    assert!(error.raw_output.is_none());
    assert!(error.message.chars().count() <= 320);
    assert!(!error.message.chars().any(char::is_control));
    assert!(error.message.ends_with("..."));
    assert_non_retryable_invalid_attempt(&error);
}

#[tokio::test]
async fn anthropic_non_natural_stop_preserves_request_id_usage_and_raw_evidence() {
    let mut native = response(
        200,
        json!({
            "stop_reason": "max_tokens",
            "content": [
                { "type": "thinking", "thinking": "ignored" },
                { "type": "text", "text": "{\"answer\":\"valid\"}" }
            ],
            "usage": { "input_tokens": 8, "output_tokens": 4 }
        })
        .to_string(),
    );
    native.headers.insert(
        HeaderName::from_static("request-id"),
        HeaderValue::from_static("req_anthropic_limit"),
    );
    let http = ScriptedHttpClient::new([Ok(native)]);
    let client = scripted_client(http, MapEnv::new().with("ANTHROPIC_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("anthropic/claude-opus-4.8"))
        .await
        .expect_err("non-natural stop");

    assert_non_retryable_invalid_attempt(&error);
    assert_eq!(error.raw_output.as_deref(), Some("{\"answer\":\"valid\"}"));
    assert!(error.message.contains("max_tokens"));
    assert!(!error.message.contains("valid"));
    let usage = error.usage.as_ref().expect("non-natural usage");
    assert_eq!(usage.request_id.as_deref(), Some("req_anthropic_limit"));
    let tokens = usage.tokens.as_ref().expect("tokens");
    assert_eq!(tokens.input, Some(8));
    assert_eq!(tokens.output, Some(4));
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn gemini_prompt_block_preserves_reason_message_and_usage() {
    let http = ScriptedHttpClient::new([Ok(response(
        200,
        json!({
            "promptFeedback": {
                "blockReason": "SAFETY",
                "blockReasonMessage": "policy threshold"
            },
            "usageMetadata": { "promptTokenCount": 9, "totalTokenCount": 9 },
            "additiveProviderField": true
        })
        .to_string(),
    ))]);
    let client = scripted_client(http, MapEnv::new().with("GEMINI_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("gemini/x"))
        .await
        .expect_err("prompt block");

    assert_non_retryable_invalid_attempt(&error);
    assert!(error.raw_output.is_none());
    assert!(error.message.contains("SAFETY"));
    assert!(error.message.contains("policy threshold"));
    let tokens = error
        .usage
        .as_ref()
        .and_then(|usage| usage.tokens.as_ref())
        .expect("blocked usage");
    assert_eq!(tokens.input, Some(9));
    assert_eq!(tokens.output, Some(9));
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn gemini_empty_candidates_is_a_non_natural_completion() {
    let http = ScriptedHttpClient::new([Ok(response(
        200,
        json!({
            "candidates": [],
            "usageMetadata": { "promptTokenCount": 2, "totalTokenCount": 2 }
        })
        .to_string(),
    ))]);
    let client = scripted_client(http, MapEnv::new().with("GEMINI_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("gemini/x"))
        .await
        .expect_err("empty candidates");

    assert_non_retryable_invalid_attempt(&error);
    assert!(error.raw_output.is_none());
    assert!(error.message.contains("no candidates"));
    assert!(error.usage.is_some());
}

#[tokio::test]
async fn gemini_non_natural_finish_preserves_partial_text_before_extraction() {
    for (finish_reason, message, partial_text) in [
        (
            "MAX_TOKENS",
            "output limit reached",
            "{\"answer\":\"valid\"}",
        ),
        ("SAFETY", "candidate safety filter", ""),
    ] {
        let candidate = if partial_text.is_empty() {
            json!({
                "finishReason": finish_reason,
                "finishMessage": message
            })
        } else {
            json!({
                "content": { "parts": [{ "text": partial_text }] },
                "finishReason": finish_reason,
                "finishMessage": message
            })
        };
        let http = ScriptedHttpClient::new([Ok(response(
            200,
            json!({
                "candidates": [candidate],
                "usageMetadata": { "promptTokenCount": 3, "totalTokenCount": 8 }
            })
            .to_string(),
        ))]);
        let client = scripted_client(http, MapEnv::new().with("GEMINI_API_KEY", SENTINEL));

        let error = client
            .invoke(&request_for("gemini/x"))
            .await
            .expect_err("non-natural finish");

        assert_non_retryable_invalid_attempt(&error);
        let expected = (!partial_text.is_empty()).then_some(partial_text);
        assert_eq!(error.raw_output.as_deref(), expected);
        assert!(error.message.contains(finish_reason));
        assert!(error.message.contains(message));
        assert!(!error.message.contains("answer"));
        let tokens = error
            .usage
            .as_ref()
            .and_then(|usage| usage.tokens.as_ref())
            .expect("non-natural usage");
        assert_eq!(tokens.input, Some(3));
        assert_eq!(tokens.output, Some(8));
    }
}

#[tokio::test]
async fn ollama_non_natural_finish_preserves_usage_and_partial_text() {
    let http = ScriptedHttpClient::new([
        Ok(response(200, "{}")),
        Ok(response(
            200,
            json!({
                "choices": [{
                    "finish_reason": "length",
                    "message": { "content": "{\"answer\":\"valid\"}" }
                }],
                "usage": { "prompt_tokens": 5, "completion_tokens": 3 }
            })
            .to_string(),
        )),
    ]);
    let client = scripted_client(
        http,
        MapEnv::new().with("SERGENT_OLLAMA_BASE_URL", OLLAMA_BASE),
    );

    let error = client
        .invoke(&request_for("ollama/x"))
        .await
        .expect_err("non-natural finish");

    assert_non_retryable_invalid_attempt(&error);
    assert_eq!(error.raw_output.as_deref(), Some("{\"answer\":\"valid\"}"));
    assert!(error.message.contains("length"));
    assert!(!error.message.contains("valid"));
    let tokens = error
        .usage
        .as_ref()
        .and_then(|usage| usage.tokens.as_ref())
        .expect("non-natural usage");
    assert_eq!(tokens.input, Some(5));
    assert_eq!(tokens.output, Some(3));
}
