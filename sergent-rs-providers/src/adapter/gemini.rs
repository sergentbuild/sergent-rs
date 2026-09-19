//! The Gemini generateContent adapter: schema in
//! `generationConfig.responseJsonSchema` plus `responseMimeType`; system text in
//! `systemInstruction`. Model remainder is placed in the URL path.
//! @sergent-rs-providers/docs/providers.md

use serde::Deserialize;
use serde_json::{Value, json};

use super::contract::{BuildInput, EnvelopeOutcome, PreparedRequest};
use super::shared::{credential_headers, system_text, token_counts, user_messages};

/// Build one generateContent request with the canonical schema unchanged.
pub(super) fn build(input: &BuildInput<'_>) -> PreparedRequest {
    let mut contents = Vec::new();
    for message in user_messages(input.messages) {
        let mut parts = vec![json!({ "text": message.content() })];
        for image in message.images() {
            parts.push(json!({
                "inlineData": { "mimeType": image.media_type(), "data": image.data_base64() }
            }));
        }
        contents.push(json!({ "role": "user", "parts": parts }));
    }

    let mut body = json!({
        "contents": contents,
        "generationConfig": {
            "responseMimeType": "application/json",
            "responseJsonSchema": input.schema.json_schema(),
            "maxOutputTokens": input.settings.max_output_tokens.get(),
        },
    });
    if let Some(system) = system_text(input.messages) {
        body.as_object_mut().expect("json object").insert(
            "systemInstruction".to_owned(),
            json!({ "parts": [ { "text": system } ] }),
        );
    }

    PreparedRequest {
        url: input.endpoint.with_segments([
            "v1beta",
            "models",
            &format!("{}:generateContent", input.model),
        ]),
        headers: credential_headers(input.auth),
        body,
    }
}

/// Cross one Gemini success body directly into the typed fields that own
/// completion classification. Additive provider fields remain allowed.
pub(super) fn extract(body: &[u8]) -> Option<EnvelopeOutcome> {
    serde_json::from_slice(body).ok().map(classify)
}

/// Admit only non-blocked `STOP` candidates and concatenate their text parts.
fn classify(envelope: GeminiEnvelope) -> EnvelopeOutcome {
    let tokens = envelope.usage_metadata.as_ref().and_then(|usage| {
        token_counts(
            usage.get("promptTokenCount").and_then(Value::as_u64),
            usage.get("totalTokenCount").and_then(Value::as_u64),
        )
    });
    let text = candidate_text(&envelope.candidates);

    if let Some(feedback) = &envelope.prompt_feedback
        && let Some(reason) = &feedback.block_reason
    {
        let base = format!("gemini prompt blocked: {reason}");
        let message = feedback
            .block_reason_message
            .as_ref()
            .and_then(Value::as_str);
        return EnvelopeOutcome::NonNatural {
            reason: semantic_reason(base, message),
            text,
            tokens,
        };
    }

    if envelope.candidates.is_empty() {
        return EnvelopeOutcome::NonNatural {
            reason: "gemini response contained no candidates".to_owned(),
            text: None,
            tokens,
        };
    }

    for candidate in &envelope.candidates {
        if candidate.finish_reason != "STOP" {
            let base = format!("gemini candidate finished with {}", candidate.finish_reason);
            let message = candidate.finish_message.as_ref().and_then(Value::as_str);
            return EnvelopeOutcome::NonNatural {
                reason: semantic_reason(base, message),
                text,
                tokens,
            };
        }
    }

    EnvelopeOutcome::Completed {
        text: text.unwrap_or_default(),
        tokens,
    }
}

/// The semantic text this envelope carries, concatenated in provider order: the
/// completed output on admission, the partial output otherwise.
fn candidate_text(candidates: &[GeminiCandidate]) -> Option<String> {
    let mut text = String::new();
    let mut observed = false;
    for part in candidates
        .iter()
        .flat_map(|candidate| candidate.content.parts.iter())
    {
        if let Some(chunk) = &part.text {
            observed = true;
            text.push_str(chunk);
        }
    }
    observed.then_some(text)
}

/// Append a nonempty provider message to its non-natural completion reason.
fn semantic_reason(base: String, message: Option<&str>) -> String {
    match message {
        Some(message) if !message.is_empty() => format!("{base}: {message}"),
        _ => base,
    }
}

/// Minimal generateContent view that owns admission: only the completion state
/// and the consumed content carriers are typed, so an unexpected usage or
/// message shape degrades to absent evidence instead of rejecting the envelope.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiEnvelope {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    prompt_feedback: Option<GeminiPromptFeedback>,
    #[serde(default)]
    usage_metadata: Option<Value>,
}

/// Optional prompt-block facts: the reason discriminates a blocked prompt, its
/// message may enrich only the bounded human failure prose.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPromptFeedback {
    #[serde(default)]
    block_reason: Option<String>,
    #[serde(default)]
    block_reason_message: Option<Value>,
}

/// One candidate with its required completion state and optional text content.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    #[serde(default)]
    content: GeminiContent,
    finish_reason: String,
    #[serde(default)]
    finish_message: Option<Value>,
}

/// Candidate content blocks consumed in provider order.
#[derive(Default, Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

/// One content block whose text, when present, enters semantic output.
#[derive(Deserialize)]
struct GeminiPart {
    #[serde(default)]
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_the_gemini_completion_matrix() {
        assert_completed(
            &json!({
                "candidates": [ {
                    "content": { "parts": [
                        { "text": "{\"answer\":" },
                        { "text": "\"ok\"}" }
                    ] },
                    "finishReason": "STOP"
                } ],
                "additiveProviderField": true
            }),
            "{\"answer\":\"ok\"}",
        );

        for envelope in [
            json!({ "candidates": [] }),
            json!({ "promptFeedback": { "blockReason": "SAFETY" } }),
            json!({ "candidates": [ {
                "content": { "parts": [ { "text": "{}" } ] },
                "finishReason": "MAX_TOKENS"
            } ] }),
        ] {
            assert_refused(&envelope);
        }

        for envelope in [
            json!({ "candidates": [ { "content": { "parts": [] } } ] }),
            json!({ "candidates": [ { "finishReason": 7 } ] }),
            json!({ "candidates": {} }),
            json!({ "candidates": [ {
                "content": { "parts": {} },
                "finishReason": "STOP"
            } ] }),
            json!({ "candidates": [ {
                "content": { "parts": [ { "text": 7 } ] },
                "finishReason": "STOP"
            } ] }),
            json!({ "promptFeedback": { "blockReason": 7 } }),
            json!({ "promptFeedback": "blocked" }),
        ] {
            assert_invalid(&envelope);
        }
    }

    #[test]
    fn wrong_typed_usage_degrades_to_absent_evidence() {
        for usage in [
            json!("9"),
            json!(7),
            json!({ "promptTokenCount": "9", "totalTokenCount": 1.5 }),
        ] {
            let envelope = json!({
                "candidates": [ {
                    "content": { "parts": [ { "text": "{\"answer\":\"ok\"}" } ] },
                    "finishReason": "STOP"
                } ],
                "usageMetadata": usage
            });
            match extract(envelope.to_string().as_bytes()) {
                Some(EnvelopeOutcome::Completed { text, tokens }) => {
                    assert_eq!(text, "{\"answer\":\"ok\"}");
                    assert!(tokens.is_none(), "usage cannot gate admission: {envelope}");
                }
                _ => panic!("expected completed Gemini envelope: {envelope}"),
            }
        }
        assert_completed(
            &json!({
                "candidates": [ {
                    "content": { "parts": [ { "text": "{}" } ] },
                    "finishReason": "STOP"
                } ],
                "usageMetadata": { "promptTokenCount": "9", "totalTokenCount": 8 }
            }),
            "{}",
        );
    }

    #[test]
    fn wrong_typed_provider_messages_never_gate_admission() {
        assert_completed(
            &json!({
                "candidates": [ {
                    "content": { "parts": [ { "text": "{\"answer\":\"ok\"}" } ] },
                    "finishReason": "STOP",
                    "finishMessage": { "structured": "message" }
                } ],
                "promptFeedback": { "blockReasonMessage": 7 }
            }),
            "{\"answer\":\"ok\"}",
        );

        assert_non_natural_facts(
            &json!({
                "candidates": [ {
                    "content": { "parts": [ { "text": "{\"answer\":\"partial\"}" } ] },
                    "finishReason": "MAX_TOKENS",
                    "finishMessage": [ "structured" ]
                } ]
            }),
            "gemini candidate finished with MAX_TOKENS",
            Some("{\"answer\":\"partial\"}"),
        );
    }

    /// Assert that a native envelope yields the expected completed text.
    fn assert_completed(envelope: &Value, expected_text: &str) {
        match extract(envelope.to_string().as_bytes()) {
            Some(EnvelopeOutcome::Completed { text, .. }) => assert_eq!(text, expected_text),
            _ => panic!("expected completed Gemini envelope: {envelope}"),
        }
    }

    /// Assert that a recognized non-natural completion is retained as such.
    fn assert_refused(envelope: &Value) {
        assert!(
            matches!(
                extract(envelope.to_string().as_bytes()),
                Some(EnvelopeOutcome::NonNatural { .. })
            ),
            "expected non-natural Gemini envelope: {envelope}"
        );
    }

    /// Assert native reason prose and exact observed model text stay separate.
    fn assert_non_natural_facts(
        envelope: &Value,
        expected_reason: &str,
        expected_text: Option<&str>,
    ) {
        match extract(envelope.to_string().as_bytes()) {
            Some(EnvelopeOutcome::NonNatural { reason, text, .. }) => {
                assert_eq!(reason, expected_reason);
                assert_eq!(text.as_deref(), expected_text);
            }
            _ => panic!("expected non-natural Gemini envelope: {envelope}"),
        }
    }

    /// Assert that malformed required native data fails envelope decoding.
    fn assert_invalid(envelope: &Value) {
        assert!(
            extract(envelope.to_string().as_bytes()).is_none(),
            "expected invalid Gemini envelope: {envelope}"
        );
    }
}
