//! Terminal close and observer delivery: the progress and observer fan-out, the
//! terminal facts, and the two functions that seal a run into its
//! `SergentResult`.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! `finish` closes the Run Record, delivers the final progress snapshot, builds the
//! result, and delivers that accumulating result to every observer slot;
//! `finish_cancelled` is its pre-commit cancellation form.

use std::sync::{Arc, Mutex};

use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::ids::RunId;
use sergent_rs_core::registry::DecodeError;
use sergent_rs_core::run_record::{Cancellation, RunOutcome, RunRecord, SergentResult};
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::vocab::{ProgressStatus, Stage, TerminalStatus};

use crate::clock::sample;
use crate::observer::{RunObserver, deliver_finished, deliver_progress};
use crate::progress::ProgressSnapshot;
use crate::run_record::RunRecordBuilder;

/// The bundled progress cell and observer slots threaded through the pipeline,
/// accumulating contained delivery failures beside the Run Record.
pub(super) struct Delivery<'a, Scene> {
    progress: &'a Arc<Mutex<ProgressSnapshot>>,
    observers: &'a [&'a dyn RunObserver<Scene>],
    errors: Vec<RunError>,
}

impl<'a, Scene> Delivery<'a, Scene> {
    /// Binds progress and observer slots under one delivery owner for the run.
    pub(super) fn new(
        progress: &'a Arc<Mutex<ProgressSnapshot>>,
        observers: &'a [&'a dyn RunObserver<Scene>],
    ) -> Self {
        Self {
            progress,
            observers,
            errors: Vec::new(),
        }
    }

    /// Returns the run identity owned by the delivery progress state.
    pub(super) fn run_id(&self) -> RunId {
        lock(self.progress).run_id.clone()
    }

    /// Publishes a sanitized stage snapshot while containing observer errors beside the Run Record.
    pub(super) fn emit(&mut self, stage: Stage, status: ProgressStatus) {
        let snapshot = {
            let mut guard = lock(self.progress);
            guard.stage = stage;
            guard.status = status;
            guard.clone()
        };
        deliver_progress(self.observers, &snapshot, &mut self.errors);
    }

    /// Adds observed Scene authority to shared progress before later stage emissions.
    pub(super) fn observe(&mut self, identity: &SceneIdentity) {
        lock(self.progress).observe(identity);
    }
}

/// Acquires shared runtime state while recovering ownership from a poisoned mutex.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Maps terminal status onto its sanitized progress projection.
fn progress_status(status: TerminalStatus) -> ProgressStatus {
    match status {
        TerminalStatus::Success => ProgressStatus::Success,
        TerminalStatus::Failure => ProgressStatus::Failure,
        TerminalStatus::Cancelled => ProgressStatus::Cancelled,
    }
}

/// Builds the stable cancellation-shaped error for a checkpoint observed before commit.
pub(super) fn cancelled_error() -> RunError {
    RunError::cancelled("run cancelled before the commit boundary")
}

/// Maps registry decode rejection to a model-output error with safe structural metadata.
pub(super) fn decode_error_to_run_error(error: &DecodeError) -> RunError {
    let run_error = RunError::of(ErrorKind::SchemaValidationFailed, error.to_string());
    match error {
        DecodeError::InvalidPayload { index, call, .. }
        | DecodeError::UnknownCall { index, call } => run_error
            .with("operation_index", *index as u64)
            .with("call", call.clone()),
        DecodeError::MalformedOperation { index }
        | DecodeError::MissingCall { index }
        | DecodeError::CallNotString { index } => run_error.with("operation_index", *index as u64),
        DecodeError::UnknownEnvelopeField { field } => run_error.with("field", field.clone()),
        _ => run_error,
    }
}

/// Close the run, deliver terminal progress, then pass the current
/// `SergentResult<Scene>` through every terminal observer in order.
pub(super) fn finish<S>(
    record_builder: RunRecordBuilder,
    outcome: RunOutcome,
    stage: Stage,
    scene: S,
    mut delivery: Delivery<'_, S>,
) -> SergentResult<S> {
    let finished_at = sample();
    let status = outcome.terminal_status();
    let record: RunRecord = record_builder.into_record(outcome, finished_at);
    delivery.emit(stage, progress_status(status));
    let result = SergentResult::new(stage, scene, record, delivery.errors);
    deliver_finished(delivery.observers, result)
}

/// Close a run that was cancelled before the commit boundary. The
/// caller has already closed the reached step; `RunRecord` derives its
/// cancelled status projection from the outcome below.
pub(super) fn finish_cancelled<S>(
    mut record_builder: RunRecordBuilder,
    stage: Stage,
    scene: S,
    cancellation: Cancellation,
    delivery: Delivery<'_, S>,
) -> SergentResult<S> {
    record_builder.set_cancellation(cancellation);
    finish(
        record_builder,
        RunOutcome::Cancelled {
            error: cancelled_error(),
        },
        stage,
        scene,
        delivery,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use sergent_rs_core::ids::{RunId, SceneId};
    use sergent_rs_core::run_record::{CancellationCheckpoint, RunRecordHeader};
    use sergent_rs_core::timing::Timestamp;
    use sergent_rs_core::vocab::RunStepName;

    use crate::run_record::{ProcessInputEvidence, ProcessOutputEvidence};

    use super::*;

    #[test]
    fn finish_cancelled_keeps_request_time_separate_from_observation_checkpoint() {
        let run_id = RunId::mint();
        let identity = SceneIdentity {
            scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
            revision: 7,
        };
        let header = RunRecordHeader::new(run_id.clone(), "provider/model".to_owned());
        let record_builder = RunRecordBuilder::new(header, sample(), &identity);
        let mut step = record_builder.open_step(RunStepName::ProcessInput, sample());
        step.capture_input(&ProcessInputEvidence {
            observation: "test observation",
        });
        let target = "test target";
        step.capture_output(&ProcessOutputEvidence {
            selected_target: Some(&target),
        });
        let record_builder = step.close_success(sample());
        let progress = Arc::new(Mutex::new(ProgressSnapshot::initial(run_id)));
        let observers: [&dyn RunObserver<()>; 0] = [];

        let result = finish_cancelled(
            record_builder,
            Stage::Started,
            (),
            Cancellation::new(
                Timestamp::from_unix_micros(41),
                Some(CancellationCheckpoint::BeforeIntent),
            ),
            Delivery {
                progress: &progress,
                observers: &observers,
                errors: Vec::new(),
            },
        );

        let evidence = result.run_record().cancellation().unwrap();

        assert_eq!(evidence.requested_at(), Timestamp::from_unix_micros(41));
        assert_eq!(
            evidence.checkpoint(),
            Some(CancellationCheckpoint::BeforeIntent)
        );
    }
}
