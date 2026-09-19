//! The recipe fakes: `ModelIntentRecipe` (model-backed Intent, with per-test
//! failure and cancellation switches) and `PassThroughRecipe` (no Intent call),
//! plus the `DocProposal` Intent-proposal type they decode.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};

use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::mindbuf::MindBuf;
use sergent_rs_core::model::{Message, ParsedJsonObject};
use sergent_rs_core::plan::{ExecutionPlan, Patch};
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::scene::SceneIdentity;

use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::intent_proposals::IntentProposalPassThrough;
use sergent_rs_runtime::sergent::ConfiguredRecipe;

use super::operations::append_registry;
use super::scene::{Doc, DocIntent, DocMind, IntentKind, Spot};

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocProposal {
    /// Whether the model wants to continue.
    pub go: bool,
}

pub fn intent_proposal_json() -> ParsedJsonObject {
    super::object(json!({ "go": true }))
}

/// A model-backed recipe whose behavior is fully configured for one test.
pub struct ModelIntentRecipe {
    pub intent_kind: IntentKind,
    pub fail_build_intent: bool,
    pub fail_build_plan: bool,
    pub fail_derive_intent: bool,
    pub fail_validate_intent: bool,
    pub fail_validate_plan: bool,
    pub trip_in_validate_intent: Option<CancelToken>,
    pub trip_in_validate_plan: Option<CancelToken>,
}

impl ModelIntentRecipe {
    pub fn new() -> Self {
        Self {
            intent_kind: IntentKind::Continue,
            fail_build_intent: false,
            fail_build_plan: false,
            fail_derive_intent: false,
            fail_validate_intent: false,
            fail_validate_plan: false,
            trip_in_validate_intent: None,
            trip_in_validate_plan: None,
        }
    }

    pub fn with_intent(mut self, kind: IntentKind) -> Self {
        self.intent_kind = kind;
        self
    }
}

impl Default for ModelIntentRecipe {
    fn default() -> Self {
        Self::new()
    }
}

/// Configure the common model-backed test Recipe with the append registry.
pub fn configured_model_intent(recipe: ModelIntentRecipe) -> ConfiguredRecipe<ModelIntentRecipe> {
    ConfiguredRecipe::plan_capable(recipe, append_registry(None))
}

impl SergentRecipe for ModelIntentRecipe {
    type Scene = Doc;
    type MindBuf = DocMind;
    type IntentProposal = DocProposal;
    type Intent = DocIntent;
    type Target = Spot;

    fn no_target_error(&self) -> RunError {
        RunError {
            kind: "no_target".to_owned(),
            message: "no target selected".to_owned(),
            metadata: Map::new(),
        }
    }

    fn build_intent_messages(
        &self,
        _scene: &Doc,
        mindbuf: &DocMind,
        _target: &Spot,
    ) -> Result<Vec<Message>, RunError> {
        if self.fail_build_intent {
            return Err(RunError::new(
                "recipe_request_error",
                "intent builder failed",
            ));
        }
        Ok(vec![
            Message::system("decide"),
            Message::user(mindbuf.export()),
        ])
    }

    fn derive_intent(
        &self,
        _scene: &Doc,
        _identity: &SceneIdentity,
        _target: &Spot,
        _proposal: &DocProposal,
    ) -> Result<DocIntent, RunError> {
        if self.fail_derive_intent {
            return Err(RunError::of(
                ErrorKind::ValidationError,
                "cannot derive intent",
            ));
        }
        Ok(DocIntent {
            kind: self.intent_kind,
        })
    }

    fn validate_intent(
        &self,
        _scene: &Doc,
        _identity: &SceneIdentity,
        _intent: &DocIntent,
    ) -> Result<(), RunError> {
        if let Some(token) = &self.trip_in_validate_intent {
            token.cancel();
        }
        if self.fail_validate_intent {
            return Err(RunError::of(ErrorKind::ValidationError, "intent rejected"));
        }
        Ok(())
    }

    fn build_plan_messages(
        &self,
        _scene: &Doc,
        _target: &Spot,
        _intent: &DocIntent,
        mindbuf: &DocMind,
    ) -> Result<Vec<Message>, RunError> {
        if self.fail_build_plan {
            return Err(RunError::new("recipe_request_error", "plan builder failed"));
        }
        Ok(vec![
            Message::system("plan"),
            Message::user(mindbuf.export()),
        ])
    }

    fn validate_plan(
        &self,
        _scene: &Doc,
        _identity: &SceneIdentity,
        _target: &Spot,
        _intent: &DocIntent,
        _plan: &sergent_rs_core::plan::ExecutionPlan<Doc, DocIntent, Spot>,
    ) -> Result<(), RunError> {
        if let Some(token) = &self.trip_in_validate_plan {
            token.cancel();
        }
        if self.fail_validate_plan {
            return Err(RunError::of(ErrorKind::ValidationError, "plan rejected"));
        }
        Ok(())
    }
}

/// A pass-through recipe: no Intent provider call; the sentinel is supplied per
/// run. Recipe-owned state selects semantics and records instrumentation;
/// `ConfiguredRecipe` selects registry capability externally.
pub struct PassThroughRecipe {
    pub intent_kind: IntentKind,
    pub plan_validations: Arc<AtomicUsize>,
    /// The ordered Operation ids of the Patch this recipe last compiled.
    pub compiled_op_ids: Arc<Mutex<Vec<String>>>,
    compile_patch: CompilePatch,
}

#[derive(Clone, Copy)]
enum CompilePatch {
    Default,
    Fail,
    Empty,
    WrongBase,
}

impl PassThroughRecipe {
    pub fn new() -> Self {
        Self {
            intent_kind: IntentKind::Continue,
            plan_validations: Arc::new(AtomicUsize::new(0)),
            compiled_op_ids: Arc::new(Mutex::new(Vec::new())),
            compile_patch: CompilePatch::Default,
        }
    }

    pub fn with_intent(mut self, kind: IntentKind) -> Self {
        self.intent_kind = kind;
        self
    }

    pub fn with_compile_error(mut self) -> Self {
        self.compile_patch = CompilePatch::Fail;
        self
    }

    pub fn with_empty_patch(mut self) -> Self {
        self.compile_patch = CompilePatch::Empty;
        self
    }

    /// Compile every Operation faithfully but bind the Patch to a Scene
    /// identity the run never observed.
    pub fn with_wrong_base_patch(mut self) -> Self {
        self.compile_patch = CompilePatch::WrongBase;
        self
    }
}

impl Default for PassThroughRecipe {
    fn default() -> Self {
        Self::new()
    }
}

/// Configure the common pass-through test Recipe with the append registry.
pub fn configured_pass_through(recipe: PassThroughRecipe) -> ConfiguredRecipe<PassThroughRecipe> {
    ConfiguredRecipe::plan_capable(recipe, append_registry(None))
}

impl SergentRecipe for PassThroughRecipe {
    type Scene = Doc;
    type MindBuf = DocMind;
    type IntentProposal = IntentProposalPassThrough;
    type Intent = DocIntent;
    type Target = Spot;

    fn no_target_error(&self) -> RunError {
        RunError {
            kind: "no_target".to_owned(),
            message: "no target selected".to_owned(),
            metadata: Map::new(),
        }
    }

    fn passthrough_proposal(&self) -> Option<IntentProposalPassThrough> {
        Some(IntentProposalPassThrough::default())
    }

    fn derive_intent(
        &self,
        _scene: &Doc,
        _identity: &SceneIdentity,
        _target: &Spot,
        _proposal: &IntentProposalPassThrough,
    ) -> Result<DocIntent, RunError> {
        Ok(DocIntent {
            kind: self.intent_kind,
        })
    }

    fn build_plan_messages(
        &self,
        _scene: &Doc,
        _target: &Spot,
        _intent: &DocIntent,
        mindbuf: &DocMind,
    ) -> Result<Vec<Message>, RunError> {
        Ok(vec![
            Message::system("plan"),
            Message::user(mindbuf.export()),
        ])
    }

    fn validate_plan(
        &self,
        _scene: &Doc,
        _identity: &SceneIdentity,
        _target: &Spot,
        _intent: &DocIntent,
        _plan: &sergent_rs_core::plan::ExecutionPlan<Doc, DocIntent, Spot>,
    ) -> Result<(), RunError> {
        self.plan_validations.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn compile_patch(
        &self,
        plan: &ExecutionPlan<Doc, DocIntent, Spot>,
    ) -> Result<Patch<Doc, DocIntent, Spot>, RunError> {
        let patch = match self.compile_patch {
            CompilePatch::Default => plan.compile_isolated_patch(),
            CompilePatch::Fail => {
                return Err(RunError::of(
                    ErrorKind::ValidationError,
                    "patch compilation rejected the plan",
                ));
            }
            CompilePatch::Empty => Patch::for_rebase(plan.base().clone(), Vec::new()),
            CompilePatch::WrongBase => Patch::for_rebase(
                SceneIdentity {
                    scene_id: plan.base().scene_id.clone(),
                    revision: plan.base().revision + 1,
                },
                plan.steps()
                    .iter()
                    .map(|step| step.isolated_copy())
                    .collect(),
            ),
        };
        *self.compiled_op_ids.lock().unwrap() = patch
            .steps()
            .iter()
            .map(|step| step.op_id().as_str().to_owned())
            .collect();
        Ok(patch)
    }
}
