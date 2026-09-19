//! Patch envelope and effect proofs completed before commit authority mutates.

use sergent_rs_core::error::RunError;
use sergent_rs_core::plan::Patch;
use sergent_rs_core::scene::{SceneActions, SceneIdentity, identity_transition_issues};
use sergent_rs_core::target::Target;

/// The exact trusted facts used by one evolving Patch rehearsal.
pub(crate) struct DryRunContext<'a, S, I, T> {
    scene: &'a S,
    intent: &'a I,
    target: &'a T,
    expected_identity: Option<&'a SceneIdentity>,
}

impl<'a, S, I, T> DryRunContext<'a, S, I, T> {
    /// Borrow one original or current Scene context without copying run facts.
    pub(crate) fn new(
        scene: &'a S,
        intent: &'a I,
        target: &'a T,
        expected_identity: Option<&'a SceneIdentity>,
    ) -> Self {
        Self {
            scene,
            intent,
            target,
            expected_identity,
        }
    }
}

/// One dry-run rejection with its original error and rebase check identity.
pub(crate) struct DryRunFailure {
    error: RunError,
    rebase_kind: Option<&'static str>,
}

impl DryRunFailure {
    /// Preserve an application Operation fault without reclassification.
    fn application(error: RunError) -> Self {
        Self {
            error,
            rebase_kind: None,
        }
    }

    /// Bind one framework rejection to its implementation-native rebase name.
    fn framework(error: RunError, rebase_kind: &'static str) -> Self {
        Self {
            error,
            rebase_kind: Some(rebase_kind),
        }
    }

    /// Preserve the original-run error shape.
    pub(crate) fn into_run_error(self) -> RunError {
        self.error
    }

    /// Preserve application errors and name a framework check for rebase evidence.
    pub(crate) fn into_rebase_error(mut self) -> RunError {
        if let Some(kind) = self.rebase_kind {
            self.error.kind = kind.to_owned();
        }
        self.error
    }
}

/// Validate the patch envelope returned by the app-overridable compile hook:
/// base identity matches the run snapshot and the target still exists (trust
/// boundary 1 tail). Its non-empty leg is the `PatchEvidence` the caller mints
/// from the same Patch before this call.
pub(crate) fn validate_patch<S, I, T, A>(
    patch: &Patch<S, I, T>,
    base: &SceneIdentity,
    scene: &S,
    target: &T,
    actions: &A,
) -> Result<(), RunError>
where
    T: Target,
    A: SceneActions<Scene = S, Intent = I, Target = T>,
{
    if patch.steps().is_empty() {
        return Err(RunError::patch_validation(
            "compiled Patch must contain at least one Operation",
        ));
    }
    if patch.base() != base {
        return Err(RunError::patch_validation(
            "patch base identity does not match the run snapshot",
        ));
    }
    if !actions.has_target(scene, target) {
        return Err(RunError::patch_validation(
            "selected target no longer exists on the scene",
        ));
    }
    Ok(())
}

/// The only phase that applies the full script in order to one evolving
/// isolated copy, then verifies and, when embedded identity is
/// declared, checks the identity transition.
pub(crate) fn dry_run<S, I, T, A>(
    patch: &Patch<S, I, T>,
    context: DryRunContext<'_, S, I, T>,
    actions: &A,
) -> Result<S, DryRunFailure>
where
    T: Target,
    A: SceneActions<Scene = S, Intent = I, Target = T>,
{
    let mut after = actions.clone_scene(context.scene);
    actions
        .apply(&mut after, context.intent, context.target, patch.steps())
        .map_err(|fault| DryRunFailure::application(fault.into_run_error()))?;
    let report = actions.verify(context.scene, &after, context.target, patch.steps());
    if !report.is_ok() {
        return Err(DryRunFailure::framework(
            RunError::verification(
                "scene verification rejected the patch",
                report.issues().iter().cloned(),
            ),
            "scene_verification",
        ));
    }
    if let Some(expected_identity) = context.expected_identity {
        let after_identity = actions.identity(&after);
        let issues = identity_transition_issues(expected_identity, &after_identity);
        if !issues.is_empty() {
            return Err(DryRunFailure::framework(
                RunError::embedded_identity(
                    "committed scene identity drifted from the declared embedded revision",
                    issues,
                    expected_identity,
                    &after_identity,
                ),
                "embedded_identity",
            ));
        }
    }
    Ok(after)
}
