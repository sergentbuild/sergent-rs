//! Plain and shared Scene commit authority, including deterministic stale rebase.

use serde_json::{Map, Value};
use sergent_rs_core::error::RunError;
use sergent_rs_core::plan::Patch;
use sergent_rs_core::run_record::PatchSummary;
use sergent_rs_core::scene::{SceneActions, SceneIdentity};
use sergent_rs_core::target::Target;

use crate::admissibility::{AdmissibilityContext, AdmissibilityRejection, check_operations};
use crate::scene_state::{
    LiveCommitInput, LiveInner, RebaseContext, RebaseOutcome, SceneRebase, SceneState, StalePolicy,
    StrictRevision, advance_identity,
};

use super::safety::{DryRunContext, dry_run, validate_patch};

/// The outcome of a successful commit: the final Scene, committed revision,
/// and the accepted original or rebased Patch evidence.
pub(crate) struct CommitOk<S> {
    pub(crate) scene: S,
    pub(crate) revision_after: u64,
    pub(crate) commit_kind: &'static str,
    pub(crate) metadata: Map<String, Value>,
}

/// One shared-commit result already proven safe for atomic installation.
struct LiveCommitCandidate<S> {
    after_scene: S,
    next_identity: SceneIdentity,
    commit_kind: &'static str,
    metadata: Map<String, Value>,
}

/// A rebased Patch selected by stale policy after checking revision capacity.
struct SelectedRebasedPatch<S, I, T: Target> {
    patch: Patch<S, I, T>,
    next_identity: SceneIdentity,
    metadata: Map<String, Value>,
}

/// Commit against a plain snapshot with no current-state comparison.
pub(crate) fn commit_plain<S>(after: S, revision_after: u64) -> CommitOk<S> {
    CommitOk {
        scene: after,
        revision_after,
        commit_kind: "plain",
        metadata: Map::new(),
    }
}

/// Projects shared admissibility rejection into the commit boundary contract.
fn rebase_admissibility_error(rejection: AdmissibilityRejection) -> RunError {
    RunError::operation_admissibility(
        format!(
            "rebased operation is inadmissible against the current scene: {}",
            rejection.reason
        ),
        rejection.operation_index,
        rejection.operation.call(),
        rejection.operation.op_id(),
    )
}

/// Prepares the already accepted exact after-Scene for shared installation.
fn prepare_exact_commit<S>(
    current_identity: &SceneIdentity,
    exact_after: S,
) -> Result<LiveCommitCandidate<S>, RunError> {
    Ok(LiveCommitCandidate {
        after_scene: exact_after,
        next_identity: advance_identity(current_identity)?,
        commit_kind: "exact",
        metadata: Map::new(),
    })
}

/// Selects strict stale failure or one application-rebased Patch. Rebase-capable
/// authority checks revision advancement before invoking application code.
fn select_rebased_patch<S, I, T, P>(
    policy: &StalePolicy<P>,
    context: RebaseContext<'_, S, I, T>,
) -> Result<SelectedRebasedPatch<S, I, T>, RunError>
where
    T: Target,
    P: SceneRebase<S, I, T>,
{
    match policy {
        StalePolicy::Strict => Err(StrictRevision::stale_error(&context)),
        StalePolicy::Rebase(rebaser) => {
            let next_identity = advance_identity(context.current_identity())?;
            let (patch, metadata) = match rebaser.rebase(context) {
                RebaseOutcome::Conflict(error) => {
                    let summary = PatchSummary::from_patch(context.original_patch());
                    return Err(RunError::merge_conflict(
                        error.message,
                        context.original_patch().base().revision,
                        context.current_identity().revision,
                        &summary,
                        error.metadata,
                    ));
                }
                RebaseOutcome::Rebased { patch, metadata } => (patch, metadata),
            };
            Ok(SelectedRebasedPatch {
                patch,
                next_identity,
                metadata,
            })
        }
    }
}

/// Requires a rebase to preserve the complete ordered Operation identity sequence.
fn ensure_rebased_operation_ids<S, I, T: Target>(
    original: &Patch<S, I, T>,
    rebased: &Patch<S, I, T>,
) -> Result<(), RunError> {
    if original
        .steps()
        .iter()
        .map(|step| step.op_id())
        .eq(rebased.steps().iter().map(|step| step.op_id()))
    {
        return Ok(());
    }
    Err(RunError::new(
        "operation_identity",
        "rebased patch changed the ordered Operation identity sequence",
    ))
}

/// Wrap one rejected returned Patch with the exact summary and Scene mapping.
fn rejected_rebase<S, I, T: Target>(
    error: RunError,
    input: &LiveCommitInput<'_, S, I, T>,
    current: &LiveInner<S>,
    rejected: &Patch<S, I, T>,
    metadata: Map<String, Value>,
) -> RunError {
    let summary = PatchSummary::from_patch(rejected);
    RunError::rebased_merge_conflict(
        input.patch.base().revision,
        current.identity.revision,
        &summary,
        metadata,
        error,
    )
}

/// Validate the returned Patch envelope, identity sequence, and Operations.
fn validate_rebased_patch<S, I, T, A>(
    selected: &SelectedRebasedPatch<S, I, T>,
    input: &LiveCommitInput<'_, S, I, T>,
    current: &LiveInner<S>,
    actions: &A,
) -> Result<(), RunError>
where
    T: Target,
    A: SceneActions<Scene = S, Intent = I, Target = T>,
{
    let wrap = |error| {
        rejected_rebase(
            error,
            input,
            current,
            &selected.patch,
            selected.metadata.clone(),
        )
    };
    validate_patch(
        &selected.patch,
        &current.identity,
        &current.scene,
        input.target,
        actions,
    )
    .map_err(&wrap)?;
    ensure_rebased_operation_ids(input.patch, &selected.patch).map_err(&wrap)?;
    check_operations(
        selected.patch.steps(),
        AdmissibilityContext::new(&current.scene, input.intent, input.target),
        actions,
    )
    .map_err(|rejection| wrap(rebase_admissibility_error(rejection)))
}

/// Runs the complete current-authority rebase safety sequence and prepares one
/// candidate without mutating live state.
fn prepare_rebased_commit<S, I, T, A, P>(
    state: &SceneState<S, P>,
    actions: &A,
    input: &LiveCommitInput<'_, S, I, T>,
    current: &LiveInner<S>,
) -> Result<LiveCommitCandidate<S>, RunError>
where
    T: Target,
    A: SceneActions<Scene = S, Intent = I, Target = T>,
    P: SceneRebase<S, I, T>,
{
    let context = RebaseContext::new(input, current);
    let selected = select_rebased_patch(state.policy(), context)?;
    validate_rebased_patch(&selected, input, current, actions)?;
    let after_scene = dry_run(
        &selected.patch,
        DryRunContext::new(
            &current.scene,
            input.intent,
            input.target,
            state.is_embedded().then_some(&selected.next_identity),
        ),
        actions,
    )
    .map_err(|failure| {
        rejected_rebase(
            failure.into_rebase_error(),
            input,
            current,
            &selected.patch,
            selected.metadata.clone(),
        )
    })?;
    Ok(LiveCommitCandidate {
        after_scene,
        next_identity: selected.next_identity,
        commit_kind: "rebased",
        metadata: selected.metadata,
    })
}

/// Clones the result Scene before the only shared-commit Scene and identity assignment.
fn install_live_commit<S, I, T, A>(
    current: &mut LiveInner<S>,
    actions: &A,
    candidate: LiveCommitCandidate<S>,
) -> CommitOk<S>
where
    T: Target,
    A: SceneActions<Scene = S, Intent = I, Target = T>,
{
    let returned = actions.clone_scene(&candidate.after_scene);
    let revision_after = candidate.next_identity.revision;
    current.scene = candidate.after_scene;
    current.identity = candidate.next_identity;
    CommitOk {
        scene: returned,
        revision_after,
        commit_kind: candidate.commit_kind,
        metadata: candidate.metadata,
    }
}

/// Commit against shared live state under one lock, preparing an exact or
/// rebased candidate before installing it through one mutation seam.
pub(crate) fn commit_live<S, I, T, A, P>(
    state: &SceneState<S, P>,
    actions: &A,
    input: LiveCommitInput<'_, S, I, T>,
    exact_after: S,
) -> Result<CommitOk<S>, RunError>
where
    T: Target,
    A: SceneActions<Scene = S, Intent = I, Target = T>,
    P: SceneRebase<S, I, T>,
{
    let mut guard = state.lock();
    let exact = guard.identity == *input.base_identity;
    let candidate = if exact {
        prepare_exact_commit(&guard.identity, exact_after)?
    } else {
        prepare_rebased_commit(state, actions, &input, &guard)?
    };
    Ok(install_live_commit(&mut guard, actions, candidate))
}
