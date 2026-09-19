//! Intent-stage ownership, including its exact proposal crossing and flow gate.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use std::sync::Arc;

use serde_json::Value;
use sergent_rs_core::error::{ErrorKind, RunError, sanitize_model_output_display};
use sergent_rs_core::intent::{Intent, IntentFlow};
use sergent_rs_core::model::{ModelClient, ModelRequestInput, ParsedJsonObject};
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::run_record::CompletedModelCall;
use sergent_rs_core::run_record::{
    Cancellation, CancellationCheckpoint, RunOutcome, RunTerminal, SergentResult,
};
use sergent_rs_core::scene::SceneActions;
use sergent_rs_core::target::Target;
use sergent_rs_core::timing::Timestamp;
use sergent_rs_core::vocab::{ProgressStatus, RunStepName, Stage};

use crate::cancel::CancelToken;
use crate::clock::sample;
use crate::run_record::{IntentOutputEvidence, OpenStep, RunRecordBuilder};
use crate::scene_state::SceneRebase;

use super::model_invocation::ModelInvocation;
use super::process_input::ProcessedInput;
use super::runtime::IntentMode;
use super::scene_authority::RunScene;
use super::terminal::{Delivery, cancelled_error, finish, finish_cancelled};
use super::{RunSettings, Sergent};

/// A reached Intent step owning all facts needed to close or advance it once.
struct IntentStage<'a, S, T, P> {
    scene: RunScene<S, P>,
    target: T,
    step: OpenStep,
    delivery: Delivery<'a, S>,
}

/// A validated Continue Intent with the exact captured registry needed by Plan.
pub(super) struct ContinuingIntent<'a, S, I, T: Target, P> {
    pub(super) scene: RunScene<S, P>,
    pub(super) target: T,
    pub(super) record_builder: RunRecordBuilder,
    pub(super) delivery: Delivery<'a, S>,
    pub(super) intent: I,
    pub(super) registry: Arc<OperationRegistry<S, I, T>>,
}

enum IntentFailure {
    Failed {
        error: RunError,
        stage: Stage,
    },
    Cancelled {
        requested_at: Timestamp,
        stage: Stage,
        checkpoint: CancellationCheckpoint,
    },
}

impl<'a, S, T, P> IntentStage<'a, S, T, P> {
    /// Opens the Intent Step Record from successfully processed run input.
    fn begin(input: ProcessedInput<'a, S, T, P>) -> Self {
        let (scene, target, record_builder, delivery) = input.into_parts();
        Self {
            scene,
            target,
            step: record_builder.open_step(RunStepName::Intent, sample()),
            delivery,
        }
    }

    /// Closes a failed Intent boundary without changing the captured Scene.
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

    /// Closes cancellation at the Intent phase's observed checkpoint.
    fn cancel(
        self,
        requested_at: Timestamp,
        stage: Stage,
        checkpoint: CancellationCheckpoint,
    ) -> SergentResult<S> {
        let record_builder = self.step.close_cancelled(cancelled_error(), sample());
        finish_cancelled(
            record_builder,
            stage,
            self.scene.into_base_scene(),
            Cancellation::new(requested_at, Some(checkpoint)),
            self.delivery,
        )
    }

    /// Closes a validated Stop Intent as terminal success without mutation.
    fn stop<I: Intent>(self, intent: &I) -> SergentResult<S> {
        let mut record_builder = self.step.close_success(sample());
        record_builder.commit_scene(self.scene.base_identity().revision);
        let terminal = RunTerminal::capture(intent.terminal_message(), intent.terminal_metadata());
        finish(
            record_builder,
            RunOutcome::Success { terminal },
            Stage::Intent,
            self.scene.into_base_scene(),
            self.delivery,
        )
    }

    /// Closes the Intent step and retains the exact registry for Plan.
    fn continuing<I>(
        self,
        intent: I,
        registry: Arc<OperationRegistry<S, I, T>>,
    ) -> ContinuingIntent<'a, S, I, T, P>
    where
        T: Target,
    {
        let record_builder = self.step.close_success(sample());
        ContinuingIntent {
            scene: self.scene,
            target: self.target,
            record_builder,
            delivery: self.delivery,
            intent,
            registry,
        }
    }
}

impl IntentFailure {
    /// Consumes the reached Intent owner through its exact terminal projection.
    fn finish<S, T, P>(self, stage_owner: IntentStage<'_, S, T, P>) -> SergentResult<S> {
        match self {
            Self::Failed { error, stage } => stage_owner.fail(error, stage),
            Self::Cancelled {
                requested_at,
                stage,
                checkpoint,
            } => stage_owner.cancel(requested_at, stage, checkpoint),
        }
    }
}

impl<R, A, M> Sergent<R, A, M>
where
    R: SergentRecipe,
    A: SceneActions<Scene = R::Scene, Intent = R::Intent, Target = R::Target>,
    M: ModelClient,
{
    /// Resolves, derives, validates, and gates one Intent stage.
    pub(super) async fn resolve_intent<'a, P>(
        &'a self,
        input: ProcessedInput<'a, R::Scene, R::Target, P>,
        mindbuf: &'a R::MindBuf,
        settings: &RunSettings,
        cancel: &CancelToken,
    ) -> Result<ContinuingIntent<'a, R::Scene, R::Intent, R::Target, P>, SergentResult<R::Scene>>
    where
        P: SceneRebase<R::Scene, R::Intent, R::Target>,
    {
        let mut stage = IntentStage::begin(input);
        let intent = match self
            .obtain_intent(&mut stage, mindbuf, settings, cancel)
            .await
        {
            Ok(intent) => intent,
            Err(failure) => return Err(failure.finish(stage)),
        };
        stage.step.capture_output(&IntentOutputEvidence {
            derived_intent: &intent,
            flow: intent.flow(),
        });
        if let Err(error) = self.recipe.validate_intent(
            stage.scene.base_scene(),
            stage.scene.base_identity(),
            &intent,
        ) {
            return Err(stage.fail(error, Stage::Intent));
        }
        if let Some(requested_at) = cancel.requested_at() {
            return Err(stage.cancel(
                requested_at,
                Stage::Intent,
                CancellationCheckpoint::AfterIntentValidation,
            ));
        }
        if intent.flow() == IntentFlow::Stop {
            return Err(stage.stop(&intent));
        }
        let Some(registry) = self.registry.as_ref().map(Arc::clone) else {
            let error = RunError::new(
                "recipe_contract_error",
                "continue Intent requires an Operation registry, but configuration is Intent-only",
            );
            return Err(stage.fail(error, Stage::Intent));
        };
        Ok(stage.continuing(intent, registry))
    }

    /// Resolves the captured pass-through or model-backed proposal into Intent.
    async fn obtain_intent<P>(
        &self,
        stage: &mut IntentStage<'_, R::Scene, R::Target, P>,
        mindbuf: &R::MindBuf,
        settings: &RunSettings,
        cancel: &CancelToken,
    ) -> Result<R::Intent, IntentFailure> {
        match &self.intent_mode {
            IntentMode::PassThrough { proposal } => {
                stage.delivery.emit(Stage::Intent, ProgressStatus::Running);
                self.recipe
                    .derive_intent(
                        stage.scene.base_scene(),
                        stage.scene.base_identity(),
                        &stage.target,
                        proposal,
                    )
                    .map_err(|error| IntentFailure::Failed {
                        error,
                        stage: Stage::Intent,
                    })
            }
            IntentMode::ModelBacked { schema } => {
                self.obtain_model_intent(stage, mindbuf, settings, schema, cancel)
                    .await
            }
        }
    }

    /// Invokes the model-backed Intent request, crosses its type, and derives Intent.
    async fn obtain_model_intent<P>(
        &self,
        stage: &mut IntentStage<'_, R::Scene, R::Target, P>,
        mindbuf: &R::MindBuf,
        settings: &RunSettings,
        schema: &Arc<sergent_rs_core::proposal::ProposalSchema>,
        cancel: &CancelToken,
    ) -> Result<R::Intent, IntentFailure> {
        stage
            .delivery
            .emit(Stage::IntentCall, ProgressStatus::Running);
        let messages = self
            .recipe
            .build_intent_messages(stage.scene.base_scene(), mindbuf, &stage.target)
            .map_err(|error| IntentFailure::Failed {
                error,
                stage: Stage::IntentCall,
            })?;
        let request = ModelRequestInput::new(
            settings.model_name().to_owned(),
            settings.intent(),
            Arc::clone(schema),
        )
        .into_request(messages);
        let (call, parsed_json) = match self.invoke_model(&request, cancel).await {
            ModelInvocation::Cancelled { requested_at, call } => {
                stage.step.attach_model_call(*call);
                return Err(IntentFailure::Cancelled {
                    requested_at,
                    stage: Stage::IntentCall,
                    checkpoint: CancellationCheckpoint::TaskCancelled,
                });
            }
            ModelInvocation::ProviderFailure { call, error } => {
                stage.step.attach_model_call(*call);
                return Err(IntentFailure::Failed {
                    error,
                    stage: Stage::IntentCall,
                });
            }
            ModelInvocation::Completed { call, parsed_json } => (*call, parsed_json),
        };
        let proposal = self.cross_intent_proposal(stage, call, &parsed_json)?;
        stage.delivery.emit(Stage::Intent, ProgressStatus::Running);
        self.recipe
            .derive_intent(
                stage.scene.base_scene(),
                stage.scene.base_identity(),
                &stage.target,
                &proposal,
            )
            .map_err(|error| IntentFailure::Failed {
                error,
                stage: Stage::Intent,
            })
    }

    /// Performs the one exact typed Intent crossing and records its truthful evidence.
    fn cross_intent_proposal<P>(
        &self,
        stage: &mut IntentStage<'_, R::Scene, R::Target, P>,
        call: CompletedModelCall,
        parsed_json: &ParsedJsonObject,
    ) -> Result<R::IntentProposal, IntentFailure> {
        let proposal =
            match serde_json::from_value::<R::IntentProposal>(Value::Object(parsed_json.clone())) {
                Ok(proposal) => proposal,
                Err(decode_error) => {
                    stage.step.attach_model_call(call.rejected());
                    return Err(IntentFailure::Failed {
                        error: RunError::of(
                            ErrorKind::SchemaValidationFailed,
                            format!(
                                "intent proposal decode failed: {}",
                                sanitize_model_output_display(&decode_error)
                            ),
                        ),
                        stage: Stage::IntentCall,
                    });
                }
            };
        stage.step.attach_model_call(call.accepted(&proposal));
        Ok(proposal)
    }
}
