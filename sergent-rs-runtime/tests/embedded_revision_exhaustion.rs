//! Shared Scene authority preserves stale precedence and rejects terminal
//! revision before invoking application edit, apply, or stale-rebase callbacks.

mod harness;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use harness::*;
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::vocab::{Stage, TerminalStatus};
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::{SceneEditError, SceneSource, SceneState};
use sergent_rs_runtime::sergent::Sergent;

fn no_observers<'a>() -> [&'a dyn RunObserver<Doc>; 0] {
    []
}

#[tokio::test]
async fn strict_stale_commit_at_terminal_revision_reports_stale_before_exhaustion() {
    let scene = doc("base ", 3);
    let state = SceneState::new(
        scene.clone(),
        SceneIdentity {
            scene_id: scene.scene_id.clone(),
            revision: u64::MAX - 1,
        },
    );
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&["agent"]));
    let gate = client.gate();
    let entered = client.entered();
    let actions = DocActions::new();
    let apply_calls = Arc::clone(&actions.apply_calls);
    let sergent = Arc::new(
        Sergent::new(
            configured_pass_through(PassThroughRecipe::new()),
            actions,
            client,
        )
        .unwrap(),
    );
    let handle = sergent.start(
        SceneSource::Live(state.clone()),
        DocMind,
        run_settings_for("p/m"),
        Vec::new(),
    );
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

    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::Commit);
    assert_eq!(result.error().unwrap().kind, "stale_patch");
    assert_eq!(
        result.error().unwrap().metadata["base_revision"],
        u64::MAX - 1
    );
    assert_eq!(
        result.error().unwrap().metadata["current_revision"],
        u64::MAX
    );
    assert_eq!(
        apply_calls.load(Ordering::SeqCst),
        1,
        "strict stale selection must not attempt another dry-run"
    );
    let (authoritative, identity) = state.snapshot(&DocActions::new());
    assert_eq!(authoritative.text, "base human");
    assert_eq!(identity.revision, u64::MAX);
    assert_eq!(result.scene().text, "base ");
    assert_no_commit_revision(result.run_record());
}

#[test]
fn embedded_application_edit_reports_exhaustion_before_invoking_edit() {
    let actions = DocActions::new();
    let scene = doc("base", u64::MAX);
    let state = SceneState::embedded(&actions, scene);
    let invoked = AtomicBool::new(false);

    let result = state.try_edit(&actions, u64::MAX, |current| {
        invoked.store(true, Ordering::SeqCst);
        Ok::<_, std::convert::Infallible>(current.clone())
    });

    let Err(SceneEditError::RevisionExhausted(error)) = result else {
        panic!("embedded edit should report revision exhaustion");
    };
    assert_eq!(error.kind, "revision_exhausted");
    assert_eq!(error.metadata["revision"], u64::MAX);
    assert!(!invoked.load(Ordering::SeqCst));
    let (snapshot, identity) = state.snapshot(&actions);
    assert_eq!(snapshot.text, "base");
    assert_eq!(snapshot.revision, u64::MAX);
    assert_eq!(identity.revision, u64::MAX);
}

#[tokio::test]
async fn embedded_exact_live_commit_reports_exhaustion_before_apply() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let actions = DocActions::new();
    let apply_calls = Arc::clone(&actions.apply_calls);
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        actions,
        client,
    )
    .unwrap();
    let scene = doc("base ", u64::MAX);
    let state = SceneState::embedded(&DocActions::new(), scene.clone());
    let cancel = sergent_rs_runtime::cancel::CancelToken::new();
    let observers = no_observers();

    let result = sergent
        .run(
            SceneSource::Live(state.clone()),
            &DocMind,
            run_settings_for("p/m"),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::DryRun);
    assert_eq!(result.error().unwrap().kind, "revision_exhausted");
    assert_eq!(result.error().unwrap().metadata["revision"], u64::MAX);
    assert_eq!(apply_calls.load(Ordering::SeqCst), 0);
    let (authoritative, identity) = state.snapshot(&DocActions::new());
    assert_eq!(authoritative.text, "base ");
    assert_eq!(authoritative.revision, u64::MAX);
    assert_eq!(identity.revision, u64::MAX);
    assert_no_commit_revision(result.run_record());
}

#[tokio::test]
async fn embedded_stale_commit_reports_exhaustion_before_rebase_or_rebased_apply() {
    let scene = doc("base ", u64::MAX - 1);
    let rebaser = TestRebaser::new(RebaseMode::AcceptSameOps);
    let rebase_calls = Arc::clone(&rebaser.calls);
    let state = SceneState::embedded_rebasing(&DocActions::new(), scene.clone(), rebaser);
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&["agent"]));
    let gate = client.gate();
    let entered = client.entered();
    let actions = DocActions::new();
    let apply_calls = Arc::clone(&actions.apply_calls);
    let sergent = Arc::new(
        Sergent::new(
            configured_pass_through(PassThroughRecipe::new()),
            actions,
            client,
        )
        .unwrap(),
    );
    let handle = sergent.start(
        SceneSource::Live(state.clone()),
        DocMind,
        run_settings_for("p/m"),
        Vec::new(),
    );
    entered.notified().await;

    state
        .try_edit(&DocActions::new(), u64::MAX - 1, |current| {
            let mut edited = current.clone();
            edited.text.push_str("human");
            edited.revision = u64::MAX;
            Ok::<_, std::convert::Infallible>(edited)
        })
        .unwrap();
    gate.notify_one();
    let result = handle.result().await;

    assert_eq!(result.status(), TerminalStatus::Failure);
    assert_eq!(result.stage(), Stage::Commit);
    assert_eq!(result.error().unwrap().kind, "revision_exhausted");
    assert_eq!(result.error().unwrap().metadata["revision"], u64::MAX);
    assert_eq!(
        apply_calls.load(Ordering::SeqCst),
        1,
        "only the original dry-run may apply before live authority is rechecked"
    );
    assert_eq!(rebase_calls.load(Ordering::SeqCst), 0);
    let (authoritative, identity) = state.snapshot(&DocActions::new());
    assert_eq!(authoritative.text, "base human");
    assert_eq!(authoritative.revision, u64::MAX);
    assert_eq!(identity.revision, u64::MAX);
    assert_no_commit_revision(result.run_record());
}
