//! The concrete async model transport. Facades bind production and scripted
//! dependencies to one private invocation engine, while one wire-request owner
//! classifies completed generation requests.
//! @sergent/docs/trust-boundaries.md boundary 5

mod facade;
mod invocation;
mod wire_request;

#[cfg(test)]
pub(crate) use wire_request::MAX_GENERATION_BODY_BYTES;

pub use facade::LlmClient;
#[cfg(test)]
pub(crate) use facade::ScriptedLlmClient;
pub(crate) use invocation::{MAX_ATTEMPTS, extract_json_object};
