//! The closed run vocabulary as Rust enums with exact serialized strings.
//! @sergent/docs/execution-model.md

use serde::Serialize;

use crate::error::RunError;

/// The progress stages of a Run. `intent_call` appears only when Intent is
/// model-backed. @sergent/docs/execution-model.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// The run is scheduled but not started.
    Queued,
    /// The run has started.
    Started,
    /// A model-backed Intent proposal call is in flight.
    IntentCall,
    /// Intent has been derived and validated.
    Intent,
    /// The Plan proposal call is in flight.
    PlanCall,
    /// The ExecutionPlan has been derived and validated.
    ExecutionPlan,
    /// A patch has been compiled.
    Patch,
    /// The patch is being dry-run in isolation.
    DryRun,
    /// The patch is being committed.
    Commit,
}

/// The sanitized progress status a caller may observe without awaiting a run.
/// @sergent/docs/execution-model.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStatus {
    /// Scheduled, not yet running.
    Queued,
    /// Actively running.
    Running,
    /// Completed successfully.
    Success,
    /// Completed with a contained failure.
    Failure,
    /// Cancelled before the commit boundary.
    Cancelled,
}

/// The terminal status of a closed run.
/// @sergent/docs/execution-model.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    /// The run succeeded.
    Success,
    /// The run failed with a contained error.
    Failure,
    /// The run was cancelled.
    Cancelled,
}

/// The coherent terminal status of one closed Step Record. Failure and
/// cancellation own their required error evidence. @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunStepStatus {
    /// The step completed successfully.
    Success,
    /// The step failed with a contained error.
    Failure {
        /// The step failure.
        error: RunError,
    },
    /// The step was cancelled before commit.
    Cancelled {
        /// The cancellation-shaped failure.
        error: RunError,
    },
}

impl RunStepStatus {
    /// The terminal run status corresponding to this closed step.
    pub fn terminal_status(&self) -> TerminalStatus {
        match self {
            Self::Success => TerminalStatus::Success,
            Self::Failure { .. } => TerminalStatus::Failure,
            Self::Cancelled { .. } => TerminalStatus::Cancelled,
        }
    }

    /// The terminal error, when the step failed or was cancelled.
    pub fn error(&self) -> Option<&RunError> {
        match self {
            Self::Success => None,
            Self::Failure { error } | Self::Cancelled { error } => Some(error),
        }
    }
}

/// The Step Record names. Model calls nest inside their awaiting step: the
/// Intent call in `intent`, the Plan call in `execution_plan`.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStepName {
    /// Observation capture and selection of the run's Target.
    ProcessInput,
    /// Intent derivation and validation (nests the Intent call).
    Intent,
    /// ExecutionPlan derivation and validation (nests the Plan call).
    ExecutionPlan,
    /// Patch compilation and dry-run.
    Patch,
    /// Revision-checked commit.
    Commit,
}
