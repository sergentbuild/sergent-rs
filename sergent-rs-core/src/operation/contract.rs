//! The typed Operation contract.

use crate::target::Target;

use super::{Inadmissible, OperationFault};

/// One typed call over the algorithmic layer's Programming Interface: the
/// model-visible building block of an ExecutionPlan. It exists only inside an
/// ExecutionPlan. @sergent/docs/framework.md
///
/// Per-verb structs are application defined. Each carries only action operands
/// and never run identity or the framework operation ID; the id lives on the
/// enclosing `PlanStep`, so it is absent from the derived schema and from
/// serialized call data, and an echoed `op_id` fails the typed crossing as an
/// unknown field. The `Send + Sync` bound lets a plan and patch cross await
/// points in the async runtime.
pub trait Operation: Send + Sync {
    /// The Scene this Operation applies to.
    type Scene;
    /// The validated Intent under which this Operation is admissible.
    type Intent;
    /// The selected Target supplied during application.
    type Target: Target;

    /// Decide whether this Operation is admissible for one bounded run context.
    /// Deterministic and read-only; admissible by default.
    /// @sergent/docs/framework.md
    fn check_admissible(
        &self,
        scene: &Self::Scene,
        intent: &Self::Intent,
        target: &Self::Target,
    ) -> Result<(), Inadmissible> {
        let _ = (scene, intent, target);
        Ok(())
    }

    /// Apply this Operation with the exact validated Intent and Target.
    /// @sergent/docs/framework.md
    fn apply(
        &self,
        scene: &mut Self::Scene,
        intent: &Self::Intent,
        target: &Self::Target,
    ) -> Result<(), OperationFault>;
}
