//! Battery-only containment proof for one structured provider timeout.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sergent_rs::testing::{StaticLlmClient, StaticLlmOutcome};
use sergent_rs::{
    CancelToken, ConfiguredRecipe, ErrorKind, IntentContinue, IntentProposalPassThrough, Message,
    MindBuf, ModelCallRecord, Operation, OperationFault, OperationRegistry, PlanStep, RunError,
    RunObserver, RunSettings, RunStepName, SceneActions, SceneId, SceneIdentity, SceneSource,
    Sergent, SergentRecipe, SergentResult, Stage, Target, TargetId, TerminalStatus,
    VerificationReport,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct FailureScene {
    identity: SceneIdentity,
    target_id: TargetId,
    value: u32,
}

#[derive(Clone, Serialize)]
struct FailureTarget(TargetId);

impl Target for FailureTarget {
    fn target_id(&self) -> &TargetId {
        &self.0
    }
}

struct FailureMindBuf;

impl MindBuf for FailureMindBuf {
    fn export(&self) -> String {
        "timeout containment".to_owned()
    }
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct SetValue {
    /// Replacement value applied only after a successful model call.
    value: u32,
}

impl Operation for SetValue {
    type Scene = FailureScene;
    type Intent = IntentContinue;
    type Target = FailureTarget;

    fn apply(
        &self,
        scene: &mut FailureScene,
        _intent: &IntentContinue,
        _target: &FailureTarget,
    ) -> Result<(), OperationFault> {
        scene.value = self.value;
        scene.identity.revision += 1;
        Ok(())
    }
}

struct FailureActions;

impl SceneActions for FailureActions {
    type Scene = FailureScene;
    type Intent = IntentContinue;
    type Target = FailureTarget;

    fn identity(&self, scene: &FailureScene) -> SceneIdentity {
        scene.identity.clone()
    }

    sergent_rs::clone_scene_via_clone!();

    fn select_target(&self, scene: &FailureScene) -> Option<FailureTarget> {
        Some(FailureTarget(scene.target_id.clone()))
    }

    fn has_target(&self, scene: &FailureScene, target: &FailureTarget) -> bool {
        scene.target_id == target.0
    }

    fn apply(
        &self,
        scene: &mut FailureScene,
        intent: &IntentContinue,
        target: &FailureTarget,
        operations: &[PlanStep<FailureScene, IntentContinue, FailureTarget>],
    ) -> Result<(), OperationFault> {
        for step in operations {
            step.operation().apply(scene, intent, target)?;
        }
        Ok(())
    }

    fn verify(
        &self,
        _before: &FailureScene,
        _after: &FailureScene,
        _target: &FailureTarget,
        _operations: &[PlanStep<FailureScene, IntentContinue, FailureTarget>],
    ) -> VerificationReport {
        VerificationReport::accepted()
    }
}

struct FailureRecipe;

impl SergentRecipe for FailureRecipe {
    type Scene = FailureScene;
    type MindBuf = FailureMindBuf;
    type IntentProposal = IntentProposalPassThrough;
    type Intent = IntentContinue;
    type Target = FailureTarget;

    fn no_target_error(&self) -> RunError {
        RunError::of(ErrorKind::ValidationError, "failure target is absent")
    }

    fn passthrough_proposal(&self) -> Option<IntentProposalPassThrough> {
        Some(IntentProposalPassThrough::default())
    }

    fn derive_intent(
        &self,
        _scene: &FailureScene,
        _identity: &SceneIdentity,
        _target: &FailureTarget,
        _proposal: &IntentProposalPassThrough,
    ) -> Result<IntentContinue, RunError> {
        Ok(IntentContinue)
    }

    fn build_plan_messages(
        &self,
        _scene: &FailureScene,
        _target: &FailureTarget,
        _intent: &IntentContinue,
        mindbuf: &FailureMindBuf,
    ) -> Result<Vec<Message>, RunError> {
        Ok(vec![Message::user(mindbuf.export())])
    }
}

fn configured_recipe() -> ConfiguredRecipe<FailureRecipe> {
    let registry = OperationRegistry::builder()
        .register::<SetValue>("set_value")
        .unwrap()
        .build(Some(1))
        .unwrap();
    ConfiguredRecipe::plan_capable(FailureRecipe, registry)
}

fn scene() -> FailureScene {
    FailureScene {
        identity: SceneIdentity {
            scene_id: SceneId::mint("failure").unwrap(),
            revision: 4,
        },
        target_id: TargetId::mint("target").unwrap(),
        value: 7,
    }
}

fn plan_call(result: &SergentResult<FailureScene>) -> &ModelCallRecord {
    result
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::ExecutionPlan)
        .and_then(|step| step.model_call())
        .expect("failed Plan model call")
}

fn assert_timeout_evidence(result: &SergentResult<FailureScene>) {
    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::PlanCall);
    let error = result.error().expect("terminal timeout");
    assert_eq!(error.kind, ErrorKind::Timeout.as_str());
    assert_eq!(
        error.metadata,
        json!({ "retryable": true }).as_object().unwrap().clone()
    );
    assert!(result.run_record().scene().revision_after().is_none());

    let call = plan_call(result);
    assert_eq!(call.proposal_schema().name(), "PlanProposal");
    assert_eq!(call.model_name(), "openai/gpt");
    assert_eq!(call.identity().unwrap().provider, "openai");
    assert_eq!(call.identity().unwrap().model, "gpt");
    assert_eq!(
        call.payloads().request().value().unwrap()["model_name"],
        "openai/gpt"
    );
    assert!(call.payloads().raw_response().is_none());
    assert!(call.payloads().parsed_json().is_none());
    assert!(call.payloads().parsed_proposal().is_none());
    assert!(call.usage().is_none());
    assert_eq!(call.attempts().len(), 2);
    for attempt in call.attempts() {
        assert!(!attempt.is_success());
        assert_eq!(attempt.retryable(), Some(true));
        let error = attempt.error().expect("failed timeout attempt");
        assert_eq!(error.kind, ErrorKind::Timeout.as_str());
        assert_eq!(
            error.metadata,
            json!({ "retryable": true }).as_object().unwrap().clone()
        );
    }
}

#[tokio::test]
async fn timeout_is_exactly_contained_through_the_battery_surface() {
    let original = scene();
    let sergent = Sergent::new(
        configured_recipe(),
        FailureActions,
        StaticLlmClient::scripted([StaticLlmOutcome::Timeout]),
    )
    .unwrap();
    let cancel = CancelToken::new();
    let observers: [&dyn RunObserver<FailureScene>; 0] = [];

    let result = sergent
        .run(
            SceneSource::plain(original.clone()),
            &FailureMindBuf,
            RunSettings::new("openai/gpt"),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.scene(), &original);
    assert_timeout_evidence(&result);
}
