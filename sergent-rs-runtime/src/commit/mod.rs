//! Deterministic execution safety and revision-checked commit authority.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

mod authority;
mod safety;

pub(crate) use authority::{CommitOk, commit_live, commit_plain};
pub(crate) use safety::{DryRunContext, dry_run, validate_patch};
