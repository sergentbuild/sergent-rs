//! Request-side settings, images, messages, and proposal requests.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use std::borrow::Cow;
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::proposal::ProposalSchema;

const IMAGE_PART_MAX_BYTES: usize = 1_000_000;
const IMAGE_PART_DECODE_WITNESS_BASE64_BYTES: usize = (IMAGE_PART_MAX_BYTES / 3 + 1) * 4;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// The thinking-effort knob for a model call. Providers map this to their
/// native reasoning control. @sergent/docs/execution-model.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingEffort {
    /// Least thinking.
    Low,
    /// Moderate thinking.
    Medium,
    /// Most thinking.
    High,
}

/// Per-request output-token, timeout, and thinking-effort controls.
/// Defaults are effort high, output cap 4096, timeout 60 seconds; the
/// non-zero types encode the at-least-one rule.
/// @sergent/docs/execution-model.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ModelSettings {
    /// The thinking-effort knob.
    pub thinking_effort: ThinkingEffort,
    /// The output-token cap.
    pub max_output_tokens: NonZeroU32,
    /// The request timeout in seconds.
    pub timeout_secs: NonZeroU32,
}

impl Default for ModelSettings {
    /// Use high thinking effort, a 4096-token output cap, and a 60-second
    /// timeout.
    fn default() -> Self {
        Self {
            thinking_effort: ThinkingEffort::High,
            max_output_tokens: NonZeroU32::new(4096).expect("4096 is non-zero"),
            timeout_secs: NonZeroU32::new(60).expect("60 is non-zero"),
        }
    }
}

/// One bounded PNG image input, carried as validated base64 text. Run Record
/// capture retains only media type and decoded byte count;
/// transport reads the base64 through `data_base64`.
/// @sergent/docs/execution-model.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagePart {
    data_base64: String,
    decoded_byte_count: usize,
}

impl Serialize for ImagePart {
    /// Serialize only image metadata for Run Record capture, excluding payload.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut part = serializer.serialize_struct("ImagePart", 2)?;
        part.serialize_field("media_type", self.media_type())?;
        part.serialize_field("bytes", &self.decoded_byte_count)?;
        part.end()
    }
}

/// Rejection of an image input that is not a bounded base64 PNG.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImageError {
    /// The text is not valid base64.
    #[error("image data must be valid base64")]
    NotBase64,
    /// The decoded bytes exceed the size bound.
    #[error("decoded image bytes must be <= {IMAGE_PART_MAX_BYTES}")]
    TooLarge,
    /// The decoded bytes do not start with the PNG signature.
    #[error("image data must contain PNG bytes")]
    NotPng,
}

impl ImagePart {
    /// Admit one PNG image from base64 text with bounded decoding work.
    pub fn png<'a>(data_base64: impl Into<Cow<'a, str>>) -> Result<Self, ImageError> {
        let data_base64 = data_base64.into();
        let (decode_input, has_unread_input) = bounded_decode_input(&data_base64)?;
        let decoded = STANDARD
            .decode(decode_input.as_bytes())
            .map_err(|_| ImageError::NotBase64)?;
        if decoded.len() > IMAGE_PART_MAX_BYTES {
            return Err(ImageError::TooLarge);
        }
        if has_unread_input {
            return Err(ImageError::NotBase64);
        }
        if !decoded.starts_with(&PNG_SIGNATURE) {
            return Err(ImageError::NotPng);
        }
        Ok(Self {
            data_base64: data_base64.into_owned(),
            decoded_byte_count: decoded.len(),
        })
    }

    /// The image media type.
    pub fn media_type(&self) -> &'static str {
        "image/png"
    }

    /// The validated base64 payload, for provider transport.
    pub fn data_base64(&self) -> &str {
        &self.data_base64
    }

    /// The decoded PNG byte count.
    pub fn decoded_byte_count(&self) -> usize {
        self.decoded_byte_count
    }
}

/// Select the shortest prefix that can prove overflow, without splitting a
/// non-ASCII code point. A split code point is already invalid base64.
fn bounded_decode_input(data_base64: &str) -> Result<(&str, bool), ImageError> {
    if data_base64.len() <= IMAGE_PART_DECODE_WITNESS_BASE64_BYTES {
        return Ok((data_base64, false));
    }
    data_base64
        .get(..IMAGE_PART_DECODE_WITNESS_BASE64_BYTES)
        .map(|prefix| (prefix, true))
        .ok_or(ImageError::NotBase64)
}

/// The author of one prompt message. @sergent/docs/execution-model.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    /// A system instruction message.
    System,
    /// A user message; only user messages may carry images.
    User,
}

/// One bounded system or user prompt message. @sergent/docs/execution-model.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Message {
    role: MessageRole,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<ImagePart>,
}

impl Message {
    /// A system instruction message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            images: Vec::new(),
        }
    }

    /// A text-only user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            images: Vec::new(),
        }
    }

    /// A user message with bounded image inputs.
    pub fn user_with_images(
        content: impl Into<String>,
        images: impl IntoIterator<Item = ImagePart>,
    ) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            images: images.into_iter().collect(),
        }
    }

    /// Read the message author.
    pub fn role(&self) -> MessageRole {
        self.role
    }

    /// Borrow the prompt text.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Borrow the user image inputs.
    pub fn images(&self) -> &[ImagePart] {
        &self.images
    }
}

/// The framework-owned facts runtime combines after a Recipe returns messages.
/// The consuming conversion preserves the captured proposal schema allocation.
/// @sergent/docs/execution-model.md
pub struct ModelRequestInput {
    model_name: String,
    model_settings: ModelSettings,
    proposal_schema: Arc<ProposalSchema>,
}

impl ModelRequestInput {
    /// Bind the caller's model selection and settings to the captured schema.
    pub fn new(
        model_name: String,
        model_settings: ModelSettings,
        proposal_schema: Arc<ProposalSchema>,
    ) -> Self {
        Self {
            model_name,
            model_settings,
            proposal_schema,
        }
    }

    /// Consume the framework input and attach the Recipe-authored messages.
    pub fn into_request(self, messages: Vec<Message>) -> ModelRequest {
        ModelRequest {
            model_name: self.model_name,
            messages,
            model_settings: self.model_settings,
            proposal_schema: self.proposal_schema,
        }
    }
}

/// One typed proposal request carried across the model-call boundary. The
/// captured canonical schema rides behind an `Arc` so runtime transfers its
/// exact allocation without copying it.
/// @sergent/docs/execution-model.md @sergent/docs/trust-boundaries.md
#[derive(Clone, Debug)]
pub struct ModelRequest {
    model_name: String,
    messages: Vec<Message>,
    model_settings: ModelSettings,
    proposal_schema: Arc<ProposalSchema>,
}

impl ModelRequest {
    /// Borrow the caller's full `provider/model` selection.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Borrow the Recipe-authored ordered messages.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Read the caller-owned output controls.
    pub fn model_settings(&self) -> ModelSettings {
        self.model_settings
    }

    /// Borrow the captured canonical proposal schema.
    pub fn proposal_schema(&self) -> &Arc<ProposalSchema> {
        &self.proposal_schema
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::sync::Arc;

    use schemars::JsonSchema;
    use serde::Deserialize;

    use crate::proposal::derive_proposal_schema;

    use super::{
        IMAGE_PART_DECODE_WITNESS_BASE64_BYTES, ImageError, ImagePart, Message, ModelRequestInput,
        ModelSettings, ThinkingEffort,
    };

    /// A closed object used only to derive the request-input test schema.
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct TestProposal {
        /// Whether the proposal is accepted.
        _accepted: bool,
    }

    #[test]
    fn request_input_preserves_all_facts_and_schema_identity() {
        let schema = Arc::new(derive_proposal_schema::<TestProposal>().unwrap());
        let settings = ModelSettings {
            thinking_effort: ThinkingEffort::Low,
            max_output_tokens: NonZeroU32::new(17).unwrap(),
            timeout_secs: NonZeroU32::new(23).unwrap(),
        };
        let messages = vec![Message::system("decide"), Message::user("context")];

        let request =
            ModelRequestInput::new("provider/model".to_owned(), settings, Arc::clone(&schema))
                .into_request(messages.clone());

        assert_eq!(request.model_name(), "provider/model");
        assert_eq!(request.model_settings(), settings);
        assert_eq!(request.messages(), messages);
        assert!(Arc::ptr_eq(request.proposal_schema(), &schema));
    }

    #[test]
    fn image_admission_stops_at_the_first_malformed_or_overflow_witness() {
        let valid_oversize = "A".repeat(IMAGE_PART_DECODE_WITNESS_BASE64_BYTES * 4);
        assert_eq!(
            ImagePart::png(valid_oversize.as_str()),
            Err(ImageError::TooLarge)
        );

        let malformed_before_overflow = format!("!{}", valid_oversize);
        assert_eq!(
            ImagePart::png(malformed_before_overflow.as_str()),
            Err(ImageError::NotBase64)
        );

        let malformed_after_overflow = format!("{}!", valid_oversize);
        assert_eq!(
            ImagePart::png(malformed_after_overflow.as_str()),
            Err(ImageError::TooLarge)
        );
    }
}
