//! Framework identity and fixed-call binding for one Plan step.

use crate::ids::OperationId;
use crate::run_record::CapturedValue;
use crate::target::Target;
use serde::Serialize;
use serde_json::Value;

use super::{ErasedOperationBox, Operation};

/// One typed Operation bound to its framework-minted identity and the fixed
/// `call` discriminator of the branch it decoded from. Patch compilation copies
/// each step preserving both. @sergent/docs/framework.md
pub struct PlanStep<S, I, T: Target> {
    op_id: OperationId,
    call: &'static str,
    operation: ErasedOperationBox<S, I, T>,
}

impl<S, I, T: Target> PlanStep<S, I, T> {
    /// Wrap a decoded Operation with a freshly minted identity and its branch's
    /// fixed `call` discriminator.
    pub(crate) fn decoded(call: &'static str, operation: ErasedOperationBox<S, I, T>) -> Self {
        Self {
            op_id: OperationId::mint(),
            call,
            operation,
        }
    }

    /// Borrow the framework-owned Operation identity.
    pub fn op_id(&self) -> &OperationId {
        &self.op_id
    }

    /// Read the fixed registered call discriminator.
    pub fn call(&self) -> &'static str {
        self.call
    }

    /// Borrow the typed Operation.
    pub fn operation(&self) -> &dyn Operation<Scene = S, Intent = I, Target = T> {
        self.operation.as_ref()
    }

    /// Produce an isolated copy that preserves the operation id and call.
    pub fn isolated_copy(&self) -> Self {
        Self {
            op_id: self.op_id.clone(),
            call: self.call,
            operation: self.operation.clone_erased(),
        }
    }

    /// Capture only the concrete Operation fields, excluding framework
    /// identity and the registered call discriminator.
    pub fn captured_operation(&self) -> CapturedValue {
        self.operation.captured_value()
    }

    /// Project one concrete Operation for a custom enclosing evidence owner.
    pub(crate) fn projected_operation(&self) -> Result<Value, String> {
        self.operation.project_value()
    }

    /// Project one decoded Operation into its original proposal shape by
    /// restoring the registry-owned fixed call discriminator.
    pub(crate) fn projected_proposal_operation(&self) -> Result<Value, String> {
        let mut object = self
            .projected_operation()?
            .as_object()
            .cloned()
            .ok_or_else(|| "registered Operation did not project to an object".to_owned())?;
        object.insert("call".to_owned(), Value::String(self.call.to_owned()));
        Ok(Value::Object(object))
    }

    /// Replace only the operand-bearing Operation for deterministic rebase.
    ///
    /// The replacement supplies only behavior. The existing framework identity
    /// and fixed call are inherited, and no identity is minted.
    /// @sergent/docs/framework.md
    pub fn with_operation<Op>(&self, operation: Op) -> Self
    where
        Op: Operation<Scene = S, Intent = I, Target = T> + Clone + Serialize + 'static,
    {
        Self {
            op_id: self.op_id.clone(),
            call: self.call,
            operation: Box::new(operation),
        }
    }
}
