//! Exact reached pipeline Step Records. @sergent/docs/run-record-spec.md

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::error::RunError;
use crate::timing::TimeSpan;
use crate::vocab::{RunStepName, RunStepStatus};

use super::{CapturedValue, ModelCallRecord};

/// The three independently optional evidence channels on one reached step.
pub struct RunStepEvidence {
    input: Option<CapturedValue>,
    output: Option<CapturedValue>,
    model_call: Option<ModelCallRecord>,
}

impl RunStepEvidence {
    /// Bind exact input, output, and model-call facts reached by one step.
    pub fn new(
        input: Option<CapturedValue>,
        output: Option<CapturedValue>,
        model_call: Option<ModelCallRecord>,
    ) -> Self {
        Self {
            input,
            output,
            model_call,
        }
    }
}

/// One immutable Step Record. Only work that started receives a record.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunStepRecord {
    name: RunStepName,
    status: RunStepStatus,
    timing: TimeSpan,
    input: Option<CapturedValue>,
    output: Option<CapturedValue>,
    model_call: Option<ModelCallRecord>,
}

impl RunStepRecord {
    /// Construct one reached step from its coherent status and evidence.
    pub fn new(
        name: RunStepName,
        status: RunStepStatus,
        timing: TimeSpan,
        evidence: RunStepEvidence,
    ) -> Self {
        assert!(
            evidence.input.is_none() || name == RunStepName::ProcessInput,
            "only ProcessInput may carry step input"
        );
        assert!(
            evidence.model_call.is_none()
                || matches!(name, RunStepName::Intent | RunStepName::ExecutionPlan),
            "model-call evidence must nest inside Intent or ExecutionPlan"
        );
        Self {
            name,
            status,
            timing,
            input: evidence.input,
            output: evidence.output,
            model_call: evidence.model_call,
        }
    }

    /// The closed step name.
    pub fn name(&self) -> RunStepName {
        self.name
    }

    /// Borrow the coherent terminal step status.
    pub fn status(&self) -> &RunStepStatus {
        &self.status
    }

    /// Borrow the closed step timing.
    pub fn timing(&self) -> &TimeSpan {
        &self.timing
    }

    /// Borrow captured step input, when this is ProcessInput.
    pub fn input(&self) -> Option<&CapturedValue> {
        self.input.as_ref()
    }

    /// Borrow captured merged output facts reached by this step.
    pub fn output(&self) -> Option<&CapturedValue> {
        self.output.as_ref()
    }

    /// Borrow the step failure or cancellation error, when present.
    pub fn error(&self) -> Option<&RunError> {
        self.status.error()
    }

    /// Borrow the nested model call, when this step made one.
    pub fn model_call(&self) -> Option<&ModelCallRecord> {
        self.model_call.as_ref()
    }
}

impl Serialize for RunStepRecord {
    /// Serialize every declared field, using null for absent evidence.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut step = serializer.serialize_struct("RunStepRecord", 7)?;
        step.serialize_field("name", &self.name)?;
        step.serialize_field(
            "status",
            match self.status {
                RunStepStatus::Success => "success",
                RunStepStatus::Failure { .. } => "failure",
                RunStepStatus::Cancelled { .. } => "cancelled",
            },
        )?;
        step.serialize_field("timing", &self.timing)?;
        step.serialize_field("input", &self.input)?;
        step.serialize_field("output", &self.output)?;
        step.serialize_field("error", &self.error())?;
        step.serialize_field("model_call", &self.model_call)?;
        step.end()
    }
}
