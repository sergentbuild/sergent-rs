//! Concrete async model transport and provider adapters for the Rust reference
//! implementation of the Sergent Specification: the external-systems trust
//! boundary (boundary 5).
//! @sergent/docs/trust-boundaries.md @sergent-rs-providers/docs/KNOWLEDGE.md
//!
//! [`LlmClient`] implements the core `ModelClient` interface and may use
//! process discovery or an immutable [`ProviderConfig`]. For one request it
//! resolves the caller's opaque `provider/model`
//! name, discovers credentials, runs the Ollama existence preflight, drives a
//! fixed two-attempt retry loop in a private statically dispatched engine, and
//! crosses one byte-bounded provider envelope into one JSON object through a
//! single strict parse.
//! Production specializes the engine's typed HTTP crossing to reqwest. Typed
//! proposal validation belongs to the runtime (boundary 1), never here.
//!
//! [`CREDENTIAL_ENV_VARS`] is the closed credential deny-list hermetic test
//! suites strip. [`testing::StaticLlmClient`] and
//! [`testing::StaticLlmOutcome`] form the sanctioned deterministic test client.

mod adapter;
mod client;
mod credentials;
mod evidence;
mod http;
mod lowering;
mod selection;
mod settings;

pub mod testing;

pub use client::LlmClient;
pub use credentials::{CREDENTIAL_ENV_VARS, ProviderConfig};

#[cfg(test)]
mod tests_client;
#[cfg(test)]
mod tests_selection;
#[cfg(test)]
mod tests_static;
