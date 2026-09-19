//! A plain Scene is isolated immediately, before any model await.

mod harness;

use std::sync::{Arc, Mutex};

use harness::{DocMind, ParkedClient, intent_proposal_json, plan_envelope, run_settings_for};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Map;
use sergent_rs_core::error::RunError;
use sergent_rs_core::ids::{SceneId, TargetId};
use sergent_rs_core::mindbuf::MindBuf;
use sergent_rs_core::model::Message;
use sergent_rs_core::operation::{Operation, OperationFault, PlanStep};
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::scene::{SceneActions, SceneIdentity, VerificationReport};
use sergent_rs_core::target::Target;
use sergent_rs_core::vocab::TerminalStatus;
use sergent_rs_runtime::intent_proposals::IntentProposalPassThrough;
use sergent_rs_runtime::intents::IntentContinue;
use sergent_rs_runtime::scene_state::{SceneSource, SceneState};
use sergent_rs_runtime::sergent::{ConfiguredRecipe, Sergent};

struct AliasedScene {
    scene_id: SceneId,
    revision: u64,
    text: Arc<Mutex<String>>,
}

#[derive(Serialize)]
struct AliasTarget {
    id: TargetId,
}

impl Target for AliasTarget {
    fn target_id(&self) -> &TargetId {
        &self.id
    }
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct AliasAppend {
    /// Text appended to the isolated document.
    text: String,
}

impl Operation for AliasAppend {
    type Scene = AliasedScene;
    type Intent = IntentContinue;
    type Target = AliasTarget;

    fn apply(
        &self,
        scene: &mut AliasedScene,
        _intent: &IntentContinue,
        _target: &AliasTarget,
    ) -> Result<(), OperationFault> {
        scene.text.lock().unwrap().push_str(&self.text);
        scene.revision += 1;
        Ok(())
    }
}

struct AliasActions;

impl SceneActions for AliasActions {
    type Scene = AliasedScene;
    type Intent = IntentContinue;
    type Target = AliasTarget;

    fn identity(&self, scene: &AliasedScene) -> SceneIdentity {
        SceneIdentity {
            scene_id: scene.scene_id.clone(),
            revision: scene.revision,
        }
    }

    fn clone_scene(&self, scene: &AliasedScene) -> AliasedScene {
        AliasedScene {
            scene_id: scene.scene_id.clone(),
            revision: scene.revision,
            text: Arc::new(Mutex::new(scene.text.lock().unwrap().clone())),
        }
    }

    fn select_target(&self, _scene: &AliasedScene) -> Option<AliasTarget> {
        Some(AliasTarget {
            id: TargetId::parse("text_00000000000000000000000000000000").unwrap(),
        })
    }

    fn has_target(&self, _scene: &AliasedScene, _target: &AliasTarget) -> bool {
        true
    }

    fn apply(
        &self,
        scene: &mut AliasedScene,
        intent: &IntentContinue,
        target: &AliasTarget,
        operations: &[PlanStep<AliasedScene, IntentContinue, AliasTarget>],
    ) -> Result<(), OperationFault> {
        for step in operations {
            step.operation().apply(scene, intent, target)?;
        }
        Ok(())
    }

    fn verify(
        &self,
        _before: &AliasedScene,
        _after: &AliasedScene,
        _target: &AliasTarget,
        _operations: &[PlanStep<AliasedScene, IntentContinue, AliasTarget>],
    ) -> VerificationReport {
        VerificationReport::accepted()
    }
}

struct AliasRecipe;

fn configured_alias_recipe() -> ConfiguredRecipe<AliasRecipe> {
    let registry = OperationRegistry::builder()
        .register::<AliasAppend>("append")
        .unwrap()
        .build(None)
        .unwrap();
    ConfiguredRecipe::plan_capable(AliasRecipe, registry)
}

impl SergentRecipe for AliasRecipe {
    type Scene = AliasedScene;
    type MindBuf = DocMind;
    type IntentProposal = IntentProposalPassThrough;
    type Intent = IntentContinue;
    type Target = AliasTarget;

    fn no_target_error(&self) -> RunError {
        RunError {
            kind: "no_target".to_owned(),
            message: "no text target".to_owned(),
            metadata: Map::new(),
        }
    }

    fn passthrough_proposal(&self) -> Option<IntentProposalPassThrough> {
        Some(IntentProposalPassThrough::default())
    }

    fn derive_intent(
        &self,
        _scene: &AliasedScene,
        _identity: &SceneIdentity,
        _target: &AliasTarget,
        _proposal: &IntentProposalPassThrough,
    ) -> Result<IntentContinue, RunError> {
        Ok(IntentContinue)
    }

    fn build_plan_messages(
        &self,
        _scene: &AliasedScene,
        _target: &AliasTarget,
        _intent: &IntentContinue,
        mindbuf: &DocMind,
    ) -> Result<Vec<Message>, RunError> {
        Ok(vec![Message::user(mindbuf.export())])
    }
}

#[tokio::test]
async fn caller_alias_cannot_change_a_parked_plain_run_snapshot() {
    let caller_alias = Arc::new(Mutex::new("base".to_owned()));
    let scene = AliasedScene {
        scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
        revision: 1,
        text: Arc::clone(&caller_alias),
    };
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&[" model"]));
    let entered = client.entered();
    let gate = client.gate();
    let sergent = Arc::new(Sergent::new(configured_alias_recipe(), AliasActions, client).unwrap());
    let handle = sergent.start(
        SceneSource::plain(scene),
        DocMind,
        run_settings_for("p/m"),
        Vec::new(),
    );
    entered.notified().await;

    *caller_alias.lock().unwrap() = "foreground".to_owned();
    gate.notify_one();
    let result = handle.result().await;

    assert_eq!(result.status(), TerminalStatus::Success);
    assert_eq!(&*caller_alias.lock().unwrap(), "foreground");
    assert_eq!(&*result.scene().text.lock().unwrap(), "base model");
    assert_eq!(result.run_record().scene().revision_before(), 1);
}

#[test]
fn plain_source_defaults_to_strict_policy_without_live_state() {
    let scene = AliasedScene {
        scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
        revision: 1,
        text: Arc::new(Mutex::new("base".to_owned())),
    };
    let source: SceneSource<AliasedScene> = SceneSource::plain(scene);
    assert!(matches!(source, SceneSource::Plain(_)));

    let live = SceneState::new(
        AliasedScene {
            scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
            revision: 1,
            text: Arc::new(Mutex::new("base".to_owned())),
        },
        SceneIdentity {
            scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
            revision: 1,
        },
    );
    let _: SceneState<AliasedScene> = live;
}
