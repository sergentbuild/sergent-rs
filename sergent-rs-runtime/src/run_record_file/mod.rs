//! The exact opt-in JSONL Run Record harness.
//! @sergent/docs/run-record-file-format.md

mod encoding;
mod error;
mod event;
mod finite;
mod finite_compound;
#[cfg(test)]
mod finite_tests;
#[cfg(test)]
mod golden_tests;
mod harness;
mod identity;
mod writer;

pub use error::{RunRecordFileError, RunRecordFileErrorKind};
pub use event::{
    RunRecordCorrelation, RunRecordEvent, RunRecordEventArray, RunRecordEventObject,
    RunRecordEventValue,
};
pub use harness::JsonlRunRecordWriter;
pub use identity::{RunRecordApplicationName, RunRecordFileId};
