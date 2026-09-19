//! Registration and typed model-output decode failures.

use crate::proposal::SchemaError;

/// A registration-time failure. @sergent/docs/framework.md
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// A registration supplied an empty `call` discriminator.
    #[error("operation {type_name} declares an empty call discriminator")]
    EmptyDiscriminator {
        /// The offending Operation type.
        type_name: &'static str,
    },
    /// Two registrations supplied the same `call` discriminator.
    #[error("call discriminator {call:?} is declared by both {first} and {second}")]
    DuplicateDiscriminator {
        /// The duplicated discriminator value.
        call: String,
        /// The first declaring type.
        first: &'static str,
        /// The second declaring type.
        second: &'static str,
    },
    /// An Operation derives to a schema that is not a JSON object.
    #[error("operation {type_name} must derive to a JSON object schema")]
    BranchNotObject {
        /// The offending Operation type.
        type_name: &'static str,
    },
    /// An Operation struct declares a reserved `call` field.
    #[error(
        "operation {type_name} must not declare a field named 'call' (it is framework injected)"
    )]
    ReservedCallField {
        /// The offending Operation type.
        type_name: &'static str,
    },
    /// Two definitions share a `$defs` name with different bodies.
    #[error(
        "definition {name:?} (via {owner}) collides with a different definition of the same name"
    )]
    DefinitionCollision {
        /// The colliding definition name.
        name: String,
        /// The Operation whose composition introduced the collision.
        owner: &'static str,
    },
    /// No Operation was registered.
    #[error("an operation registry must register at least one operation")]
    EmptyRegistry,
    /// A configured maximum below one.
    #[error("max_operations must be at least 1 but was {max}")]
    InvalidMaximum {
        /// The rejected maximum.
        max: usize,
    },
    /// The composed envelope failed the dialect (should not occur for in-profile
    /// Operations; surfaces an out-of-dialect operand type).
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

/// A structured failure of the two-stage typed decode at the model-output
/// crossing. The runtime records schema_validation_failed at the plan_call
/// stage. @sergent/docs/run-record-spec.md
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    /// The envelope carried a field other than `operations` (for example an
    /// echoed top-level run id).
    #[error("unexpected envelope field {field:?}")]
    UnknownEnvelopeField {
        /// The unexpected field name.
        field: String,
    },
    /// The envelope had no `operations`.
    #[error("plan envelope must have an operations array")]
    MissingOperations,
    /// `operations` was not an array.
    #[error("operations must be an array")]
    OperationsNotArray,
    /// `operations` was empty.
    #[error("operations must contain at least one entry")]
    EmptyOperations,
    /// `operations` exceeded the configured maximum.
    #[error("operations has {actual} entries but the maximum is {max}")]
    TooManyOperations {
        /// The configured maximum.
        max: usize,
        /// The actual count.
        actual: usize,
    },
    /// One operation entry was not a JSON object.
    #[error("operation at index {index} must be a JSON object")]
    MalformedOperation {
        /// The operation index.
        index: usize,
    },
    /// One operation entry was missing the `call` discriminator.
    #[error("operation at index {index} is missing the call discriminator")]
    MissingCall {
        /// The operation index.
        index: usize,
    },
    /// One operation entry had a non-string `call` discriminator.
    #[error("operation at index {index} has a non-string call discriminator")]
    CallNotString {
        /// The operation index.
        index: usize,
    },
    /// One operation entry named a `call` with no registered branch.
    #[error("operation at index {index} has unknown call {call:?}")]
    UnknownCall {
        /// The operation index.
        index: usize,
        /// The unknown discriminator.
        call: String,
    },
    /// One operation payload did not decode into its registered type (unknown
    /// field, wrong scalar type, missing field, or echoed bookkeeping id).
    #[error("operation at index {index} (call {call:?}) has an invalid payload: {message}")]
    InvalidPayload {
        /// The operation index.
        index: usize,
        /// The discriminator.
        call: String,
        /// The decode failure detail.
        message: String,
    },
}
