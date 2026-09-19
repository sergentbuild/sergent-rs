//! Exact model-call payload, usage, identity, and attempt serialization.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::model::{
    Attempt, CallUsage, Message, ModelError, ModelIdentity, ModelRequestInput, ModelResponse,
    ModelSettings, TokenCounts,
};
use sergent_rs_core::proposal::derive_proposal_schema;
use sergent_rs_core::run_record::OpenModelCall;
use sergent_rs_core::timing::{TimeSpan, Timestamp};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct CallProposal {
    answer: String,
}

fn span() -> TimeSpan {
    TimeSpan::closed(
        Timestamp::from_unix_micros(0),
        Timestamp::from_unix_micros(1_000),
        1,
    )
}

fn request() -> sergent_rs_core::model::ModelRequest {
    ModelRequestInput::new(
        "openai/gpt".to_owned(),
        ModelSettings::default(),
        Arc::new(derive_proposal_schema::<CallProposal>().unwrap()),
    )
    .into_request(vec![Message::system("decide"), Message::user("context")])
}

fn identity() -> ModelIdentity {
    ModelIdentity {
        provider: "openai".to_owned(),
        model: "gpt".to_owned(),
        sdk_package: None,
        sdk_version: None,
    }
}

fn usage() -> CallUsage {
    CallUsage {
        latency_ms: 12,
        tokens: Some(TokenCounts {
            input: Some(3),
            output: Some(5),
        }),
        request_id: None,
    }
}

fn failed_attempt(retryable: bool, message: &str) -> Attempt {
    Attempt::failure(
        span(),
        retryable,
        RunError::of(ErrorKind::ProviderUnavailable, message).with("retryable", retryable),
    )
}

fn request_capture(request: &sergent_rs_core::model::ModelRequest) -> Value {
    json!({
        "value": {
            "model_name": request.model_name(),
            "messages": [
                { "role": "system", "content": "decide" },
                { "role": "user", "content": "context" }
            ],
            "model_settings": serde_json::to_value(request.model_settings()).unwrap()
        },
        "value_type": std::any::type_name::<sergent_rs_core::model::ModelRequest>(),
        "error": null,
        "status": "captured"
    })
}

fn call_record(
    request: &sergent_rs_core::model::ModelRequest,
    identity: Option<ModelIdentity>,
    raw_response: Option<&str>,
    parsed_json: Option<Value>,
    parsed_proposal: Option<Value>,
    usage: Option<CallUsage>,
    attempts: Vec<Attempt>,
) -> Value {
    json!({
        "proposal_schema": serde_json::to_value(request.proposal_schema().as_ref()).unwrap(),
        "model_name": request.model_name(),
        "identity": identity,
        "payloads": {
            "request": request_capture(request),
            "raw_response": raw_response,
            "parsed_json": parsed_json,
            "parsed_proposal": parsed_proposal
        },
        "usage": usage,
        "attempts": attempts
    })
}

fn proposal_capture() -> Value {
    json!({
        "value": { "answer": "ok" },
        "value_type": std::any::type_name::<CallProposal>(),
        "error": null,
        "status": "captured"
    })
}

#[test]
fn interrupted_call_serializes_exact_request_only_evidence() {
    let request = request();
    let record = OpenModelCall::new(&request).interrupted();
    assert_eq!(
        serde_json::to_value(record).unwrap(),
        call_record(&request, None, None, None, None, None, Vec::new())
    );
}

#[test]
fn completed_and_accepted_call_captures_the_concrete_proposal() {
    let request = request();
    let parsed = serde_json::Map::from_iter([("answer".to_owned(), json!("ok"))]);
    let response = ModelResponse {
        identity: identity(),
        raw_output: r#"{"answer":"ok"}"#.to_owned(),
        attempts: vec![Attempt::success(span())],
        usage: usage(),
    };
    let proposal = CallProposal {
        answer: "ok".to_owned(),
    };
    let record = OpenModelCall::new(&request)
        .completed(&response, &parsed)
        .accepted(&proposal);
    assert_eq!(
        serde_json::to_value(record).unwrap(),
        call_record(
            &request,
            Some(identity()),
            Some(&response.raw_output),
            Some(Value::Object(parsed)),
            Some(proposal_capture()),
            Some(usage()),
            vec![Attempt::success(span())]
        )
    );
}

#[test]
fn failed_typed_crossing_retains_completion_and_null_proposal() {
    let request = request();
    let parsed = serde_json::Map::from_iter([("answer".to_owned(), json!(7))]);
    let response = ModelResponse {
        identity: identity(),
        raw_output: r#"{"answer":7}"#.to_owned(),
        attempts: vec![Attempt::success(span())],
        usage: usage(),
    };
    let record = OpenModelCall::new(&request)
        .completed(&response, &parsed)
        .rejected();
    assert_eq!(
        serde_json::to_value(record).unwrap(),
        call_record(
            &request,
            Some(identity()),
            Some(&response.raw_output),
            Some(Value::Object(parsed)),
            None,
            Some(usage()),
            vec![Attempt::success(span())]
        )
    );
}

#[test]
fn provider_failure_retains_only_reached_response_facts() {
    let request = request();
    let response_usage = CallUsage {
        latency_ms: 12,
        tokens: None,
        request_id: None,
    };
    let error = ModelError {
        kind: "provider_overload".to_owned(),
        retryable: true,
        message: "busy".to_owned(),
        raw_output: Some("partial".to_owned()),
        identity: Some(identity()),
        attempts: vec![failed_attempt(true, "busy")],
        usage: Some(response_usage.clone()),
    };
    assert_eq!(
        serde_json::to_value(OpenModelCall::new(&request).failed(&error)).unwrap(),
        call_record(
            &request,
            Some(identity()),
            Some("partial"),
            None,
            None,
            Some(response_usage),
            error.attempts
        )
    );
}

#[test]
fn response_less_provider_failure_does_not_invent_completion_facts() {
    let request = request();
    let error = ModelError {
        kind: "transport".to_owned(),
        retryable: true,
        message: "offline".to_owned(),
        raw_output: None,
        identity: None,
        attempts: Vec::new(),
        usage: None,
    };

    assert_eq!(
        serde_json::to_value(OpenModelCall::new(&request).failed(&error)).unwrap(),
        call_record(&request, None, None, None, None, None, Vec::new())
    );
}

#[test]
fn accepted_call_retains_every_retry_attempt_in_order() {
    let request = request();
    let parsed = serde_json::Map::from_iter([("answer".to_owned(), json!("ok"))]);
    let attempts = vec![failed_attempt(true, "retry"), Attempt::success(span())];
    let response = ModelResponse {
        identity: identity(),
        raw_output: r#"{"answer":"ok"}"#.to_owned(),
        attempts: attempts.clone(),
        usage: usage(),
    };
    let record = OpenModelCall::new(&request)
        .completed(&response, &parsed)
        .accepted(&CallProposal {
            answer: "ok".to_owned(),
        });

    assert_eq!(
        serde_json::to_value(record).unwrap(),
        call_record(
            &request,
            Some(identity()),
            Some(&response.raw_output),
            Some(Value::Object(parsed)),
            Some(proposal_capture()),
            Some(usage()),
            attempts
        )
    );
}

#[test]
fn failed_attempts_emit_exact_metadata_matching_the_sibling_retryability() {
    for retryable in [true, false] {
        assert_eq!(
            serde_json::to_value(failed_attempt(retryable, "failed")).unwrap(),
            json!({
                "timing": serde_json::to_value(span()).unwrap(),
                "status": "failure",
                "retryable": retryable,
                "error": {
                    "kind": "provider_unavailable",
                    "message": "failed",
                    "metadata": { "retryable": retryable }
                }
            })
        );
    }
}

#[test]
fn attempts_and_optional_evidence_always_emit_exact_nulls() {
    assert_eq!(
        serde_json::to_value(Attempt::success(span())).unwrap(),
        json!({
            "timing": serde_json::to_value(span()).unwrap(),
            "status": "success",
            "retryable": null,
            "error": null
        })
    );
    assert_eq!(
        serde_json::to_value(ModelIdentity {
            provider: "p".to_owned(),
            model: "m".to_owned(),
            sdk_package: None,
            sdk_version: None,
        })
        .unwrap(),
        json!({
            "provider": "p",
            "model": "m",
            "sdk_package": null,
            "sdk_version": null
        })
    );
}

#[test]
fn call_usage_serializes_only_reported_token_counts() {
    for (tokens, expected) in [
        (
            Some(TokenCounts {
                input: Some(0),
                output: Some(5),
            }),
            json!({ "input": 0, "output": 5 }),
        ),
        (
            Some(TokenCounts {
                input: Some(3),
                output: None,
            }),
            json!({ "input": 3 }),
        ),
        (
            Some(TokenCounts {
                input: None,
                output: Some(5),
            }),
            json!({ "output": 5 }),
        ),
        (None, Value::Null),
    ] {
        assert_eq!(
            serde_json::to_value(CallUsage {
                latency_ms: 1,
                tokens,
                request_id: None,
            })
            .unwrap(),
            json!({ "latency_ms": 1, "tokens": expected, "request_id": null })
        );
    }
}
