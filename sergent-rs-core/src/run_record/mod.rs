//! The authoritative ordered evidence for one run and the terminal result.
//! @sergent/docs/run-record-spec.md
//!
//! `RunRecord` is inert data. The runtime constructs it and the higher layers
//! decide persistence; the complete Run Record is sensitive forensic data
//! because prompts, model output, and scene projections may be present.

mod captured;
mod coherence;
mod model_call;
mod outcome;
mod patch_summary;
mod record;
mod result;
mod run_facts;
mod step;

pub use captured::CapturedValue;
pub use model_call::{CompletedModelCall, ModelCallPayloads, ModelCallRecord, OpenModelCall};
pub use outcome::RunOutcome;
pub use patch_summary::PatchSummary;
pub use record::{OutputTokenTotal, RunRecord, RunRecordCompletion, RunRecordHeader};
pub use result::SergentResult;
pub use run_facts::{Cancellation, CancellationCheckpoint, RunTerminal, SceneTransition};
pub use step::{RunStepEvidence, RunStepRecord};
