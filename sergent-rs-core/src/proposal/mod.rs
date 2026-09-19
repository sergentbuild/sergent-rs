//! The canonical `ProposalSchema`, its derivation from a typed proposal, the
//! closed normalizer, and the closed dialect validator.
//! @sergent/docs/framework.md @sergent-rs-core/docs/KNOWLEDGE.md
//!
//! The canonical schema is derived from the exact typed proposal definition
//! that later receives provider output, proven to conform to the
//! language-agnostic dialect at construction before any provider call. The
//! schema constrains generation; it is never rerun as a validator of model
//! output.
//!
//! The normalizer drops only non-authoritative emitter metadata (title,
//! default, format) and applies the closed conversion list. It never drops an
//! authored structural constraint: a keyword the dialect cannot express fails
//! construction here rather than being silently weakened.

mod acyclic;
mod derive;
mod dialect;
mod normalize;
mod reference;
mod schema;

pub use derive::derive_proposal_schema;
pub use schema::{ProposalSchema, SchemaError};

pub(crate) use derive::raw_schema;
pub(crate) use dialect::validate_dialect;
pub(crate) use normalize::{normalize_node, strengthen_required};
pub(crate) use reference::definition_reference;
