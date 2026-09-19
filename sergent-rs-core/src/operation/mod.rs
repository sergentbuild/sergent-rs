//! The Operation vocabulary and its framework-minted identity.
//! @sergent/docs/framework.md

mod contract;
mod errors;
mod isolated;
mod step;

pub use contract::Operation;
pub use errors::{Inadmissible, OperationFault};
pub(crate) use isolated::ErasedOperationBox;
pub use step::PlanStep;
