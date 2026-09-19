//! Exact timestamp and cancellation-checkpoint serialization.

use serde_json::{Value, json};
use sergent_rs_core::run_record::{Cancellation, CancellationCheckpoint};
use sergent_rs_core::timing::{TimeSpan, Timestamp};

#[test]
fn timestamps_serialize_as_exact_microsecond_utc_strings() {
    let cases = [
        (0, "1970-01-01T00:00:00.000000Z"),
        (1, "1970-01-01T00:00:00.000001Z"),
        (1_234_567, "1970-01-01T00:00:01.234567Z"),
        (86_400_000_000, "1970-01-02T00:00:00.000000Z"),
        (951_782_400_000_000, "2000-02-29T00:00:00.000000Z"),
        (1_709_164_800_000_000, "2024-02-29T00:00:00.000000Z"),
    ];

    for (microseconds, expected) in cases {
        let timestamp = Timestamp::from_unix_micros(microseconds);
        assert_eq!(serde_json::to_value(timestamp).unwrap(), expected);
        assert_eq!(timestamp.as_unix_micros(), microseconds);
    }
}

#[test]
fn timestamp_supplies_the_exact_compact_utc_form() {
    let timestamp = Timestamp::from_unix_micros(1_709_164_801_234_567);
    assert_eq!(timestamp.compact_utc(), "20240229T000001234567Z");
}

#[test]
fn closed_time_span_preserves_all_declared_fields() {
    let span = TimeSpan::closed(
        Timestamp::from_unix_micros(1),
        Timestamp::from_unix_micros(2),
        0,
    );
    assert_eq!(
        serde_json::to_value(span).unwrap(),
        json!({
            "started_at": "1970-01-01T00:00:00.000001Z",
            "finished_at": "1970-01-01T00:00:00.000002Z",
            "duration_ms": 0
        })
    );
}

#[test]
fn cancellation_checkpoint_uses_the_closed_vocabulary_and_exact_null() {
    let timestamp = Timestamp::from_unix_micros(0);
    let cases = [
        (CancellationCheckpoint::BeforeIntent, "before_intent"),
        (
            CancellationCheckpoint::AfterIntentValidation,
            "after_intent_validation",
        ),
        (CancellationCheckpoint::BeforeDryRun, "before_dry_run"),
        (CancellationCheckpoint::BeforeCommit, "before_commit"),
        (CancellationCheckpoint::TaskCancelled, "task_cancelled"),
    ];
    for (checkpoint, expected) in cases {
        let value = serde_json::to_value(Cancellation::new(timestamp, Some(checkpoint))).unwrap();
        assert_eq!(value["checkpoint"], expected);
    }

    let unknown = serde_json::to_value(Cancellation::new(timestamp, None)).unwrap();
    assert_eq!(unknown["checkpoint"], Value::Null);
    assert!(unknown.get("checkpoint").is_some());
}
