//! Fixed outer retry, returned-status, request-identity, and timing contracts.

use reqwest::header::{HeaderValue, LOCATION};
use serde_json::json;

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::ModelClient;

use super::{
    GEMINI_BASE, OLLAMA_BASE, SENTINEL, assert_closed_failure_projection,
    assert_error_evidence_excludes_secret, assert_zero_latency_response_usage, ollama_success,
    openai_success, parsed_object, request_for, response, scripted_client,
    scripted_client_with_clock,
};
use crate::credentials::MapEnv;
use crate::evidence::ManualClock;
use crate::http::{ScriptedHttpClient, ScriptedTimingStep};

#[tokio::test]
async fn retryable_500_exhausts_exactly_two_identical_requests() {
    let echoed = format!("provider body echoed {SENTINEL}");
    let http =
        ScriptedHttpClient::new([Ok(response(500, echoed.clone())), Ok(response(500, echoed))]);
    let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));
    let request = request_for("openai/x");

    let error = client
        .invoke(&request)
        .await
        .expect_err("retry budget exhausts");
    assert_closed_failure_projection(&request, &error);

    assert_eq!(error.kind, ErrorKind::ProviderUnavailable.as_str());
    assert!(error.retryable);
    assert_eq!(error.message, "provider returned HTTP 500");
    assert!(error.raw_output.is_none());
    assert_zero_latency_response_usage(&error, None);
    assert_eq!(error.attempts.len(), 2);
    for attempt in &error.attempts {
        assert!(!attempt.is_success());
        assert_eq!(attempt.retryable(), Some(true));
        assert_eq!(
            attempt.error().map(|failure| failure.kind.as_str()),
            Some(ErrorKind::ProviderUnavailable.as_str())
        );
        assert_eq!(
            attempt.error().expect("failure").metadata,
            serde_json::json!({ "retryable": true })
                .as_object()
                .unwrap()
                .clone()
        );
    }
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn retryable_500_then_success_records_failure_then_success() {
    let http = ScriptedHttpClient::new([
        Ok(response(500, "first failure")),
        Ok(openai_success("{\"answer\":\"ok\"}")),
    ]);
    let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));

    let (model_response, parsed) = client
        .invoke(&request_for("openai/retry-model"))
        .await
        .expect("second outcome succeeds");

    assert_eq!(parsed_object(parsed), json!({ "answer": "ok" }));
    assert_eq!(model_response.attempts.len(), 2);
    assert!(!model_response.attempts[0].is_success());
    assert_eq!(
        model_response.attempts[0]
            .error()
            .map(|failure| failure.kind.as_str()),
        Some(ErrorKind::ProviderUnavailable.as_str())
    );
    assert!(model_response.attempts[1].is_success());
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
}

#[tokio::test]
async fn rate_limit_429_then_success_consumes_the_second_attempt() {
    let http = ScriptedHttpClient::new([
        Ok(response(429, "rate limited")),
        Ok(openai_success("{\"answer\":\"recovered\"}")),
    ]);
    let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));

    let model_response = client
        .invoke(&request_for("openai/x"))
        .await
        .expect("rate-limit retry succeeds")
        .0;

    assert_eq!(model_response.attempts.len(), 2);
    assert_eq!(
        model_response.attempts[0]
            .error()
            .map(|failure| failure.kind.as_str()),
        Some(ErrorKind::RateLimited.as_str())
    );
    assert_eq!(model_response.attempts[0].retryable(), Some(true));
    assert!(model_response.attempts[1].is_success());
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
}

#[tokio::test]
async fn cloud_400_is_one_nonretryable_provider_error_attempt() {
    let http = ScriptedHttpClient::new([Ok(response(400, "bad request"))]);
    let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("openai/x"))
        .await
        .expect_err("400 is terminal");

    assert_eq!(error.kind, ErrorKind::ProviderError.as_str());
    assert!(!error.retryable);
    assert_eq!(error.attempts.len(), 1);
    assert_eq!(error.attempts[0].retryable(), Some(false));
    assert_eq!(http.recorded_requests().len(), 1);
}

#[tokio::test]
async fn redirect_status_is_one_nonretryable_attempt_at_the_original_url() {
    let mut redirect = response(307, "redirect body");
    redirect.headers.insert(
        LOCATION,
        HeaderValue::from_static("https://credential-sink.invalid/collect"),
    );
    let http = ScriptedHttpClient::new([Ok(redirect)]);
    let client = scripted_client(http.clone(), MapEnv::new().with("GEMINI_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("gemini/x"))
        .await
        .expect_err("redirect status is terminal");

    assert_eq!(error.kind, ErrorKind::ProviderError.as_str());
    assert!(!error.retryable);
    assert_eq!(error.attempts.len(), 1);
    assert_eq!(error.attempts[0].retryable(), Some(false));
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url.as_str(),
        format!("{GEMINI_BASE}/v1beta/models/x:generateContent")
    );
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn monotonic_call_latency_brackets_discovery_preflight_and_retries() {
    let (clock, manual) = ManualClock::new(10_000);
    let http = ScriptedHttpClient::new_timed(
        [
            Ok(response(200, "{}")),
            Ok(response(500, "first generation failure")),
            Ok(ollama_success("{\"answer\":\"ok\"}")),
        ],
        manual.clone(),
        [
            ScriptedTimingStep::elapsed(7),
            ScriptedTimingStep::elapsed(11),
            ScriptedTimingStep::elapsed(13).finishing_wall_at(1_000),
        ],
    );
    let env = MapEnv::new()
        .with("SERGENT_OLLAMA_BASE_URL", OLLAMA_BASE)
        .advancing_clock_on_read(manual, 3);
    let client = scripted_client_with_clock(http, env, clock);

    let model_response = client
        .invoke(&request_for("ollama/x"))
        .await
        .expect("generation retry succeeds")
        .0;

    assert_eq!(model_response.usage.latency_ms, 37);
    assert_eq!(model_response.attempts.len(), 2);
    assert_eq!(model_response.attempts[0].timing().duration_ms(), 11);
    assert_eq!(model_response.attempts[1].timing().duration_ms(), 13);
    assert!(
        model_response.attempts[1].timing().finished_at()
            < model_response.attempts[1].timing().started_at(),
        "wall movement remains inert while monotonic elapsed stays truthful"
    );
}
