//! The application-owned Operation registry: schema composition and the
//! two-stage typed decode at the model-output crossing.
//! @sergent/docs/framework.md @sergent-rs-core/docs/KNOWLEDGE.md
//!
//! One declaration drives all three concerns. A registered branch owns its
//! discriminator, its schema branch, and its decode function, so the schema the
//! model sees and the typed crossing cannot drift.

mod branch;
mod builder;
mod decode;
mod errors;

pub use builder::OperationRegistryBuilder;
pub use decode::OperationRegistry;
pub use errors::{DecodeError, RegistryError};
