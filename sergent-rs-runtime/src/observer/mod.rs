//! Scene-typed observer contracts and ordered delivery containment.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

mod contract;
mod delivery;

pub use contract::RunObserver;
pub(crate) use delivery::{deliver_finished, deliver_progress};
