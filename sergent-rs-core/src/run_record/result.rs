//! The terminal return assembled from a closed run record.

use std::sync::LazyLock;

use serde_json::{Map, Value};

use crate::error::RunError;
use crate::scene::SceneIdentity;
use crate::vocab::{Stage, TerminalStatus};

use super::{RunRecord, coherence};

/// The empty metadata borrowed when the Run Record outcome carries no terminal
/// data, so the metadata accessor stays a plain borrow of one owned fact.
static NO_TERMINAL_METADATA: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);

/// The terminal return from one Run: the final Scene, closed Run Record, last
/// reached stage, terminal message and metadata, and contained observer delivery
/// errors. Terminal status, error, and terminal data come from the Run Record.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug)]
pub struct SergentResult<Scene> {
    stage: Stage,
    scene: Scene,
    run_record: RunRecord,
    observer_errors: Vec<RunError>,
}

impl<Scene> SergentResult<Scene> {
    /// Construct a result from one final Scene and its closed Run Record.
    pub fn new(
        stage: Stage,
        scene: Scene,
        run_record: RunRecord,
        observer_errors: Vec<RunError>,
    ) -> Self {
        coherence::assert_result_stage(&run_record, stage);
        Self {
            stage,
            scene,
            run_record,
            observer_errors,
        }
    }

    /// The terminal stage.
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Borrow the final Scene.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Consume the result and return the final Scene.
    pub fn into_scene(self) -> Scene {
        self.scene
    }

    /// Borrow the closed authoritative Run Record.
    pub fn run_record(&self) -> &RunRecord {
        &self.run_record
    }

    /// Borrow the optional terminal message from the authoritative Run Record.
    pub fn terminal_message(&self) -> Option<&str> {
        match &self.run_record.outcome {
            super::RunOutcome::Success { .. } => self
                .run_record
                .outcome
                .terminal()
                .map(|terminal| terminal.message())
                .and_then(super::CapturedValue::value)
                .and_then(Value::as_str),
            super::RunOutcome::Failure { error } | super::RunOutcome::Cancelled { error } => {
                Some(&error.message)
            }
        }
    }

    /// Borrow the terminal metadata from the authoritative Run Record, empty when
    /// the outcome carries no terminal data.
    pub fn terminal_metadata(&self) -> &Map<String, Value> {
        match &self.run_record.outcome {
            super::RunOutcome::Success { .. } => self
                .run_record
                .outcome
                .terminal()
                .map(|terminal| terminal.metadata())
                .and_then(super::CapturedValue::value)
                .and_then(Value::as_object)
                .unwrap_or(&NO_TERMINAL_METADATA),
            super::RunOutcome::Failure { error } | super::RunOutcome::Cancelled { error } => {
                &error.metadata
            }
        }
    }

    /// Borrow observer delivery failures contained after Run Record closure.
    pub fn observer_errors(&self) -> &[RunError] {
        &self.observer_errors
    }

    /// Consume the result and append one isolated observer delivery failure.
    pub fn with_observer_error(mut self, error: RunError) -> Self {
        self.observer_errors.push(error);
        self
    }

    /// The terminal status from the authoritative Run Record.
    pub fn status(&self) -> TerminalStatus {
        self.run_record.outcome.terminal_status()
    }

    /// The terminal failure from the authoritative Run Record, when present.
    pub fn error(&self) -> Option<&RunError> {
        self.run_record.outcome.error()
    }

    /// The entering or committed Scene identity from the Run Record.
    pub fn identity(&self) -> SceneIdentity {
        let transition = &self.run_record.scene;
        SceneIdentity {
            scene_id: transition.scene_id.clone(),
            revision: transition
                .revision_after
                .unwrap_or(transition.revision_before),
        }
    }
}
