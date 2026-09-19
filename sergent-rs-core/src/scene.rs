//! Scene identity, verification, and the deterministic Scene-action contract.
//! @sergent/docs/framework.md @sergent/docs/execution-model.md

use serde::Serialize;

use crate::ids::SceneId;
use crate::operation::{OperationFault, PlanStep};
use crate::target::Target;

/// Implement `SceneActions::clone_scene` through `Clone::clone` for the common
/// value-Scene case. Use this only when `Clone` produces an isolated snapshot,
/// including every nested mutable value. It is an opt-in convenience, not an
/// isolation proof or a trait default. @sergent/docs/execution-model.md
#[macro_export]
macro_rules! clone_scene_via_clone {
    () => {
        fn clone_scene(&self, scene: &Self::Scene) -> Self::Scene {
            ::core::clone::Clone::clone(scene)
        }
    };
}

/// A stable Scene identity bound to one non-negative revision. The revision
/// records what a run observed. @sergent/docs/framework.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SceneIdentity {
    /// The stable Scene identifier.
    pub scene_id: SceneId,
    /// The observed revision.
    pub revision: u64,
}

/// A machine-readable Scene verification result. @sergent/docs/framework.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VerificationReport {
    issues: Vec<String>,
}

impl VerificationReport {
    /// An accepted report with no issues.
    pub fn accepted() -> Self {
        Self { issues: Vec::new() }
    }

    /// A rejected report carrying at least one issue.
    pub fn rejected(
        first_issue: impl Into<String>,
        additional_issues: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut issues = vec![first_issue.into()];
        issues.extend(additional_issues);
        Self { issues }
    }

    /// Whether verification found no issues.
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Borrow the application-owned verification issues.
    pub fn issues(&self) -> &[String] {
        &self.issues
    }
}

/// Report drift between an expected Scene identity and an observed one: a
/// changed Scene id or an unequal revision. Runtime enforces embedded-revision
/// consistency with it at `try_edit` and dry-run, advancing the identity of the
/// Scene base being committed by exactly one; applications reach this rule only
/// through runtime behavior. @sergent/docs/framework.md
pub fn identity_transition_issues(
    expected: &SceneIdentity,
    observed: &SceneIdentity,
) -> Vec<String> {
    let mut issues = Vec::new();
    if observed.scene_id != expected.scene_id {
        issues.push("scene identity changed".to_owned());
    }
    if observed.revision != expected.revision {
        issues.push("scene revision mismatch".to_owned());
    }
    issues
}

/// The synchronous deterministic authority over one Scene type required by the
/// runtime: identity, isolated clone, target selection and existence,
/// target-aware apply, and verification. This is the Rust Scene API used by
/// Operations and the ExecutionPlan.
/// @sergent/docs/execution-model.md
pub trait SceneActions {
    /// The application Scene type.
    type Scene;
    /// The validated Intent type carried by plan Operations.
    type Intent;
    /// The selected Target type.
    type Target: Target;

    /// Return the stable Scene identity and current revision view.
    fn identity(&self, scene: &Self::Scene) -> SceneIdentity;

    /// Return an isolated snapshot copy of the Scene.
    fn clone_scene(&self, scene: &Self::Scene) -> Self::Scene;

    /// Choose a bounded Target through a deterministic Scene read, or none.
    fn select_target(&self, scene: &Self::Scene) -> Option<Self::Target>;

    /// Return whether the selected Target still exists.
    fn has_target(&self, scene: &Self::Scene, target: &Self::Target) -> bool;

    /// Apply the Operations with the validated Intent and selected Target.
    fn apply(
        &self,
        scene: &mut Self::Scene,
        intent: &Self::Intent,
        target: &Self::Target,
        operations: &[PlanStep<Self::Scene, Self::Intent, Self::Target>],
    ) -> Result<(), OperationFault>;

    /// Reject invalid deterministic mutation before commit.
    fn verify(
        &self,
        before: &Self::Scene,
        after: &Self::Scene,
        target: &Self::Target,
        operations: &[PlanStep<Self::Scene, Self::Intent, Self::Target>],
    ) -> VerificationReport;
}

#[cfg(test)]
mod tests {
    use super::VerificationReport;

    #[test]
    fn accepted_and_rejected_reports_have_coherent_issue_sets() {
        let accepted = VerificationReport::accepted();
        assert!(accepted.is_ok());
        assert!(accepted.issues().is_empty());

        let rejected = VerificationReport::rejected("first", std::iter::empty());
        assert!(!rejected.is_ok());
        assert_eq!(rejected.issues(), ["first"]);
    }
}
