//! Provider selection: split the caller's opaque `provider/model` name at the
//! first slash, resolve the adapter, and assemble endpoint identity. A
//! malformed name and an unknown provider are local failures with no wire
//! request. @sergent/docs/trust-boundaries.md

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::{ModelError, ModelIdentity};

/// The four baseline provider adapters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Provider {
    OpenAi,
    Anthropic,
    Gemini,
    Ollama,
}

impl Provider {
    /// Recognizes only the exact baseline provider prefixes, with no trimming
    /// or normalization.
    fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            "openai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            "gemini" => Some(Self::Gemini),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }
}

/// A resolved selection: the adapter, the provider-native model remainder, and
/// the endpoint identity carried on every piece of evidence for this call.
#[derive(Debug)]
pub(crate) struct Selection {
    pub provider: Provider,
    pub model: String,
    pub identity: ModelIdentity,
}

/// Resolve the caller's opaque `provider/model` name. The remainder passes
/// through byte-for-byte, so whitespace, colons, and later slashes remain
/// provider-native model content.
// The Err is the core-owned `ModelError` so a selection failure composes
// directly into the `ModelClient::invoke` result without a boundary box/unbox.
#[allow(clippy::result_large_err)]
pub(crate) fn resolve(model_name: &str) -> Result<Selection, ModelError> {
    let Some((prefix, remainder)) = model_name.split_once('/') else {
        return Err(local_failure(
            ErrorKind::InvalidModelName,
            "model name must be in provider/model form",
        ));
    };
    if prefix.is_empty() || remainder.is_empty() {
        return Err(local_failure(
            ErrorKind::InvalidModelName,
            "model name must be in provider/model form",
        ));
    }
    let Some(provider) = Provider::from_prefix(prefix) else {
        return Err(local_failure(
            ErrorKind::UnknownProvider,
            format!("no adapter for provider {prefix:?}"),
        ));
    };
    let identity = ModelIdentity {
        provider: prefix.to_owned(),
        model: remainder.to_owned(),
        sdk_package: None,
        sdk_version: None,
    };
    Ok(Selection {
        provider,
        model: remainder.to_owned(),
        identity,
    })
}

/// Constructs a non-retryable pre-call failure with no identity, attempts,
/// usage, or raw output.
fn local_failure(kind: ErrorKind, message: impl Into<String>) -> ModelError {
    ModelError {
        kind: kind.as_str().to_owned(),
        retryable: false,
        message: message.into(),
        raw_output: None,
        identity: None,
        attempts: Vec::new(),
        usage: None,
    }
}
