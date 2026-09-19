//! The canonical `ProposalSchema` value and its construction-time `SchemaError`
//! vocabulary, with the proposal-name grammar check and the shared dialect
//! error constructor the validators build. @sergent/docs/framework.md

use serde::Serialize;
use serde_json::Value;

/// The one canonical structural representation carried by a model-backed
/// request. Its name is the proposal type name (1-64 ASCII characters of
/// `[A-Za-z0-9_-]`). @sergent/docs/framework.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalSchema {
    name: String,
    json_schema: Value,
}

impl ProposalSchema {
    /// Borrow the stable proposal name used by provider schema facilities.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrow the proved canonical JSON Schema document.
    pub fn json_schema(&self) -> &Value {
        &self.json_schema
    }

    /// Assemble parts whose name grammar and canonical dialect have already
    /// been proved by their framework owner.
    pub(crate) fn from_proved_parts(name: String, json_schema: Value) -> Self {
        Self { name, json_schema }
    }
}

/// A construction-time schema failure. @sergent/docs/framework.md
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    /// The proposal name is not 1-64 characters of `[A-Za-z0-9_-]`.
    #[error("proposal schema name {name:?} must be 1-64 chars of [A-Za-z0-9_-]")]
    InvalidName {
        /// The rejected name.
        name: String,
    },
    /// A derivation input the dialect cannot express, located by proposal
    /// name and JSON pointer.
    #[error("proposal {proposal:?} schema at {pointer:?}: {message}")]
    Dialect {
        /// The proposal name.
        proposal: String,
        /// The JSON pointer to the offending node.
        pointer: String,
        /// What is out of dialect.
        message: String,
    },
}

/// Return whether a proposal name is 1-64 ASCII alphanumeric, underscore, or
/// hyphen characters.
pub(super) fn name_is_valid(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Build a dialect error carrying copied proposal and JSON-pointer context.
pub(super) fn dialect(proposal: &str, pointer: &str, message: impl Into<String>) -> SchemaError {
    SchemaError::Dialect {
        proposal: proposal.to_owned(),
        pointer: pointer.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::name_is_valid;

    #[test]
    fn proposal_name_grammar_is_closed() {
        assert!(name_is_valid("Fine_Name-1"));
        assert!(!name_is_valid("has space"));
        assert!(!name_is_valid(""));
        assert!(!name_is_valid(&"x".repeat(65)));
    }
}
