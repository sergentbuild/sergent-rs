//! Ordered callback delivery that contains ordinary returned errors beside the
//! closed Run Record. Rust panics remain process-control escape.

use std::any::type_name;

use sergent_rs_core::error::RunError;
use sergent_rs_core::run_record::SergentResult;

use super::RunObserver;
use crate::progress::ProgressSnapshot;

/// Project one returned callback failure into the exact observer-error shape.
fn contained<Scene>(
    callback: &'static str,
    stage: sergent_rs_core::vocab::Stage,
    observer: &dyn RunObserver<Scene>,
    error: RunError,
) -> RunError {
    let message = format!("observer {callback} callback failed: {}", error.message);
    RunError::observer_error(
        message,
        callback,
        observer.observer_type_name(),
        type_name::<RunError>(),
        stage,
    )
}

/// Deliver progress to every slot and retain each ordinary returned error.
pub(crate) fn deliver_progress<Scene>(
    observers: &[&dyn RunObserver<Scene>],
    progress: &ProgressSnapshot,
    errors: &mut Vec<RunError>,
) {
    for observer in observers {
        if let Err(error) = observer.on_progress(progress) {
            errors.push(contained("progress", progress.stage, *observer, error));
        }
    }
}

/// Deliver the accumulating terminal result to every slot in order.
pub(crate) fn deliver_finished<Scene>(
    observers: &[&dyn RunObserver<Scene>],
    mut result: SergentResult<Scene>,
) -> SergentResult<Scene> {
    for observer in observers {
        if let Err(error) = observer.on_finished(&result) {
            let error = contained("finished", result.stage(), *observer, error);
            result = result.with_observer_error(error);
        }
    }
    result
}
