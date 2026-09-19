//! Process-input ownership: capture one Scene, open its Run Record, select the
//! exact Target, and observe the first cancellation checkpoint.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use sergent_rs_core::mindbuf::MindBuf;
use sergent_rs_core::model::ModelClient;
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::run_record::{
    Cancellation, CancellationCheckpoint, RunOutcome, RunRecordHeader, SergentResult,
};
use sergent_rs_core::scene::SceneActions;
use sergent_rs_core::vocab::{ProgressStatus, RunStepName, Stage};

use crate::cancel::CancelToken;
use crate::clock::sample;
use crate::run_record::{ProcessInputEvidence, ProcessOutputEvidence, RunRecordBuilder};
use crate::scene_state::{SceneRebase, SceneSource};

use super::scene_authority::RunScene;
use super::terminal::{Delivery, finish, finish_cancelled};
use super::{RunSettings, Sergent};

/// The captured Scene authority, exact Target, Run Record construction, and delivery state after
/// successful process-input handling.
pub(super) struct ProcessedInput<'a, S, T, P> {
    scene: RunScene<S, P>,
    target: T,
    record_builder: RunRecordBuilder,
    delivery: Delivery<'a, S>,
}

/// A consuming process-input transition or its already closed terminal result.
pub(super) type ProcessInputResult<'a, R, P> = Result<
    ProcessedInput<'a, <R as SergentRecipe>::Scene, <R as SergentRecipe>::Target, P>,
    Box<SergentResult<<R as SergentRecipe>::Scene>>,
>;

impl<'a, S, T, P> ProcessedInput<'a, S, T, P> {
    /// Binds the facts produced by successful process-input handling.
    fn new(
        scene: RunScene<S, P>,
        target: T,
        record_builder: RunRecordBuilder,
        delivery: Delivery<'a, S>,
    ) -> Self {
        Self {
            scene,
            target,
            record_builder,
            delivery,
        }
    }

    /// Transfers every owned fact into the next reached stage.
    pub(super) fn into_parts(self) -> (RunScene<S, P>, T, RunRecordBuilder, Delivery<'a, S>) {
        (self.scene, self.target, self.record_builder, self.delivery)
    }
}

impl<R, A, M> Sergent<R, A, M>
where
    R: SergentRecipe,
    A: SceneActions<Scene = R::Scene, Intent = R::Intent, Target = R::Target>,
    M: ModelClient,
{
    /// Captures run input, selects the Target once, and owns the pre-Intent checkpoint.
    pub(super) fn process_input<'a, P>(
        &'a self,
        source: SceneSource<R::Scene, P>,
        mindbuf: &R::MindBuf,
        settings: &RunSettings,
        cancel: &CancelToken,
        mut delivery: Delivery<'a, R::Scene>,
    ) -> ProcessInputResult<'a, R, P>
    where
        P: SceneRebase<R::Scene, R::Intent, R::Target>,
    {
        let run_id = delivery.run_id();
        let started_at = sample();
        let process_started_at = sample();
        let scene = RunScene::capture(source, &self.actions);
        let header = RunRecordHeader::new(run_id, settings.model_name().to_owned());
        let record_builder = RunRecordBuilder::new(header, started_at, scene.base_identity());
        let mut step = record_builder.open_step(RunStepName::ProcessInput, process_started_at);
        let observation = mindbuf.export();
        step.capture_input(&ProcessInputEvidence {
            observation: &observation,
        });
        delivery.observe(scene.base_identity());
        delivery.emit(Stage::Started, ProgressStatus::Running);

        let Some(target) = self.actions.select_target(scene.base_scene()) else {
            step.capture_output(&ProcessOutputEvidence::<R::Target> {
                selected_target: None,
            });
            let record_builder = step.close_failure(self.no_target_error.clone(), sample());
            return Err(Box::new(finish(
                record_builder,
                RunOutcome::Failure {
                    error: self.no_target_error.clone(),
                },
                Stage::Started,
                scene.into_base_scene(),
                delivery,
            )));
        };
        step.capture_output(&ProcessOutputEvidence {
            selected_target: Some(&target),
        });
        let record_builder = step.close_success(sample());
        if let Some(requested_at) = cancel.requested_at() {
            return Err(Box::new(finish_cancelled(
                record_builder,
                Stage::Started,
                scene.into_base_scene(),
                Cancellation::new(requested_at, Some(CancellationCheckpoint::BeforeIntent)),
                delivery,
            )));
        }

        Ok(ProcessedInput::new(scene, target, record_builder, delivery))
    }
}
