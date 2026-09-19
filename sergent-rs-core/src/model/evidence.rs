//! Request capture, model identity, usage, and attempt evidence.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::error::RunError;
use crate::timing::TimeSpan;

use super::{Message, ModelRequest, ModelSettings};

/// The Run Record projection of a model request. The enclosing model
/// call record owns the canonical schema separately, so this projection omits
/// it by construction. @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelRequestCapture {
    model_name: String,
    messages: Vec<Message>,
    model_settings: ModelSettings,
}

impl From<&ModelRequest> for ModelRequestCapture {
    /// Copy the captured request fields while omitting the separately owned
    /// canonical schema.
    fn from(request: &ModelRequest) -> Self {
        Self {
            model_name: request.model_name().to_owned(),
            messages: request.messages().to_vec(),
            model_settings: request.model_settings(),
        }
    }
}

impl ModelRequestCapture {
    /// Borrow the caller's full model selection.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Borrow the ordered prompt messages.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Read the request settings.
    pub fn model_settings(&self) -> ModelSettings {
        self.model_settings
    }
}

/// The resolved endpoint and SDK that served a model call.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelIdentity {
    /// The resolved provider.
    pub provider: String,
    /// The unchanged provider-native model remainder.
    pub model: String,
    /// The SDK or adapter package, when known.
    pub sdk_package: Option<String>,
    /// The SDK or adapter version, when known.
    pub sdk_version: Option<String>,
}

/// The mapping of provider-reported integer token counts. Unreported
/// directions are absent. @sergent/docs/observability.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TokenCounts {
    /// Input tokens, when reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    /// Output tokens, when reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
}

/// The provider-measured facts for one model call: the sole owner of latency,
/// token counts, and request id.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CallUsage {
    /// The measured call latency in milliseconds.
    pub latency_ms: u64,
    /// Token counts, when the provider supplied them.
    pub tokens: Option<TokenCounts>,
    /// The provider request id, when supplied.
    pub request_id: Option<String>,
}

impl CallUsage {
    /// The measured output tokens, when supplied.
    pub fn output_tokens(&self) -> Option<u64> {
        self.tokens.and_then(|t| t.output)
    }
}

/// The closed evidence for one completed model-call attempt: a closed time
/// span and one coherent success or failure verdict. Failure owns retryability
/// and its required structured error. An interrupted await creates no attempt.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attempt {
    /// The attempt completed successfully.
    Success {
        /// The closed wall-clock span of the attempt.
        timing: TimeSpan,
    },
    /// The attempt failed.
    Failure {
        /// The closed wall-clock span of the attempt.
        timing: TimeSpan,
        /// Whether the failure is retryable.
        retryable: bool,
        /// The structured attempt failure.
        error: RunError,
    },
}

impl Serialize for Attempt {
    /// Serialize the exact four-field attempt record while the enum preserves
    /// coherent construction in memory.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("Attempt", 4)?;
        record.serialize_field("timing", self.timing())?;
        record.serialize_field(
            "status",
            if self.is_success() {
                "success"
            } else {
                "failure"
            },
        )?;
        record.serialize_field("retryable", &self.retryable())?;
        record.serialize_field("error", &self.error())?;
        record.end()
    }
}

impl Attempt {
    /// Construct a successful completed attempt.
    pub fn success(timing: TimeSpan) -> Self {
        Self::Success { timing }
    }

    /// Construct a failed completed attempt.
    pub fn failure(timing: TimeSpan, retryable: bool, error: RunError) -> Self {
        Self::Failure {
            timing,
            retryable,
            error,
        }
    }

    /// Borrow the closed attempt timing.
    pub fn timing(&self) -> &TimeSpan {
        match self {
            Self::Success { timing } | Self::Failure { timing, .. } => timing,
        }
    }

    /// Whether the attempt succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Failure retryability, when this is a failed attempt.
    pub fn retryable(&self) -> Option<bool> {
        match self {
            Self::Success { .. } => None,
            Self::Failure { retryable, .. } => Some(*retryable),
        }
    }

    /// The structured failure, when this is a failed attempt.
    pub fn error(&self) -> Option<&RunError> {
        match self {
            Self::Success { .. } => None,
            Self::Failure { error, .. } => Some(error),
        }
    }
}
