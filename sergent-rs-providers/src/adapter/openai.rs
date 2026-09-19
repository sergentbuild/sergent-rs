//! The OpenAI Responses adapter: schema in `text.format` json_schema strict.
//! @sergent-rs-providers/docs/providers.md

use serde::Deserialize;
use serde_json::{Value, json};

use sergent_rs_core::model::MessageRole;

use super::contract::{BuildInput, EnvelopeOutcome, PreparedRequest};
use super::shared::{credential_headers, data_uri, token_counts};
use crate::settings::effort_str;

/// Build one Responses request with strict schema mode and translated settings.
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
            let mut parts = vec![json!({ "type": "input_text", "text": message.content() })];
            for image in message.images() {
                parts.push(json!({ "type": "input_image", "image_url": data_uri(image) }));
            }
            Value::Array(parts)
        };
        messages.push(json!({ "role": role, "content": content }));
    }
    let body = json!({
        "model": input.model,
        "input": messages,
        "text": { "format": {
            "type": "json_schema",
            "name": input.schema.name(),
            "schema": input.schema.json_schema(),
            "strict": true,
        } },
        "reasoning": { "effort": effort_str(input.settings.thinking_effort) },
        "max_output_tokens": input.settings.max_output_tokens.get(),
    });
    PreparedRequest {
        url: input.endpoint.with_segments(["v1", "responses"]),
        headers: credential_headers(input.auth),
        body,
    }
}

/// Decode and classify one OpenAI success body through its minimal view.
pub(super) fn extract(body: &[u8]) -> Option<EnvelopeOutcome> {
    serde_json::from_slice(body).ok().map(classify)
}

/// Admit only completed, refusal-free output while preserving usage evidence.
fn classify(envelope: OpenAiEnvelope) -> EnvelopeOutcome {
    let tokens = envelope.usage.as_ref().and_then(|usage| {
        token_counts(
            usage.get("input_tokens").and_then(Value::as_u64),
            usage.get("output_tokens").and_then(Value::as_u64),
        )
    });
    let mut text = String::new();
    let mut observed_text = false;
    let mut refusal = None;
    for item in envelope.output {
        if let OpenAiOutputItem::Message { content } = item {
            for part in content {
                match part {
                    OpenAiContent::OutputText { text: chunk } => {
                        observed_text = true;
                        text.push_str(&chunk);
                    }
                    OpenAiContent::Refusal {
                        refusal: provider_reason,
                    } => {
                        if refusal.is_none() {
                            refusal = Some(provider_reason);
                        }
                    }
                }
            }
        }
    }

    if let Some(provider_reason) = refusal {
        return EnvelopeOutcome::NonNatural {
            reason: format!("openai refused: {provider_reason}"),
            text: observed_text.then_some(text),
            tokens,
        };
    }

    match envelope.status {
        OpenAiStatus::Completed => EnvelopeOutcome::Completed { text, tokens },
        status => {
            let mut reason = format!("openai response status was {}", status.as_str());
            if status == OpenAiStatus::Incomplete
                && let Some(detail) = envelope
                    .incomplete_details
                    .as_ref()
                    .and_then(|details| details.get("reason"))
                    .and_then(Value::as_str)
            {
                reason.push_str(": ");
                reason.push_str(detail);
            }
            EnvelopeOutcome::NonNatural {
                reason,
                text: observed_text.then_some(text),
                tokens,
            }
        }
    }
}

/// Minimal Responses success envelope required for completion admission.
#[derive(Deserialize)]
struct OpenAiEnvelope {
    status: OpenAiStatus,
    output: Vec<OpenAiOutputItem>,
    #[serde(default)]
    incomplete_details: Option<Value>,
    #[serde(default)]
    usage: Option<Value>,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OpenAiStatus {
    Completed,
    Failed,
    InProgress,
    Cancelled,
    Queued,
    Incomplete,
}

impl OpenAiStatus {
    /// Return the provider spelling used in bounded non-natural failure prose.
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::InProgress => "in_progress",
            Self::Cancelled => "cancelled",
            Self::Queued => "queued",
            Self::Incomplete => "incomplete",
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum OpenAiOutputItem {
    #[serde(rename = "message")]
    Message { content: Vec<OpenAiContent> },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum OpenAiContent {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "refusal")]
    Refusal { refusal: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_the_closed_openai_status_matrix() {
        let completed = json!({
            "status": "completed",
            "output": [
                { "type": "reasoning", "summary": [] },
                { "type": "message", "content": [
                    { "type": "output_text", "text": "{\"answer\":" }
                ] },
                { "type": "message", "content": [
                    { "type": "output_text", "text": "\"ok\"}" }
                ] }
            ],
            "additive": true
        });
        assert_completed(&completed, "{\"answer\":\"ok\"}");

        for status in ["failed", "in_progress", "cancelled", "queued", "incomplete"] {
            assert_refused(&json!({ "status": status, "output": [] }));
        }

        for envelope in [
            json!({ "output": [] }),
            json!({ "status": 7, "output": [] }),
            json!({ "status": "future_status", "output": [] }),
            json!({ "status": "completed" }),
            json!({ "status": "completed", "output": {} }),
            json!({ "status": "completed", "output": [ { "content": [] } ] }),
            json!({ "status": "completed", "output": [ {
                "type": "message", "content": {}
            } ] }),
            json!({ "status": "completed", "output": [ {
                "type": "message", "content": [ {
                    "type": "output_text", "text": 7
                } ]
            } ] }),
            json!({ "status": "completed", "output": [ {
                "type": "message", "content": [ {
                    "type": "refusal", "refusal": 7
                } ]
            } ] }),
            json!({ "status": "completed", "output": [ {
                "type": "message", "content": [ { "type": "future_content" } ]
            } ] }),
        ] {
            assert_invalid(&envelope);
        }
    }

    #[test]
    fn completed_status_with_a_refusal_is_not_accepted() {
        assert_refused(&json!({
            "status": "completed",
            "output": [ { "type": "message", "content": [
                { "type": "output_text", "text": "{\"answer\":\"partial\"}" },
                { "type": "refusal", "refusal": "policy" }
            ] } ]
        }));
    }

    /// Assert that a native envelope yields the expected completed text.
    fn assert_completed(envelope: &Value, expected_text: &str) {
        match extract(envelope.to_string().as_bytes()) {
            Some(EnvelopeOutcome::Completed { text, .. }) => assert_eq!(text, expected_text),
            _ => panic!("expected completed OpenAI envelope: {envelope}"),
        }
    }

    /// Assert that a recognized non-natural response is retained as such.
    fn assert_refused(envelope: &Value) {
        assert!(
            matches!(
                extract(envelope.to_string().as_bytes()),
                Some(EnvelopeOutcome::NonNatural { .. })
            ),
            "expected non-natural OpenAI envelope: {envelope}"
        );
    }

    /// Assert that malformed required native data fails envelope decoding.
    fn assert_invalid(envelope: &Value) {
        assert!(
            extract(envelope.to_string().as_bytes()).is_none(),
            "expected invalid OpenAI envelope: {envelope}"
        );
    }
}
