//! Runtime-provided Intent variants.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! All runtime-owned Intent variants live in this one module. Adding a variant
//! extends this family rather than creating a per-variant module.

use serde::Serialize;
use sergent_rs_core::intent::{Intent, IntentFlow};

/// The minimal continue-flow Intent. Applications use it directly when their
/// continue-flow Intent carries no decision data beyond identity, or author
/// their own Intent (optionally wrapping this one) when it does. Its flow is
/// fixed to continue. @sergent/docs/execution-model.md
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct IntentContinue;

impl Intent for IntentContinue {
    /// Selects the Plan-producing branch unconditionally for this minimal Intent.
    fn flow(&self) -> IntentFlow {
        IntentFlow::Continue
    }
}
