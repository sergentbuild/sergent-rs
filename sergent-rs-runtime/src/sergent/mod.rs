//! The Sergent runtime object and the execution pipeline.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! `Sergent` is generic over the application recipe, its scene actions, and a
//! model client. Construction captures the Intent mode and reachable schemas
//! once; a run never rereads recipe configuration. There is
//! exactly one async run entry plus a `start`/`RunHandle` surface, no sync
//! twin; the deterministic tail (patch validation, dry-run, commit)
//! is synchronous with no await points.

mod configured_recipe;
mod deterministic;
mod execution_plan;
mod intent;
mod model_invocation;
mod pipeline;
mod planned_run;
mod process_input;
mod runtime;
mod scene_authority;
mod settings;
mod terminal;

pub use configured_recipe::ConfiguredRecipe;
pub use runtime::{ConstructionError, Sergent};
pub use settings::RunSettings;
