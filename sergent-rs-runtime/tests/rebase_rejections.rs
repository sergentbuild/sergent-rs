//! Public-wiring rejection proofs for deterministic shared-state rebase.

mod harness;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use harness::*;
use serde::Serialize;
use serde_json::{Map, json};
use sergent_rs_core::intent::Intent;
use sergent_rs_core::operation::{Inadmissible, Operation, OperationFault};
use sergent_rs_core::plan::Patch;
use sergent_rs_core::target::Target;
use sergent_rs_core::vocab::{RunStepName, Stage, TerminalStatus};
use sergent_rs_runtime::scene_state::{
    RebaseContext, RebaseOutcome, SceneRebase, SceneSource, SceneState,
};
use sergent_rs_runtime::sergent::Sergent;

enum HostileResult {
    Empty,
    Removed,
    Reordered,
    Duplicated,
    WrongBase,
    InsertedForeign(Patch<Doc, DocIntent, Spot>),
    Inadmissible,
    ApplicationFault,
    FirstRejectionStopsLater,
    Same,
}

#[derive(Clone, Copy, Serialize)]
struct RecordingAppend;

impl Operation for RecordingAppend {
    type Scene = Doc;
    type Intent = DocIntent;
    type Target = Spot;

    fn check_admissible(
        &self,
        _scene: &Doc,
        _intent: &DocIntent,
        _target: &Spot,
    ) -> Result<(), Inadmissible> {
        panic!("later Operation admissibility ran after the first rejection")
    }

    fn apply(
        &self,
        _scene: &mut Doc,
        _intent: &DocIntent,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize)]
struct FaultingAppend;

impl Operation for FaultingAppend {
    type Scene = Doc;
    type Intent = DocIntent;
    type Target = Spot;

    fn apply(
        &self,
        _scene: &mut Doc,
        _intent: &DocIntent,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        Err(OperationFault::new(
            "application_fault",
            "replacement application rejected",
            Map::from_iter([("domain_fact".to_owned(), json!("exact"))]),
        ))
    }
}

struct HostileRebaser {
    result: HostileResult,
    original_ids: Arc<Mutex<Vec<String>>>,
}

impl SceneRebase<Doc, DocIntent, Spot> for HostileRebaser {
    fn rebase(
        &self,
        context: RebaseContext<'_, Doc, DocIntent, Spot>,
    ) -> RebaseOutcome<Doc, DocIntent, Spot> {
        assert_eq!(
            context.base_scene().revision,
            context.original_patch().base().revision
        );
        assert_eq!(
            context.current_scene().revision,
            context.current_identity().revision
        );
        assert_eq!(
            context.original_intent().flow(),
            sergent_rs_core::intent::IntentFlow::Continue
        );
        assert_eq!(context.original_target().target_id(), &spot_id());

        let original = context.original_patch().steps();
        *self.original_ids.lock().unwrap() = original
            .iter()
            .map(|step| step.op_id().as_str().to_owned())
            .collect();
        let current_base = context.current_identity().clone();
        let (base, steps) = match &self.result {
            HostileResult::Empty => (current_base, Vec::new()),
            HostileResult::Removed => (
                current_base,
                original[..original.len() - 1]
                    .iter()
                    .map(|step| step.isolated_copy())
                    .collect(),
            ),
            HostileResult::Reordered => (
                current_base,
                original
                    .iter()
                    .rev()
                    .map(|step| step.isolated_copy())
                    .collect(),
            ),
            HostileResult::Duplicated => (
                current_base,
                vec![original[0].isolated_copy(), original[0].isolated_copy()],
            ),
            HostileResult::WrongBase => (
                context.original_patch().base().clone(),
                original.iter().map(|step| step.isolated_copy()).collect(),
            ),
            HostileResult::InsertedForeign(foreign) => {
                let mut steps = vec![original[0].isolated_copy()];
                steps.push(foreign.steps()[0].isolated_copy());
                steps.extend(original[1..].iter().map(|step| step.isolated_copy()));
                (current_base, steps)
            }
            HostileResult::Inadmissible => (
                current_base,
                original
                    .iter()
                    .enumerate()
                    .map(|(index, step)| {
                        if index == 1 {
                            step.with_operation(Append {
                                text: String::new(),
                            })
                        } else {
                            step.isolated_copy()
                        }
                    })
                    .collect(),
            ),
            HostileResult::FirstRejectionStopsLater => (
                current_base,
                original
                    .iter()
                    .enumerate()
                    .map(|(index, step)| {
                        if index == 0 {
                            step.with_operation(Append {
                                text: String::new(),
                            })
                        } else {
                            step.with_operation(RecordingAppend)
                        }
                    })
                    .collect(),
            ),
            HostileResult::ApplicationFault => (
                current_base,
                original
                    .iter()
                    .map(|step| step.with_operation(FaultingAppend))
                    .collect(),
            ),
            HostileResult::Same => (
                current_base,
                original.iter().map(|step| step.isolated_copy()).collect(),
            ),
        };
        RebaseOutcome::Rebased {
            patch: Patch::for_rebase(base, steps),
            metadata: matches!(&self.result, HostileResult::ApplicationFault)
                .then(|| Map::from_iter([("resolution".to_owned(), json!("human_wins"))]))
                .unwrap_or_default(),
        }
    }
}

fn operation_trace(result: &sergent_rs_core::run_record::SergentResult<Doc>) -> Vec<String> {
    result
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Patch)
        .unwrap()
        .output()
        .unwrap()
        .value()
        .unwrap()["compiled_patch"]["operation_trace_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|operation_id| operation_id.as_str().unwrap().to_owned())
        .collect()
}

async fn assert_rebase_rejected(
    result: HostileResult,
) -> sergent_rs_core::run_record::SergentResult<Doc> {
    let observed_ids = Arc::new(Mutex::new(Vec::new()));
    let policy = HostileRebaser {
        result,
        original_ids: Arc::clone(&observed_ids),
    };
    let scene = doc("base ", 3);
    let state = SceneState::rebasing(scene.clone(), scene_identity(&scene), policy);

    let recipe = PassThroughRecipe::new();
    let validations = Arc::clone(&recipe.plan_validations);
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&["B", "C"]));
    let requests = Arc::clone(&client.inner.requests);
    let entered = client.entered();
    let gate = client.gate();
    let sergent =
        Arc::new(Sergent::new(configured_pass_through(recipe), DocActions::new(), client).unwrap());
    let handle = sergent.start(
        SceneSource::Live(state.clone()),
        DocMind,
        run_settings_for("p/m"),
        Vec::new(),
    );
    entered.notified().await;

    let writer = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        CannedClient::new(intent_proposal_json(), plan_envelope(&["A"])),
    )
    .unwrap();
    let writer_result = writer
        .run(
            SceneSource::Live(state.clone()),
            &DocMind,
            run_settings_for("p/m"),
            &sergent_rs_runtime::cancel::CancelToken::new(),
            &[],
        )
        .await;
    assert_eq!(writer_result.status(), TerminalStatus::Success);

    gate.notify_one();
    let rejected = handle.result().await;
    assert_eq!(rejected.status(), TerminalStatus::Failure);
    assert_eq!(rejected.stage(), Stage::Commit);
    assert_eq!(rejected.error().unwrap().kind, "merge_conflict");
    assert_eq!(rejected.scene().text, "base ");
    assert_no_commit_revision(rejected.run_record());
    assert_eq!(operation_trace(&rejected), *observed_ids.lock().unwrap());
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(validations.load(Ordering::SeqCst), 1);

    let (current, identity) = state.snapshot(&DocActions::new());
    assert_eq!(current.text, "base A");
    assert_eq!(identity.revision, 4);
    rejected
}

#[tokio::test]
async fn rebase_rejects_an_empty_patch() {
    assert_rebase_rejected(HostileResult::Empty).await;
}

#[tokio::test]
async fn rebase_rejects_a_removed_operation() {
    assert_rebase_rejected(HostileResult::Removed).await;
}

#[tokio::test]
async fn rebase_rejects_reordered_operations() {
    assert_rebase_rejected(HostileResult::Reordered).await;
}

#[tokio::test]
async fn rebase_rejects_a_duplicated_operation() {
    assert_rebase_rejected(HostileResult::Duplicated).await;
}

#[tokio::test]
async fn rebase_rejects_a_patch_with_the_wrong_base() {
    assert_rebase_rejected(HostileResult::WrongBase).await;
}

#[tokio::test]
async fn rebase_rejects_an_inserted_foreign_operation_id() {
    let registry = append_registry(None);
    let foreign = registry
        .decode(&plan_envelope(&["foreign"]))
        .unwrap()
        .bind_to_scene(scene_identity(&doc("foreign", 9)))
        .compile_isolated_patch();
    assert_rebase_rejected(HostileResult::InsertedForeign(foreign)).await;
}

#[tokio::test]
async fn rebase_inadmissibility_names_the_exact_returned_operation() {
    let rejected = assert_rebase_rejected(HostileResult::Inadmissible).await;
    let error = rejected.error().unwrap();
    let validation = &error.metadata["validation_error"];
    assert_eq!(validation["metadata"]["index"], 1);
    assert_eq!(validation["metadata"]["call"], "append");
    assert!(
        validation["metadata"]["operation_id"]
            .as_str()
            .unwrap()
            .starts_with("op_")
    );
    assert_eq!(validation["kind"], "operation_admissibility");
    assert!(
        validation["message"]
            .as_str()
            .unwrap()
            .contains("append text must not be empty")
    );
}

#[tokio::test]
async fn rebased_application_fault_preserves_exact_error_and_rejected_summary() {
    let rejected = assert_rebase_rejected(HostileResult::ApplicationFault).await;
    let error = rejected.error().unwrap();

    assert_eq!(error.metadata["base_revision"], 3);
    assert_eq!(error.metadata["current_live_revision"], 4);
    assert_eq!(
        error.metadata["scene_metadata"],
        json!({ "resolution": "human_wins" })
    );
    assert_eq!(
        error.metadata["validation_error"],
        json!({
            "kind": "application_fault",
            "message": "replacement application rejected",
            "metadata": { "domain_fact": "exact" }
        })
    );
    assert_eq!(error.metadata["patch"]["operation_count"], 2);
    assert!(
        error.metadata["patch"]["operations"][0]["value"]
            .get("text")
            .is_none()
    );
}

#[tokio::test]
async fn rebase_admissibility_stops_after_the_first_rejection() {
    assert_rebase_rejected(HostileResult::FirstRejectionStopsLater).await;
}

#[tokio::test]
async fn rebase_rejects_when_the_original_target_is_missing() {
    let observed_ids = Arc::new(Mutex::new(Vec::new()));
    let policy = HostileRebaser {
        result: HostileResult::Same,
        original_ids: Arc::clone(&observed_ids),
    };
    let scene = doc("base ", 3);
    let state = SceneState::rebasing(scene.clone(), scene_identity(&scene), policy);
    let recipe = PassThroughRecipe::new();
    let validations = Arc::clone(&recipe.plan_validations);
    let client = ParkedClient::new(intent_proposal_json(), plan_envelope(&["B", "C"]));
    let requests = Arc::clone(&client.inner.requests);
    let entered = client.entered();
    let gate = client.gate();
    let sergent =
        Arc::new(Sergent::new(configured_pass_through(recipe), DocActions::new(), client).unwrap());
    let handle = sergent.start(
        SceneSource::Live(state.clone()),
        DocMind,
        run_settings_for("p/m"),
        Vec::new(),
    );
    entered.notified().await;

    state
        .try_edit(&DocActions::new(), 3, |current| {
            let mut edited = current.clone();
            edited.has_spot = false;
            edited.revision = 4;
            Ok::<_, std::convert::Infallible>(edited)
        })
        .unwrap();
    gate.notify_one();
    let rejected = handle.result().await;

    assert_eq!(rejected.error().unwrap().kind, "merge_conflict");
    assert_eq!(rejected.stage(), Stage::Commit);
    assert_no_commit_revision(rejected.run_record());
    assert_eq!(operation_trace(&rejected), *observed_ids.lock().unwrap());
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(validations.load(Ordering::SeqCst), 1);
    let (current, identity) = state.snapshot(&DocActions::new());
    assert!(!current.has_spot);
    assert_eq!(current.text, "base ");
    assert_eq!(identity.revision, 4);
}
