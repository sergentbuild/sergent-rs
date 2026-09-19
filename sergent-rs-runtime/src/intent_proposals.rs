//! Runtime-provided Intent proposal variants.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! All runtime-owned Intent proposal variants live in this one module. Adding a
//! variant extends this family rather than creating a per-variant module.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The empty typed sentinel a recipe returns from `passthrough_proposal` to
/// select the deterministic Intent mode. It is neither provider
/// output nor the derived Intent: it keeps the recipe's derivation interface
/// uniform while signalling that the Intent model call is absent. It is
/// intended to Continue to Execution Planning, but a derived Stop remains legal.
/// @sergent/docs/execution-model.md
#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntentProposalPassThrough {}
