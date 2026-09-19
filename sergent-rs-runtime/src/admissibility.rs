//! One Operation admissibility pass shared by initial Plan and stale rebase.
//! @sergent/docs/execution-model.md

use sergent_rs_core::ids::OperationId;
use sergent_rs_core::operation::PlanStep;
use sergent_rs_core::scene::SceneActions;
use sergent_rs_core::target::Target;

/// Private rejection locator copied from one authoritative PlanStep.
pub(crate) struct OperationRef {
    op_id: OperationId,
    call: &'static str,
}

impl OperationRef {
    /// Copy fixed framework identity from one visited step.
    fn from_step<S, I, T: Target>(step: &PlanStep<S, I, T>) -> Self {
        Self {
            op_id: step.op_id().clone(),
            call: step.call(),
        }
    }

    /// Borrow framework Operation identity.
    pub(crate) fn op_id(&self) -> &OperationId {
        &self.op_id
    }

    /// Read fixed call discriminator.
    pub(crate) fn call(&self) -> &'static str {
        self.call
    }
}

/// The stable Scene, validated Intent, and exact Target used by one pass.
pub(crate) struct AdmissibilityContext<'a, S, I, T: Target> {
    scene: &'a S,
    intent: &'a I,
    target: &'a T,
}

impl<'a, S, I, T: Target> AdmissibilityContext<'a, S, I, T> {
    /// Binds the stable pass facts without adding another validation boundary.
    pub(crate) fn new(scene: &'a S, intent: &'a I, target: &'a T) -> Self {
        Self {
            scene,
            intent,
            target,
        }
    }
}

/// The first inadmissible Operation and its application-owned reason.
pub(crate) struct AdmissibilityRejection {
    pub(crate) operation_index: usize,
    pub(crate) operation: OperationRef,
    pub(crate) reason: String,
}

/// Visits Operations in order, checking each against a fresh isolated clone of
/// the same stable pass Scene and stopping at the first rejection.
pub(crate) fn check_operations<S, I, T, A>(
    steps: &[PlanStep<S, I, T>],
    context: AdmissibilityContext<'_, S, I, T>,
    actions: &A,
) -> Result<(), AdmissibilityRejection>
where
    T: Target,
    A: SceneActions<Scene = S, Intent = I, Target = T>,
{
    for (operation_index, step) in steps.iter().enumerate() {
        let scene = actions.clone_scene(context.scene);
        if let Err(inadmissible) =
            step.operation()
                .check_admissible(&scene, context.intent, context.target)
        {
            return Err(AdmissibilityRejection {
                operation_index,
                operation: OperationRef::from_step(step),
                reason: inadmissible.reason,
            });
        }
    }
    Ok(())
}
