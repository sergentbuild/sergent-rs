//! Checked revision-exhaustion failures for exact and rebased live commit.

mod harness;

use std::sync::Arc;

use harness::*;
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::vocab::{Stage, TerminalStatus};
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::{SceneSource, SceneState};
use sergent_rs_runtime::sergent::Sergent;

#[tokio::test]
async fn exact_live_commit_reports_revision_exhaustion_without_mutation() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let scene = doc("base ", 3);
    let state = SceneState::new(
        scene.clone(),
        SceneIdentity {
            scene_id: scene.scene_id.clone(),
            revision: u64::MAX,
        },
    );
    let cancel = sergent_rs_runtime::cancel::CancelToken::new();
    let observers: [&dyn RunObserver<Doc>; 0] = [];

    let result = sergent
        .run(
            SceneSource::Live(state.clone()),
            &DocMind,
            run_settings_for("p/m"),
            &cancel,
            &observers,
        )
        .await;

    assert_exhausted(&result, &state, "base ");
}

#[tokio::test]
async fn rebased_live_commit_reports_revision_exhaustion_without_mutation() {
    let scene = doc("base ", 3);
    let state = SceneState::rebasing(
        scene.clone(),
        SceneIdentity {
            scene_id: scene.scene_id.clone(),
            revision: u64::MAX - 1,
        },
        TestRebaser::new(RebaseMode::AcceptSameOps),
    );
    let (handle, gate, entered) = spawn_live(&state, "agent");
    entered.notified().await;

    state
        .try_edit(&DocActions::new(), u64::MAX - 1, |current| {
            let mut edited = current.clone();
            edited.text.push_str("human");
            Ok::<_, std::convert::Infallible>(edited)
        })
        .unwrap();
    gate.notify_one();
    let result = handle.result().await;

    assert_exhausted(&result, &state, "base human");
}

fn assert_exhausted<P>(
    result: &sergent_rs_core::run_record::SergentResult<Doc>,
    state: &SceneState<Doc, P>,
    expected_text: &str,
) {
    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::Commit);
    assert_eq!(result.error().unwrap().kind, "revision_exhausted");
    assert_eq!(result.error().unwrap().metadata["revision"], u64::MAX);
    let (authoritative, identity) = state.snapshot(&DocActions::new());
    assert_eq!(authoritative.text, expected_text);
    assert_eq!(authoritative.revision, 3);
    assert_eq!(identity.revision, u64::MAX);
    assert_no_commit_revision(result.run_record());
}

type LiveHandle = sergent_rs_runtime::handle::RunHandle<Doc>;
type Gate = Arc<tokio::sync::Notify>;

fn spawn_live(state: &SceneState<Doc, TestRebaser>, text: &str) -> (LiveHandle, Gate, Gate) {
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&[text]));
    let gate = client.gate();
    let entered = client.entered();
    let recipe = configured_pass_through(PassThroughRecipe::new());
    let sergent = Arc::new(Sergent::new(recipe, DocActions::new(), client).unwrap());
    let handle = sergent.start(
        SceneSource::Live(state.clone()),
        DocMind,
        run_settings_for("p/m"),
        Vec::new(),
    );
    (handle, gate, entered)
}
