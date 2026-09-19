//! Operation admissibility and application failures.

use serde_json::{Map, Value};

use crate::error::RunError;

/// An admissibility rejection for one Operation. @sergent/docs/framework.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inadmissible {
    /// Why the Operation is not admissible in this context.
    pub reason: String,
}

impl Inadmissible {
    /// Reject an Operation with a reason.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

/// An executable-defense failure raised while applying an Operation.
/// @sergent/docs/framework.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationFault(RunError);

impl OperationFault {
    /// Fault application with its open kind, message, and structured facts.
    pub fn new(
        kind: impl Into<String>,
        message: impl Into<String>,
        metadata: Map<String, Value>,
    ) -> Self {
        let mut error = RunError::new(kind, message);
        error.metadata = metadata;
        Self(error)
    }

    /// Consume the transparent owner and preserve its exact application error.
    pub fn into_run_error(self) -> RunError {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::OperationFault;

    #[test]
    fn consuming_fault_preserves_the_complete_application_error() {
        let mut metadata = Map::new();
        metadata.insert("coordinate".to_owned(), json!({ "row": 2, "column": 4 }));

        let error =
            OperationFault::new("board_rule", "occupied", metadata.clone()).into_run_error();

        assert_eq!(error.kind, "board_rule");
        assert_eq!(error.message, "occupied");
        assert_eq!(error.metadata, metadata);
    }
}
