//! The single per-run reached-evidence and RunRecord assembly owner.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::Map;

use sergent_rs_core::error::RunError;
use sergent_rs_core::ids::SceneId;
use sergent_rs_core::plan::{ExecutionPlan, Patch};
use sergent_rs_core::run_record::{
    Cancellation, CapturedValue, ModelCallRecord, PatchSummary, RunOutcome, RunRecord,
    RunRecordCompletion, RunRecordHeader, RunStepEvidence, RunStepRecord, SceneTransition,
};
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::target::Target;
use sergent_rs_core::vocab::{RunStepName, RunStepStatus};

use crate::clock::{ClockSample, span_closed};

/// ProcessInput's required rendered Observation.
#[derive(Serialize)]
pub(crate) struct ProcessInputEvidence<'a> {
    pub(crate) observation: &'a str,
}

/// ProcessInput's sole Target capture location.
#[derive(Serialize)]
pub(crate) struct ProcessOutputEvidence<'a, T> {
    pub(crate) selected_target: Option<&'a T>,
}

/// Intent facts reached after derivation.
#[derive(Serialize)]
pub(crate) struct IntentOutputEvidence<'a, I> {
    pub(crate) derived_intent: &'a I,
    pub(crate) flow: sergent_rs_core::intent::IntentFlow,
}

/// ExecutionPlan facts reached after Recipe derivation.
pub(crate) struct ExecutionPlanOutputEvidence<'a, S, I, T: Target> {
    pub(crate) derived_execution_plan: &'a ExecutionPlan<S, I, T>,
}

impl<S, I, T: Target> Serialize for ExecutionPlanOutputEvidence<'_, S, I, T> {
    /// Project only the exact derived ExecutionPlan fact.
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        let mut output = serializer.serialize_struct("ExecutionPlanOutputEvidence", 1)?;
        output.serialize_field("derived_execution_plan", self.derived_execution_plan)?;
        output.end()
    }
}

/// One failed Patch envelope marker.
#[derive(Serialize)]
struct PatchValidationEvidence {
    status: &'static str,
}

/// One successful dry-run Scene identity.
#[derive(Serialize)]
struct DryRunEvidence {
    after_identity: SceneIdentity,
}

/// Patch facts accumulated without reopening a closed step.
#[derive(Serialize)]
pub(crate) struct PatchEvidence {
    compiled_patch: PatchSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    patch_validation: Option<PatchValidationEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dry_run: Option<DryRunEvidence>,
}

impl PatchEvidence {
    /// Capture the original compiled Patch exactly once.
    pub(crate) fn of<S, I, T: Target>(patch: &Patch<S, I, T>) -> Self {
        Self {
            compiled_patch: PatchSummary::from_patch(patch),
            patch_validation: None,
            dry_run: None,
        }
    }

    /// Add the reached failed envelope marker before closing the Patch step.
    pub(crate) fn mark_validation_failure(&mut self) {
        self.patch_validation = Some(PatchValidationEvidence { status: "failure" });
    }

    /// Add the successful isolated after-Scene identity.
    pub(crate) fn mark_dry_run(&mut self, after_identity: SceneIdentity) {
        self.dry_run = Some(DryRunEvidence { after_identity });
    }

    /// Capture the complete set of Patch facts reached so far.
    fn captured_output(&self) -> CapturedValue {
        CapturedValue::capture(self)
    }
}

/// Commit facts supplied by plain or shared Scene authority.
#[derive(Serialize)]
pub(crate) struct CommitOutputEvidence<'a> {
    pub(crate) commit_kind: &'a str,
    pub(crate) metadata: &'a Map<String, serde_json::Value>,
}

/// One opened step that owns Run Record construction until exactly one close transition.
pub(crate) struct OpenStep {
    record_builder: RunRecordBuilder,
    name: RunStepName,
    started_at: ClockSample,
    input: Option<CapturedValue>,
    output: Option<CapturedValue>,
    model_call: Option<ModelCallRecord>,
}

/// Commit step opened after the Patch step is already closed.
pub(crate) struct OpenCommitStep {
    step: OpenStep,
}

/// Tracks authoritative Scene identity while the Run Record is open.
struct SceneFacts {
    scene_id: SceneId,
    revision_before: u64,
    revision_after: Option<u64>,
}

/// Builds one Run Record, keeping open evidence outside its immutable Step Record sequence.
pub(crate) struct RunRecordBuilder {
    header: RunRecordHeader,
    started_at: ClockSample,
    scene: SceneFacts,
    steps: Vec<RunStepRecord>,
    cancellation: Option<Cancellation>,
}

impl RunRecordBuilder {
    /// Open one Run Record with no fabricated after revision.
    pub(crate) fn new(
        header: RunRecordHeader,
        started_at: ClockSample,
        identity: &SceneIdentity,
    ) -> Self {
        Self {
            header,
            started_at,
            scene: SceneFacts {
                scene_id: identity.scene_id.clone(),
                revision_before: identity.revision,
                revision_after: None,
            },
            steps: Vec::new(),
            cancellation: None,
        }
    }

    /// Record the authoritative revision reached by Stop or commit.
    pub(crate) fn commit_scene(&mut self, revision_after: u64) {
        self.scene.revision_after = Some(revision_after);
    }

    /// Transfer Run Record construction into one timed reached step.
    pub(crate) fn open_step(self, name: RunStepName, at: ClockSample) -> OpenStep {
        OpenStep {
            record_builder: self,
            name,
            started_at: at,
            input: None,
            output: None,
            model_call: None,
        }
    }

    /// Open Commit after the Patch step has already closed.
    pub(crate) fn open_commit(self, at: ClockSample) -> OpenCommitStep {
        OpenCommitStep {
            step: self.open_step(RunStepName::Commit, at),
        }
    }

    /// Attach first-request time and the observing cancellation checkpoint.
    pub(crate) fn set_cancellation(&mut self, cancellation: Cancellation) {
        self.cancellation = Some(cancellation);
    }

    /// Close the run into its exact inert record.
    pub(crate) fn into_record(self, outcome: RunOutcome, finished_at: ClockSample) -> RunRecord {
        let transition = match self.scene.revision_after {
            None => SceneTransition::uncommitted(self.scene.scene_id, self.scene.revision_before),
            Some(after) if after == self.scene.revision_before => {
                SceneTransition::unchanged(self.scene.scene_id, after)
            }
            Some(after) => {
                SceneTransition::committed(self.scene.scene_id, self.scene.revision_before, after)
            }
        };
        RunRecord::new(
            self.header,
            span_closed(self.started_at, finished_at),
            transition,
            self.steps,
            RunRecordCompletion::new(outcome, self.cancellation),
        )
    }
}

impl OpenStep {
    /// Capture exact step input from one reached typed fact owner.
    pub(crate) fn capture_input<T: Serialize + ?Sized>(&mut self, input: &T) {
        self.input = Some(CapturedValue::capture(input));
    }

    /// Replace merged output capture with the complete facts reached so far.
    pub(crate) fn capture_output<T: Serialize + ?Sized>(&mut self, output: &T) {
        self.output = Some(CapturedValue::capture(output));
    }

    /// Attach one request or completed model-call record.
    pub(crate) fn attach_model_call(&mut self, call: ModelCallRecord) {
        self.model_call = Some(call);
    }

    /// Close a successfully completed step.
    pub(crate) fn close_success(self, finished_at: ClockSample) -> RunRecordBuilder {
        self.close(RunStepStatus::Success, finished_at)
    }

    /// Close the final reached step with its owning failure.
    pub(crate) fn close_failure(
        self,
        error: RunError,
        finished_at: ClockSample,
    ) -> RunRecordBuilder {
        self.close(RunStepStatus::Failure { error }, finished_at)
    }

    /// Close the final reached step with its cancellation error.
    pub(crate) fn close_cancelled(
        self,
        error: RunError,
        finished_at: ClockSample,
    ) -> RunRecordBuilder {
        self.close(RunStepStatus::Cancelled { error }, finished_at)
    }

    /// Close a successful Patch step after adding dry-run identity.
    pub(crate) fn close_patch_success(
        mut self,
        evidence: &PatchEvidence,
        finished_at: ClockSample,
    ) -> RunRecordBuilder {
        self.output = Some(evidence.captured_output());
        self.close_success(finished_at)
    }

    /// Materialize one exact closed step.
    fn close(mut self, status: RunStepStatus, finished_at: ClockSample) -> RunRecordBuilder {
        let record = RunStepRecord::new(
            self.name,
            status,
            span_closed(self.started_at, finished_at),
            RunStepEvidence::new(self.input, self.output, self.model_call),
        );
        self.record_builder.steps.push(record);
        self.record_builder
    }
}

impl OpenCommitStep {
    /// Capture commit kind and exact Scene-supplied metadata.
    pub(crate) fn capture_output(
        &mut self,
        commit_kind: &str,
        metadata: &Map<String, serde_json::Value>,
    ) {
        self.step.capture_output(&CommitOutputEvidence {
            commit_kind,
            metadata,
        });
    }

    /// Close successful Commit and record its authoritative after revision.
    pub(crate) fn close_success(
        self,
        revision_after: u64,
        finished_at: ClockSample,
    ) -> RunRecordBuilder {
        let mut record_builder = self.step.close_success(finished_at);
        record_builder.commit_scene(revision_after);
        record_builder
    }

    /// Close failed Commit with no after revision.
    pub(crate) fn close_failure(
        self,
        error: RunError,
        finished_at: ClockSample,
    ) -> RunRecordBuilder {
        self.step.close_failure(error, finished_at)
    }

    /// Close cancelled Commit with no after revision.
    pub(crate) fn close_cancelled(
        self,
        error: RunError,
        finished_at: ClockSample,
    ) -> RunRecordBuilder {
        self.step.close_cancelled(error, finished_at)
    }
}
