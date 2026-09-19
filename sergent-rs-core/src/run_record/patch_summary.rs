//! The single inert evidence projection of one compiled or rebased Patch.
//! @sergent/docs/run-record-spec.md

use serde::Serialize;

use crate::ids::OperationId;
use crate::plan::Patch;
use crate::scene::SceneIdentity;
use crate::target::Target;

use super::CapturedValue;

/// One exact Patch summary. Its parallel arrays derive from the same ordered
/// steps and therefore cannot drift. @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PatchSummary {
    base: SceneIdentity,
    operation_count: usize,
    operation_ids: Vec<OperationId>,
    operation_call_names: Vec<String>,
    operation_trace_ids: Vec<OperationId>,
    operations: Vec<CapturedValue>,
}

impl PatchSummary {
    /// Project one Patch once from its authoritative ordered steps.
    pub fn from_patch<S, I, T: Target>(patch: &Patch<S, I, T>) -> Self {
        let operation_ids: Vec<_> = patch
            .steps()
            .iter()
            .map(|step| step.op_id().clone())
            .collect();
        Self {
            base: patch.base().clone(),
            operation_count: patch.steps().len(),
            operation_trace_ids: operation_ids.clone(),
            operation_call_names: patch
                .steps()
                .iter()
                .map(|step| step.call().to_owned())
                .collect(),
            operations: patch
                .steps()
                .iter()
                .map(|step| step.captured_operation())
                .collect(),
            operation_ids,
        }
    }

    /// Borrow the Scene identity the Patch is based on.
    pub fn base(&self) -> &SceneIdentity {
        &self.base
    }

    /// The number of ordered Operations represented by every parallel array.
    pub fn operation_count(&self) -> usize {
        self.operation_count
    }

    /// Borrow framework Operation identities in Patch order.
    pub fn operation_ids(&self) -> &[OperationId] {
        &self.operation_ids
    }

    /// Borrow fixed call discriminators in Patch order.
    pub fn operation_call_names(&self) -> &[String] {
        &self.operation_call_names
    }

    /// Borrow trace Operation identities in Patch order.
    pub fn operation_trace_ids(&self) -> &[OperationId] {
        &self.operation_trace_ids
    }

    /// Borrow independently captured concrete Operation data in Patch order.
    pub fn operations(&self) -> &[CapturedValue] {
        &self.operations
    }
}
