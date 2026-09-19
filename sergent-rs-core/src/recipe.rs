//! Application policy for the Intent, Plan, and Patch stages, with mechanical
//! framework defaults. @sergent/docs/framework.md

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::RunError;
use crate::intent::Intent;
use crate::mindbuf::MindBuf;
use crate::model::Message;
use crate::plan::{ExecutionPlan, Patch, PlanProposal};
use crate::scene::SceneIdentity;
use crate::target::Target;

/// Application-owned policy around the Intent and Plan stages. The semantic
/// hooks (Intent derivation and validation, Plan validation, message building)
/// are application owned; ExecutionPlan derivation and Patch compilation are
/// mechanical defaults. @sergent/docs/framework.md
///
/// Registry capability is configured beside the recipe at runtime
/// construction. A pass-through Intent recipe returns a
/// locally-constructed `passthrough_proposal` and makes no Intent call; a
/// model-backed recipe returns `None` there and implements
/// `build_intent_messages`. MindBuf observation is statically associated;
/// caller-owned request facts never enter Recipe authority.
pub trait SergentRecipe {
    /// The application Scene type.
    type Scene;
    /// The concrete per-run MindBuf observation type.
    type MindBuf: MindBuf;
    /// The exact typed Intent proposal received at the model-output crossing.
    /// It must follow the trusted derive-only binding profile documented in
    /// the crate knowledge base.
    type IntentProposal: DeserializeOwned + JsonSchema + Serialize;
    /// The validated application Intent type.
    type Intent: Intent;
    /// The selected Target type.
    type Target: Target;

    /// The single error fact for a run that selects no Target.
    fn no_target_error(&self) -> RunError;

    /// The deterministic pass-through Intent proposal, when this recipe derives
    /// Intent without a provider call. @sergent/docs/framework.md
    fn passthrough_proposal(&self) -> Option<Self::IntentProposal> {
        None
    }

    /// Build the application-authored messages for a model-backed Intent phase.
    /// @sergent/docs/framework.md
    fn build_intent_messages(
        &self,
        scene: &Self::Scene,
        mindbuf: &Self::MindBuf,
        target: &Self::Target,
    ) -> Result<Vec<Message>, RunError> {
        let _ = (scene, mindbuf, target);
        Err(RunError::new(
            "recipe_contract_error",
            "model-backed Intent recipe must implement build_intent_messages",
        ))
    }

    /// Derive application Intent from a typed proposal and bounded context.
    /// @sergent/docs/framework.md
    fn derive_intent(
        &self,
        scene: &Self::Scene,
        identity: &SceneIdentity,
        target: &Self::Target,
        proposal: &Self::IntentProposal,
    ) -> Result<Self::Intent, RunError>;

    /// Reject Intent that violates application semantic rules; empty by
    /// default. @sergent/docs/framework.md
    fn validate_intent(
        &self,
        scene: &Self::Scene,
        identity: &SceneIdentity,
        intent: &Self::Intent,
    ) -> Result<(), RunError> {
        let _ = (scene, identity, intent);
        Ok(())
    }

    /// Build the application-authored messages for a continuing Intent's Plan.
    /// @sergent/docs/framework.md
    fn build_plan_messages(
        &self,
        scene: &Self::Scene,
        target: &Self::Target,
        intent: &Self::Intent,
        mindbuf: &Self::MindBuf,
    ) -> Result<Vec<Message>, RunError> {
        let _ = (scene, target, intent, mindbuf);
        Err(RunError::new(
            "recipe_contract_error",
            "continue-flow recipe must implement build_plan_messages",
        ))
    }

    /// Derive an ExecutionPlan from the decoded Plan proposal; the default
    /// binds the decoded steps to the observed identity.
    /// @sergent/docs/framework.md
    // The three app-relevant type projections are the honest signature; the
    // runtime crate aliases them locally.
    #[allow(clippy::type_complexity)]
    fn derive_plan(
        &self,
        scene: &Self::Scene,
        identity: &SceneIdentity,
        target: &Self::Target,
        intent: &Self::Intent,
        proposal: PlanProposal<Self::Scene, Self::Intent, Self::Target>,
    ) -> Result<ExecutionPlan<Self::Scene, Self::Intent, Self::Target>, RunError> {
        let _ = (scene, target, intent);
        Ok(proposal.bind_to_scene(identity.clone()))
    }

    /// Reject an ExecutionPlan that violates ordered or whole-plan legality;
    /// empty by default. @sergent/docs/framework.md
    fn validate_plan(
        &self,
        scene: &Self::Scene,
        identity: &SceneIdentity,
        target: &Self::Target,
        intent: &Self::Intent,
        plan: &ExecutionPlan<Self::Scene, Self::Intent, Self::Target>,
    ) -> Result<(), RunError> {
        let _ = (scene, identity, target, intent, plan);
        Ok(())
    }

    /// Compile the validated ExecutionPlan into a Patch of isolated copies that
    /// preserve operation ids. @sergent/docs/framework.md
    #[allow(clippy::type_complexity)]
    fn compile_patch(
        &self,
        plan: &ExecutionPlan<Self::Scene, Self::Intent, Self::Target>,
    ) -> Result<Patch<Self::Scene, Self::Intent, Self::Target>, RunError> {
        Ok(plan.compile_isolated_patch())
    }
}
