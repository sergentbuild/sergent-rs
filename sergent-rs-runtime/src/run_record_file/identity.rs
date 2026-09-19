//! Validated filename identities and exact harness-owned path construction.

use std::path::{Path, PathBuf};

use sergent_rs_core::timing::Timestamp;

use super::RunRecordFileError;

const MAX_IDENTITY_BYTES: usize = 64;

/// A validated application identity used only in a Run Record filename.
/// @sergent/docs/run-record-file-format.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRecordApplicationName(String);

impl RunRecordApplicationName {
    /// Admit an application name matching the portable filename grammar.
    pub fn parse(value: impl Into<String>) -> Result<Self, RunRecordFileError> {
        let value = value.into();
        validate_identity("application name", &value)?;
        Ok(Self(value))
    }

    /// Borrow the exact admitted name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated log identity for one exclusively created Run Record file.
/// @sergent/docs/run-record-file-format.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRecordFileId(String);

impl RunRecordFileId {
    /// Admit a caller-selected log identity matching the filename grammar.
    pub fn parse(value: impl Into<String>) -> Result<Self, RunRecordFileError> {
        let value = value.into();
        validate_identity("log id", &value)?;
        Ok(Self(value))
    }

    /// Borrow the exact admitted identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Mint the harness default `log_` framework identity.
    pub(crate) fn mint() -> Self {
        Self(format!("log_{}", uuid::Uuid::new_v4().simple()))
    }
}

/// Join validated identities and compact UTC time into the exact filename.
pub(crate) fn path_for(
    directory: &Path,
    application: &RunRecordApplicationName,
    log_id: &RunRecordFileId,
    timestamp: Timestamp,
) -> PathBuf {
    directory.join(format!(
        "{}+{}+{}.jsonl",
        application.as_str(),
        log_id.as_str(),
        timestamp.compact_utc()
    ))
}

/// Enforce the shared portable application-name and log-id grammar.
fn validate_identity(label: &'static str, value: &str) -> Result<(), RunRecordFileError> {
    let mut bytes = value.bytes();
    let valid_first = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric());
    let valid_rest =
        bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'));
    if value.len() <= MAX_IDENTITY_BYTES && valid_first && valid_rest {
        return Ok(());
    }
    Err(RunRecordFileError::validation(format!(
        "{label} must match ^[A-Za-z0-9][A-Za-z0-9_.-]{{0,63}}$"
    )))
}

#[cfg(test)]
mod tests {
    use sergent_rs_core::timing::Timestamp;

    use super::{RunRecordApplicationName, RunRecordFileId, path_for};

    #[test]
    fn identities_and_filename_follow_the_exact_grammar() {
        let app = RunRecordApplicationName::parse("App.one-2").unwrap();
        let id = RunRecordFileId::parse("session_3").unwrap();
        let path = path_for(
            std::path::Path::new("logs"),
            &app,
            &id,
            Timestamp::from_unix_micros(1_709_164_801_234_567),
        );
        assert_eq!(
            path,
            std::path::Path::new("logs/App.one-2+session_3+20240229T000001234567Z.jsonl")
        );
        for invalid in [
            "",
            "_bad",
            "bad/name",
            "bad name",
            "bad+name",
            &"a".repeat(65),
        ] {
            assert!(RunRecordApplicationName::parse(invalid).is_err());
            assert!(RunRecordFileId::parse(invalid).is_err());
        }
    }

    #[test]
    fn minted_log_identity_uses_the_framework_prefix_and_grammar() {
        let id = RunRecordFileId::mint();
        assert!(id.as_str().starts_with("log_"));
        assert_eq!(id.as_str().len(), 36);
        RunRecordFileId::parse(id.as_str()).unwrap();
    }
}
