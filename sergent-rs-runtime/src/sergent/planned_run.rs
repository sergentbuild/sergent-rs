//! Typed handoff from the async proposal pipeline to deterministic execution.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use sergent_rs_core::plan::ExecutionPlan;
use sergent_rs_core::target::Target;

use super::scene_authority::RunScene;

/// One derived Plan bound to its exact Intent, Target, and per-run Scene authority.
pub(super) struct PlannedRun<S, I, T: Target, P> {
    scene: RunScene<S, P>,
    target: T,
    intent: I,
    plan: ExecutionPlan<S, I, T>,
}

impl<S, I, T: Target, P> PlannedRun<S, I, T, P> {
    /// Binds the complete deterministic input at the async-to-sync boundary.
    pub(super) fn new(
        scene: RunScene<S, P>,
        target: T,
        intent: I,
        plan: ExecutionPlan<S, I, T>,
    ) -> Self {
        Self {
            scene,
            target,
            intent,
            plan,
        }
    }

    /// Consumes the boundary value for deterministic execution.
    pub(super) fn into_parts(self) -> (RunScene<S, P>, T, I, ExecutionPlan<S, I, T>) {
        (self.scene, self.target, self.intent, self.plan)
    }
}
