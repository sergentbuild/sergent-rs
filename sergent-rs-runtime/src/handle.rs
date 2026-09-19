//! The start-and-handle surface for a backgrounded run.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! `RunHandle` exposes a synchronous progress snapshot, a done check, a
//! cancel trip, and an awaitable result, without a synchronous run twin. A task
//! that panics is process-control escape: the result surface
//! resumes the panic rather than converting it to a `Result`.

use std::sync::{Arc, Mutex};

use sergent_rs_core::run_record::SergentResult;
use tokio::task::JoinHandle;

use crate::cancel::CancelToken;
use crate::progress::ProgressSnapshot;

/// A handle to a run started with `Sergent::start`. @sergent/docs/execution-model.md
pub struct RunHandle<Scene> {
    task: JoinHandle<SergentResult<Scene>>,
    cancel: CancelToken,
    progress: Arc<Mutex<ProgressSnapshot>>,
}

impl<Scene> RunHandle<Scene> {
    /// Binds the spawned task, cancellation token, and sanitized progress cell into one handle.
    pub(crate) fn new(
        task: JoinHandle<SergentResult<Scene>>,
        cancel: CancelToken,
        progress: Arc<Mutex<ProgressSnapshot>>,
    ) -> Self {
        Self {
            task,
            cancel,
            progress,
        }
    }

    /// The current sanitized progress snapshot, without awaiting the run.
    pub fn snapshot(&self) -> ProgressSnapshot {
        self.progress
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Whether the run task has returned.
    pub fn done(&self) -> bool {
        self.task.is_finished()
    }

    /// Request cancellation; the run cooperatively cancels at the next
    /// checkpoint or interrupts an in-flight provider await.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Await the terminal result. A task panic (process control) is resumed
    /// here, not converted into a `Result`.
    pub async fn result(self) -> SergentResult<Scene> {
        match self.task.await {
            Ok(result) => result,
            Err(join_error) => std::panic::resume_unwind(join_error.into_panic()),
        }
    }
}
