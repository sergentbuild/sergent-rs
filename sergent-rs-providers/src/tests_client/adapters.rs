//! Full provider-engine request and response shapes for the cloud adapters.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use sergent_rs_core::model::{
    Message, ModelClient, ModelRequestInput, ModelResponse, ModelSettings, ThinkingEffort,
};
use sergent_rs_core::proposal::{ProposalSchema, derive_proposal_schema};

use super::{
    ANTHROPIC_BASE, GEMINI_BASE, IMAGE_BASE64, OPENAI_BASE, SENTINEL,
    assert_evidence_excludes_secret, assert_sensitive_header, openai_success, parsed_object,
    request_with_image_for, request_with_settings_for, response, schema, scripted_client,
};
use crate::credentials::MapEnv;
use crate::http::{HttpPostRequest, ScriptedHttpClient};

#[tokio::test]
async fn openai_request_and_response_shape_crosses_the_provider_engine() {
    let raw = " \n{\"answer\":\"hi\"}\t ";
    let mut native = openai_success(raw);
    native.headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("req_openai_1"),
    );
    let http = ScriptedHttpClient::new([Ok(native)]);
    let client = scripted_client(http.clone(), MapEnv::new().with("OPENAI_API_KEY", SENTINEL));

    let (model_response, parsed) = client
        .invoke(&request_with_image_for("openai/gpt-5.5  "))
        .await
        .expect("OpenAI response succeeds");

    assert_openai_response(&model_response, parsed, raw);
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    let body = request.json_body();
    assert_openai_transport_and_model(request);
    assert_openai_messages(&body);
    assert_openai_schema_and_settings(&body);
}

/// Verifies OpenAI semantic output and returned call evidence.
fn assert_openai_response(
    model_response: &ModelResponse,
    parsed: Map<String, Value>,
    expected_raw: &str,
) {
    assert_eq!(parsed_object(parsed), json!({ "answer": "hi" }));
    assert_eq!(model_response.identity.model, "gpt-5.5  ");
    assert_eq!(model_response.raw_output, expected_raw);
    assert_eq!(model_response.attempts.len(), 1);
    assert!(model_response.attempts[0].is_success());
    let tokens = model_response
        .usage
        .tokens
        .as_ref()
        .expect("token evidence");
    assert_eq!(tokens.input, Some(2));
    assert_eq!(tokens.output, Some(3));
    assert_eq!(
        model_response.usage.request_id.as_deref(),
        Some("req_openai_1"),
        "the body resource id is not request-id evidence"
    );
    assert_evidence_excludes_secret(model_response);
}

/// Verifies OpenAI endpoint, model placement, timeout, and authentication.
fn assert_openai_transport_and_model(request: &HttpPostRequest) {
    assert_eq!(request.url.as_str(), format!("{OPENAI_BASE}/v1/responses"));
    assert!(!request.url.as_str().contains(SENTINEL));
    assert_eq!(request.timeout, Duration::from_secs(60));
    assert_sensitive_header(
        request,
        AUTHORIZATION.as_str(),
        &format!("Bearer {SENTINEL}"),
    );
    assert_eq!(request.json_body()["model"], "gpt-5.5  ");
}

/// Verifies OpenAI native system, user, and image content.
fn assert_openai_messages(body: &Value) {
    assert_eq!(body["input"][0]["role"], "system");
    assert_eq!(body["input"][0]["content"], "be terse");
    assert_eq!(body["input"][1]["role"], "user");
    assert_eq!(body["input"][1]["content"][0]["type"], "input_text");
    assert_eq!(body["input"][1]["content"][0]["text"], "inspect");
    assert_eq!(body["input"][1]["content"][1]["type"], "input_image");
    assert_eq!(
        body["input"][1]["content"][1]["image_url"],
        format!("data:image/png;base64,{IMAGE_BASE64}")
    );
}

/// Verifies OpenAI canonical schema placement and settings translation.
fn assert_openai_schema_and_settings(body: &Value) {
    assert_eq!(body["text"]["format"]["type"], "json_schema");
    assert_eq!(body["text"]["format"]["name"], "TestProposal");
    assert_eq!(body["text"]["format"]["schema"], *schema().json_schema());
    assert_eq!(body["text"]["format"]["strict"], true);
    assert_eq!(body["reasoning"]["effort"], "high");
    assert_eq!(body["max_output_tokens"], 4096);
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
/// Proves Anthropic lowering moves numeric refinements without weakening the integer structure.
struct Bounded {
    #[schemars(range(min = 0, max = 9))]
    count: u32,
}

#[tokio::test]
async fn anthropic_request_and_response_shape_lowers_schema_at_the_engine_boundary() {
    let mut native = response(
        200,
        json!({
            "id": "body-message-not-request-id",
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "{\"count\":2}" }],
            "usage": { "input_tokens": 10, "output_tokens": 4 }
        })
        .to_string(),
    );
    native.headers.insert(
        HeaderName::from_static("request-id"),
        HeaderValue::from_static("req_anthropic_1"),
    );
    let http = ScriptedHttpClient::new([Ok(native)]);
    let bounded = derive_proposal_schema::<Bounded>().expect("bounded schema");
    let model_request =
        ModelRequestInput::new(
            "anthropic/claude-opus-4.8".to_owned(),
            ModelSettings::default(),
            Arc::new(bounded),
        )
        .into_request(vec![
            Message::system("sys"),
            Message::user_with_images(
                "inspect",
                [sergent_rs_core::model::ImagePart::png(IMAGE_BASE64)
                    .expect("bounded PNG test image")],
            ),
        ]);
    let client = scripted_client(
        http.clone(),
        MapEnv::new().with("ANTHROPIC_API_KEY", SENTINEL),
    );

    let (model_response, parsed) = client
        .invoke(&model_request)
        .await
        .expect("Anthropic response succeeds");

    assert_anthropic_response(&model_response, parsed);
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    let body = request.json_body();
    assert_anthropic_transport_and_model(request);
    assert_anthropic_messages(&body);
    assert_anthropic_schema_and_settings(&body);
    assert_anthropic_lowering(&body, model_request.proposal_schema().as_ref());
}

/// Verifies Anthropic semantic output and returned call evidence.
fn assert_anthropic_response(model_response: &ModelResponse, parsed: Map<String, Value>) {
    assert_eq!(parsed_object(parsed), json!({ "count": 2 }));
    assert_eq!(model_response.raw_output, "{\"count\":2}");
    assert_eq!(
        model_response.usage.request_id.as_deref(),
        Some("req_anthropic_1"),
        "the body Message id is not request-id evidence"
    );
    let tokens = model_response
        .usage
        .tokens
        .as_ref()
        .expect("token evidence");
    assert_eq!(tokens.input, Some(10));
    assert_eq!(tokens.output, Some(4));
    assert_eq!(model_response.attempts.len(), 1);
    assert!(model_response.attempts[0].is_success());
    assert_evidence_excludes_secret(model_response);
}

/// Verifies Anthropic endpoint, model placement, timeout, and authentication.
fn assert_anthropic_transport_and_model(request: &HttpPostRequest) {
    assert_eq!(
        request.url.as_str(),
        format!("{ANTHROPIC_BASE}/v1/messages")
    );
    assert_eq!(request.timeout, Duration::from_secs(60));
    assert_sensitive_header(request, "x-api-key", SENTINEL);
    assert_eq!(
        request
            .headers
            .get("anthropic-version")
            .expect("version header"),
        "2023-06-01"
    );
    assert_eq!(request.json_body()["model"], "claude-opus-4.8");
}

/// Verifies Anthropic native system, user, and image content.
fn assert_anthropic_messages(body: &Value) {
    assert_eq!(body["system"], "sys");
    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(body["messages"][0]["content"][0]["text"], "inspect");
    let image = &body["messages"][0]["content"][1];
    assert_eq!(image["type"], "image");
    assert_eq!(image["source"]["type"], "base64");
    assert_eq!(image["source"]["media_type"], "image/png");
    assert_eq!(image["source"]["data"], IMAGE_BASE64);
}

/// Verifies Anthropic schema facility and settings translation.
fn assert_anthropic_schema_and_settings(body: &Value) {
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(body["output_config"]["effort"], "high");
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
}

/// Verifies Anthropic lowering preserves integer structure and canonical input.
fn assert_anthropic_lowering(body: &Value, canonical_schema: &ProposalSchema) {
    let count = &body["output_config"]["format"]["schema"]["properties"]["count"];
    assert!(count.get("minimum").is_none());
    assert!(count.get("maximum").is_none());
    assert_eq!(count["type"], "integer");
    let description = count["description"].as_str().expect("lowering note");
    assert!(description.contains("minimum: 0"));
    assert!(description.contains("maximum: 9"));

    let canonical_count = &canonical_schema.json_schema()["properties"]["count"];
    assert_eq!(canonical_count["minimum"], 0);
    assert_eq!(canonical_count["maximum"], 9);
    assert_eq!(canonical_count["type"], "integer");
}

#[tokio::test]
async fn gemini_request_and_response_shape_uses_header_auth_and_path_model() {
    let native = response(
        200,
        json!({
            "candidates": [{
                "content": { "parts": [{ "text": "{\"answer\":\"g\"}" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 11,
                "candidatesTokenCount": 5,
                "thoughtsTokenCount": 7,
                "totalTokenCount": 23
            },
            "additiveProviderField": true
        })
        .to_string(),
    );
    let http = ScriptedHttpClient::new([Ok(native)]);
    let client = scripted_client(http.clone(), MapEnv::new().with("GEMINI_API_KEY", SENTINEL));

    let (model_response, parsed) = client
        .invoke(&request_with_image_for("gemini/gemini-2.5-flash"))
        .await
        .expect("Gemini response succeeds");

    assert_gemini_response(&model_response, parsed);
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    let body = request.json_body();
    assert_gemini_transport(request);
    assert_gemini_schema_and_settings(&body);
    assert_gemini_messages(&body);
}

/// Verifies Gemini semantic output and returned call evidence.
fn assert_gemini_response(model_response: &ModelResponse, parsed: Map<String, Value>) {
    assert_eq!(parsed_object(parsed), json!({ "answer": "g" }));
    assert_eq!(model_response.raw_output, "{\"answer\":\"g\"}");
    assert_eq!(model_response.attempts.len(), 1);
    assert!(model_response.attempts[0].is_success());
    let tokens = model_response
        .usage
        .tokens
        .as_ref()
        .expect("token evidence");
    assert_eq!(tokens.input, Some(11));
    assert_eq!(
        tokens.output,
        Some(23),
        "Gemini normalizes totalTokenCount as output evidence"
    );
    assert!(model_response.usage.request_id.is_none());
    assert_evidence_excludes_secret(model_response);
}

/// Verifies Gemini model path, timeout, and header-only authentication.
fn assert_gemini_transport(request: &HttpPostRequest) {
    assert_eq!(
        request.url.as_str(),
        format!("{GEMINI_BASE}/v1beta/models/gemini-2.5-flash:generateContent")
    );
    assert!(!request.url.as_str().contains('?'));
    assert!(!request.url.as_str().contains(SENTINEL));
    assert_eq!(request.timeout, Duration::from_secs(60));
    assert_sensitive_header(request, "x-goog-api-key", SENTINEL);
}

/// Verifies Gemini canonical schema placement and settings translation.
fn assert_gemini_schema_and_settings(body: &Value) {
    assert_eq!(
        body["generationConfig"]["responseMimeType"],
        "application/json"
    );
    assert_eq!(
        body["generationConfig"]["responseJsonSchema"],
        *schema().json_schema()
    );
    assert_eq!(body["generationConfig"]["maxOutputTokens"], 4096);
    assert!(
        body["generationConfig"].get("thinkingConfig").is_none(),
        "Gemini does not receive the shared thinking-effort setting"
    );
}

/// Verifies Gemini native system, user, and image content.
fn assert_gemini_messages(body: &Value) {
    assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be terse");
    assert_eq!(body["contents"][0]["role"], "user");
    assert_eq!(body["contents"][0]["parts"][0]["text"], "inspect");
    let image = &body["contents"][0]["parts"][1]["inlineData"];
    assert_eq!(image["mimeType"], "image/png");
    assert_eq!(image["data"], IMAGE_BASE64);
}

/// Caller effort values and their promised native spellings.
const EFFORT_CASES: [(ThinkingEffort, &str); 3] = [
    (ThinkingEffort::Low, "low"),
    (ThinkingEffort::Medium, "medium"),
    (ThinkingEffort::High, "high"),
];

/// Capture real cloud-adapter requests while varying only caller effort.
async fn cloud_bodies_for_efforts(model: &str, credential: &str, native: Value) -> Vec<Value> {
    let http = ScriptedHttpClient::new(EFFORT_CASES.map(|_| Ok(response(200, native.to_string()))));
    let client = scripted_client(http.clone(), MapEnv::new().with(credential, SENTINEL));
    for (thinking_effort, _) in EFFORT_CASES {
        let request = request_with_settings_for(
            model,
            ModelSettings {
                thinking_effort,
                ..ModelSettings::default()
            },
        );
        client.invoke(&request).await.expect("generation succeeds");
    }
    let requests = http.recorded_requests();
    assert_eq!(requests.len(), EFFORT_CASES.len());
    requests.iter().map(HttpPostRequest::json_body).collect()
}

#[tokio::test]
async fn openai_preserves_all_three_thinking_efforts() {
    let bodies = cloud_bodies_for_efforts(
        "openai/x",
        "OPENAI_API_KEY",
        json!({
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{ "type": "output_text", "text": "{}" }]
            }]
        }),
    )
    .await;
    for (body, (_, expected)) in bodies.iter().zip(EFFORT_CASES) {
        assert_eq!(body["reasoning"]["effort"], expected);
    }
}

#[tokio::test]
async fn anthropic_enables_adaptive_thinking_at_all_three_efforts() {
    let bodies = cloud_bodies_for_efforts(
        "anthropic/x",
        "ANTHROPIC_API_KEY",
        json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "{}" }]
        }),
    )
    .await;
    for (body, (_, expected)) in bodies.iter().zip(EFFORT_CASES) {
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], expected);
    }
}

#[tokio::test]
async fn gemini_uses_provider_defaults_at_all_three_thinking_efforts() {
    let bodies = cloud_bodies_for_efforts(
        "gemini/x",
        "GEMINI_API_KEY",
        json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": { "parts": [{ "text": "{}" }] }
            }]
        }),
    )
    .await;
    for body in &bodies {
        assert!(body["generationConfig"].get("thinkingConfig").is_none());
    }
    assert!(bodies.windows(2).all(|pair| pair[0] == pair[1]));
}
