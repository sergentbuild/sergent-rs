//! The short async orchestrator for process input, Intent, and ExecutionPlan.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use sergent_rs_core::model::ModelClient;
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::run_record::SergentResult;
use sergent_rs_core::scene::SceneActions;
use sergent_rs_core::vocab::{ProgressStatus, Stage};

use crate::cancel::CancelToken;
use crate::scene_state::{SceneRebase, SceneSource};

use super::terminal::Delivery;
use super::{RunSettings, Sergent};

impl<R, A, M> Sergent<R, A, M>
where
    R: SergentRecipe,
    A: SceneActions<Scene = R::Scene, Intent = R::Intent, Target = R::Target>,
    M: ModelClient,
{
    /// Orchestrates the three async-front owners, then returns deterministic execution.
    pub(super) async fn run_inner<'a, P>(
        &'a self,
        source: SceneSource<R::Scene, P>,
        mindbuf: &'a R::MindBuf,
        settings: RunSettings,
        cancel: &'a CancelToken,
        mut delivery: Delivery<'a, R::Scene>,
    ) -> SergentResult<R::Scene>
    where
        P: SceneRebase<R::Scene, R::Intent, R::Target>,
    {
        delivery.emit(Stage::Queued, ProgressStatus::Queued);
        let processed = match self.process_input(source, mindbuf, &settings, cancel, delivery) {
            Ok(processed) => processed,
            Err(result) => return *result,
        };
        let continuing = match self
            .resolve_intent(processed, mindbuf, &settings, cancel)
            .await
        {
            Ok(continuing) => continuing,
            Err(result) => return result,
        };
        self.execution_plan(continuing, mindbuf, &settings, cancel)
            .await
    }
}
