//! Specification values and the three interfaces of the Sergent Rust reference
//! implementation. See @sergent/docs/framework.md.
//!
//! This crate is the bottom of the stack: a typed vocabulary plus the
//! `ModelClient`, `SceneActions`, and `SergentRecipe` interfaces that the
//! runtime, provider, and application layers exchange. It performs no run, no
//! I/O, and no async work of its own. The one async surface is the
//! `ModelClient` trait; everything else is deterministic synchronous data and
//! logic.
//!
//! Modules are exposed directly; application code consumes the curated
//! `sergent-rs` battery crate rather than these submodules.

pub mod error;
pub mod ids;
pub mod intent;
pub mod mindbuf;
pub mod model;
pub mod operation;
pub mod plan;
pub mod proposal;
pub mod recipe;
pub mod registry;
pub mod run_record;
pub mod scene;
pub mod target;
pub mod timing;
pub mod vocab;
