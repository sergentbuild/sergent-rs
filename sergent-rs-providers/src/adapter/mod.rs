//! Closed provider request building and response extraction. Adapters receive
//! the canonical schema, never proposal types, Operation types, or the
//! Operation registry. @sergent-rs-providers/docs/providers.md

mod anthropic;
mod contract;
mod gemini;
mod ollama;
mod openai;
mod shared;

use reqwest::header::{HeaderMap, HeaderName};

use sergent_rs_core::error::ErrorKind;

use crate::selection::Provider;

pub(crate) use contract::{BuildInput, EnvelopeOutcome, PreparedRequest};
pub(crate) use shared::credential_headers;

impl Provider {
    /// Build the provider-native request through closed static dispatch.
    pub(crate) fn build(self, input: &BuildInput<'_>) -> PreparedRequest {
        match self {
            Self::OpenAi => openai::build(input),
            Self::Anthropic => anthropic::build(input),
            Self::Gemini => gemini::build(input),
            Self::Ollama => ollama::build(input),
        }
    }

    /// Parse and classify one provider-native success envelope through the
    /// adapter's minimal typed view. Unrelated additive fields remain allowed.
    pub(crate) fn extract(self, body: &[u8]) -> Option<EnvelopeOutcome> {
        match self {
            Self::Gemini => gemini::extract(body),
            Self::OpenAi => openai::extract(body),
            Self::Anthropic => anthropic::extract(body),
            Self::Ollama => ollama::extract(body),
        }
    }

    /// Map a non-success generation status through the closed provider set.
    pub(crate) fn map_generation_status(self, status: u16) -> (ErrorKind, bool) {
        match self {
            Self::Ollama => ollama::map_generation_status(status),
            Self::OpenAi | Self::Anthropic | Self::Gemini => shared::generic_status(status),
        }
    }

    /// Capture the provider's HTTP request correlation header. Body resource
    /// identifiers are intentionally excluded.
    pub(crate) fn request_id(self, headers: &HeaderMap) -> Option<String> {
        let name = match self {
            Self::OpenAi => HeaderName::from_static("x-request-id"),
            Self::Anthropic => HeaderName::from_static("request-id"),
            Self::Gemini | Self::Ollama => return None,
        };
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use sergent_rs_core::model::{ImagePart, Message, ModelSettings};
    use sergent_rs_core::proposal::derive_proposal_schema;

    /// Test proposal used to verify native image blocks preserve schema mode.
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct ImageProposal {
        accepted: bool,
    }

    #[test]
    fn all_adapters_carry_user_image_input() {
        let image = ImagePart::png("iVBORw0KGgo=").unwrap();
        let messages = [Message::user_with_images("inspect", [image])];
        let schema = derive_proposal_schema::<ImageProposal>().unwrap();
        let endpoint = crate::credentials::Endpoint::parse("https://example.invalid").unwrap();
        let input = BuildInput {
            model: "model",
            messages: &messages,
            settings: ModelSettings::default(),
            schema: &schema,
            endpoint: &endpoint,
            auth: None,
        };

        let openai = Provider::OpenAi.build(&input).body;
        assert_eq!(openai["input"][0]["content"][1]["type"], "input_image");

        let anthropic = Provider::Anthropic.build(&input).body;
        assert_eq!(
            anthropic["messages"][0]["content"][1]["source"]["type"],
            "base64"
        );

        let gemini = Provider::Gemini.build(&input).body;
        assert_eq!(
            gemini["contents"][0]["parts"][1]["inlineData"]["mimeType"],
            "image/png"
        );

        let ollama = Provider::Ollama.build(&input).body;
        assert_eq!(ollama["messages"][0]["content"][1]["type"], "image_url");
    }
}
