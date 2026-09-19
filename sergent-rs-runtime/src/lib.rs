//! Execution engine of the Sergent Rust reference implementation.
//! See @sergent/docs/execution-model.md.
//!
//! This crate materializes the Sergent Runtime: a deterministic synchronous
//! execution tail reached through at most two model-calling async proposal
//! points. Model output is always a proposal; the runtime is the sole authority
//! that derives, validates, and commits it. It depends only on `sergent-rs-core`
//! and never on the provider crate; concrete transport reaches a Run only
//! through the core `ModelClient` interface.
//!
//! Application code consumes the curated `sergent-rs` battery crate; these
//! modules are the framework's own surface, exposed directly for the battery to
//! re-export.

mod admissibility;
mod clock;
mod commit;
mod model_call;
mod run_record;

pub mod cancel;
pub mod handle;
pub mod intent_proposals;
pub mod intents;
pub mod observer;
pub mod progress;
pub mod run_record_file;
pub mod scene_state;
pub mod sergent;
