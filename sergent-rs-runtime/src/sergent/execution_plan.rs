//! ExecutionPlan-stage ownership, including its exact registry crossing and
//! handoff into unchanged deterministic execution.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use std::sync::Arc;

use sergent_rs_core::error::RunError;
use sergent_rs_core::model::{ModelClient, ModelRequestInput};
use sergent_rs_core::plan::{ExecutionPlan, PlanProposal};
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::run_record::{
    Cancellation, CancellationCheckpoint, RunOutcome, SergentResult,
};
use sergent_rs_core::scene::SceneActions;
use sergent_rs_core::target::Target;
use sergent_rs_core::timing::Timestamp;
use sergent_rs_core::vocab::{ProgressStatus, RunStepName, Stage};

use crate::cancel::CancelToken;
use crate::clock::sample;
use crate::run_record::{ExecutionPlanOutputEvidence, OpenStep};
use crate::scene_state::SceneRebase;

use super::intent::ContinuingIntent;
use super::model_invocation::ModelInvocation;
use super::planned_run::PlannedRun;
use super::scene_authority::RunScene;
use super::terminal::{
    Delivery, cancelled_error, decode_error_to_run_error, finish, finish_cancelled,
};
use super::{RunSettings, Sergent};

/// A reached Plan call with exact continuing facts and one open Step Record.
struct PlanCallStage<'a, S, I, T: Target, P> {
    scene: RunScene<S, P>,
    target: T,
    intent: I,
    registry: Arc<OperationRegistry<S, I, T>>,
    step: OpenStep,
    delivery: Delivery<'a, S>,
}

enum PlanFailure {
    Failed { error: RunError, stage: Stage },
    Cancelled { requested_at: Timestamp },
}

impl<'a, S, I, T: Target, P> PlanCallStage<'a, S, I, T, P> {
    /// Opens the Execution Plan Step Record and emits its reached call stage.
    fn begin(continuing: ContinuingIntent<'a, S, I, T, P>) -> Self {
        let ContinuingIntent {
            scene,
            target,
            record_builder,
            mut delivery,
            intent,
            registry,
        } = continuing;
        let step = record_builder.open_step(RunStepName::ExecutionPlan, sample());
        delivery.emit(Stage::PlanCall, ProgressStatus::Running);
        Self {
            scene,
            target,
            intent,
            registry,
            step,
            delivery,
        }
    }

    /// Closes a failed Plan or derivation boundary without changing the Scene.
    fn fail(self, error: RunError, stage: Stage) -> SergentResult<S> {
        let record_builder = self.step.close_failure(error.clone(), sample());
        finish(
            record_builder,
            RunOutcome::Failure { error },
            stage,
            self.scene.into_base_scene(),
            self.delivery,
        )
    }

    /// Closes cancellation observed while awaiting the Plan provider.
    fn cancel(self, requested_at: Timestamp) -> SergentResult<S> {
        let record_builder = self.step.close_cancelled(cancelled_error(), sample());
        finish_cancelled(
            record_builder,
            Stage::PlanCall,
            self.scene.into_base_scene(),
            Cancellation::new(requested_at, Some(CancellationCheckpoint::TaskCancelled)),
            self.delivery,
        )
    }

    /// Creates the exact planned-run handoff while leaving the Plan step open.
    fn handoff(
        self,
        plan: ExecutionPlan<S, I, T>,
    ) -> (OpenStep, Delivery<'a, S>, PlannedRun<S, I, T, P>) {
        let planned = PlannedRun::new(self.scene, self.target, self.intent, plan);
        (self.step, self.delivery, planned)
    }
}

impl PlanFailure {
    /// Consumes the reached Plan owner through its exact terminal projection.
    fn finish<S, I, T: Target, P>(
        self,
        stage_owner: PlanCallStage<'_, S, I, T, P>,
    ) -> SergentResult<S> {
        match self {
            Self::Failed { error, stage } => stage_owner.fail(error, stage),
            Self::Cancelled { requested_at } => stage_owner.cancel(requested_at),
        }
    }
}

impl<R, A, M> Sergent<R, A, M>
where
    R: SergentRecipe,
    A: SceneActions<Scene = R::Scene, Intent = R::Intent, Target = R::Target>,
    M: ModelClient,
{
    /// Owns Plan request, typed decode, derivation, progress, and deterministic handoff.
    pub(super) async fn execution_plan<P>(
        &self,
        continuing: ContinuingIntent<'_, R::Scene, R::Intent, R::Target, P>,
        mindbuf: &R::MindBuf,
        settings: &RunSettings,
        cancel: &CancelToken,
    ) -> SergentResult<R::Scene>
    where
        P: SceneRebase<R::Scene, R::Intent, R::Target>,
    {
        let mut stage = PlanCallStage::begin(continuing);
        let proposal = match self
            .request_plan(&mut stage, mindbuf, settings, cancel)
            .await
        {
            Ok(proposal) => proposal,
            Err(failure) => return failure.finish(stage),
        };
        stage
            .delivery
            .emit(Stage::ExecutionPlan, ProgressStatus::Running);
        let plan = match self.recipe.derive_plan(
            stage.scene.base_scene(),
            stage.scene.base_identity(),
            &stage.target,
            &stage.intent,
            proposal,
        ) {
            Ok(plan) => plan,
            Err(error) => return stage.fail(error, Stage::ExecutionPlan),
        };
        stage.step.capture_output(&ExecutionPlanOutputEvidence {
            derived_execution_plan: &plan,
        });
        let (step, delivery, planned) = stage.handoff(plan);
        self.commit_pipeline(step, delivery, planned, cancel)
    }

    /// Builds, invokes, and decodes the exact registry-owned Plan proposal.
    async fn request_plan<P>(
        &self,
        stage: &mut PlanCallStage<'_, R::Scene, R::Intent, R::Target, P>,
        mindbuf: &R::MindBuf,
        settings: &RunSettings,
        cancel: &CancelToken,
    ) -> Result<PlanProposal<R::Scene, R::Intent, R::Target>, PlanFailure> {
        let plan_schema = stage.registry.plan_schema();
        let messages = self
            .recipe
            .build_plan_messages(
                stage.scene.base_scene(),
                &stage.target,
                &stage.intent,
                mindbuf,
            )
            .map_err(|error| PlanFailure::Failed {
                error,
                stage: Stage::PlanCall,
            })?;
        let request = ModelRequestInput::new(
            settings.model_name().to_owned(),
            settings.plan(),
            Arc::clone(&plan_schema),
        )
        .into_request(messages);
        let (call, parsed_json) = match self.invoke_model(&request, cancel).await {
            ModelInvocation::Cancelled { requested_at, call } => {
                stage.step.attach_model_call(*call);
                return Err(PlanFailure::Cancelled { requested_at });
            }
            ModelInvocation::ProviderFailure { call, error } => {
                stage.step.attach_model_call(*call);
                return Err(PlanFailure::Failed {
                    error,
                    stage: Stage::PlanCall,
                });
            }
            ModelInvocation::Completed { call, parsed_json } => (*call, parsed_json),
        };
        let proposal = match stage.registry.decode(&parsed_json) {
            Ok(proposal) => proposal,
            Err(decode_error) => {
                stage.step.attach_model_call(call.rejected());
                return Err(PlanFailure::Failed {
                    error: decode_error_to_run_error(&decode_error),
                    stage: Stage::PlanCall,
                });
            }
        };
        stage.step.attach_model_call(call.accepted(&proposal));
        Ok(proposal)
    }
}
