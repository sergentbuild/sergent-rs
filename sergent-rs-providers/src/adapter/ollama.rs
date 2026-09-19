//! The Ollama adapter: OpenAI-compatible chat completions against the
//! configured local daemon; schema in `response_format` json_schema strict.
//! Generation 400/422 map to invalid_payload and 404 to model_not_found.
//! @sergent-rs-providers/docs/providers.md

use serde::Deserialize;
use serde_json::{Value, json};

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::MessageRole;

use super::contract::{BuildInput, EnvelopeOutcome, PreparedRequest};
use super::shared::{credential_headers, data_uri, generic_status, token_counts};
use crate::settings::effort_str;

/// Build one OpenAI-compatible chat request with strict schema mode.
pub(super) fn build(input: &BuildInput<'_>) -> PreparedRequest {
    let mut messages = Vec::new();
    for message in input.messages {
        let role = match message.role() {
            MessageRole::System => "system",
            MessageRole::User => "user",
        };
        let content = if message.images().is_empty() {
            Value::String(message.content().to_owned())
        } else {
            let mut parts = vec![json!({ "type": "text", "text": message.content() })];
            for image in message.images() {
                parts.push(json!({ "type": "image_url", "image_url": { "url": data_uri(image) } }));
            }
            Value::Array(parts)
        };
        messages.push(json!({ "role": role, "content": content }));
    }

    let body = json!({
        "model": input.model,
        "messages": messages,
        "response_format": { "type": "json_schema", "json_schema": {
            "name": input.schema.name(),
            "schema": input.schema.json_schema(),
            "strict": true,
        } },
        "max_tokens": input.settings.max_output_tokens.get(),
        "reasoning_effort": effort_str(input.settings.thinking_effort),
    });

    PreparedRequest {
        url: input.endpoint.with_segments(["v1", "chat", "completions"]),
        headers: credential_headers(input.auth),
        body,
    }
}

/// Decode an Ollama success body and require its single-choice contract.
pub(super) fn extract(body: &[u8]) -> Option<EnvelopeOutcome> {
    serde_json::from_slice(body).ok().and_then(classify)
}

/// Admit exactly one `stop` choice while retaining usage and partial text.
fn classify(envelope: OllamaEnvelope) -> Option<EnvelopeOutcome> {
    let tokens = envelope.usage.as_ref().and_then(|usage| {
        token_counts(
            usage.get("prompt_tokens").and_then(Value::as_u64),
            usage.get("completion_tokens").and_then(Value::as_u64),
        )
    });
    let [choice] = <[OllamaChoice; 1]>::try_from(envelope.choices).ok()?;
    Some(match choice.finish_reason {
        OllamaFinishReason::Stop => EnvelopeOutcome::Completed {
            text: choice.message.content,
            tokens,
        },
        finish_reason => EnvelopeOutcome::NonNatural {
            reason: format!("ollama finish_reason was {}", finish_reason.as_str()),
            text: Some(choice.message.content),
            tokens,
        },
    })
}

/// Minimal chat-completions success view used for admission and usage.
#[derive(Deserialize)]
struct OllamaEnvelope {
    choices: Vec<OllamaChoice>,
    #[serde(default)]
    usage: Option<Value>,
}

/// One completion choice with the two carriers required for admission.
#[derive(Deserialize)]
struct OllamaChoice {
    finish_reason: OllamaFinishReason,
    message: OllamaMessage,
}

/// The assistant content consumed as semantic output or partial evidence.
#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OllamaFinishReason {
    Stop,
    Length,
    ToolCalls,
}

impl OllamaFinishReason {
    /// Return the provider spelling used in bounded non-natural failure prose.
    fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ToolCalls => "tool_calls",
        }
    }
}

/// Apply Ollama payload and model errors before shared HTTP status mapping.
pub(super) fn map_generation_status(status: u16) -> (ErrorKind, bool) {
    match status {
        400 | 422 => (ErrorKind::InvalidPayload, false),
        404 => (ErrorKind::ModelNotFound, false),
        other => generic_status(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_status_matrix() {
        assert_eq!(
            map_generation_status(400),
            (ErrorKind::InvalidPayload, false)
        );
        assert_eq!(
            map_generation_status(422),
            (ErrorKind::InvalidPayload, false)
        );
        assert_eq!(
            map_generation_status(404),
            (ErrorKind::ModelNotFound, false)
        );
        assert_eq!(map_generation_status(429), (ErrorKind::RateLimited, true));
        assert_eq!(
            map_generation_status(500),
            (ErrorKind::ProviderUnavailable, true)
        );
    }

    #[test]
    fn classifies_the_closed_ollama_finish_reason_matrix() {
        assert_completed(
            &json!({
                "choices": [ {
                    "finish_reason": "stop",
                    "message": { "content": "{\"answer\":\"ok\"}" },
                    "index": 99
                } ],
                "additive": true
            }),
            "{\"answer\":\"ok\"}",
        );

        for finish_reason in ["length", "tool_calls"] {
            assert_refused(&json!({
                "choices": [ {
                    "finish_reason": finish_reason,
                    "message": { "content": "{\"answer\":\"partial\"}" }
                } ]
            }));
        }

        for envelope in [
            json!({ "choices": [ { "message": { "content": "{}" } } ] }),
            json!({ "choices": [ {
                "finish_reason": 7,
                "message": { "content": "{}" }
            } ] }),
            json!({ "choices": [ {
                "finish_reason": "future_reason",
                "message": { "content": "{}" }
            } ] }),
            json!({ "choices": [ {
                "finish_reason": "stop",
                "message": {}
            } ] }),
            json!({ "choices": [ {
                "finish_reason": "stop",
                "message": { "content": 7 }
            } ] }),
            json!({ "choices": [] }),
            json!({ "choices": [
                { "finish_reason": "stop", "message": { "content": "{}" } },
                { "finish_reason": "stop", "message": { "content": "{}" } }
            ] }),
        ] {
            assert_invalid(&envelope);
        }
    }

    /// Assert that a native envelope yields the expected completed text.
    fn assert_completed(envelope: &Value, expected_text: &str) {
        match extract(envelope.to_string().as_bytes()) {
            Some(EnvelopeOutcome::Completed { text, .. }) => assert_eq!(text, expected_text),
            _ => panic!("expected completed Ollama envelope: {envelope}"),
        }
    }

    /// Assert that a recognized non-natural finish state is retained as such.
    fn assert_refused(envelope: &Value) {
        assert!(
            matches!(
                extract(envelope.to_string().as_bytes()),
                Some(EnvelopeOutcome::NonNatural { .. })
            ),
            "expected non-natural Ollama envelope: {envelope}"
        );
    }

    /// Assert that malformed required native data fails envelope decoding.
    fn assert_invalid(envelope: &Value) {
        assert!(
            extract(envelope.to_string().as_bytes()).is_none(),
            "expected invalid Ollama envelope: {envelope}"
        );
    }
}
