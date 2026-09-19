//! Run Record Scene, cancellation, and terminal facts.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::ids::SceneId;
use crate::timing::Timestamp;

/// The closed checkpoints where cooperative cancellation may take effect.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationCheckpoint {
    /// Before the Intent phase begins.
    BeforeIntent,
    /// After Intent validation and before its flow gate.
    AfterIntentValidation,
    /// Before deterministic dry-run begins.
    BeforeDryRun,
    /// Immediately before commit authority is entered.
    BeforeCommit,
    /// While a provider future is interrupted by cancellation.
    TaskCancelled,
}

/// Scene identity and revisions entering and leaving a run.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SceneTransition {
    pub(super) scene_id: SceneId,
    pub(super) revision_before: u64,
    pub(super) revision_after: Option<u64>,
}

impl SceneTransition {
    /// Construct a failed or cancelled transition with no committed revision.
    pub fn uncommitted(scene_id: SceneId, revision_before: u64) -> Self {
        Self {
            scene_id,
            revision_before,
            revision_after: None,
        }
    }

    /// Construct a Stop success whose Scene revision remained unchanged.
    pub fn unchanged(scene_id: SceneId, revision: u64) -> Self {
        Self {
            scene_id,
            revision_before: revision,
            revision_after: Some(revision),
        }
    }

    /// Construct a successful commit with its authoritative after revision.
    pub fn committed(scene_id: SceneId, revision_before: u64, revision_after: u64) -> Self {
        Self {
            scene_id,
            revision_before,
            revision_after: Some(revision_after),
        }
    }

    /// Borrow the stable Scene identifier.
    pub fn scene_id(&self) -> &SceneId {
        &self.scene_id
    }

    /// The revision the run observed.
    pub fn revision_before(&self) -> u64 {
        self.revision_before
    }

    /// The terminal revision after the run.
    pub fn revision_after(&self) -> Option<u64> {
        self.revision_after
    }
}

/// Cancellation request facts, present when cancellation was requested.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Cancellation {
    requested_at: Timestamp,
    pub(super) checkpoint: Option<CancellationCheckpoint>,
}

impl Cancellation {
    /// Construct cancellation request evidence.
    pub fn new(requested_at: Timestamp, checkpoint: Option<CancellationCheckpoint>) -> Self {
        Self {
            requested_at,
            checkpoint,
        }
    }

    /// When cancellation was requested.
    pub fn requested_at(&self) -> Timestamp {
        self.requested_at
    }

    /// The checkpoint at which cancellation was observed, when known.
    pub fn checkpoint(&self) -> Option<CancellationCheckpoint> {
        self.checkpoint
    }
}

/// Bounded terminal success data exposed beside the final Scene.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunTerminal {
    message: super::CapturedValue,
    metadata: super::CapturedValue,
}

impl RunTerminal {
    /// Independently capture supplied terminal facts, returning no record when
    /// both application facts are absent.
    pub fn capture(message: Option<String>, metadata: Map<String, Value>) -> Option<Self> {
        (message.is_some() || !metadata.is_empty()).then(|| Self {
            message: super::CapturedValue::capture(&message),
            metadata: super::CapturedValue::capture(&metadata),
        })
    }

    /// Borrow independently captured terminal message evidence.
    pub fn message(&self) -> &super::CapturedValue {
        &self.message
    }

    /// Borrow independently captured terminal metadata evidence.
    pub fn metadata(&self) -> &super::CapturedValue {
        &self.metadata
    }
}

#[cfg(test)]
mod tests {
    use serde::ser::Error as _;
    use serde::{Serialize, Serializer};
    use serde_json::{Value, json};

    use super::RunTerminal;
    use crate::run_record::CapturedValue;

    /// Test value whose serializer deterministically refuses capture.
    struct Failing(&'static str);

    impl Serialize for Failing {
        /// Return the fixture's exact configured capture failure.
        fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(S::Error::custom(self.0))
        }
    }

    #[test]
    fn terminal_capture_failures_do_not_blank_the_neighboring_fact() {
        let failed_message = RunTerminal {
            message: CapturedValue::capture(&Failing("message")),
            metadata: CapturedValue::capture(&json!({ "fact": 1 })),
        };
        let message_value = serde_json::to_value(failed_message).unwrap();
        assert_eq!(message_value["message"]["status"], "capture_error");
        assert_eq!(message_value["metadata"]["value"], json!({ "fact": 1 }));

        let failed_metadata = RunTerminal {
            message: CapturedValue::capture(&"done"),
            metadata: CapturedValue::capture(&Failing("metadata")),
        };
        let metadata_value = serde_json::to_value(failed_metadata).unwrap();
        assert_eq!(metadata_value["message"]["value"], "done");
        assert_eq!(metadata_value["metadata"]["status"], "capture_error");
        assert_ne!(
            metadata_value["metadata"]["value"],
            Value::String("done".to_owned())
        );
    }
}
