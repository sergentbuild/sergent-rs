//! Ollama adapter and once-per-invoke preflight contracts.

use std::time::Duration;

use reqwest::header::{AUTHORIZATION, HeaderValue, LOCATION};
use serde_json::{Map, Value, json};

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::{ModelClient, ModelResponse, ModelSettings, ThinkingEffort};

use super::{
    IMAGE_BASE64, OLLAMA_BASE, SENTINEL, assert_closed_failure_projection,
    assert_error_evidence_excludes_secret, assert_evidence_excludes_secret,
    assert_response_less_failure, assert_sensitive_header, ollama_success, parsed_object,
    request_for, request_with_image_for, request_with_settings_for, response, schema,
    scripted_client,
};
use crate::credentials::MapEnv;
use crate::http::{HttpFailure, HttpPostRequest, ScriptedHttpClient};

/// Supplies the hermetic Ollama endpoint used by the scripted transport tests.
fn ollama_env() -> MapEnv {
    MapEnv::new().with("SERGENT_OLLAMA_BASE_URL", OLLAMA_BASE)
}

#[tokio::test]
async fn ollama_preflight_and_adapter_shape_cross_the_provider_engine() {
    let http = ScriptedHttpClient::new([
        Ok(response(200, json!({ "model": "llama3:8b" }).to_string())),
        Ok(ollama_success("{\"answer\":\"o\"}")),
    ]);
    let client = scripted_client(http.clone(), ollama_env().with("OLLAMA_API_KEY", SENTINEL));

    let (model_response, parsed) = client
        .invoke(&request_with_image_for("ollama/llama3:8b"))
        .await
        .expect("Ollama response succeeds");

    assert_ollama_response(&model_response, parsed);
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 2, "preflight then generation");
    assert_ollama_preflight(&requests[0]);
    assert_ollama_generation(&requests[1]);
}

/// Verifies Ollama semantic output and returned call evidence.
fn assert_ollama_response(model_response: &ModelResponse, parsed: Map<String, Value>) {
    assert_eq!(parsed_object(parsed), json!({ "answer": "o" }));
    assert_eq!(model_response.raw_output, "{\"answer\":\"o\"}");
    assert_eq!(model_response.attempts.len(), 1);
    assert!(model_response.attempts[0].is_success());
    let tokens = model_response
        .usage
        .tokens
        .as_ref()
        .expect("token evidence");
    assert_eq!(tokens.input, Some(2));
    assert_eq!(tokens.output, Some(3));
    assert!(model_response.usage.request_id.is_none());
    assert_evidence_excludes_secret(model_response);
}

/// Verifies the once-per-invoke Ollama model-existence preflight.
fn assert_ollama_preflight(request: &HttpPostRequest) {
    assert_eq!(request.url.as_str(), format!("{OLLAMA_BASE}/api/show"));
    assert_eq!(request.json_body(), json!({ "model": "llama3:8b" }));
    assert_eq!(request.timeout, Duration::from_secs(60));
    assert_sensitive_header(
        request,
        AUTHORIZATION.as_str(),
        &format!("Bearer {SENTINEL}"),
    );
}

/// Verifies the Ollama generation endpoint, native body, and settings.
fn assert_ollama_generation(request: &HttpPostRequest) {
    assert_eq!(
        request.url.as_str(),
        format!("{OLLAMA_BASE}/v1/chat/completions")
    );
    assert_eq!(request.timeout, Duration::from_secs(60));
    assert_sensitive_header(
        request,
        AUTHORIZATION.as_str(),
        &format!("Bearer {SENTINEL}"),
    );
    let body = request.json_body();
    assert_eq!(body["model"], "llama3:8b");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "be terse");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][1]["content"][0]["type"], "text");
    assert_eq!(body["messages"][1]["content"][0]["text"], "inspect");
    assert_eq!(body["messages"][1]["content"][1]["type"], "image_url");
    assert_eq!(
        body["messages"][1]["content"][1]["image_url"]["url"],
        format!("data:image/png;base64,{IMAGE_BASE64}")
    );
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert_eq!(
        body["response_format"]["json_schema"]["name"],
        "TestProposal"
    );
    assert_eq!(
        body["response_format"]["json_schema"]["schema"],
        *schema().json_schema()
    );
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(body["reasoning_effort"], "high");
    assert!(body.get("think").is_none());
}

#[tokio::test]
async fn ollama_preserves_all_three_reasoning_efforts() {
    let http = ScriptedHttpClient::new([
        Ok(response(200, "{}")),
        Ok(ollama_success("{\"answer\":\"low\"}")),
        Ok(response(200, "{}")),
        Ok(ollama_success("{\"answer\":\"medium\"}")),
        Ok(response(200, "{}")),
        Ok(ollama_success("{\"answer\":\"high\"}")),
    ]);
    let client = scripted_client(http.clone(), ollama_env());

    for effort in [
        ThinkingEffort::Low,
        ThinkingEffort::Medium,
        ThinkingEffort::High,
    ] {
        let request = request_with_settings_for(
            "ollama/x",
            ModelSettings {
                thinking_effort: effort,
                ..ModelSettings::default()
            },
        );
        client.invoke(&request).await.expect("generation succeeds");
    }

    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 6);
    let efforts = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/v1/chat/completions"))
        .map(|request| {
            request.json_body()["reasoning_effort"]
                .as_str()
                .expect("reasoning effort")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(efforts, ["low", "medium", "high"]);
}

#[tokio::test]
async fn ollama_preflight_occurs_once_before_generation_retry() {
    let http = ScriptedHttpClient::new([
        Ok(response(200, "{}")),
        Ok(response(500, "first generation failure")),
        Ok(ollama_success("{\"answer\":\"retried\"}")),
    ]);
    let client = scripted_client(http.clone(), ollama_env());

    let model_response = client
        .invoke(&request_for("ollama/retry-model"))
        .await
        .expect("generation retry succeeds")
        .0;

    assert_eq!(model_response.attempts.len(), 2);
    assert!(!model_response.attempts[0].is_success());
    assert!(model_response.attempts[1].is_success());
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].url.as_str(), format!("{OLLAMA_BASE}/api/show"));
    assert_eq!(
        requests[1].url.as_str(),
        format!("{OLLAMA_BASE}/v1/chat/completions")
    );
    assert_eq!(requests[1], requests[2]);
}

#[tokio::test]
async fn ollama_preflight_404_is_model_not_found_without_generation_attempt() {
    let http = ScriptedHttpClient::new([Ok(response(404, "missing"))]);
    let client = scripted_client(http.clone(), ollama_env());
    let request = request_for("ollama/missing");

    let error = client.invoke(&request).await.expect_err("preflight fails");
    assert_closed_failure_projection(&request, &error);

    assert_eq!(error.kind, ErrorKind::ModelNotFound.as_str());
    assert!(!error.retryable);
    assert!(error.attempts.is_empty());
    let usage = error.usage.as_ref().expect("completed preflight usage");
    assert_eq!(usage.latency_ms, 0);
    assert!(usage.tokens.is_none());
    assert!(usage.request_id.is_none());
    assert!(error.raw_output.is_none());
    assert_eq!(http.recorded_requests().len(), 1);
}

#[tokio::test]
async fn ollama_preflight_500_is_provider_unavailable_without_generation_attempt() {
    let http = ScriptedHttpClient::new([Ok(response(500, "unavailable"))]);
    let client = scripted_client(http.clone(), ollama_env());

    let error = client
        .invoke(&request_for("ollama/x"))
        .await
        .expect_err("preflight fails");

    assert_eq!(error.kind, ErrorKind::ProviderUnavailable.as_str());
    assert!(!error.retryable);
    assert!(error.attempts.is_empty());
    assert_eq!(http.recorded_requests().len(), 1);
}

#[tokio::test]
async fn ollama_preflight_redirect_is_provider_error_without_a_second_request() {
    let mut redirect = response(307, "redirect");
    redirect.headers.insert(
        LOCATION,
        HeaderValue::from_static("https://credential-sink.invalid/api/show"),
    );
    let http = ScriptedHttpClient::new([Ok(redirect)]);
    let client = scripted_client(http.clone(), ollama_env().with("OLLAMA_API_KEY", SENTINEL));

    let error = client
        .invoke(&request_for("ollama/x"))
        .await
        .expect_err("preflight redirect fails");

    assert_eq!(error.kind, ErrorKind::ProviderError.as_str());
    assert!(!error.retryable);
    assert!(error.attempts.is_empty());
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.as_str(), format!("{OLLAMA_BASE}/api/show"));
    assert_error_evidence_excludes_secret(&error);
}

#[tokio::test]
async fn ollama_preflight_typed_transport_failure_is_provider_unavailable() {
    let http = ScriptedHttpClient::new([Err(HttpFailure::Connection)]);
    let client = scripted_client(http.clone(), ollama_env());

    let error = client
        .invoke(&request_for("ollama/x"))
        .await
        .expect_err("preflight transport failure");

    assert_eq!(error.kind, ErrorKind::ProviderUnavailable.as_str());
    assert!(!error.retryable);
    assert_eq!(error.message, "the ollama daemon is unreachable");
    assert!(error.attempts.is_empty());
    assert_response_less_failure(&error);
    assert_eq!(http.recorded_requests().len(), 1);
}

#[tokio::test]
async fn ollama_invalid_endpoint_fails_before_preflight_or_generation() {
    let http = ScriptedHttpClient::new([]);
    let client = scripted_client(
        http.clone(),
        MapEnv::new().with(
            "SERGENT_OLLAMA_BASE_URL",
            "http://ollama.scripted.invalid/api",
        ),
    );

    let error = client
        .invoke(&request_for("ollama/x"))
        .await
        .expect_err("invalid endpoint");

    assert_eq!(error.kind, ErrorKind::ProviderUnavailable.as_str());
    assert!(!error.retryable);
    assert!(error.attempts.is_empty());
    assert_response_less_failure(&error);
    assert!(http.recorded_requests().is_empty());
}

#[tokio::test]
async fn ollama_generation_400_is_one_nonretryable_invalid_payload_attempt() {
    let http = ScriptedHttpClient::new([
        Ok(response(200, "{}")),
        Ok(response(400, "invalid generation payload")),
    ]);
    let client = scripted_client(http.clone(), ollama_env());

    let error = client
        .invoke(&request_for("ollama/x"))
        .await
        .expect_err("generation status fails");

    assert_eq!(error.kind, ErrorKind::InvalidPayload.as_str());
    assert!(!error.retryable);
    assert_eq!(error.attempts.len(), 1);
    assert_eq!(error.attempts[0].retryable(), Some(false));
    assert_eq!(http.recorded_requests().len(), 2);
}
