//! The deterministic execution core and commit (the run's synchronous tail):
//! per-Operation admissibility, whole-plan validation, patch compile and
//! dry-run, then revision-checked commit through plain or shared live authority.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! This tail has no await points, so a shared-live commit critical section is
//! atomic by construction. It receives the derived plan and the open
//! Execution Plan Step Record from the async front and seals the run.

use sergent_rs_core::error::RunError;
use sergent_rs_core::model::ModelClient;
use sergent_rs_core::plan::{ExecutionPlan, Patch};
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::run_record::{
    Cancellation, CancellationCheckpoint, RunOutcome, SergentResult,
};
use sergent_rs_core::scene::SceneActions;
use sergent_rs_core::target::Target;
use sergent_rs_core::vocab::{ProgressStatus, RunStepName, Stage};

use crate::admissibility::{AdmissibilityContext, AdmissibilityRejection, check_operations};
use crate::cancel::CancelToken;
use crate::clock::sample;
use crate::commit::{DryRunContext, dry_run, validate_patch};
use crate::run_record::{OpenStep, PatchEvidence, RunRecordBuilder};
use crate::scene_state::SceneRebase;

use super::scene_authority::RunScene;
use super::terminal::{Delivery, cancelled_error, finish, finish_cancelled};
use super::{Sergent, planned_run::PlannedRun};

/// Stable ownership that lets every deterministic stage borrow the exact selected Target.
type SelectedTarget<T> = Box<T>;
/// Stable ownership that lets every deterministic stage borrow the exact validated Intent.
type ValidatedIntent<I> = Box<I>;

/// Owns the derived Plan and its open Step Record until semantic validation succeeds.
struct ExecutionPlanStage<'a, S, I, T: Target, P> {
    step: OpenStep,
    delivery: Delivery<'a, S>,
    scene: RunScene<S, P>,
    target: SelectedTarget<T>,
    intent: ValidatedIntent<I>,
    plan: ExecutionPlan<S, I, T>,
}

impl<'a, S, I, T: Target, P> ExecutionPlanStage<'a, S, I, T, P> {
    /// Consumes the approved async-to-deterministic handoff into the first stage owner.
    fn new(step: OpenStep, delivery: Delivery<'a, S>, planned: PlannedRun<S, I, T, P>) -> Self {
        let (scene, target, intent, plan) = planned.into_parts();
        Self {
            step,
            delivery,
            scene,
            target: Box::new(target),
            intent: Box::new(intent),
            plan,
        }
    }
}

/// Owns the validated Plan and its closed Step Record while its Patch is prepared.
struct PatchPreparationStage<'a, S, I, T: Target, P> {
    record_builder: RunRecordBuilder,
    delivery: Delivery<'a, S>,
    scene: RunScene<S, P>,
    target: SelectedTarget<T>,
    intent: ValidatedIntent<I>,
    plan: ExecutionPlan<S, I, T>,
}

/// Owns a compiled, envelope-validated Patch and its still-open Step Record.
struct DryRunStage<'a, S, I, T: Target, P> {
    step: OpenStep,
    evidence: PatchEvidence,
    delivery: Delivery<'a, S>,
    scene: RunScene<S, P>,
    target: SelectedTarget<T>,
    intent: ValidatedIntent<I>,
    patch: Patch<S, I, T>,
}

/// Owns a successfully rehearsed Patch until commit authority accepts or rejects it.
struct CommitStage<'a, S, I, T: Target, P> {
    record_builder: RunRecordBuilder,
    delivery: Delivery<'a, S>,
    scene: RunScene<S, P>,
    target: SelectedTarget<T>,
    intent: ValidatedIntent<I>,
    patch: Patch<S, I, T>,
    dry: S,
}

/// A consuming deterministic-stage transition or an already closed terminal result.
type StageResult<T, S> = Result<T, Box<SergentResult<S>>>;

/// The transition from accepted Plan semantics to Patch preparation.
type ExecutionPlanStageResult<'a, R, P> = StageResult<
    PatchPreparationStage<
        'a,
        <R as SergentRecipe>::Scene,
        <R as SergentRecipe>::Intent,
        <R as SergentRecipe>::Target,
        P,
    >,
    <R as SergentRecipe>::Scene,
>;

/// The transition from an accepted Patch envelope to its dry-run checkpoint.
type PatchPreparationStageResult<'a, R, P> = StageResult<
    DryRunStage<
        'a,
        <R as SergentRecipe>::Scene,
        <R as SergentRecipe>::Intent,
        <R as SergentRecipe>::Target,
        P,
    >,
    <R as SergentRecipe>::Scene,
>;

/// The transition from successful dry-run proof to the commit checkpoint.
type DryRunStageResult<'a, R, P> = StageResult<
    CommitStage<
        'a,
        <R as SergentRecipe>::Scene,
        <R as SergentRecipe>::Intent,
        <R as SergentRecipe>::Target,
        P,
    >,
    <R as SergentRecipe>::Scene,
>;

impl<R, A, M> Sergent<R, A, M>
where
    R: SergentRecipe,
    A: SceneActions<Scene = R::Scene, Intent = R::Intent, Target = R::Target>,
    M: ModelClient,
{
    /// The deterministic tail: per-Operation admissibility,
    /// whole-plan validation, patch compile and dry-run, then revision-checked
    /// commit through plain or shared live authority. It runs with no await
    /// points, so a shared-live commit is atomic.
    pub(super) fn commit_pipeline<P>(
        &self,
        step: OpenStep,
        delivery: Delivery<'_, R::Scene>,
        planned: PlannedRun<R::Scene, R::Intent, R::Target, P>,
        cancel: &CancelToken,
    ) -> SergentResult<R::Scene>
    where
        P: SceneRebase<R::Scene, R::Intent, R::Target>,
    {
        let stage = ExecutionPlanStage::new(step, delivery, planned);
        let stage = match self.validate_execution_plan_stage(stage) {
            Ok(stage) => stage,
            Err(result) => return *result,
        };
        let stage = match self.prepare_patch_stage(stage) {
            Ok(stage) => stage,
            Err(result) => return *result,
        };
        let stage = match self.dry_run_stage(stage, cancel) {
            Ok(stage) => stage,
            Err(result) => return *result,
        };
        self.commit_stage(stage, cancel)
    }

    /// Checks Operations in order before recipe-owned ordered and whole-plan validation.
    fn validate_execution_plan_stage<'a, P>(
        &self,
        stage: ExecutionPlanStage<'a, R::Scene, R::Intent, R::Target, P>,
    ) -> ExecutionPlanStageResult<'a, R, P> {
        let ExecutionPlanStage {
            step,
            delivery,
            scene,
            target,
            intent,
            plan,
        } = stage;
        if let Err(rejection) = check_operations(
            plan.steps(),
            AdmissibilityContext::new(scene.base_scene(), &intent, &target),
            &self.actions,
        ) {
            let error = initial_admissibility_error(rejection);
            let record_builder = step.close_failure(error.clone(), sample());
            return Err(Box::new(finish(
                record_builder,
                RunOutcome::Failure { error },
                Stage::ExecutionPlan,
                scene.into_base_scene(),
                delivery,
            )));
        }
        if let Err(error) = self.recipe.validate_plan(
            scene.base_scene(),
            scene.base_identity(),
            &target,
            &intent,
            &plan,
        ) {
            let record_builder = step.close_failure(error.clone(), sample());
            return Err(Box::new(finish(
                record_builder,
                RunOutcome::Failure { error },
                Stage::ExecutionPlan,
                scene.into_base_scene(),
                delivery,
            )));
        }
        let record_builder = step.close_success(sample());
        Ok(PatchPreparationStage {
            record_builder,
            delivery,
            scene,
            target,
            intent,
            plan,
        })
    }

    /// Compiles the validated Plan, mints its ordered trace, and validates the Patch envelope.
    fn prepare_patch_stage<'a, P>(
        &self,
        stage: PatchPreparationStage<'a, R::Scene, R::Intent, R::Target, P>,
    ) -> PatchPreparationStageResult<'a, R, P> {
        let PatchPreparationStage {
            record_builder,
            mut delivery,
            scene,
            target,
            intent,
            plan,
        } = stage;
        let mut step = record_builder.open_step(RunStepName::Patch, sample());
        delivery.emit(Stage::Patch, ProgressStatus::Running);
        let patch = match self.recipe.compile_patch(&plan) {
            Ok(patch) => patch,
            Err(error) => {
                let record_builder = step.close_failure(error.clone(), sample());
                return Err(patch_failure(record_builder, error, scene, delivery));
            }
        };
        let mut evidence = PatchEvidence::of(&patch);
        step.capture_output(&evidence);
        if let Err(error) = validate_patch(
            &patch,
            scene.base_identity(),
            scene.base_scene(),
            &target,
            &self.actions,
        ) {
            evidence.mark_validation_failure();
            step.capture_output(&evidence);
            let record_builder = step.close_failure(error.clone(), sample());
            return Err(patch_failure(record_builder, error, scene, delivery));
        }
        Ok(DryRunStage {
            step,
            evidence,
            delivery,
            scene,
            target,
            intent,
            patch,
        })
    }

    /// Observes the dry-run checkpoint, preflights embedded revision, and rehearses once.
    fn dry_run_stage<'a, P>(
        &self,
        stage: DryRunStage<'a, R::Scene, R::Intent, R::Target, P>,
        cancel: &CancelToken,
    ) -> DryRunStageResult<'a, R, P> {
        let DryRunStage {
            step,
            mut evidence,
            mut delivery,
            scene,
            target,
            intent,
            patch,
        } = stage;
        delivery.emit(Stage::DryRun, ProgressStatus::Running);
        if let Some(requested_at) = cancel.requested_at() {
            let record_builder = step.close_cancelled(cancelled_error(), sample());
            return Err(Box::new(finish_cancelled(
                record_builder,
                Stage::DryRun,
                scene.into_base_scene(),
                Cancellation::new(requested_at, Some(CancellationCheckpoint::BeforeDryRun)),
                delivery,
            )));
        }
        let dry = match self.rehearse_patch(&scene, &intent, &target, &patch) {
            Ok(dry) => dry,
            Err(error) => {
                let record_builder = step.close_failure(error.clone(), sample());
                return Err(Box::new(finish(
                    record_builder,
                    RunOutcome::Failure { error },
                    Stage::DryRun,
                    scene.into_base_scene(),
                    delivery,
                )));
            }
        };
        evidence.mark_dry_run(self.actions.identity(&dry));
        let record_builder = step.close_patch_success(&evidence, sample());
        Ok(CommitStage {
            record_builder,
            delivery,
            scene,
            target,
            intent,
            patch,
            dry,
        })
    }

    /// Preflights embedded revision capacity, then performs the sole evolving simulation.
    fn rehearse_patch<P>(
        &self,
        scene: &RunScene<R::Scene, P>,
        intent: &R::Intent,
        target: &R::Target,
        patch: &Patch<R::Scene, R::Intent, R::Target>,
    ) -> Result<R::Scene, RunError> {
        let expected_identity = scene.expected_identity()?;
        dry_run(
            patch,
            DryRunContext::new(
                scene.base_scene(),
                intent,
                target,
                expected_identity.as_ref(),
            ),
            &self.actions,
        )
        .map_err(|failure| failure.into_run_error())
    }

    /// Observes the final checkpoint, enters existing authority, and closes terminal evidence.
    fn commit_stage<P>(
        &self,
        stage: CommitStage<'_, R::Scene, R::Intent, R::Target, P>,
        cancel: &CancelToken,
    ) -> SergentResult<R::Scene>
    where
        P: SceneRebase<R::Scene, R::Intent, R::Target>,
    {
        let CommitStage {
            record_builder,
            mut delivery,
            scene,
            target,
            intent,
            patch,
            dry,
        } = stage;
        let mut step = record_builder.open_commit(sample());
        delivery.emit(Stage::Commit, ProgressStatus::Running);
        if let Some(requested_at) = cancel.requested_at() {
            let record_builder = step.close_cancelled(cancelled_error(), sample());
            return finish_cancelled(
                record_builder,
                Stage::Commit,
                scene.into_base_scene(),
                Cancellation::new(requested_at, Some(CancellationCheckpoint::BeforeCommit)),
                delivery,
            );
        }
        let commit_ok = match scene.commit(&patch, &intent, &target, dry, &self.actions) {
            Ok(ok) => ok,
            Err(error) => {
                let record_builder = step.close_failure(error.clone(), sample());
                return finish(
                    record_builder,
                    RunOutcome::Failure { error },
                    Stage::Commit,
                    scene.into_base_scene(),
                    delivery,
                );
            }
        };
        step.capture_output(commit_ok.commit_kind, &commit_ok.metadata);
        let terminal = scene.terminal(&commit_ok);
        let record_builder = step.close_success(commit_ok.revision_after, sample());
        finish(
            record_builder,
            RunOutcome::Success { terminal },
            Stage::Commit,
            commit_ok.scene,
            delivery,
        )
    }
}

/// Closes a rejected Patch stage over its already closed step, leaving the
/// captured Scene unchanged.
fn patch_failure<S, P>(
    record_builder: RunRecordBuilder,
    error: RunError,
    scene: RunScene<S, P>,
    delivery: Delivery<'_, S>,
) -> Box<SergentResult<S>> {
    Box::new(finish(
        record_builder,
        RunOutcome::Failure { error },
        Stage::Patch,
        scene.into_base_scene(),
        delivery,
    ))
}

/// Projects initial admissibility rejection into the model-output boundary contract.
fn initial_admissibility_error(rejection: AdmissibilityRejection) -> RunError {
    RunError::admissibility(
        format!(
            "operation is inadmissible for the run context: {}",
            rejection.reason
        ),
        rejection.operation_index,
        rejection.operation.call(),
        rejection.operation.op_id(),
    )
}
