//! Registry ownership and two-stage Plan proposal decode orchestration.

use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::{sanitize_model_output_diagnostic, sanitize_model_output_display};
use crate::model::ParsedJsonObject;
use crate::operation::PlanStep;
use crate::plan::PlanProposal;
use crate::proposal::ProposalSchema;
use crate::target::Target;

use super::branch::Branch;
use super::builder::OperationRegistryBuilder;
use super::errors::DecodeError;

/// The closed set of Operation definitions available to a Plan-capable run. It
/// composes the Plan `ProposalSchema` and decodes model output; Intent-only
/// runtime configuration owns no registry.
/// @sergent/docs/framework.md
pub struct OperationRegistry<S, I, T: Target> {
    pub(super) branches: Vec<Branch<S, I, T>>,
    pub(super) by_call: BTreeMap<String, usize>,
    pub(super) max_operations: Option<usize>,
    pub(super) plan_schema: Arc<ProposalSchema>,
}

impl<S, I, T: Target> OperationRegistry<S, I, T> {
    /// Start building a registry.
    pub fn builder() -> OperationRegistryBuilder<S, I, T> {
        OperationRegistryBuilder {
            branches: Vec::new(),
            by_call: BTreeMap::new(),
        }
    }

    /// The composed `PlanProposal` envelope schema, shared for capture.
    pub fn plan_schema(&self) -> Arc<ProposalSchema> {
        Arc::clone(&self.plan_schema)
    }

    /// Decode a Plan proposal envelope into typed plan steps, minting one
    /// framework operation id per step.
    pub fn decode(
        &self,
        envelope: &ParsedJsonObject,
    ) -> Result<PlanProposal<S, I, T>, DecodeError> {
        for field in envelope.keys() {
            if field != "operations" {
                return Err(DecodeError::UnknownEnvelopeField {
                    field: sanitize_model_output_diagnostic(field),
                });
            }
        }
        let operations = envelope
            .get("operations")
            .ok_or(DecodeError::MissingOperations)?;
        let array = operations
            .as_array()
            .ok_or(DecodeError::OperationsNotArray)?;
        if array.is_empty() {
            return Err(DecodeError::EmptyOperations);
        }
        if let Some(max) = self.max_operations
            && array.len() > max
        {
            return Err(DecodeError::TooManyOperations {
                max,
                actual: array.len(),
            });
        }
        let mut steps = Vec::with_capacity(array.len());
        for (index, item) in array.iter().enumerate() {
            steps.push(self.decode_one(index, item)?);
        }
        Ok(PlanProposal::decoded(steps))
    }

    /// Select a registered branch by `call`, decode its discriminator-free
    /// payload, and mint the enclosing Plan step.
    fn decode_one(&self, index: usize, item: &Value) -> Result<PlanStep<S, I, T>, DecodeError> {
        let object = item
            .as_object()
            .ok_or(DecodeError::MalformedOperation { index })?;
        let call = object
            .get("call")
            .ok_or(DecodeError::MissingCall { index })?
            .as_str()
            .ok_or(DecodeError::CallNotString { index })?;
        let &branch_index = self
            .by_call
            .get(call)
            .ok_or_else(|| DecodeError::UnknownCall {
                index,
                call: sanitize_model_output_diagnostic(call),
            })?;
        let branch = &self.branches[branch_index];

        // Stage two decodes the per-verb struct with the discriminator removed
        // and deny_unknown_fields, so an echoed op_id/run_id or any other
        // unknown field fails.
        let mut payload = object.clone();
        payload.remove("call");
        let operation = (branch.decode)(Value::Object(payload)).map_err(|source| {
            DecodeError::InvalidPayload {
                index,
                call: sanitize_model_output_diagnostic(call),
                message: sanitize_model_output_display(&source),
            }
        })?;
        Ok(PlanStep::decoded(branch.call, operation))
    }
}
