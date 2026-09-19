//! The start-and-handle surface: a sync progress snapshot and done check while a
//! run is parked at a provider await, and an awaitable success result.

mod harness;

use std::num::NonZeroU32;
use std::sync::Arc;

use harness::*;
use sergent_rs_core::model::{ModelSettings, ThinkingEffort};
use sergent_rs_core::vocab::{ProgressStatus, Stage, TerminalStatus};
use sergent_rs_runtime::scene_state::SceneSource;
use sergent_rs_runtime::sergent::{RunSettings, Sergent};

#[tokio::test]
async fn a_handle_exposes_progress_and_done_while_parked_then_the_result() {
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let entered = client.entered();
    let gate = client.gate();
    let requests = Arc::clone(&client.inner.requests);
    let sergent = Arc::new(
        Sergent::new(
            configured_pass_through(PassThroughRecipe::new()),
            DocActions::new(),
            client,
        )
        .unwrap(),
    );
    let plan_settings = ModelSettings {
        thinking_effort: ThinkingEffort::Low,
        max_output_tokens: NonZeroU32::new(17).unwrap(),
        timeout_secs: NonZeroU32::new(23).unwrap(),
    };

    let handle = sergent.start(
        SceneSource::plain(doc("start ", 4)),
        DocMind,
        RunSettings::new("prov/model").with_plan(plan_settings),
        Vec::new(),
    );

    // The run is parked at the Plan call.
    entered.notified().await;
    let snapshot = handle.snapshot();
    assert_eq!(snapshot.stage, Stage::PlanCall);
    assert_eq!(snapshot.status, ProgressStatus::Running);
    assert!(snapshot.scene_id.is_some());
    assert_eq!(snapshot.revision, 4);
    assert!(!handle.done());
    assert_eq!(requests.lock().unwrap()[0].model_settings(), plan_settings);

    // Release the provider call and await the terminal result.
    gate.notify_one();
    let result = handle.result().await;
    assert_eq!(result.status(), TerminalStatus::Success);
    assert_eq!(result.scene().text, "start x");
}
