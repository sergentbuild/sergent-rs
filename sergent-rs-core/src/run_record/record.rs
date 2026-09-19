//! The exact closed authoritative run record and its accessors.
//! @sergent/docs/run-record-spec.md

use serde::Serialize;

use crate::ids::RunId;
use crate::model::CallUsage;
use crate::timing::TimeSpan;

use super::{Cancellation, RunOutcome, RunStepRecord, SceneTransition, coherence};

/// The evidence state of output-token aggregation across reached model calls.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputTokenTotal {
    /// Every reached call reported usage and the exact sum is representable.
    Complete(u64),
    /// At least one reached call omitted output-token usage.
    Incomplete,
    /// Reported counts exceed the representable `u64` range.
    Overflow,
}

/// Runtime-only opening facts for one Run Record.
pub struct RunRecordHeader {
    run_id: RunId,
    model_name: String,
}

impl RunRecordHeader {
    /// Bind run identity and exact caller-selected model name.
    pub fn new(run_id: RunId, model_name: String) -> Self {
        Self { run_id, model_name }
    }
}

/// Runtime-only terminal facts for one Run Record.
pub struct RunRecordCompletion {
    outcome: RunOutcome,
    cancellation: Option<Cancellation>,
}

impl RunRecordCompletion {
    /// Bind one terminal outcome to matching cancellation evidence.
    pub fn new(outcome: RunOutcome, cancellation: Option<Cancellation>) -> Self {
        match (&outcome, &cancellation) {
            (RunOutcome::Cancelled { .. }, Some(_))
            | (RunOutcome::Success { .. } | RunOutcome::Failure { .. }, None) => {}
            _ => panic!("cancellation evidence must match a cancelled outcome"),
        }
        Self {
            outcome,
            cancellation,
        }
    }
}

/// The inert authoritative evidence for one returned run.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunRecord {
    pub(super) run_id: RunId,
    pub(super) model_name: String,
    pub(super) timing: TimeSpan,
    pub(super) scene: SceneTransition,
    pub(super) steps: Vec<RunStepRecord>,
    pub(super) outcome: RunOutcome,
    pub(super) cancellation: Option<Cancellation>,
}

impl RunRecord {
    /// Construct one exact closed record from runtime-owned lifecycle facts.
    pub fn new(
        header: RunRecordHeader,
        timing: TimeSpan,
        scene: SceneTransition,
        steps: Vec<RunStepRecord>,
        completion: RunRecordCompletion,
    ) -> Self {
        let RunRecordHeader { run_id, model_name } = header;
        let RunRecordCompletion {
            outcome,
            cancellation,
        } = completion;
        coherence::assert_run_record(&steps, &outcome, &scene, cancellation.as_ref());
        Self {
            run_id,
            model_name,
            timing,
            scene,
            steps,
            outcome,
            cancellation,
        }
    }

    /// Borrow stable run identity.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// Borrow exact caller-selected model name.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Borrow closed run timing.
    pub fn timing(&self) -> &TimeSpan {
        &self.timing
    }

    /// Borrow mandatory Scene transition evidence.
    pub fn scene(&self) -> &SceneTransition {
        &self.scene
    }

    /// Borrow the Run Record Ledger: immutable Step Records in execution order.
    pub fn steps(&self) -> &[RunStepRecord] {
        &self.steps
    }

    /// Borrow coherent terminal outcome.
    pub fn outcome(&self) -> &RunOutcome {
        &self.outcome
    }

    /// Borrow cancellation request evidence, when present.
    pub fn cancellation(&self) -> Option<&Cancellation> {
        self.cancellation.as_ref()
    }

    /// Aggregate output-token evidence without manufacturing a total.
    /// @sergent/docs/run-record-spec.md
    pub fn total_output_tokens(&self) -> OutputTokenTotal {
        let mut total: u64 = 0;
        let mut incomplete = false;
        for step in &self.steps {
            if let Some(call) = step.model_call() {
                match call.usage().and_then(CallUsage::output_tokens) {
                    Some(tokens) => match total.checked_add(tokens) {
                        Some(sum) => total = sum,
                        None => return OutputTokenTotal::Overflow,
                    },
                    None => incomplete = true,
                }
            }
        }
        if incomplete {
            OutputTokenTotal::Incomplete
        } else {
            OutputTokenTotal::Complete(total)
        }
    }
}
