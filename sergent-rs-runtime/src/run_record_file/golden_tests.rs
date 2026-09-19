//! Byte-exact full-line fixtures for every Run Record file event class.

use std::io;
use std::sync::{Arc, Mutex};

use serde_json::json;
use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::ids::{RunId, SceneId};
use sergent_rs_core::run_record::{
    Cancellation, CancellationCheckpoint, RunOutcome, RunRecord, RunRecordCompletion,
    RunRecordHeader, RunStepEvidence, RunStepRecord, RunTerminal, SceneTransition, SergentResult,
};
use sergent_rs_core::timing::{TimeSpan, Timestamp};
use sergent_rs_core::vocab::{ProgressStatus, RunStepName, RunStepStatus, Stage};

use super::writer::RecordFileWriter;
use super::{JsonlRunRecordWriter, RunRecordCorrelation, RunRecordEvent, RunRecordEventValue};
use crate::observer::RunObserver;
use crate::progress::ProgressSnapshot;

/// An in-memory writer retaining the exact bytes passed by the harness.
struct MemoryWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl RecordFileWriter for MemoryWriter {
    /// Retain every byte and report one complete write.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    /// Memory has no buffered external stream.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Memory has no storage synchronization step.
    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Memory close has no external effect.
    fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Build one harness whose every line receives the fixed golden timestamp.
fn harness() -> (JsonlRunRecordWriter, Arc<Mutex<Vec<u8>>>) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer = MemoryWriter {
        bytes: Arc::clone(&bytes),
    };
    (
        JsonlRunRecordWriter::from_writer_with_clock(Box::new(writer), line_timestamp),
        bytes,
    )
}

/// The deterministic timestamp stamped on every golden line.
fn line_timestamp() -> Timestamp {
    Timestamp::from_unix_micros(1_709_164_801_234_567)
}

/// Read the sole complete line without normalizing its bytes.
fn captured(bytes: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8(bytes.lock().unwrap().clone()).unwrap()
}

#[test]
fn run_start_full_line_is_byte_exact() {
    let (log, bytes) = harness();
    let snapshot = ProgressSnapshot {
        run_id: run_id(0x10),
        scene_id: Some(scene_id(0x20)),
        stage: Stage::Started,
        status: ProgressStatus::Running,
        revision: 6,
    };
    <JsonlRunRecordWriter as RunObserver<()>>::on_progress(&log, &snapshot).unwrap();
    assert_eq!(
        captured(&bytes),
        "{\"activity\":\"sergent_activity\",\"event\":\"run.start\",\"payload\":{\"snapshot\":{\"revision\":6,\"run_id\":\"run_00000000000000000000000000000010\",\"scene_id\":\"doc_00000000000000000000000000000020\",\"stage\":\"started\",\"status\":\"running\"}},\"revision\":6,\"run_id\":\"run_00000000000000000000000000000010\",\"scene_id\":\"doc_00000000000000000000000000000020\",\"schema_version\":\"sergent.run_record.v1\",\"timestamp_utc\":\"2024-02-29T00:00:01.234567Z\"}\n"
    );
}

#[test]
fn successful_run_end_full_line_is_byte_exact() {
    let (log, bytes) = harness();
    let result = success_result();
    <JsonlRunRecordWriter as RunObserver<()>>::on_finished(&log, &result).unwrap();
    assert_eq!(
        captured(&bytes),
        "{\"activity\":\"sergent_activity\",\"event\":\"run.end\",\"payload\":{\"run_record\":{\"cancellation\":null,\"model_name\":\"prov/model\",\"outcome\":{\"error\":null,\"status\":\"success\",\"terminal\":{\"message\":{\"error\":null,\"status\":\"captured\",\"value\":null,\"value_type\":\"MESSAGE_TYPE\"},\"metadata\":{\"error\":null,\"status\":\"captured\",\"value\":{\"resolution\":\"kept\"},\"value_type\":\"METADATA_TYPE\"}}},\"run_id\":\"run_00000000000000000000000000000011\",\"scene\":{\"revision_after\":7,\"revision_before\":7,\"scene_id\":\"doc_00000000000000000000000000000021\"},\"steps\":[{\"error\":null,\"input\":null,\"model_call\":null,\"name\":\"process_input\",\"output\":null,\"status\":\"success\",\"timing\":{\"duration_ms\":1,\"finished_at\":\"1970-01-01T00:00:00.000003Z\",\"started_at\":\"1970-01-01T00:00:00.000002Z\"}},{\"error\":null,\"input\":null,\"model_call\":null,\"name\":\"intent\",\"output\":null,\"status\":\"success\",\"timing\":{\"duration_ms\":1,\"finished_at\":\"1970-01-01T00:00:00.000005Z\",\"started_at\":\"1970-01-01T00:00:00.000004Z\"}}],\"timing\":{\"duration_ms\":8,\"finished_at\":\"1970-01-01T00:00:00.000009Z\",\"started_at\":\"1970-01-01T00:00:00.000001Z\"}}},\"revision\":7,\"run_id\":\"run_00000000000000000000000000000011\",\"scene_id\":\"doc_00000000000000000000000000000021\",\"schema_version\":\"sergent.run_record.v1\",\"timestamp_utc\":\"2024-02-29T00:00:01.234567Z\"}\n"
            .replace("MESSAGE_TYPE", std::any::type_name::<Option<String>>())
            .replace("METADATA_TYPE", std::any::type_name::<serde_json::Map<String, serde_json::Value>>())
    );
}

#[test]
fn failed_run_end_full_line_is_byte_exact() {
    let (log, bytes) = harness();
    let result = failure_result();
    <JsonlRunRecordWriter as RunObserver<()>>::on_finished(&log, &result).unwrap();
    assert_eq!(
        captured(&bytes),
        "{\"activity\":\"sergent_activity\",\"event\":\"run.end\",\"payload\":{\"run_record\":{\"cancellation\":null,\"model_name\":\"prov/model\",\"outcome\":{\"error\":{\"kind\":\"test_failure\",\"message\":\"failed\",\"metadata\":{}},\"status\":\"failure\",\"terminal\":null},\"run_id\":\"run_00000000000000000000000000000012\",\"scene\":{\"revision_after\":null,\"revision_before\":8,\"scene_id\":\"doc_00000000000000000000000000000022\"},\"steps\":[{\"error\":{\"kind\":\"test_failure\",\"message\":\"failed\",\"metadata\":{}},\"input\":null,\"model_call\":null,\"name\":\"process_input\",\"output\":null,\"status\":\"failure\",\"timing\":{\"duration_ms\":1,\"finished_at\":\"1970-01-01T00:00:00.000003Z\",\"started_at\":\"1970-01-01T00:00:00.000002Z\"}}],\"timing\":{\"duration_ms\":8,\"finished_at\":\"1970-01-01T00:00:00.000009Z\",\"started_at\":\"1970-01-01T00:00:00.000001Z\"}}},\"revision\":8,\"run_id\":\"run_00000000000000000000000000000012\",\"scene_id\":\"doc_00000000000000000000000000000022\",\"schema_version\":\"sergent.run_record.v1\",\"timestamp_utc\":\"2024-02-29T00:00:01.234567Z\"}\n"
    );
}

#[test]
fn cancelled_run_end_full_line_is_byte_exact() {
    let (log, bytes) = harness();
    let result = cancelled_result();
    <JsonlRunRecordWriter as RunObserver<()>>::on_finished(&log, &result).unwrap();
    assert_eq!(
        captured(&bytes),
        "{\"activity\":\"sergent_activity\",\"event\":\"run.end\",\"payload\":{\"run_record\":{\"cancellation\":{\"checkpoint\":\"before_intent\",\"requested_at\":\"1970-01-01T00:00:00.000006Z\"},\"model_name\":\"prov/model\",\"outcome\":{\"error\":{\"kind\":\"cancelled\",\"message\":\"stopped\",\"metadata\":{}},\"status\":\"cancelled\",\"terminal\":null},\"run_id\":\"run_00000000000000000000000000000013\",\"scene\":{\"revision_after\":null,\"revision_before\":9,\"scene_id\":\"doc_00000000000000000000000000000023\"},\"steps\":[{\"error\":{\"kind\":\"cancelled\",\"message\":\"stopped\",\"metadata\":{}},\"input\":null,\"model_call\":null,\"name\":\"process_input\",\"output\":null,\"status\":\"cancelled\",\"timing\":{\"duration_ms\":1,\"finished_at\":\"1970-01-01T00:00:00.000003Z\",\"started_at\":\"1970-01-01T00:00:00.000002Z\"}}],\"timing\":{\"duration_ms\":8,\"finished_at\":\"1970-01-01T00:00:00.000009Z\",\"started_at\":\"1970-01-01T00:00:00.000001Z\"}}},\"revision\":9,\"run_id\":\"run_00000000000000000000000000000013\",\"scene_id\":\"doc_00000000000000000000000000000023\",\"schema_version\":\"sergent.run_record.v1\",\"timestamp_utc\":\"2024-02-29T00:00:01.234567Z\"}\n"
    );
}

#[test]
fn app_event_full_line_is_byte_exact() {
    let (log, bytes) = harness();
    log.record_app(
        RunRecordEvent::new(
            "note.\u{4e16}",
            Some(RunRecordEventValue::json(
                json!({ "z": null, "a": "\u{1f600}" }),
            )),
            RunRecordCorrelation::new(
                Some(run_id(0x14).to_string()),
                Some(scene_id(0x24).to_string()),
                Some(4),
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        captured(&bytes),
        "{\"activity\":\"app_activity\",\"event\":\"note.\\u4e16\",\"payload\":{\"a\":\"\\ud83d\\ude00\",\"z\":null},\"revision\":4,\"run_id\":\"run_00000000000000000000000000000014\",\"scene_id\":\"doc_00000000000000000000000000000024\",\"schema_version\":\"sergent.run_record.v1\",\"timestamp_utc\":\"2024-02-29T00:00:01.234567Z\"}\n"
    );
}

#[test]
fn user_event_full_line_is_byte_exact() {
    let (log, bytes) = harness();
    log.record_user(
        RunRecordEvent::new("keypress", None, RunRecordCorrelation::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        captured(&bytes),
        "{\"activity\":\"user_activity\",\"event\":\"keypress\",\"payload\":{},\"schema_version\":\"sergent.run_record.v1\",\"timestamp_utc\":\"2024-02-29T00:00:01.234567Z\"}\n"
    );
}

#[test]
fn direct_events_preserve_json_roots_and_distinguish_absent_payload() {
    let (writer, bytes) = harness();
    let payloads = [
        None,
        Some(json!(null)),
        Some(json!({ "nested": [1, null] })),
        Some(json!([true, { "value": 2 }])),
        Some(json!("\u{1f600}".repeat(4096))),
        Some(json!(12.5)),
        Some(json!(false)),
    ];
    for record in [
        JsonlRunRecordWriter::record_app,
        JsonlRunRecordWriter::record_user,
    ] {
        for payload in &payloads {
            record(
                &writer,
                RunRecordEvent::new(
                    "direct",
                    payload.clone().map(RunRecordEventValue::json),
                    RunRecordCorrelation::default(),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
    writer.close().unwrap();
    let raw = captured(&bytes);
    assert!(raw.is_ascii());
    let rows: Vec<serde_json::Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2 * payloads.len());
    for (rows, activity) in rows
        .chunks(payloads.len())
        .zip(["app_activity", "user_activity"])
    {
        for (row, payload) in rows.iter().zip(&payloads) {
            assert_eq!(row["activity"], activity);
            assert_eq!(
                row.get("payload"),
                Some(&payload.clone().unwrap_or_else(|| json!({})))
            );
        }
    }
}

#[test]
fn persisted_fallback_text_is_bounded_in_both_native_classes() {
    let (writer, bytes) = harness();
    let text = "\u{1f600}".repeat(4096);
    let error = io::Error::other(text.clone());
    let cases = [
        (
            RunRecordEventValue::exception(&error),
            json!({
                "type": std::any::type_name::<io::Error>(),
                "message": "\u{1f600}".repeat(2048),
            }),
        ),
        (
            RunRecordEventValue::degraded(&text),
            json!({
                "type": std::any::type_name::<String>(),
                "repr": format!("\"{}", "\u{1f600}".repeat(2047)),
            }),
        ),
    ];
    for (payload, _) in &cases {
        writer
            .record_app(
                RunRecordEvent::new(
                    "fallback",
                    Some(payload.clone()),
                    RunRecordCorrelation::default(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    writer.close().unwrap();
    let rows: Vec<serde_json::Value> = captured(&bytes)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), cases.len());
    for (row, (_, expected)) in rows.iter().zip(&cases) {
        assert_eq!(&row["payload"], expected);
    }
}

/// Build the fixed metadata-only success used by its complete line fixture.
fn success_result() -> SergentResult<()> {
    let steps = vec![
        step(RunStepName::ProcessInput, RunStepStatus::Success, 2, 3),
        step(RunStepName::Intent, RunStepStatus::Success, 4, 5),
    ];
    let record = record(
        run_id(0x11),
        SceneTransition::unchanged(scene_id(0x21), 7),
        steps,
        RunOutcome::Success {
            terminal: RunTerminal::capture(
                None,
                serde_json::Map::from_iter([("resolution".to_owned(), json!("kept"))]),
            ),
        },
        None,
    );
    SergentResult::new(Stage::Intent, (), record, Vec::new())
}

/// Build the fixed failed result used by its complete line fixture.
fn failure_result() -> SergentResult<()> {
    let error = RunError::new("test_failure", "failed");
    let steps = vec![step(
        RunStepName::ProcessInput,
        RunStepStatus::Failure {
            error: error.clone(),
        },
        2,
        3,
    )];
    let record = record(
        run_id(0x12),
        SceneTransition::uncommitted(scene_id(0x22), 8),
        steps,
        RunOutcome::Failure { error },
        None,
    );
    SergentResult::new(Stage::Started, (), record, Vec::new())
}

/// Build the fixed cancelled result used by its complete line fixture.
fn cancelled_result() -> SergentResult<()> {
    let error = RunError::of(ErrorKind::Cancelled, "stopped");
    let steps = vec![step(
        RunStepName::ProcessInput,
        RunStepStatus::Cancelled {
            error: error.clone(),
        },
        2,
        3,
    )];
    let cancellation = Cancellation::new(
        Timestamp::from_unix_micros(6),
        Some(CancellationCheckpoint::BeforeIntent),
    );
    let record = record(
        run_id(0x13),
        SceneTransition::uncommitted(scene_id(0x23), 9),
        steps,
        RunOutcome::Cancelled { error },
        Some(cancellation),
    );
    SergentResult::new(Stage::Started, (), record, Vec::new())
}

/// Construct one minimal closed step with exact timing and null evidence.
fn step(name: RunStepName, status: RunStepStatus, started: u64, finished: u64) -> RunStepRecord {
    RunStepRecord::new(
        name,
        status,
        TimeSpan::closed(
            Timestamp::from_unix_micros(started),
            Timestamp::from_unix_micros(finished),
            finished - started,
        ),
        RunStepEvidence::new(None, None, None),
    )
}

/// Construct one exact record from a coherent terminal fixture.
fn record(
    run_id: RunId,
    scene: SceneTransition,
    steps: Vec<RunStepRecord>,
    outcome: RunOutcome,
    cancellation: Option<Cancellation>,
) -> RunRecord {
    RunRecord::new(
        RunRecordHeader::new(run_id, "prov/model".to_owned()),
        TimeSpan::closed(
            Timestamp::from_unix_micros(1),
            Timestamp::from_unix_micros(9),
            8,
        ),
        scene,
        steps,
        RunRecordCompletion::new(outcome, cancellation),
    )
}

/// Parse one fixed run identifier whose last byte names the fixture.
fn run_id(suffix: u8) -> RunId {
    RunId::parse(format!("run_{suffix:032x}")).unwrap()
}

/// Parse one fixed Scene identifier whose last byte names the fixture.
fn scene_id(suffix: u8) -> SceneId {
    SceneId::parse(format!("doc_{suffix:032x}")).unwrap()
}
