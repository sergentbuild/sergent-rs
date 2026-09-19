//! The Anthropic Messages adapter: adaptive thinking, schema in
//! `output_config.format` json_schema after client-side lowering, and system
//! text carried in the `system` field.
//! @sergent-rs-providers/docs/providers.md

use reqwest::header::{HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};

use super::contract::{BuildInput, EnvelopeOutcome, PreparedRequest};
use super::shared::{credential_headers, system_text, token_counts, user_messages};
use crate::lowering::lower_for_anthropic;
use crate::settings::effort_str;

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Build one Messages request with lowered schema and translated settings.
pub(super) fn build(input: &BuildInput<'_>) -> PreparedRequest {
    let mut messages = Vec::new();
    for message in user_messages(input.messages) {
        let content = if message.images().is_empty() {
            Value::String(message.content().to_owned())
        } else {
            let mut parts = vec![json!({ "type": "text", "text": message.content() })];
            for image in message.images() {
                parts.push(json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": image.media_type(),
                        "data": image.data_base64(),
                    }
                }));
            }
            Value::Array(parts)
        };
        messages.push(json!({ "role": "user", "content": content }));
    }

    let lowered = lower_for_anthropic(input.schema.json_schema());
    let mut body = json!({
        "model": input.model,
        "max_tokens": input.settings.max_output_tokens.get(),
        "messages": messages,
        "thinking": { "type": "adaptive" },
        "output_config": {
            "format": { "type": "json_schema", "schema": lowered },
            "effort": effort_str(input.settings.thinking_effort),
        },
    });
    if let Some(system) = system_text(input.messages) {
        body.as_object_mut()
            .expect("json object")
            .insert("system".to_owned(), Value::String(system));
    }

    let mut headers = credential_headers(input.auth);
    headers.insert(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    PreparedRequest {
        url: input.endpoint.with_segments(["v1", "messages"]),
        headers,
        body,
    }
}

/// Decode and classify one Anthropic success body through its minimal view.
pub(super) fn extract(body: &[u8]) -> Option<EnvelopeOutcome> {
    serde_json::from_slice(body).ok().map(classify)
}

/// Accept only `end_turn`, concatenating text and normalizing optional usage.
fn classify(envelope: AnthropicEnvelope) -> EnvelopeOutcome {
    let tokens = envelope.usage.as_ref().and_then(|usage| {
        token_counts(
            usage.get("input_tokens").and_then(Value::as_u64),
            usage.get("output_tokens").and_then(Value::as_u64),
        )
    });
    let mut text = String::new();
    let mut observed_text = false;
    for block in envelope.content {
        if let AnthropicContent::Text { text: chunk } = block {
            observed_text = true;
            text.push_str(&chunk);
        }
    }

    match envelope.stop_reason {
        AnthropicStopReason::EndTurn => EnvelopeOutcome::Completed { text, tokens },
        stop_reason => EnvelopeOutcome::NonNatural {
            reason: format!("anthropic stop_reason was {}", stop_reason.as_str()),
            text: observed_text.then_some(text),
            tokens,
        },
    }
}

/// Minimal Messages success envelope required for completion admission.
#[derive(Deserialize)]
struct AnthropicEnvelope {
    stop_reason: AnthropicStopReason,
    content: Vec<AnthropicContent>,
    #[serde(default)]
    usage: Option<Value>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AnthropicStopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    PauseTurn,
    Refusal,
    ModelContextWindowExceeded,
}

impl AnthropicStopReason {
    /// Return the provider spelling used in bounded non-natural failure prose.
    fn as_str(self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::MaxTokens => "max_tokens",
            Self::StopSequence => "stop_sequence",
            Self::ToolUse => "tool_use",
            Self::PauseTurn => "pause_turn",
            Self::Refusal => "refusal",
            Self::ModelContextWindowExceeded => "model_context_window_exceeded",
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_the_closed_anthropic_stop_reason_matrix() {
        let completed = json!({
            "stop_reason": "end_turn",
            "content": [
                { "type": "thinking", "thinking": "ignored" },
                { "type": "text", "text": "{\"answer\":" },
                { "type": "future_block" },
                { "type": "text", "text": "\"ok\"}" }
            ],
            "additive": true
        });
        assert_completed(&completed, "{\"answer\":\"ok\"}");

        for stop_reason in [
            "max_tokens",
            "stop_sequence",
            "tool_use",
            "pause_turn",
            "refusal",
            "model_context_window_exceeded",
        ] {
            assert_refused(&json!({ "stop_reason": stop_reason, "content": [] }));
        }

        for envelope in [
            json!({ "content": [] }),
            json!({ "stop_reason": 7, "content": [] }),
            json!({ "stop_reason": "future_reason", "content": [] }),
            json!({ "stop_reason": "end_turn" }),
            json!({ "stop_reason": "end_turn", "content": {} }),
            json!({ "stop_reason": "end_turn", "content": [ {} ] }),
            json!({ "stop_reason": "end_turn", "content": [ { "type": 7 } ] }),
            json!({ "stop_reason": "end_turn", "content": [ {
                "type": "text", "text": 7
            } ] }),
        ] {
            assert_invalid(&envelope);
        }
    }

    /// Assert that a native envelope yields the expected completed text.
    fn assert_completed(envelope: &Value, expected_text: &str) {
        match extract(envelope.to_string().as_bytes()) {
            Some(EnvelopeOutcome::Completed { text, .. }) => assert_eq!(text, expected_text),
            _ => panic!("expected completed Anthropic envelope: {envelope}"),
        }
    }

    /// Assert that a recognized non-natural stop state is retained as such.
    fn assert_refused(envelope: &Value) {
        assert!(
            matches!(
                extract(envelope.to_string().as_bytes()),
                Some(EnvelopeOutcome::NonNatural { .. })
            ),
            "expected non-natural Anthropic envelope: {envelope}"
        );
    }

    /// Assert that malformed required native data fails envelope decoding.
    fn assert_invalid(envelope: &Value) {
        assert!(
            extract(envelope.to_string().as_bytes()).is_none(),
            "expected invalid Anthropic envelope: {envelope}"
        );
    }
}
