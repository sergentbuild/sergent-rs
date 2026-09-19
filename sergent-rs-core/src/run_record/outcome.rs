//! Coherent terminal run verdicts and status projection.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::error::RunError;
use crate::vocab::TerminalStatus;

use super::RunTerminal;

/// One coherent terminal run verdict. Success may carry terminal data, while
/// failure and cancellation own their required error.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// The run succeeded, optionally with terminal data.
    Success {
        /// Terminal success data, when the run exposes any.
        terminal: Option<RunTerminal>,
    },
    /// The run failed with a contained error.
    Failure {
        /// The failure.
        error: RunError,
    },
    /// The run was cancelled before commit.
    Cancelled {
        /// The cancellation-shaped failure.
        error: RunError,
    },
}

impl Serialize for RunOutcome {
    /// Serialize the exact three-field record while variants preserve coherent
    /// status, error, and terminal combinations in memory.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut outcome = serializer.serialize_struct("RunOutcome", 3)?;
        outcome.serialize_field(
            "status",
            match self {
                Self::Success { .. } => "success",
                Self::Failure { .. } => "failure",
                Self::Cancelled { .. } => "cancelled",
            },
        )?;
        outcome.serialize_field("error", &self.error())?;
        outcome.serialize_field("terminal", &self.terminal())?;
        outcome.end()
    }
}

impl RunOutcome {
    /// The terminal status.
    pub fn terminal_status(&self) -> TerminalStatus {
        match self {
            RunOutcome::Success { .. } => TerminalStatus::Success,
            RunOutcome::Failure { .. } => TerminalStatus::Failure,
            RunOutcome::Cancelled { .. } => TerminalStatus::Cancelled,
        }
    }

    /// The terminal failure, when the run failed or was cancelled.
    pub fn error(&self) -> Option<&RunError> {
        match self {
            RunOutcome::Failure { error } | RunOutcome::Cancelled { error } => Some(error),
            RunOutcome::Success { .. } => None,
        }
    }

    /// Terminal success data, when this success exposes any.
    pub fn terminal(&self) -> Option<&RunTerminal> {
        match self {
            RunOutcome::Success { terminal } => terminal.as_ref(),
            RunOutcome::Failure { .. } | RunOutcome::Cancelled { .. } => None,
        }
    }
}
