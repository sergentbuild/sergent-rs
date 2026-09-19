//! Typed HTTP failures and local failures that reach no HTTP request.

use std::num::NonZeroU32;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{HeaderName, HeaderValue};
use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::{ModelClient, ModelSettings};

use super::{
    SENTINEL, assert_closed_failure_projection, assert_error_evidence_excludes_secret,
    assert_response_less_failure, request_for, request_with_settings_for, response_chunks,
    scripted_client,
};
use crate::credentials::MapEnv;
use crate::http::{HttpFailure, ScriptedHttpClient};

/// Verifies a retryable HTTP failure yields stable errors, identical requests, and sanitized evidence.
async fn assert_retryable_http_failure(failure: HttpFailure, kind: ErrorKind, message: &str) {
    let http = ScriptedHttpClient::new([Err(failure), Err(failure)]);
    let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));
    let request = request_with_settings_for(
        "openai/failure-model",
        ModelSettings {
            timeout_secs: NonZeroU32::new(7).expect("nonzero timeout"),
            ..ModelSettings::default()
        },
    );

    let error = client
        .invoke(&request)
        .await
        .expect_err("typed failures exhaust retry");
    assert_closed_failure_projection(&request, &error);

    assert_eq!(error.kind, kind.as_str());
    assert!(error.retryable);
    assert_eq!(error.message, message);
    assert!(error.raw_output.is_none());
    assert_response_less_failure(&error);
    assert_eq!(error.attempts.len(), 2);
    for attempt in &error.attempts {
        assert!(!attempt.is_success());
        assert_eq!(attempt.retryable(), Some(true));
        let attempt_error = attempt.error().expect("failure evidence");
        assert_eq!(attempt_error.kind, kind.as_str());
        assert_eq!(attempt_error.message, message);
        assert_eq!(
            attempt_error.metadata,
            serde_json::json!({ "retryable": true })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert!(
        requests
            .iter()
            .all(|request| request.timeout == Duration::from_secs(7))
    );
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn timeout_is_retryable_and_exhausts_two_attempts() {
    assert_retryable_http_failure(
        HttpFailure::Timeout,
        ErrorKind::Timeout,
        "provider request timed out",
    )
    .await;
}

#[tokio::test]
async fn connection_failure_is_retryable_and_exhausts_two_attempts() {
    assert_retryable_http_failure(
        HttpFailure::Connection,
        ErrorKind::ProviderUnavailable,
        "could not connect to the provider",
    )
    .await;
}

#[tokio::test]
async fn send_failure_is_retryable_and_exhausts_two_attempts() {
    assert_retryable_http_failure(
        HttpFailure::Send,
        ErrorKind::ProviderUnavailable,
        "provider transport failure",
    )
    .await;
}

#[tokio::test]
async fn body_read_failure_preserves_bounded_admitted_utf8_partial_text() {
    let mut first = response_chunks(
        200,
        [
            Ok(Bytes::from_static(b"first partial")),
            Err(HttpFailure::BodyRead),
        ],
    );
    first.headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("req_body_1"),
    );
    let mut second = response_chunks(
        200,
        [
            Ok(Bytes::from_static(b"second partial")),
            Err(HttpFailure::BodyRead),
        ],
    );
    second.headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("req_body_2"),
    );
    let http = ScriptedHttpClient::new([Ok(first), Ok(second)]);
    let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));
    let request = request_for("openai/failure-model");

    let error = client
        .invoke(&request)
        .await
        .expect_err("body failures exhaust retry");
    assert_closed_failure_projection(&request, &error);

    assert_eq!(error.kind, ErrorKind::ProviderUnavailable.as_str());
    assert!(error.retryable);
    assert_eq!(error.attempts.len(), 2);
    assert_eq!(error.raw_output.as_deref(), Some("second partial"));
    let usage = error.usage.as_ref().expect("failure call usage");
    assert_eq!(usage.request_id.as_deref(), Some("req_body_2"));
    assert!(usage.tokens.is_none());
    for attempt in &error.attempts {
        assert_eq!(
            attempt.error().expect("failure").metadata,
            serde_json::json!({ "retryable": true })
                .as_object()
                .unwrap()
                .clone()
        );
    }
    assert_eq!(http.recorded_requests().len(), 2);
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn body_read_failure_does_not_repair_partial_non_utf8_bytes() {
    let partial = [
        Ok(Bytes::from_static(b"valid then \xff")),
        Err(HttpFailure::BodyRead),
    ];
    let http = ScriptedHttpClient::new([
        Ok(response_chunks(200, partial.clone())),
        Ok(response_chunks(200, partial)),
    ]);
    let client = scripted_client(http, MapEnv::new().with("OPENAI_API_KEY", SENTINEL));
    let request = request_for("openai/failure-model");

    let error = client
        .invoke(&request)
        .await
        .expect_err("body failures exhaust retry");
    assert_closed_failure_projection(&request, &error);

    assert!(error.raw_output.is_none());
    assert!(error.usage.is_some(), "response metadata was reached");
}

#[tokio::test]
async fn missing_credentials_fail_before_any_http_request() {
    let http = ScriptedHttpClient::new([]);
    let client = scripted_client(http.clone(), MapEnv::new());
    let request = request_for("openai/x");

    let error = client
        .invoke(&request)
        .await
        .expect_err("missing credentials");
    assert_closed_failure_projection(&request, &error);

    assert_eq!(error.kind, ErrorKind::MissingCredentials.as_str());
    assert!(!error.retryable);
    assert!(error.attempts.is_empty());
    assert_response_less_failure(&error);
    assert!(error.identity.is_some());
    assert!(http.recorded_requests().is_empty());
}

#[tokio::test]
async fn invalid_credential_header_fails_before_any_http_request() {
    let http = ScriptedHttpClient::new([]);
    let client = scripted_client(
        http.clone(),
        MapEnv::new().with("OPENAI_API_KEY", "secret\nnot-a-header"),
    );

    let error = client
        .invoke(&request_for("openai/x"))
        .await
        .expect_err("invalid credential header");

    assert_eq!(error.kind, ErrorKind::MissingCredentials.as_str());
    assert!(error.attempts.is_empty());
    assert_response_less_failure(&error);
    assert!(http.recorded_requests().is_empty());
}

#[tokio::test]
async fn invalid_selection_fails_before_any_http_request() {
    let http = ScriptedHttpClient::new([]);
    let client = scripted_client(http.clone(), MapEnv::new());

    for (model_name, kind) in [
        ("openai/", ErrorKind::InvalidModelName),
        ("unknown/model", ErrorKind::UnknownProvider),
    ] {
        let request = request_for(model_name);
        let error = client
            .invoke(&request)
            .await
            .expect_err("invalid selection");
        assert_closed_failure_projection(&request, &error);
        assert_eq!(error.kind, kind.as_str(), "for {model_name:?}");
        assert!(error.attempts.is_empty(), "for {model_name:?}");
        assert_response_less_failure(&error);
        assert!(error.identity.is_none(), "for {model_name:?}");
    }
    assert!(http.recorded_requests().is_empty());
}
