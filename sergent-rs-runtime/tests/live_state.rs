//! Scene authority: plain snapshot vs shared live state, strict stale rejection,
//! scene-owned rebase accept and conflict, and embedded identity enforcement.
//! Concurrent runs share one SceneState and interleave on parked notify gates,
//! proving the lock is never held across a model await.

mod harness;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use harness::*;
use serde_json::json;
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::vocab::{RunStepName, Stage, TerminalStatus};
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::{SceneEditError, SceneSource, SceneState};
use sergent_rs_runtime::sergent::Sergent;

fn no_observers<'a>() -> [&'a dyn RunObserver<Doc>; 0] {
    []
}

fn assert_commit_output(
    result: &sergent_rs_core::run_record::SergentResult<Doc>,
    expected_kind: &str,
    expected_metadata: serde_json::Value,
) {
    let commit = result
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Commit)
        .unwrap()
        .output()
        .unwrap()
        .value()
        .unwrap();
    assert_eq!(commit["commit_kind"], expected_kind);
    assert_eq!(commit["metadata"], expected_metadata);
}

#[test]
fn application_edit_and_snapshot_share_one_atomic_revision() {
    let actions = DocActions::new();
    let scene = doc("base", 3);
    let state = SceneState::new(scene.clone(), scene_identity(&scene));

    let edited = state
        .try_edit(&actions, 3, |current| {
            let mut next = current.clone();
            next.text.push_str(" human");
            next.revision += 1;
            Ok::<_, std::convert::Infallible>(next)
        })
        .unwrap();
    let (snapshot, identity) = state.snapshot(&actions);

    assert_eq!(edited.text, "base human");
    assert_eq!(snapshot.text, "base human");
    assert_eq!(snapshot.revision, 4);
    assert_eq!(identity.revision, 4);
}

#[test]
fn ordinary_clones_share_one_live_scene_authority() {
    let actions = DocActions::new();
    let scene = doc("base", 3);
    let state = SceneState::new(scene.clone(), scene_identity(&scene));
    let clone = state.clone();

    clone
        .try_edit(&actions, 3, |current| {
            let mut next = current.clone();
            next.text.push_str(" shared");
            Ok::<_, std::convert::Infallible>(next)
        })
        .unwrap();

    let (snapshot, identity) = state.snapshot(&actions);
    assert_eq!(snapshot.text, "base shared");
    assert_eq!(identity.revision, 4);
}

#[test]
fn stale_application_edit_does_not_invoke_the_edit_function() {
    let actions = DocActions::new();
    let scene = doc("base", 3);
    let state = SceneState::new(scene.clone(), scene_identity(&scene));
    let invoked = AtomicBool::new(false);

    let result = state.try_edit(&actions, 2, |current| {
        invoked.store(true, Ordering::SeqCst);
        Ok::<_, std::convert::Infallible>(current.clone())
    });

    assert!(matches!(
        result,
        Err(SceneEditError::Stale {
            expected_revision: 2,
            current_revision: 3,
        })
    ));
    assert!(!invoked.load(Ordering::SeqCst));
    let (snapshot, identity) = state.snapshot(&actions);
    assert_eq!(snapshot.text, "base");
    assert_eq!(identity.revision, 3);
}

#[test]
fn rejected_application_edit_preserves_error_and_live_scene() {
    let actions = DocActions::new();
    let scene = doc("base", 3);
    let state = SceneState::new(scene.clone(), scene_identity(&scene));

    let result = state.try_edit(&actions, 3, |_current| Err::<Doc, _>("occupied"));

    assert!(matches!(result, Err(SceneEditError::Rejected("occupied"))));
    let (snapshot, identity) = state.snapshot(&actions);
    assert_eq!(snapshot.text, "base");
    assert_eq!(identity.revision, 3);
}

#[test]
fn externally_owned_edit_reports_revision_exhaustion_before_invoking_edit() {
    let actions = DocActions::new();
    let scene = doc("base", 3);
    let state = SceneState::new(
        scene.clone(),
        SceneIdentity {
            scene_id: scene.scene_id.clone(),
            revision: u64::MAX,
        },
    );
    let invoked = AtomicBool::new(false);

    let result = state.try_edit(&actions, u64::MAX, |current| {
        invoked.store(true, Ordering::SeqCst);
        Ok::<_, std::convert::Infallible>(current.clone())
    });

    let Err(SceneEditError::RevisionExhausted(error)) = result else {
        panic!("edit should report revision exhaustion");
    };
    assert_eq!(error.kind, "revision_exhausted");
    assert_eq!(error.metadata["revision"], u64::MAX);
    assert!(!invoked.load(Ordering::SeqCst));
    let (snapshot, identity) = state.snapshot(&actions);
    assert_eq!(snapshot.text, "base");
    assert_eq!(identity.revision, u64::MAX);
}

#[test]
fn embedded_construction_derives_the_only_initial_identity_from_the_scene() {
    let actions = DocActions::new();
    let scene = doc("base", 7);
    let unrelated = SceneIdentity {
        scene_id: scene.scene_id.clone(),
        revision: 99,
    };

    let state = SceneState::embedded(&actions, scene.clone());

    assert_eq!(state.identity(), scene_identity(&scene));
    assert_ne!(state.identity(), unrelated);
}

#[test]
fn embedded_application_edit_enforces_the_declared_identity_transition() {
    let actions = DocActions::new();
    let scene = doc("base", 3);
    let state = SceneState::embedded(&DocActions::new(), scene.clone());

    let result = state.try_edit(&actions, 3, |current| {
        let mut next = current.clone();
        next.text.push_str(" human");
        Ok::<_, std::convert::Infallible>(next)
    });

    let Err(SceneEditError::EmbeddedIdentity { issues }) = result else {
        panic!("embedded edit should reject the invalid revision transition");
    };
    assert_eq!(issues, ["scene revision mismatch"]);
    let (snapshot, identity) = state.snapshot(&actions);
    assert_eq!(snapshot.text, "base");
    assert_eq!(identity.revision, 3);
}

#[tokio::test]
async fn a_live_commit_advances_the_external_revision() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let scene = doc("base ", 3);
    let state = SceneState::new(scene.clone(), scene_identity(&scene));
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

    assert_eq!(result.status(), TerminalStatus::Success);
    assert!(result.terminal_metadata().is_empty());
    assert_commit_output(&result, "exact", json!({}));
    // External ownership advances the SceneState revision by one.
    assert_eq!(state.identity().revision, 4);
    assert_eq!(result.scene().text, "base x");
    assert_eq!(result.run_record().scene().revision_after(), Some(4));
}

#[tokio::test]
async fn concurrent_live_runs_reject_the_stale_writer() {
    let state = {
        let scene = doc("base ", 3);
        SceneState::new(scene.clone(), scene_identity(&scene))
    };

    let client_a = ParkedClient::new(intent_proposal_json(), plan_envelope(&["A"]));
    let entered_a = client_a.entered();
    let gate_a = client_a.gate();
    let recipe_a = configured_pass_through(PassThroughRecipe::new());
    let sergent_a = Arc::new(Sergent::new(recipe_a, DocActions::new(), client_a).unwrap());

    let client_b = ParkedClient::new(intent_proposal_json(), plan_envelope(&["B"]));
    let entered_b = client_b.entered();
    let gate_b = client_b.gate();
    let recipe_b = configured_pass_through(PassThroughRecipe::new());
    let sergent_b = Arc::new(Sergent::new(recipe_b, DocActions::new(), client_b).unwrap());

    let handle_a = sergent_a.start(
        SceneSource::Live(state.clone()),
        DocMind,
        run_settings_for("p/m"),
        Vec::new(),
    );
    let handle_b = sergent_b.start(
        SceneSource::Live(state.clone()),
        DocMind,
        run_settings_for("p/m"),
        Vec::new(),
    );
    // Both runs snapshot revision 3 before either commits.
    entered_a.notified().await;
    entered_b.notified().await;

    gate_a.notify_one();
    let result_a = handle_a.result().await;
    gate_b.notify_one();
    let result_b = handle_b.result().await;

    assert_eq!(result_a.status(), TerminalStatus::Success);
    assert!(result_a.terminal_metadata().is_empty());
    assert_commit_output(&result_a, "exact", json!({}));
    assert_eq!(result_b.status(), TerminalStatus::Failure);
    assert_eq!(result_b.stage(), Stage::Commit);
    assert_eq!(result_b.error().unwrap().kind, "stale_patch");
    assert_eq!(
        result_b.error().unwrap().metadata,
        serde_json::Map::from_iter([
            ("base_revision".to_owned(), json!(3)),
            ("current_revision".to_owned(), json!(4)),
        ])
    );
    // Run A committed its edit; the stale writer's snapshot work was discarded.
    assert_eq!(result_a.scene().text, "base A");
    assert_eq!(result_b.scene().text, "base ");
    assert_eq!(state.identity().revision, 4);
    assert_no_commit_revision(result_b.run_record());
}

#[tokio::test]
async fn ordinary_clones_share_one_rebase_policy_and_authority() {
    let rebaser = TestRebaser::new(RebaseMode::AcceptSameOps);
    let rebase_calls = Arc::clone(&rebaser.calls);
    let state = {
        let scene = doc("base ", 3);
        SceneState::rebasing(scene.clone(), scene_identity(&scene), rebaser)
    };

    let (handle_a, gate_a, entered_a) = spawn_live(&state, "A");
    let (handle_b, gate_b, entered_b) = spawn_live(&state, "B");
    entered_a.notified().await;
    entered_b.notified().await;

    gate_a.notify_one();
    let result_a = handle_a.result().await;
    gate_b.notify_one();
    let result_b = handle_b.result().await;

    assert_eq!(result_a.status(), TerminalStatus::Success);
    assert_eq!(result_b.status(), TerminalStatus::Success);
    assert!(result_b.terminal_metadata().is_empty());
    assert_commit_output(&result_b, "rebased", json!({}));
    // The rebased writer applied its op on top of run A's committed scene.
    assert_eq!(result_a.scene().text, "base A");
    assert_eq!(result_b.scene().text, "base AB");
    assert_eq!(state.identity().revision, 5);
    assert_eq!(result_b.run_record().scene().revision_after(), Some(5));
    assert_eq!(rebase_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_rebase_conflict_is_a_structured_merge_conflict() {
    let rebaser = TestRebaser::new(RebaseMode::Conflict);
    let state = {
        let scene = doc("base ", 3);
        SceneState::rebasing(scene.clone(), scene_identity(&scene), rebaser)
    };

    let (handle_a, gate_a, entered_a) = spawn_live(&state, "A");
    let (handle_b, gate_b, entered_b) = spawn_live(&state, "B");
    entered_a.notified().await;
    entered_b.notified().await;

    gate_a.notify_one();
    let _ = handle_a.result().await;
    gate_b.notify_one();
    let result_b = handle_b.result().await;

    assert_eq!(result_b.status(), TerminalStatus::Failure);
    assert_eq!(result_b.stage(), Stage::Commit);
    assert_eq!(result_b.error().unwrap().kind, "merge_conflict");
    let metadata = &result_b.error().unwrap().metadata;
    assert_eq!(metadata.len(), 4);
    assert_eq!(metadata["base_revision"], 3);
    assert_eq!(metadata["current_live_revision"], 4);
    assert_eq!(metadata["patch"]["base"]["revision"], 3);
    assert_eq!(metadata["patch"]["operation_count"], 1);
    assert_eq!(metadata["patch"]["operations"][0]["value"]["text"], "B");
    assert_eq!(metadata["scene_metadata"], json!({}));
    // The conflict discarded the stale writer and left the live scene at rev 4.
    assert_eq!(result_b.scene().text, "base ");
    assert_eq!(state.identity().revision, 4);
    assert_no_commit_revision(result_b.run_record());
}

#[tokio::test]
async fn embedded_identity_accepts_an_exact_delta() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let scene = doc("base ", 3);
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

    assert_eq!(result.status(), TerminalStatus::Success);
    // One append advances the embedded revision by exactly one.
    assert_eq!(state.identity().revision, 4);
}

#[tokio::test]
async fn embedded_identity_rejects_an_unexpected_delta() {
    // Two appends advance the embedded revision by two, breaking the +1 rule.
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x", "y"]));
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let scene = doc("base ", 3);
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
    assert_eq!(result.error().unwrap().kind, "patch_validation");
    assert_eq!(
        result.error().unwrap().metadata,
        serde_json::Map::from_iter([
            (
                "identity_issues".to_owned(),
                json!(["scene revision mismatch"]),
            ),
            (
                "expected_identity".to_owned(),
                json!({ "scene_id": scene.scene_id.clone(), "revision": 4 }),
            ),
            (
                "actual_identity".to_owned(),
                json!({ "scene_id": scene.scene_id.clone(), "revision": 5 }),
            ),
        ])
    );
    // The rejected run left the live scene unchanged.
    assert_eq!(state.identity().revision, 3);
    assert_no_commit_revision(result.run_record());
}

#[tokio::test]
async fn the_same_two_op_plan_commits_under_external_ownership() {
    // Without embedded enforcement, the external counter advances by one and the
    // payload delta is not checked.
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x", "y"]));
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let scene = doc("base ", 3);
    let state = SceneState::new(scene.clone(), scene_identity(&scene));
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

    assert_eq!(result.status(), TerminalStatus::Success);
    assert_eq!(state.identity().revision, 4);
    assert_eq!(result.scene().text, "base xy");
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
