//! The structured failure value carried across a run. @sergent/docs/execution-model.md

use serde::Serialize;
use serde_json::{Map, Value};
use std::fmt::{self, Write};

use crate::ids::{OperationId, SceneId};
use crate::run_record::PatchSummary;
use crate::scene::SceneIdentity;
use crate::vocab::Stage;

const MODEL_OUTPUT_DIAGNOSTIC_MAX_CHARS: usize = 256;
const OBSERVER_ERROR_MAX_CHARS: usize = 2_048;
const TRUNCATION_MARKER: &str = "...";

/// Escape controls and bound one model-derived diagnostic fragment before it
/// enters a human-readable failure. Full raw output belongs in sensitive call
/// evidence, not in this projection.
pub fn sanitize_model_output_diagnostic(value: &str) -> String {
    let mut sanitizer = DiagnosticSanitizer::new();
    let _ = sanitizer.write_str(value);
    sanitizer.finish()
}

/// Format one model-derived diagnostic directly into the bounded projection.
/// This is public for framework layers that own a model-output crossing; the
/// battery crate deliberately does not expose it to applications.
pub fn sanitize_model_output_display(value: &impl fmt::Display) -> String {
    let mut sanitizer = DiagnosticSanitizer::new();
    let _ = write!(&mut sanitizer, "{value}");
    sanitizer.finish()
}

/// A bounded escaping writer that stops its producer after the first omitted
/// character proves truncation.
struct DiagnosticSanitizer {
    output: String,
    characters: usize,
    marker_byte_index: usize,
}

impl DiagnosticSanitizer {
    /// Start an empty diagnostic projection with only bounded retained space.
    fn new() -> Self {
        Self {
            output: String::with_capacity(MODEL_OUTPUT_DIAGNOSTIC_MAX_CHARS),
            characters: 0,
            marker_byte_index: 0,
        }
    }

    /// Retain one escaped character or stop at the first truncation witness.
    fn push(&mut self, character: char) -> fmt::Result {
        if self.characters == MODEL_OUTPUT_DIAGNOSTIC_MAX_CHARS {
            self.output.truncate(self.marker_byte_index);
            self.output.push_str(TRUNCATION_MARKER);
            return Err(fmt::Error);
        }
        if self.characters == MODEL_OUTPUT_DIAGNOSTIC_MAX_CHARS - TRUNCATION_MARKER.len() {
            self.marker_byte_index = self.output.len();
        }
        self.output.push(character);
        self.characters += 1;
        Ok(())
    }

    /// Return the completed exact or marker-terminated projection.
    fn finish(self) -> String {
        self.output
    }
}

impl fmt::Write for DiagnosticSanitizer {
    /// Escape one producer fragment directly into the bounded projection.
    fn write_str(&mut self, value: &str) -> fmt::Result {
        for character in value.chars() {
            if character.is_control() {
                for escaped in character.escape_default() {
                    self.push(escaped)?;
                }
            } else {
                self.push(character)?;
            }
        }
        Ok(())
    }
}

/// Convenience vocabulary for reserved framework kinds and baseline provider
/// kinds. `RunError` and `ModelError` accept open kind text beyond this set.
/// @sergent/docs/observability.md
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// An application semantic validator rejected Intent or a plan.
    ValidationError,
    /// A stale patch could not be deterministically rebased.
    MergeConflict,
    /// The run was cancelled before the commit boundary.
    Cancelled,
    /// The typed crossing of model output failed against the registered shape.
    SchemaValidationFailed,
    /// A shared-live-state commit met an advanced revision.
    StalePatch,
    /// Externally owned shared Scene revision authority cannot advance past
    /// `u64::MAX`.
    RevisionExhausted,
    /// Patch envelope validation rejected the compiled patch.
    PatchValidation,
    /// A best-effort Run Record capture of an application value failed.
    CaptureError,
    /// A terminal observer callback failed after the Run Record closed.
    ObserverError,
    /// The provider call exceeded its timeout.
    Timeout,
    /// The provider endpoint was unreachable.
    ProviderUnavailable,
    /// The provider rate limited the request.
    RateLimited,
    /// The request payload was rejected by the provider as malformed.
    InvalidPayload,
    /// The named model does not exist at the provider.
    ModelNotFound,
    /// The provider returned an otherwise-unclassified error.
    ProviderError,
    /// The provider response could not be parsed into one JSON object.
    InvalidResponse,
    /// The caller-supplied model name was not a valid `provider/model` value.
    InvalidModelName,
    /// The `provider` portion of the model name has no registered adapter.
    UnknownProvider,
    /// Required provider credentials were absent.
    MissingCredentials,
}

impl ErrorKind {
    /// The exact Run Record serialized string for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::ValidationError => "validation_error",
            ErrorKind::MergeConflict => "merge_conflict",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::SchemaValidationFailed => "schema_validation_failed",
            ErrorKind::StalePatch => "stale_patch",
            ErrorKind::RevisionExhausted => "revision_exhausted",
            ErrorKind::PatchValidation => "patch_validation",
            ErrorKind::CaptureError => "capture_error",
            ErrorKind::ObserverError => "observer_error",
            ErrorKind::Timeout => "timeout",
            ErrorKind::ProviderUnavailable => "provider_unavailable",
            ErrorKind::RateLimited => "rate_limited",
            ErrorKind::InvalidPayload => "invalid_payload",
            ErrorKind::ModelNotFound => "model_not_found",
            ErrorKind::ProviderError => "provider_error",
            ErrorKind::InvalidResponse => "invalid_response",
            ErrorKind::InvalidModelName => "invalid_model_name",
            ErrorKind::UnknownProvider => "unknown_provider",
            ErrorKind::MissingCredentials => "missing_credentials",
        }
    }
}

/// A bounded structured failure that crosses the runtime without raising.
/// @sergent/docs/execution-model.md
///
/// `kind` is an open string so applications may record their own kinds; the
/// framework and providers use the closed `ErrorKind` strings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunError {
    /// The failure kind string.
    pub kind: String,
    /// A human-readable, sanitized message.
    pub message: String,
    /// Structured locator or context data, for example the failing Operation
    /// index, `call` discriminator, and authoritative operation ID.
    pub metadata: Map<String, Value>,
}

impl RunError {
    /// Build a failure from an open application or transport kind.
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            metadata: Map::new(),
        }
    }

    /// Build a failure from a framework error kind and message.
    pub fn of(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self::new(kind.as_str(), message)
    }

    /// Build the cancellation outcome error with its required empty metadata.
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Cancelled, message)
    }

    /// Build an initial Operation admissibility rejection.
    pub fn admissibility(
        message: impl Into<String>,
        index: usize,
        call: impl Into<String>,
        operation_id: &OperationId,
    ) -> Self {
        admissibility_error(
            ErrorKind::ValidationError.as_str(),
            message,
            index,
            call,
            operation_id,
        )
    }

    /// Build renewed Operation admissibility evidence for a stale rebase.
    pub fn operation_admissibility(
        message: impl Into<String>,
        index: usize,
        call: impl Into<String>,
        operation_id: &OperationId,
    ) -> Self {
        admissibility_error(
            "operation_admissibility",
            message,
            index,
            call,
            operation_id,
        )
    }

    /// Build a dry-run verification rejection.
    pub fn verification(
        message: impl Into<String>,
        issues: impl IntoIterator<Item = String>,
    ) -> Self {
        Self::of(ErrorKind::ValidationError, message)
            .with("verification_issues", string_array(issues))
    }

    /// Build a Patch envelope rejection with required empty metadata.
    pub fn patch_validation(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::PatchValidation, message)
    }

    /// Build an embedded Scene-identity Patch rejection.
    pub fn embedded_identity(
        message: impl Into<String>,
        issues: impl IntoIterator<Item = String>,
        expected: &SceneIdentity,
        actual: &SceneIdentity,
    ) -> Self {
        Self::of(ErrorKind::PatchValidation, message)
            .with("identity_issues", string_array(issues))
            .with("expected_identity", exact_value(expected))
            .with("actual_identity", exact_value(actual))
    }

    /// Build a strict stale-Patch rejection.
    pub fn stale_patch(
        message: impl Into<String>,
        base_revision: u64,
        current_revision: u64,
    ) -> Self {
        Self::of(ErrorKind::StalePatch, message)
            .with("base_revision", base_revision)
            .with("current_revision", current_revision)
    }

    /// Build a checked Scene-revision exhaustion failure.
    pub fn revision_exhausted(
        message: impl Into<String>,
        scene_id: &SceneId,
        revision: u64,
    ) -> Self {
        Self::of(ErrorKind::RevisionExhausted, message)
            .with("scene_id", scene_id.as_str())
            .with("revision", revision)
    }

    /// Build a Scene-declared merge conflict around the selected rejected Patch.
    /// Scene facts remain isolated under `scene_metadata`.
    /// @sergent/docs/run-record-spec.md
    pub fn merge_conflict(
        message: impl Into<String>,
        base_revision: u64,
        current_live_revision: u64,
        patch: &PatchSummary,
        scene_metadata: Map<String, Value>,
    ) -> Self {
        let mut error = Self::of(ErrorKind::MergeConflict, message);
        error.metadata =
            merge_required_metadata(scene_metadata, base_revision, current_live_revision, patch);
        error
    }

    /// Build a rejected-rebase merge conflict with exact validation evidence.
    /// @sergent/docs/run-record-spec.md
    pub fn rebased_merge_conflict(
        base_revision: u64,
        current_live_revision: u64,
        patch: &PatchSummary,
        scene_metadata: Map<String, Value>,
        validation_error: RunError,
    ) -> Self {
        let mut error = Self::merge_conflict(
            "rebased Patch failed deterministic rehearsal",
            base_revision,
            current_live_revision,
            patch,
            scene_metadata,
        );
        error.metadata.insert(
            "validation_error".to_owned(),
            exact_value(&validation_error),
        );
        error
    }

    /// Build a contained observer callback failure with exact bounded facts.
    pub fn observer_error(
        message: impl Into<String>,
        callback: impl Into<String>,
        observer_type: impl Into<String>,
        exception_type: impl Into<String>,
        stage: Stage,
    ) -> Self {
        Self::of(
            ErrorKind::ObserverError,
            truncate_chars(message.into(), OBSERVER_ERROR_MAX_CHARS),
        )
        .with("callback", callback.into())
        .with("observer_type", observer_type.into())
        .with("exception_type", exception_type.into())
        .with("stage", exact_value(&stage))
    }

    /// Attach one structured metadata entry.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Project one locally trusted typed value into exact structured metadata.
fn exact_value(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("framework metadata value is serializable")
}

/// Build either initial or renewed admissibility evidence from one owner.
fn admissibility_error(
    kind: impl Into<String>,
    message: impl Into<String>,
    index: usize,
    call: impl Into<String>,
    operation_id: &OperationId,
) -> RunError {
    RunError::new(kind, message)
        .with("index", index as u64)
        .with("call", call.into())
        .with("operation_id", operation_id.as_str())
}

/// Build the isolated Scene and framework namespaces of a merge conflict.
fn merge_required_metadata(
    scene_metadata: Map<String, Value>,
    base_revision: u64,
    current_live_revision: u64,
    patch: &PatchSummary,
) -> Map<String, Value> {
    let mut metadata = Map::new();
    metadata.insert("base_revision".to_owned(), Value::from(base_revision));
    metadata.insert(
        "current_live_revision".to_owned(),
        Value::from(current_live_revision),
    );
    metadata.insert("patch".to_owned(), exact_value(patch));
    metadata.insert("scene_metadata".to_owned(), Value::Object(scene_metadata));
    metadata
}

/// Project owned diagnostic strings into one exact JSON array.
fn string_array(values: impl IntoIterator<Item = String>) -> Value {
    Value::Array(values.into_iter().map(Value::String).collect())
}

/// Bound human prose by Unicode scalar count without changing exact short text.
fn truncate_chars(value: String, maximum: usize) -> String {
    if value.chars().count() <= maximum {
        value
    } else {
        value.chars().take(maximum).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fmt;

    use super::{
        MODEL_OUTPUT_DIAGNOSTIC_MAX_CHARS, RunError, sanitize_model_output_diagnostic,
        sanitize_model_output_display,
    };

    /// A hostile formatter that observes when bounded output stops it.
    struct LongControlDisplay<'a> {
        writes: &'a Cell<usize>,
    }

    impl fmt::Display for LongControlDisplay<'_> {
        /// Produce control characters until the destination refuses more work.
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            for _ in 0..10_000 {
                self.writes.set(self.writes.get() + 1);
                formatter.write_str("\u{1b}")?;
            }
            Ok(())
        }
    }

    #[test]
    fn diagnostic_truncation_retains_the_exact_character_bound() {
        let exact = "x".repeat(MODEL_OUTPUT_DIAGNOSTIC_MAX_CHARS);
        assert_eq!(sanitize_model_output_diagnostic(&exact), exact);

        let truncated = sanitize_model_output_diagnostic(&format!("{exact}x"));
        assert_eq!(truncated.chars().count(), MODEL_OUTPUT_DIAGNOSTIC_MAX_CHARS);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn display_sanitization_stops_a_control_heavy_producer() {
        let writes = Cell::new(0);
        let output = sanitize_model_output_display(&LongControlDisplay { writes: &writes });

        assert_eq!(output.chars().count(), MODEL_OUTPUT_DIAGNOSTIC_MAX_CHARS);
        assert!(output.ends_with("..."));
        assert!(!output.chars().any(char::is_control));
        assert!(writes.get() < 100);
    }

    #[test]
    fn open_kind_and_empty_metadata_serialize_exactly() {
        let error = RunError::new("application_owned", "rejected");

        assert_eq!(
            serde_json::to_value(error).expect("RunError should serialize"),
            serde_json::json!({
                "kind": "application_owned",
                "message": "rejected",
                "metadata": {},
            })
        );
    }
}
