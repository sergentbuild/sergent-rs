use std::any::type_name;

use sergent_rs_core::error::RunError;
use sergent_rs_core::run_record::SergentResult;

use crate::progress::ProgressSnapshot;

/// A Scene-typed observer slot with no execution authority. Callbacks default
/// to no-op so an application implements only the observation it needs.
/// @sergent/docs/observability.md
pub trait RunObserver<Scene>: Send + Sync {
    /// Return the concrete implementation name retained after trait erasure.
    #[doc(hidden)]
    fn observer_type_name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// Observe one sanitized progress snapshot.
    fn on_progress(&self, progress: &ProgressSnapshot) -> Result<(), RunError> {
        let _ = progress;
        Ok(())
    }

    /// Observe the current terminal result and all earlier delivery failures.
    fn on_finished(&self, result: &SergentResult<Scene>) -> Result<(), RunError> {
        let _ = result;
        Ok(())
    }
}
