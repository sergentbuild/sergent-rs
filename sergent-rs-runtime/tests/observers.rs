//! Observer delivery is contained per slot and never changes a run outcome.
//! A failing slot is named beside the Run Record while other slots
//! still receive every callback.

mod harness;

use std::any::type_name;
use std::collections::BTreeSet;

use harness::*;
use sergent_rs_core::error::RunError;
use sergent_rs_core::vocab::TerminalStatus;
use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::SceneSource;
use sergent_rs_runtime::sergent::Sergent;

#[tokio::test]
async fn repeated_failing_slots_accumulate_before_later_terminal_observation() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["hi"]));
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let failing = FailingObserver::default();
    let capturing = CapturingObserver::default();
    let observers: [&dyn RunObserver<Doc>; 3] = [&failing, &capturing, &failing];
    let cancel = CancelToken::new();

    let result = sergent
        .run(
            SceneSource::plain(doc("a", 1)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    // The run still committed: containment never changes status or mutation.
    assert_eq!(result.status(), TerminalStatus::Success);
    assert_eq!(result.scene().text, "ahi");

    // Every delivery failure stays beside the Run Record and has the exact
    // diagnostic metadata shape.
    assert!(!result.observer_errors().is_empty());
    for error in result.observer_errors() {
        assert_eq!(error.kind, "observer_error");
        assert_eq!(
            error
                .metadata
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            ["callback", "exception_type", "observer_type", "stage"]
                .into_iter()
                .collect()
        );
        assert_eq!(error.metadata["exception_type"], type_name::<RunError>());
        assert!(
            error.metadata["observer_type"]
                .as_str()
                .unwrap()
                .ends_with("FailingObserver")
        );
        assert_eq!(error.message.chars().count(), 2_048);
    }
    for step in result.run_record().steps() {
        assert!(step.error().is_none());
    }

    // The same failing object occupies two independent slots. The middle slot
    // still receives every callback.
    let progress_count = *failing.progress_calls.lock().unwrap();
    assert_eq!(
        (capturing.progress.lock().unwrap().len() * 2) as u32,
        progress_count,
    );
    assert_eq!(capturing.finished.lock().unwrap().len(), 1);
    assert_eq!(*failing.finished_calls.lock().unwrap(), 2);

    // The middle terminal slot sees all progress failures plus the earlier
    // finished failure. The caller additionally sees the later finished
    // failure from the repeated final slot.
    let observed_error_count = capturing.finished.lock().unwrap()[0]
        .observer_errors()
        .len();
    assert_eq!(observed_error_count as u32, progress_count + 1,);
    assert_eq!(result.observer_errors().len(), observed_error_count + 1,);
    assert_eq!(
        result.observer_errors().last().unwrap().metadata["callback"],
        "finished"
    );
}

#[tokio::test]
async fn a_stop_run_still_closes_the_record_and_delivers_finished() {
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["x"]));
    let recipe = ModelIntentRecipe::new().with_intent(IntentKind::Stop);
    let sergent = Sergent::new(configured_model_intent(recipe), DocActions::new(), client).unwrap();
    let capturing = CapturingObserver::default();
    let observers: [&dyn RunObserver<Doc>; 1] = [&capturing];
    let cancel = CancelToken::new();

    let result = sergent
        .run(
            SceneSource::plain(doc("d", 3)),
            &DocMind,
            run_settings(),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Success);
    // Every ordinary termination delivers the current result, including all
    // observer errors accumulated before this slot.
    let finished = capturing.finished.lock().unwrap();
    assert_eq!(finished.len(), 1);
    assert_eq!(
        finished[0].run_record().outcome().terminal_status(),
        TerminalStatus::Success
    );
}
