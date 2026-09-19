//! Accepted multi-Operation rebase proof: complete context, fresh
//! admissibility clones, one evolving dry-run, revision authority, and the
//! accepted trace.

mod harness;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use harness::*;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use sergent_rs_core::operation::{Inadmissible, Operation, OperationFault, PlanStep};
use sergent_rs_core::plan::Patch;
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::scene::{SceneActions, SceneIdentity, VerificationReport};
use sergent_rs_core::vocab::{RunStepName, TerminalStatus};
use sergent_rs_runtime::scene_state::{
    RebaseContext, RebaseOutcome, SceneRebase, SceneSource, SceneState,
};
use sergent_rs_runtime::sergent::{ConfiguredRecipe, Sergent};

static DECODE_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct CountedAppend {
    /// Text appended to the document.
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CountedAppendWire {
    text: String,
}

impl<'de> Deserialize<'de> for CountedAppend {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CountedAppendWire::deserialize(deserializer)?;
        DECODE_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(Self { text: wire.text })
    }
}

impl Operation for CountedAppend {
    type Scene = Doc;
    type Intent = DocIntent;
    type Target = Spot;

    fn apply(
        &self,
        scene: &mut Doc,
        _intent: &DocIntent,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        scene.text.push_str(&self.text);
        scene.revision += 1;
        Ok(())
    }
}

#[derive(Default)]
struct RebaseProbe {
    context_target: Mutex<Option<usize>>,
    context_intent: Mutex<Option<usize>>,
    original_ids: Mutex<Vec<String>>,
    rebased_trace: Mutex<Vec<(String, String)>>,
    rebase_calls: AtomicUsize,
}

#[derive(Clone, Serialize)]
struct ProbeAppend {
    text: String,
    expected_admissibility_revision: u64,
    expected_admissibility_text: String,
    expected_application_revision: u64,
    expected_application_text: String,
    expected_target: usize,
    expected_intent: usize,
}

impl Operation for ProbeAppend {
    type Scene = Doc;
    type Intent = DocIntent;
    type Target = Spot;

    fn check_admissible(
        &self,
        scene: &Doc,
        _intent: &DocIntent,
        target: &Spot,
    ) -> Result<(), Inadmissible> {
        assert_eq!(scene.revision, self.expected_admissibility_revision);
        assert_eq!(scene.text, self.expected_admissibility_text);
        assert_eq!(target as *const Spot as usize, self.expected_target);
        Ok(())
    }

    fn apply(
        &self,
        scene: &mut Doc,
        intent: &DocIntent,
        target: &Spot,
    ) -> Result<(), OperationFault> {
        assert_eq!(scene.revision, self.expected_application_revision);
        assert_eq!(scene.text, self.expected_application_text);
        assert_eq!(target as *const Spot as usize, self.expected_target);
        assert_eq!(intent as *const DocIntent as usize, self.expected_intent);
        scene.text.push_str(&self.text);
        scene.revision += 1;
        Ok(())
    }
}

struct AcceptedRebaser {
    replacements: Vec<String>,
    probe: Arc<RebaseProbe>,
}

impl SceneRebase<Doc, DocIntent, Spot> for AcceptedRebaser {
    fn rebase(
        &self,
        context: RebaseContext<'_, Doc, DocIntent, Spot>,
    ) -> RebaseOutcome<Doc, DocIntent, Spot> {
        self.probe.rebase_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(context.base_scene().text, "base ");
        assert_eq!(context.base_scene().revision, 3);
        assert_eq!(context.current_scene().text, "base A");
        assert_eq!(context.current_scene().revision, 4);
        assert_eq!(context.original_patch().base().revision, 3);
        assert_eq!(context.current_identity().revision, 4);
        assert_eq!(context.original_intent().kind, IntentKind::Continue);
        *self.probe.context_target.lock().unwrap() =
            Some(context.original_target() as *const Spot as usize);
        *self.probe.context_intent.lock().unwrap() =
            Some(context.original_intent() as *const DocIntent as usize);
        *self.probe.original_ids.lock().unwrap() = context
            .original_patch()
            .steps()
            .iter()
            .map(|step| step.op_id().as_str().to_owned())
            .collect();

        let expected_target = context.original_target() as *const Spot as usize;
        let expected_intent = context.original_intent() as *const DocIntent as usize;
        let expected_admissibility_revision = context.current_scene().revision;
        let expected_admissibility_text = context.current_scene().text.clone();
        let mut expected_application_revision = expected_admissibility_revision;
        let mut expected_application_text = expected_admissibility_text.clone();
        let steps = context
            .original_patch()
            .steps()
            .iter()
            .zip(&self.replacements)
            .map(|(step, text)| {
                let operation = ProbeAppend {
                    text: text.clone(),
                    expected_admissibility_revision,
                    expected_admissibility_text: expected_admissibility_text.clone(),
                    expected_application_revision,
                    expected_application_text: expected_application_text.clone(),
                    expected_target,
                    expected_intent,
                };
                expected_application_revision += 1;
                expected_application_text.push_str(text);
                step.with_operation(operation)
            })
            .collect::<Vec<_>>();
        *self.probe.rebased_trace.lock().unwrap() = steps
            .iter()
            .map(|step| (step.op_id().as_str().to_owned(), step.call().to_owned()))
            .collect();
        RebaseOutcome::Rebased {
            patch: Patch::for_rebase(context.current_identity().clone(), steps),
            metadata: serde_json::Map::from_iter([(
                "resolution".to_owned(),
                json!("preserved_current_work"),
            )]),
        }
    }
}

#[derive(Default)]
struct ActionProbe {
    clone_inputs: Mutex<Vec<(u64, String)>>,
    apply_inputs: Mutex<Vec<(u64, usize, usize, usize)>>,
    verify_inputs: Mutex<Vec<(u64, u64, usize)>>,
}

struct ProbeActions {
    probe: Arc<ActionProbe>,
}

impl SceneActions for ProbeActions {
    type Scene = Doc;
    type Intent = DocIntent;
    type Target = Spot;

    fn identity(&self, scene: &Doc) -> SceneIdentity {
        scene_identity(scene)
    }

    fn clone_scene(&self, scene: &Doc) -> Doc {
        self.probe
            .clone_inputs
            .lock()
            .unwrap()
            .push((scene.revision, scene.text.clone()));
        scene.clone()
    }

    fn select_target(&self, scene: &Doc) -> Option<Spot> {
        DocActions::new().select_target(scene)
    }

    fn has_target(&self, scene: &Doc, _target: &Spot) -> bool {
        scene.has_spot
    }

    fn apply(
        &self,
        scene: &mut Doc,
        intent: &DocIntent,
        target: &Spot,
        operations: &[PlanStep<Doc, DocIntent, Spot>],
    ) -> Result<(), OperationFault> {
        self.probe.apply_inputs.lock().unwrap().push((
            scene.revision,
            operations.len(),
            target as *const Spot as usize,
            intent as *const DocIntent as usize,
        ));
        for step in operations {
            step.operation().apply(scene, intent, target)?;
        }
        Ok(())
    }

    fn verify(
        &self,
        before: &Doc,
        after: &Doc,
        _target: &Spot,
        operations: &[PlanStep<Doc, DocIntent, Spot>],
    ) -> VerificationReport {
        self.probe.verify_inputs.lock().unwrap().push((
            before.revision,
            after.revision,
            operations.len(),
        ));
        VerificationReport::accepted()
    }
}

fn counted_registry() -> OperationRegistry<Doc, DocIntent, Spot> {
    OperationRegistry::builder()
        .register::<CountedAppend>("counted_append")
        .unwrap()
        .build(None)
        .unwrap()
}

fn accepted_trace(
    result: &sergent_rs_core::run_record::SergentResult<Doc>,
) -> Vec<(String, String)> {
    let summary = compiled_patch(result);
    summary["operation_trace_ids"]
        .as_array()
        .unwrap()
        .iter()
        .zip(summary["operation_call_names"].as_array().unwrap())
        .map(|(operation_id, call)| {
            (
                operation_id.as_str().unwrap().to_owned(),
                call.as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn compiled_patch(result: &sergent_rs_core::run_record::SergentResult<Doc>) -> &serde_json::Value {
    &result
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Patch)
        .unwrap()
        .output()
        .unwrap()
        .value()
        .unwrap()["compiled_patch"]
}

#[tokio::test]
async fn accepted_multi_operation_rebase_preserves_complete_authority() {
    DECODE_COUNT.store(0, Ordering::SeqCst);
    let rebase_probe = Arc::new(RebaseProbe::default());
    let action_probe = Arc::new(ActionProbe::default());
    let policy = AcceptedRebaser {
        replacements: vec!["B".to_owned(), "C".to_owned()],
        probe: Arc::clone(&rebase_probe),
    };
    let scene = doc("base ", 3);
    let state = SceneState::rebasing(scene.clone(), scene_identity(&scene), policy);

    let recipe = PassThroughRecipe::new();
    let validations = Arc::clone(&recipe.plan_validations);
    let configured = ConfiguredRecipe::plan_capable(recipe, counted_registry());
    let plan = object(json!({
        "operations": [
            {"call": "counted_append", "text": "old-b"},
            {"call": "counted_append", "text": "old-c"}
        ]
    }));
    let client = ParkedClient::new(intent_proposal_json(), plan);
    let requests = Arc::clone(&client.inner.requests);
    let entered = client.entered();
    let gate = client.gate();
    let sergent = Arc::new(
        Sergent::new(
            configured,
            ProbeActions {
                probe: Arc::clone(&action_probe),
            },
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

    let writer = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        CannedClient::new(intent_proposal_json(), plan_envelope(&["A"])),
    )
    .unwrap();
    let writer_cancel = sergent_rs_runtime::cancel::CancelToken::new();
    let writer_result = writer
        .run(
            SceneSource::Live(state.clone()),
            &DocMind,
            run_settings_for("p/m"),
            &writer_cancel,
            &[],
        )
        .await;
    assert_eq!(writer_result.status(), TerminalStatus::Success);

    gate.notify_one();
    let accepted = handle.result().await;
    assert_eq!(accepted.status(), TerminalStatus::Success);
    assert_eq!(
        accepted.terminal_metadata(),
        &serde_json::Map::from_iter([("resolution".to_owned(), json!("preserved_current_work"),)])
    );
    assert_eq!(accepted.scene().text, "base ABC");
    assert_eq!(accepted.run_record().scene().revision_after(), Some(5));
    assert_eq!(state.identity().revision, 5);
    let commit = accepted
        .run_record()
        .steps()
        .iter()
        .find(|step| step.name() == RunStepName::Commit)
        .unwrap()
        .output()
        .unwrap()
        .value()
        .unwrap();
    assert_eq!(
        commit,
        &json!({ "commit_kind": "rebased", "metadata": accepted.terminal_metadata() })
    );

    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(DECODE_COUNT.load(Ordering::SeqCst), 2);
    assert_eq!(validations.load(Ordering::SeqCst), 1);
    assert_eq!(rebase_probe.rebase_calls.load(Ordering::SeqCst), 1);

    let target = rebase_probe.context_target.lock().unwrap().unwrap();
    let intent = rebase_probe.context_intent.lock().unwrap().unwrap();

    let clones = action_probe.clone_inputs.lock().unwrap();
    assert_eq!(clones.iter().filter(|entry| entry.0 == 4).count(), 3);
    let applies = action_probe.apply_inputs.lock().unwrap();
    assert_eq!(
        applies.iter().map(|entry| entry.0).collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert!(
        applies
            .iter()
            .all(|entry| entry.1 == 2 && entry.2 == target && entry.3 == intent)
    );
    let verifies = action_probe.verify_inputs.lock().unwrap();
    assert_eq!(
        verifies.iter().filter(|entry| **entry == (4, 6, 2)).count(),
        1
    );

    let trace = accepted_trace(&accepted);
    let recorded = compiled_patch(&accepted);
    assert_eq!(recorded["operations"][0]["value"]["text"], "old-b");
    assert_eq!(recorded["operations"][1]["value"]["text"], "old-c");
    assert_eq!(trace, *rebase_probe.rebased_trace.lock().unwrap());
    let original_ids = rebase_probe.original_ids.lock().unwrap();
    assert_eq!(
        trace.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        *original_ids
    );
    assert!(trace.iter().all(|(_, call)| call == "counted_append"));
}
