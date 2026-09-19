//! Domain-neutral consumer-side proof of the battery's owner-spanning surface.

use sergent_rs::testing::StaticLlmClient;
use sergent_rs::{
    CancelToken, CancellationCheckpoint, CapturedValue, ConfiguredRecipe, ConstructionError,
    IntentContinue, IntentProposalPassThrough, JsonlRunRecordWriter, LlmClient, ModelCallPayloads,
    ModelClient, ModelSettings, OutputTokenTotal, PatchSummary, ProgressSnapshot, ProviderConfig,
    RunHandle, RunObserver, RunRecordApplicationName, RunRecordCorrelation, RunRecordEvent,
    RunRecordEventValue, RunRecordFileError, RunRecordFileId, RunSettings, SceneActions, Sergent,
    SergentRecipe, SergentResult,
};

struct SurfaceObserver;

impl RunObserver<()> for SurfaceObserver {}

#[allow(dead_code)]
fn assemble<R, A, M>(
    recipe: ConfiguredRecipe<R>,
    actions: A,
    model_client: M,
) -> Result<Sergent<R, A, M>, ConstructionError>
where
    R: SergentRecipe,
    A: SceneActions<Scene = R::Scene, Intent = R::Intent, Target = R::Target>,
    M: ModelClient,
{
    Sergent::new(recipe, actions, model_client)
}

#[test]
fn representative_application_facing_exports_are_constructible() {
    let _: ModelSettings = ModelSettings::default();
    let _: CapturedValue = CapturedValue::capture(&"surface");
    let _: CancellationCheckpoint = CancellationCheckpoint::BeforeIntent;
    let _: OutputTokenTotal = OutputTokenTotal::Complete(0);
    let _: Option<ModelCallPayloads> = None;
    let _: Option<PatchSummary> = None;
    let _: RunSettings = RunSettings::new("provider/model");
    let _: CancelToken = CancelToken::new();
    let _: Option<RunHandle<()>> = None;
    let _: Option<ProgressSnapshot> = None;
    let _: Option<SergentResult<()>> = None;
    let _: &dyn RunObserver<()> = &SurfaceObserver;
    let correlation = RunRecordCorrelation::default();
    let _: RunRecordEvent = RunRecordEvent::new(
        "surface.event",
        Some(RunRecordEventValue::json(serde_json::json!({}))),
        correlation,
    )
    .unwrap();
    let _: RunRecordApplicationName = RunRecordApplicationName::parse("surface").unwrap();
    let _: Option<RunRecordFileError> = None;
    let _: IntentContinue = IntentContinue;
    let _: IntentProposalPassThrough = IntentProposalPassThrough::default();
    let _: LlmClient = LlmClient::new();
    let _: LlmClient = LlmClient::configured(ProviderConfig::default());
    let _: StaticLlmClient = StaticLlmClient::new(Vec::<String>::new());
}

#[test]
fn jsonl_creation_direct_event_and_observer_wiring_compose_through_the_battery() {
    let identity = format!(
        "surface_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let (log, path) = JsonlRunRecordWriter::create(
        std::env::temp_dir(),
        RunRecordApplicationName::parse("surface").unwrap(),
        Some(RunRecordFileId::parse(identity).unwrap()),
    )
    .unwrap();
    let _: &dyn RunObserver<()> = &log;
    log.record_app(
        RunRecordEvent::new(
            "surface.created",
            Some(RunRecordEventValue::json(
                serde_json::json!({ "ready": true }),
            )),
            RunRecordCorrelation::default(),
        )
        .unwrap(),
    )
    .unwrap();
    log.close().unwrap();
    assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::remove_file(path).unwrap();
}
