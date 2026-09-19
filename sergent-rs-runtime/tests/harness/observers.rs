//! The `RunObserver` fakes: `CapturingObserver` records progress and finished
//! results, `FailingObserver` always fails to prove per-slot containment. Also
//! `model_call_of`, a helper that looks up a step's model-call record.

use std::sync::Mutex;

use sergent_rs_core::error::RunError;
use sergent_rs_core::run_record::{ModelCallRecord, RunRecord, SergentResult};
use sergent_rs_core::vocab::RunStepName;

use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::progress::ProgressSnapshot;

use super::Doc;

#[derive(Default)]
pub struct CapturingObserver {
    pub progress: Mutex<Vec<ProgressSnapshot>>,
    pub finished: Mutex<Vec<SergentResult<Doc>>>,
}

impl RunObserver<Doc> for CapturingObserver {
    fn on_progress(&self, progress: &ProgressSnapshot) -> Result<(), RunError> {
        self.progress.lock().unwrap().push(progress.clone());
        Ok(())
    }

    fn on_finished(&self, result: &SergentResult<Doc>) -> Result<(), RunError> {
        self.finished.lock().unwrap().push(result.clone());
        Ok(())
    }
}

/// An observer whose callbacks always fail, to prove per-slot containment.
#[derive(Default)]
pub struct FailingObserver {
    pub progress_calls: Mutex<u32>,
    pub finished_calls: Mutex<u32>,
}

impl RunObserver<Doc> for FailingObserver {
    fn on_progress(&self, _progress: &ProgressSnapshot) -> Result<(), RunError> {
        *self.progress_calls.lock().unwrap() += 1;
        Err(RunError::new("observer_fixture", "x".repeat(3_000)))
    }

    fn on_finished(&self, _result: &SergentResult<Doc>) -> Result<(), RunError> {
        *self.finished_calls.lock().unwrap() += 1;
        Err(RunError::new("observer_fixture", "x".repeat(3_000)))
    }
}

pub fn model_call_of(record: &RunRecord, step: RunStepName) -> Option<&ModelCallRecord> {
    record
        .steps()
        .iter()
        .find(|record| record.name() == step)
        .and_then(|record| record.model_call())
}
