//! One declaration tying an Operation discriminator, schema, and decoder.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::operation::{ErasedOperationBox, Operation};
use crate::target::Target;

pub(super) type DecodeFn<S, I, T> =
    fn(Value) -> Result<ErasedOperationBox<S, I, T>, serde_json::Error>;

/// Strictly deserialize one registered concrete Operation and erase only its
/// concrete type.
pub(super) fn decode_operation<Op, S, I, T>(
    value: Value,
) -> Result<ErasedOperationBox<S, I, T>, serde_json::Error>
where
    Op: Operation<Scene = S, Intent = I, Target = T>
        + Clone
        + DeserializeOwned
        + Serialize
        + 'static,
    T: Target,
{
    let operation = serde_json::from_value::<Op>(value)?;
    Ok(Box::new(operation))
}

/// Construction-time record tying one Operation's discriminator and schemas
/// to its typed decoder.
pub(super) struct Branch<S, I, T: Target> {
    pub(super) type_name: &'static str,
    pub(super) call: &'static str,
    pub(super) def_name: String,
    pub(super) def_body: Value,
    pub(super) nested_defs: Map<String, Value>,
    pub(super) decode: DecodeFn<S, I, T>,
}
