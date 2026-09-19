//! Consumer-side proof that one battery-only run preserves per-run model facts.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sergent_rs::testing::StaticLlmClient;
use sergent_rs::{
    CancelToken, ConfiguredRecipe, ErrorKind, IntentContinue, Message, MindBuf, ModelCallRecord,
    ModelClient, ModelError, ModelRequest, ModelResponse, ModelSettings, Operation, OperationFault,
    OperationRegistry, ParsedJsonObject, PlanStep, ProgressSnapshot, RunError, RunObserver,
    RunRecord, RunSettings, RunStepName, SceneActions, SceneId, SceneIdentity, SceneSource,
    Sergent, SergentRecipe, SergentResult, Stage, Target, TargetId, TerminalStatus, ThinkingEffort,
    VerificationReport,
};
use tokio::sync::Notify;

#[derive(Clone)]
struct SurfaceScene {
    identity: SceneIdentity,
    target_id: TargetId,
    text: String,
}

#[derive(Clone, Serialize)]
struct SurfaceTarget(TargetId);

impl Target for SurfaceTarget {
    fn target_id(&self) -> &TargetId {
        &self.0
    }
}

struct SurfaceMindBuf;

impl MindBuf for SurfaceMindBuf {
    fn export(&self) -> String {
        "battery surface".to_owned()
    }
}

#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct SurfaceIntentProposal {
    /// Whether this test run should continue to its Plan call.
    continue_run: bool,
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct AppendText {
    /// Text appended by the deterministic Operation.
    text: String,
}

impl Operation for AppendText {
    type Scene = SurfaceScene;
    type Intent = IntentContinue;
    type Target = SurfaceTarget;

    fn apply(
        &self,
        scene: &mut SurfaceScene,
        _intent: &IntentContinue,
        _target: &SurfaceTarget,
    ) -> Result<(), OperationFault> {
        scene.text.push_str(&self.text);
        scene.identity.revision += 1;
        Ok(())
    }
}

struct SurfaceActions;

impl SceneActions for SurfaceActions {
    type Scene = SurfaceScene;
    type Intent = IntentContinue;
    type Target = SurfaceTarget;

    fn identity(&self, scene: &SurfaceScene) -> SceneIdentity {
        scene.identity.clone()
    }

    sergent_rs::clone_scene_via_clone!();

    fn select_target(&self, scene: &SurfaceScene) -> Option<SurfaceTarget> {
        Some(SurfaceTarget(scene.target_id.clone()))
    }

    fn has_target(&self, scene: &SurfaceScene, target: &SurfaceTarget) -> bool {
        scene.target_id == target.0
    }

    fn apply(
        &self,
        scene: &mut SurfaceScene,
        intent: &IntentContinue,
        target: &SurfaceTarget,
        operations: &[PlanStep<SurfaceScene, IntentContinue, SurfaceTarget>],
    ) -> Result<(), OperationFault> {
        for step in operations {
            step.operation().apply(scene, intent, target)?;
        }
        Ok(())
    }

    fn verify(
        &self,
        _before: &SurfaceScene,
        _after: &SurfaceScene,
        _target: &SurfaceTarget,
        _operations: &[PlanStep<SurfaceScene, IntentContinue, SurfaceTarget>],
    ) -> VerificationReport {
        VerificationReport::accepted()
    }
}

struct SurfaceRecipe;

fn configured_surface_recipe() -> ConfiguredRecipe<SurfaceRecipe> {
    let registry = OperationRegistry::builder()
        .register::<AppendText>("append_text")
        .expect("register Operation")
        .build(Some(1))
        .expect("build registry");
    ConfiguredRecipe::plan_capable(SurfaceRecipe, registry)
}

impl SergentRecipe for SurfaceRecipe {
    type Scene = SurfaceScene;
    type MindBuf = SurfaceMindBuf;
    type IntentProposal = SurfaceIntentProposal;
    type Intent = IntentContinue;
    type Target = SurfaceTarget;

    fn no_target_error(&self) -> RunError {
        RunError::of(ErrorKind::ValidationError, "surface target is absent")
    }

    fn build_intent_messages(
        &self,
        _scene: &SurfaceScene,
        mindbuf: &SurfaceMindBuf,
        _target: &SurfaceTarget,
    ) -> Result<Vec<Message>, RunError> {
        Ok(vec![Message::user(mindbuf.export())])
    }

    fn derive_intent(
        &self,
        _scene: &SurfaceScene,
        _identity: &SceneIdentity,
        _target: &SurfaceTarget,
        proposal: &SurfaceIntentProposal,
    ) -> Result<IntentContinue, RunError> {
        proposal
            .continue_run
            .then_some(IntentContinue)
            .ok_or_else(|| RunError::of(ErrorKind::ValidationError, "Intent stopped"))
    }

    fn build_plan_messages(
        &self,
        _scene: &SurfaceScene,
        _target: &SurfaceTarget,
        _intent: &IntentContinue,
        mindbuf: &SurfaceMindBuf,
    ) -> Result<Vec<Message>, RunError> {
        Ok(vec![Message::user(mindbuf.export())])
    }
}

#[test]
fn both_recipe_capability_shapes_assemble_through_the_battery() {
    let intent_only = ConfiguredRecipe::intent_only(SurfaceRecipe);
    Sergent::new(
        intent_only,
        SurfaceActions,
        StaticLlmClient::new(Vec::<String>::new()),
    )
    .expect("assemble Intent-only Sergent");

    Sergent::new(
        configured_surface_recipe(),
        SurfaceActions,
        StaticLlmClient::new(Vec::<String>::new()),
    )
    .expect("assemble Plan-capable Sergent");
}

#[derive(Clone)]
struct SharedStaticClient(Arc<StaticLlmClient>);

impl SharedStaticClient {
    fn new(outputs: impl IntoIterator<Item = String>) -> Self {
        Self(Arc::new(StaticLlmClient::new(outputs)))
    }

    fn requests(&self) -> Vec<ModelRequest> {
        self.0.recorded_requests()
    }
}

impl ModelClient for SharedStaticClient {
    async fn invoke(
        &self,
        request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        self.0.invoke(request).await
    }
}

#[derive(Clone)]
struct ParkedClient {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl ParkedClient {
    fn new() -> Self {
        Self {
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        }
    }
}

impl ModelClient for ParkedClient {
    async fn invoke(
        &self,
        _request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        self.entered.notify_one();
        self.release.notified().await;
        panic!("parked test client must be cancelled before release")
    }
}

#[derive(Clone, Default)]
struct SurfaceObserver {
    progress: Arc<Mutex<Vec<ProgressSnapshot>>>,
    finished: Arc<Mutex<Vec<FinishedObservation>>>,
}

#[derive(Debug, PartialEq, Eq)]
struct FinishedObservation {
    status: TerminalStatus,
    stage: Stage,
    text: String,
    observer_error_count: usize,
}

impl RunObserver<SurfaceScene> for SurfaceObserver {
    fn on_progress(&self, progress: &ProgressSnapshot) -> Result<(), RunError> {
        self.progress.lock().unwrap().push(progress.clone());
        Ok(())
    }

    fn on_finished(&self, result: &SergentResult<SurfaceScene>) -> Result<(), RunError> {
        self.finished.lock().unwrap().push(FinishedObservation {
            status: result.status(),
            stage: result.stage(),
            text: result.scene().text.clone(),
            observer_error_count: result.observer_errors().len(),
        });
        Ok(())
    }
}

fn settings(effort: ThinkingEffort, output: u32, timeout: u32) -> ModelSettings {
    ModelSettings {
        thinking_effort: effort,
        max_output_tokens: NonZeroU32::new(output).expect("non-zero output"),
        timeout_secs: NonZeroU32::new(timeout).expect("non-zero timeout"),
    }
}

fn model_call(record: &RunRecord, step: RunStepName) -> &ModelCallRecord {
    record
        .steps()
        .iter()
        .find(|record| record.name() == step)
        .and_then(|record| record.model_call())
        .expect("reached model call")
}

fn captured_request(call: &ModelCallRecord) -> &serde_json::Value {
    call.payloads()
        .request()
        .value()
        .expect("the battery must retain the request projection")
}

#[tokio::test]
async fn configured_recipe_and_run_settings_cross_the_battery_surface() {
    let client = SharedStaticClient::new([
        r#"{"continue_run":true}"#.to_owned(),
        r#"{"operations":[{"call":"append_text","text":"!"}]}"#.to_owned(),
    ]);
    let probe = client.clone();
    let sergent = Sergent::new(configured_surface_recipe(), SurfaceActions, client).unwrap();
    let intent_settings = settings(ThinkingEffort::Low, 17, 18);
    let plan_settings = settings(ThinkingEffort::Medium, 27, 28);
    let model_name = "openai/ Mixed Model ";
    let scene = SurfaceScene {
        identity: SceneIdentity {
            scene_id: SceneId::mint("surface").unwrap(),
            revision: 0,
        },
        target_id: TargetId::mint("target").unwrap(),
        text: "start".to_owned(),
    };
    let cancel = CancelToken::new();
    let observers: [&dyn RunObserver<SurfaceScene>; 0] = [];

    let result = sergent
        .run(
            SceneSource::plain(scene),
            &SurfaceMindBuf,
            RunSettings::new(model_name)
                .with_intent(intent_settings)
                .with_plan(plan_settings),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Success);
    assert_eq!(result.scene().text, "start!");
    let requests = probe.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].model_name(), model_name);
    assert_eq!(requests[0].model_settings(), intent_settings);
    assert_eq!(requests[1].model_name(), model_name);
    assert_eq!(requests[1].model_settings(), plan_settings);
    let record = result.run_record();
    assert_eq!(record.model_name(), model_name);
    let intent_call = model_call(record, RunStepName::Intent);
    let plan_call = model_call(record, RunStepName::ExecutionPlan);
    assert_eq!(captured_request(intent_call)["model_name"], model_name);
    assert_eq!(
        captured_request(intent_call)["model_settings"],
        serde_json::to_value(intent_settings).unwrap()
    );
    assert_eq!(captured_request(plan_call)["model_name"], model_name);
    assert_eq!(
        captured_request(plan_call)["model_settings"],
        serde_json::to_value(plan_settings).unwrap()
    );
}

#[tokio::test]
async fn background_progress_cancellation_and_observation_compose_through_the_battery() {
    let client = ParkedClient::new();
    let entered = Arc::clone(&client.entered);
    let sergent =
        Arc::new(Sergent::new(configured_surface_recipe(), SurfaceActions, client).unwrap());
    let scene_id = SceneId::mint("surface").unwrap();
    let scene = SurfaceScene {
        identity: SceneIdentity {
            scene_id: scene_id.clone(),
            revision: 4,
        },
        target_id: TargetId::mint("target").unwrap(),
        text: "unchanged".to_owned(),
    };
    let observer = SurfaceObserver::default();
    let observed = observer.clone();

    let handle: sergent_rs::RunHandle<SurfaceScene> = sergent.start(
        SceneSource::plain(scene),
        SurfaceMindBuf,
        RunSettings::new("provider/model"),
        vec![Box::new(observer)],
    );
    entered.notified().await;

    let snapshot: ProgressSnapshot = handle.snapshot();
    assert_eq!(snapshot.stage, Stage::IntentCall);
    assert_eq!(snapshot.scene_id.as_ref(), Some(&scene_id));
    assert_eq!(snapshot.revision, 4);

    handle.cancel();
    let result: SergentResult<SurfaceScene> = handle.result().await;
    assert_eq!(result.status(), TerminalStatus::Cancelled);
    assert_eq!(result.stage(), Stage::IntentCall);
    assert_eq!(result.scene().text, "unchanged");

    let progress = observed.progress.lock().unwrap();
    assert_eq!(progress.first().unwrap().stage, Stage::Queued);
    assert_eq!(progress.first().unwrap().scene_id, None);
    assert_eq!(progress.first().unwrap().revision, 0);
    assert_eq!(
        progress.last().unwrap().status,
        sergent_rs::ProgressStatus::Cancelled
    );
    assert_eq!(
        observed.finished.lock().unwrap().as_slice(),
        &[FinishedObservation {
            status: TerminalStatus::Cancelled,
            stage: Stage::IntentCall,
            text: "unchanged".to_owned(),
            observer_error_count: 0,
        }]
    );
}
