//! The typed model-call boundary and the facts it produces.
//! @sergent/docs/execution-model.md @sergent/docs/trust-boundaries.md

mod client;
mod evidence;
mod request;

pub use client::{ModelClient, ModelError, ModelResponse, ParsedJsonObject};
pub use evidence::{Attempt, CallUsage, ModelIdentity, ModelRequestCapture, TokenCounts};
pub use request::{
    ImageError, ImagePart, Message, MessageRole, ModelRequest, ModelRequestInput, ModelSettings,
    ThinkingEffort,
};
