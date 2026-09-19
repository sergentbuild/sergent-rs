//! The opt-in JSONL Run Record harness: envelope shape, run boundaries, opaque
//! app/user events, ASCII escaping, and exclusive create.

mod harness;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use harness::*;
use serde::Serialize;
use serde_json::{Value, json};
use sergent_rs_core::ids::{RunId, SceneId};
use sergent_rs_core::vocab::{ProgressStatus, Stage};
use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::progress::ProgressSnapshot;
use sergent_rs_runtime::run_record_file::{
    JsonlRunRecordWriter, RunRecordApplicationName, RunRecordCorrelation, RunRecordEvent,
    RunRecordEventArray, RunRecordEventValue, RunRecordFileError, RunRecordFileId,
};
use sergent_rs_runtime::scene_state::SceneSource;
use sergent_rs_runtime::sergent::Sergent;

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
/// One nested sequence used to prove recursive non-finite rejection.
struct NestedFloat<T> {
    values: Vec<T>,
}

fn create_log() -> (JsonlRunRecordWriter, std::path::PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
    JsonlRunRecordWriter::create(
        std::env::temp_dir(),
        RunRecordApplicationName::parse("runtime-test").unwrap(),
        Some(RunRecordFileId::parse(format!("case_{nanos}_{unique}")).unwrap()),
    )
    .unwrap()
}

fn lines(path: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Compose the public serializable-value and direct-record operations exactly once.
fn record_serializable<T: Serialize + ?Sized>(
    log: &JsonlRunRecordWriter,
    value: &T,
) -> Result<(), RunRecordFileError> {
    let payload = RunRecordEventValue::from_serializable(value)?;
    let event = RunRecordEvent::new("serialized", Some(payload), RunRecordCorrelation::default())?;
    log.record_app(event)
}

fn assert_top_level_keys(value: &Value, expected: &[&str]) {
    let actual = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}

fn started_progress(run_id: &str) -> ProgressSnapshot {
    ProgressSnapshot {
        run_id: RunId::parse(run_id).unwrap(),
        scene_id: Some(SceneId::parse("doc_00000000000000000000000000000000").unwrap()),
        stage: Stage::Started,
        status: ProgressStatus::Running,
        revision: 1,
    }
}

#[test]
fn each_run_writes_one_start_and_a_started_terminal_close_writes_none() {
    let (log, path) = create_log();
    let first = started_progress("run_00000000000000000000000000000001");
    let second = started_progress("run_00000000000000000000000000000002");
    // A run that ends at stage Started (no target, or pre-Intent cancellation)
    // delivers that stage a second time carrying its terminal status.
    let first_terminal = ProgressSnapshot {
        status: ProgressStatus::Failure,
        ..first.clone()
    };

    <JsonlRunRecordWriter as RunObserver<Doc>>::on_progress(&log, &first).unwrap();
    <JsonlRunRecordWriter as RunObserver<Doc>>::on_progress(&log, &first).unwrap();
    <JsonlRunRecordWriter as RunObserver<Doc>>::on_progress(&log, &first_terminal).unwrap();
    <JsonlRunRecordWriter as RunObserver<Doc>>::on_progress(&log, &second).unwrap();
    drop(log);

    let rows = lines(&path);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["run_id"], json!(first.run_id.as_str()));
    assert_eq!(rows[1]["run_id"], json!(second.run_id.as_str()));

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn repeated_slots_write_each_run_end_for_sequential_runs() {
    let (log, path) = create_log();
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["z"]));
    let sergent = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let cancel = CancelToken::new();

    for revision in [1, 2] {
        let observers: [&dyn RunObserver<Doc>; 2] = [&log, &log];
        let result = sergent
            .run(
                SceneSource::plain(doc("a", revision)),
                &DocMind,
                run_settings(),
                &cancel,
                &observers,
            )
            .await;
        assert_eq!(
            result.status(),
            sergent_rs_core::vocab::TerminalStatus::Success
        );
    }
    drop(log);

    let rows = lines(&path);
    let events: Vec<&str> = rows
        .iter()
        .map(|row| row["event"].as_str().unwrap())
        .collect();
    assert_eq!(
        events,
        [
            "run.start",
            "run.end",
            "run.end",
            "run.start",
            "run.end",
            "run.end"
        ]
    );
    assert_ne!(rows[0]["run_id"], rows[3]["run_id"]);
    for start in [0, 3] {
        assert_eq!(rows[start]["run_id"], rows[start + 1]["run_id"]);
        assert_eq!(rows[start + 1]["run_id"], rows[start + 2]["run_id"]);
        assert_eq!(rows[start + 1]["payload"], rows[start + 2]["payload"]);
    }

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_run_persists_start_and_end_under_the_envelope() {
    let (log, path) = create_log();
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["z"]));
    let sergent = Sergent::new(
        configured_model_intent(ModelIntentRecipe::new()),
        DocActions::new(),
        client,
    )
    .unwrap();
    let cancel = CancelToken::new();

    let result = {
        let observers: [&dyn RunObserver<Doc>; 1] = [&log];
        sergent
            .run(
                SceneSource::plain(doc("a", 5)),
                &DocMind,
                run_settings(),
                &cancel,
                &observers,
            )
            .await
    };
    drop(log);

    let rows = lines(&path);
    assert_eq!(rows.len(), 2);
    let correlated_keys = [
        "schema_version",
        "timestamp_utc",
        "activity",
        "event",
        "payload",
        "run_id",
        "scene_id",
        "revision",
    ];
    let record = result.run_record();
    let run_id = json!(record.run_id().as_str());
    let scene_id = json!(record.scene().scene_id().as_str());

    let start = &rows[0];
    assert_top_level_keys(start, &correlated_keys);
    assert_eq!(start["schema_version"], "sergent.run_record.v1");
    assert_eq!(start["activity"], "sergent_activity");
    assert_eq!(start["event"], "run.start");
    assert_eq!(
        start["payload"],
        json!({
            "snapshot": ProgressSnapshot {
                run_id: record.run_id().clone(),
                scene_id: Some(record.scene().scene_id().clone()),
                stage: Stage::Started,
                status: ProgressStatus::Running,
                revision: record.scene().revision_before(),
            }
        })
    );
    assert_eq!(start["run_id"], run_id);
    assert_eq!(start["scene_id"], scene_id);
    assert_eq!(start["revision"], json!(record.scene().revision_before()));
    assert!(start["timestamp_utc"].is_string());

    let end = &rows[1];
    assert_top_level_keys(end, &correlated_keys);
    assert_eq!(end["schema_version"], "sergent.run_record.v1");
    assert_eq!(end["activity"], "sergent_activity");
    assert_eq!(end["event"], "run.end");
    assert_eq!(end["payload"], json!({ "run_record": record }));
    assert_eq!(end["run_id"], run_id);
    assert_eq!(end["scene_id"], scene_id);
    assert_eq!(end["revision"], json!(record.scene().revision_after()));
    assert!(end["timestamp_utc"].is_string());
    let step_names: Vec<&str> = end["payload"]["run_record"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        step_names,
        vec![
            "process_input",
            "intent",
            "execution_plan",
            "patch",
            "commit"
        ]
    );
    assert_eq!(end["payload"]["run_record"]["scene"]["revision_before"], 5);
    assert_eq!(end["payload"]["run_record"]["scene"]["revision_after"], 6);
    assert!(end["payload"]["run_record"]["timing"]["finished_at"].is_string());
    assert!(end["payload"]["run_record"]["timing"]["duration_ms"].is_number());
    for step in end["payload"]["run_record"]["steps"].as_array().unwrap() {
        assert_ne!(step["status"], "running");
        assert!(step["timing"]["finished_at"].is_string());
        assert!(step["timing"]["duration_ms"].is_number());
    }
    let plan_call = &end["payload"]["run_record"]["steps"][2]["model_call"];
    assert_eq!(plan_call["attempts"][0]["status"], "success");
    assert!(plan_call["payloads"]["request"]["value"].is_object());

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn failed_and_cancelled_ends_wrap_the_complete_record_without_after_revision() {
    let (failed_log, failed_path) = create_log();
    let failed_runtime = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        CannedClient::new(intent_proposal_json(), object(json!({ "operations": [] }))),
    )
    .unwrap();
    let failed_cancel = CancelToken::new();
    let failed_observers: [&dyn RunObserver<Doc>; 1] = [&failed_log];
    let failed = failed_runtime
        .run(
            SceneSource::plain(doc("a", 9)),
            &DocMind,
            run_settings(),
            &failed_cancel,
            &failed_observers,
        )
        .await;
    drop(failed_log);
    assert_terminal_end(&failed_path, &failed, "failure", 9);

    let (cancelled_log, cancelled_path) = create_log();
    let cancelled_runtime = Sergent::new(
        configured_pass_through(PassThroughRecipe::new()),
        DocActions::new(),
        CannedClient::new(intent_proposal_json(), plan_envelope(&["z"])),
    )
    .unwrap();
    let cancelled_token = CancelToken::new();
    cancelled_token.cancel();
    let cancelled_observers: [&dyn RunObserver<Doc>; 1] = [&cancelled_log];
    let cancelled = cancelled_runtime
        .run(
            SceneSource::plain(doc("a", 11)),
            &DocMind,
            run_settings(),
            &cancelled_token,
            &cancelled_observers,
        )
        .await;
    drop(cancelled_log);
    assert_terminal_end(&cancelled_path, &cancelled, "cancelled", 11);

    std::fs::remove_file(failed_path).unwrap();
    std::fs::remove_file(cancelled_path).unwrap();
}

fn assert_terminal_end(
    path: &std::path::Path,
    result: &sergent_rs_core::run_record::SergentResult<Doc>,
    status: &str,
    revision_before: u64,
) {
    let raw = std::fs::read_to_string(path).unwrap();
    assert!(raw.is_ascii());
    assert!(raw.ends_with('\n'));
    let rows = lines(path);
    let end = rows.iter().find(|row| row["event"] == "run.end").unwrap();
    assert_eq!(end["activity"], "sergent_activity");
    assert_eq!(end["revision"], revision_before);
    assert_eq!(end["payload"], json!({ "run_record": result.run_record() }));
    assert_eq!(end["payload"]["run_record"]["outcome"]["status"], status);
    assert_eq!(
        end["payload"]["run_record"]["scene"]["revision_after"],
        Value::Null
    );
}

#[tokio::test]
async fn app_and_user_events_are_persisted_opaquely() {
    let (log, path) = create_log();

    log.record_app(
        RunRecordEvent::new(
            "note.added",
            Some(RunRecordEventValue::json(json!({ "id": 7 }))),
            RunRecordCorrelation::default(),
        )
        .unwrap(),
    )
    .unwrap();
    log.record_user(
        RunRecordEvent::new(
            "keypress",
            Some(RunRecordEventValue::json(json!({ "key": "enter" }))),
            RunRecordCorrelation::new(Some("run custom".to_owned()), None, Some(8)).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    drop(log);

    let rows = lines(&path);
    assert_eq!(rows.len(), 2);
    let required_keys = [
        "schema_version",
        "timestamp_utc",
        "activity",
        "event",
        "payload",
    ];
    assert_top_level_keys(&rows[0], &required_keys);
    for row in &rows {
        assert_eq!(row["schema_version"], "sergent.run_record.v1");
        assert!(row["timestamp_utc"].is_string());
        assert!(row.get("scene_id").is_none());
    }
    assert!(rows[0].get("run_id").is_none());
    assert!(rows[0].get("revision").is_none());
    assert_eq!(rows[0]["activity"], "app_activity");
    assert_eq!(rows[0]["event"], "note.added");
    assert_eq!(rows[0]["payload"], json!({ "id": 7 }));
    assert_eq!(rows[1]["activity"], "user_activity");
    assert_eq!(rows[1]["event"], "keypress");
    assert_eq!(rows[1]["payload"], json!({ "key": "enter" }));
    assert_eq!(rows[1]["run_id"], "run custom");
    assert_eq!(rows[1]["revision"], 8);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_non_ascii_payload_is_escaped_and_persisted() {
    let (log, path) = create_log();

    // Non-ASCII payload text (routine model output or domain text, including a
    // surrogate-pair emoji) persists rather than being refused.
    log.record_app(
        RunRecordEvent::new(
            "note",
            Some(RunRecordEventValue::json(
                json!({ "text": "cafe\u{301} \u{1F600}" }),
            )),
            RunRecordCorrelation::default(),
        )
        .unwrap(),
    )
    .unwrap();
    drop(log);

    // The persisted bytes are pure ASCII: non-ASCII characters ride as \uXXXX.
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.is_ascii());
    assert!(raw.contains("\\u0301"));

    // The escape is value-preserving: parsing restores the original text.
    let rows = lines(&path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["payload"]["text"], json!("cafe\u{301} \u{1F600}"));

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn conversion_failure_occurs_before_any_byte_is_written() {
    let (log, path) = create_log();
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(record_serializable(&log, &value).is_err());
        assert!(
            record_serializable(
                &log,
                &NestedFloat {
                    values: vec![0.0, value],
                }
            )
            .is_err()
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(record_serializable(&log, &value).is_err());
        assert!(
            record_serializable(
                &log,
                &NestedFloat {
                    values: vec![0.0, value],
                }
            )
            .is_err()
        );
    }
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);

    let cyclic = RunRecordEventArray::default();
    cyclic.push(RunRecordEventValue::from(cyclic.clone()));
    let event = RunRecordEvent::new(
        "cycle",
        Some(RunRecordEventValue::from(cyclic)),
        RunRecordCorrelation::default(),
    )
    .unwrap();
    assert!(log.record_app(event).is_err());
    let non_finite = RunRecordEvent::new(
        "number",
        Some(RunRecordEventValue::number(f64::NEG_INFINITY)),
        RunRecordCorrelation::default(),
    )
    .unwrap();
    assert!(log.record_app(non_finite).is_err());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);

    log.record_app(RunRecordEvent::new("valid", None, RunRecordCorrelation::default()).unwrap())
        .unwrap();
    log.close().unwrap();
    assert_eq!(lines(&path).len(), 1);
    std::fs::remove_file(path).unwrap();
}
