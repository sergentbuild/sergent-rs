//! Private clone erasure for heterogeneous isolated Operations.

use crate::run_record::CapturedValue;
use crate::target::Target;
use serde::Serialize;
use serde_json::Value;

use super::Operation;

/// Clone-capable private view of a registered or replacement Operation.
pub(crate) trait ErasedOperation<S, I, T: Target>:
    Operation<Scene = S, Intent = I, Target = T>
{
    /// Produce an isolated erased copy for Plan-to-Patch compilation.
    fn clone_erased(&self) -> ErasedOperationBox<S, I, T>;

    /// Capture the concrete operand-bearing Operation without exposing its
    /// erased representation.
    fn captured_value(&self) -> CapturedValue;

    /// Project the concrete operand fields for an enclosing typed evidence
    /// owner such as Plan proposal capture.
    fn project_value(&self) -> Result<Value, String>;
}

impl<Op, S, I, T> ErasedOperation<S, I, T> for Op
where
    Op: Operation<Scene = S, Intent = I, Target = T> + Clone + Serialize + 'static,
    T: Target,
{
    /// Clone through the concrete Operation's isolation-preserving `Clone`.
    fn clone_erased(&self) -> ErasedOperationBox<S, I, T> {
        Box::new(self.clone())
    }

    /// Capture through the concrete Operation type before returning evidence.
    fn captured_value(&self) -> CapturedValue {
        CapturedValue::capture(self)
    }

    /// Serialize the concrete operand fields for a script-level projection.
    fn project_value(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|error| error.to_string())
    }
}

/// Private heterogeneous container whose copies retain isolation semantics.
pub(crate) type ErasedOperationBox<S, I, T> = Box<dyn ErasedOperation<S, I, T>>;
