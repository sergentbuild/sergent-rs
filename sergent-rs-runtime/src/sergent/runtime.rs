//! The configured runtime object: the facts construction captures once and the
//! two entries that reach the shared pipeline.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! `Sergent` owns the recipe, its scene actions, and the model client; a run
//! never rereads recipe configuration. `run` awaits one bounded pipeline and
//! `start` schedules the same pipeline on a task behind a `RunHandle`.

use std::sync::{Arc, Mutex};

use sergent_rs_core::ids::RunId;
use sergent_rs_core::model::ModelClient;
use sergent_rs_core::proposal::{ProposalSchema, SchemaError, derive_proposal_schema};
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::run_record::SergentResult;
use sergent_rs_core::scene::SceneActions;

use crate::cancel::CancelToken;
use crate::observer::RunObserver;
use crate::progress::ProgressSnapshot;
use crate::scene_state::{SceneRebase, SceneSource};

use super::terminal::Delivery;
use super::{ConfiguredRecipe, RunSettings};

/// The captured Intent source that combines with registry capability to form Run Kind.
pub(super) enum IntentMode<P> {
    /// No Intent provider call; every run borrows the proposal captured at
    /// construction.
    PassThrough { proposal: P },
    /// A model-backed Intent phase carrying the captured canonical schema.
    ModelBacked { schema: Arc<ProposalSchema> },
}

/// A construction-time failure of the runtime builder contract.
#[derive(Debug, thiserror::Error)]
pub enum ConstructionError {
    /// The Intent proposal type is not expressible in the canonical dialect.
    #[error("intent proposal schema derivation failed: {0}")]
    IntentSchema(#[from] SchemaError),
}

/// A configured Sergent Instance representing the Sergent Runtime.
///
/// It drives bounded Runs over one Recipe, its Scene Actions, and a model
/// client. @sergent/docs/framework.md
pub struct Sergent<R: SergentRecipe, A, M> {
    pub(super) recipe: R,
    pub(super) actions: A,
    pub(super) model_client: M,
    pub(super) no_target_error: sergent_rs_core::error::RunError,
    pub(super) intent_mode: IntentMode<R::IntentProposal>,
    #[allow(clippy::type_complexity)]
    pub(super) registry: Option<Arc<OperationRegistry<R::Scene, R::Intent, R::Target>>>,
}

impl<R, A, M> Sergent<R, A, M>
where
    R: SergentRecipe,
    A: SceneActions<Scene = R::Scene, Intent = R::Intent, Target = R::Target>,
    M: ModelClient,
{
    /// Capture configured Recipe facts, no-target failure, and Intent mode once.
    pub fn new(
        configured_recipe: ConfiguredRecipe<R>,
        actions: A,
        model_client: M,
    ) -> Result<Self, ConstructionError> {
        let (recipe, registry) = configured_recipe.into_parts();
        let no_target_error = recipe.no_target_error();
        let intent_mode = match recipe.passthrough_proposal() {
            Some(proposal) => IntentMode::PassThrough { proposal },
            None => {
                let schema = derive_proposal_schema::<R::IntentProposal>()?;
                IntentMode::ModelBacked {
                    schema: Arc::new(schema),
                }
            }
        };
        Ok(Self {
            recipe,
            actions,
            model_client,
            no_target_error,
            intent_mode,
            registry,
        })
    }

    /// Start a run with an owned concrete MindBuf and per-run settings, returning
    /// a handle for progress, cancellation, and the awaitable result.
    pub fn start<P>(
        self: Arc<Self>,
        source: SceneSource<R::Scene, P>,
        mindbuf: R::MindBuf,
        settings: RunSettings,
        observers: Vec<Box<dyn RunObserver<R::Scene>>>,
    ) -> crate::handle::RunHandle<R::Scene>
    where
        R: Send + Sync + 'static,
        A: Send + Sync + 'static,
        M: Send + Sync + 'static,
        R::Scene: Send + 'static,
        R::MindBuf: Send + Sync + 'static,
        R::IntentProposal: Send + Sync + 'static,
        R::Intent: Send + Sync + 'static,
        R::Target: Send + Sync + 'static,
        P: SceneRebase<R::Scene, R::Intent, R::Target> + 'static,
    {
        let progress = Arc::new(Mutex::new(ProgressSnapshot::initial(RunId::mint())));
        let cancel = CancelToken::new();
        let task = tokio::spawn({
            let progress = Arc::clone(&progress);
            let cancel = cancel.clone();
            async move {
                let slots: Vec<&dyn RunObserver<R::Scene>> =
                    observers.iter().map(|observer| observer.as_ref()).collect();
                let delivery = Delivery::new(&progress, &slots);
                self.run_inner(source, &mindbuf, settings, &cancel, delivery)
                    .await
            }
        });
        crate::handle::RunHandle::new(task, cancel, progress)
    }

    /// Run one bounded pipeline with a borrowed concrete MindBuf and per-run
    /// settings to a terminal `SergentResult`.
    pub async fn run<'a, P>(
        &'a self,
        source: SceneSource<R::Scene, P>,
        mindbuf: &'a R::MindBuf,
        settings: RunSettings,
        cancel: &'a CancelToken,
        observers: &'a [&'a dyn RunObserver<R::Scene>],
    ) -> SergentResult<R::Scene>
    where
        P: SceneRebase<R::Scene, R::Intent, R::Target>,
    {
        let progress = Arc::new(Mutex::new(ProgressSnapshot::initial(RunId::mint())));
        let delivery = Delivery::new(&progress, observers);
        self.run_inner(source, mindbuf, settings, cancel, delivery)
            .await
    }
}
