//! Model responses, parsed objects, failures, and the async client seam.

use serde_json::{Map, Value};
use std::future::Future;

use super::{Attempt, CallUsage, ModelIdentity, ModelRequest};

/// Successful model-call evidence, returned beside one parsed JSON object.
/// @sergent/docs/execution-model.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelResponse {
    /// The resolved endpoint identity.
    pub identity: ModelIdentity,
    /// The model's raw output text.
    pub raw_output: String,
    /// The completed attempts.
    pub attempts: Vec<Attempt>,
    /// The measured usage.
    pub usage: CallUsage,
}

/// One JSON object admitted by a model transport at the external-systems
/// boundary. The object shape is preserved through the later typed proposal
/// crossing without another root-shape probe.
/// @sergent/docs/trust-boundaries.md
pub type ParsedJsonObject = Map<String, Value>;

/// A typed model-call failure carrying whatever call evidence exists.
/// @sergent/docs/execution-model.md
#[derive(Clone, Debug, thiserror::Error)]
#[error("model call failed: {message}")]
pub struct ModelError {
    /// The open provider-owned failure kind.
    pub kind: String,
    /// Whether the failure is retryable.
    pub retryable: bool,
    /// A sanitized failure message.
    pub message: String,
    /// Exact response or model text reached before failure, including admitted
    /// full or partial UTF-8 response text. It is null when no text was reached
    /// and never contains repaired or synthetic commentary.
    pub raw_output: Option<String>,
    /// The identity resolved after successful provider/model selection. It may
    /// exist when a later failure is response-less; it is null before selection
    /// resolves.
    pub identity: Option<ModelIdentity>,
    /// The completed attempts gathered before the failure.
    pub attempts: Vec<Attempt>,
    /// The measured usage, when available.
    pub usage: Option<CallUsage>,
}

/// The asynchronous model transport: the only async surface in the framework.
/// `invoke` returns call evidence beside one parsed JSON object, or a
/// structured failure carrying evidence. Static dispatch (a generic bound); the
/// returned future is `Send` for a multi-threaded executor.
/// @sergent/docs/execution-model.md @sergent/docs/trust-boundaries.md
pub trait ModelClient: Send + Sync {
    /// Invoke the model for one request. @sergent/docs/execution-model.md
    fn invoke(
        &self,
        request: &ModelRequest,
    ) -> impl Future<Output = Result<(ModelResponse, ParsedJsonObject), ModelError>> + Send;
}
