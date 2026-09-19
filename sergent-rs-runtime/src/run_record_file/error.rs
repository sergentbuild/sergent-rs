//! Typed validation, conversion, and persistence failures for direct callers.

use std::io;
use std::sync::Arc;

/// The stable class of one Run Record file failure.
/// @sergent/docs/run-record-file-format.md
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunRecordFileErrorKind {
    /// A caller supplied an invalid name or correlation shape.
    Validation,
    /// An event value or envelope could not become JSON.
    Conversion,
    /// Exclusive file creation failed.
    Creation,
    /// An ambiguous write, flush, synchronization, or close operation failed.
    Persistence,
    /// A prior ambiguous failure permanently disabled later I/O.
    Unavailable,
    /// A healthy harness was already closed.
    Closed,
}

/// A visible Run Record file failure with the first persistence cause retained.
/// @sergent/docs/run-record-file-format.md
#[derive(Clone, Debug)]
pub struct RunRecordFileError(Arc<ErrorData>);

#[derive(Debug)]
/// Shared error facts allowing the state and direct caller to retain one cause.
struct ErrorData {
    kind: RunRecordFileErrorKind,
    message: String,
    source: Option<RunRecordFileError>,
    io_source: Option<Arc<io::Error>>,
}

impl RunRecordFileError {
    /// Return the stable failure class without inspecting human prose.
    pub fn kind(&self) -> RunRecordFileErrorKind {
        self.0.kind
    }

    /// Borrow the human-readable failure message.
    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// Build one rejected direct-call shape.
    pub(crate) fn validation(message: impl Into<String>) -> Self {
        Self::new(RunRecordFileErrorKind::Validation, message, None, None)
    }

    /// Build one pre-write value or envelope conversion failure.
    pub(crate) fn conversion(message: impl Into<String>) -> Self {
        Self::new(RunRecordFileErrorKind::Conversion, message, None, None)
    }

    /// Retain an exclusive file or directory creation failure.
    pub(crate) fn creation(path: &std::path::Path, error: io::Error) -> Self {
        Self::new(
            RunRecordFileErrorKind::Creation,
            format!(
                "cannot exclusively create Run Record file {}: {error}",
                path.display()
            ),
            None,
            Some(Arc::new(error)),
        )
    }

    /// Retain the first ambiguous persistence action and native I/O cause.
    pub(crate) fn persistence(action: &'static str, error: io::Error) -> Self {
        Self::new(
            RunRecordFileErrorKind::Persistence,
            format!("Run Record file {action} failed: {error}"),
            None,
            Some(Arc::new(error)),
        )
    }

    /// Chain later unavailability to the first retained persistence failure.
    pub(crate) fn unavailable(first: &Self) -> Self {
        Self::new(
            RunRecordFileErrorKind::Unavailable,
            "Run Record file is unavailable after its first persistence failure",
            Some(first.clone()),
            None,
        )
    }

    /// Report a direct write attempted after a healthy close.
    pub(crate) fn closed() -> Self {
        Self::new(
            RunRecordFileErrorKind::Closed,
            "Run Record file is closed",
            None,
            None,
        )
    }

    /// Assemble one immutable shared error representation.
    fn new(
        kind: RunRecordFileErrorKind,
        message: impl Into<String>,
        source: Option<Self>,
        io_source: Option<Arc<io::Error>>,
    ) -> Self {
        Self(Arc::new(ErrorData {
            kind,
            message: message.into(),
            source,
            io_source,
        }))
    }
}

impl std::fmt::Display for RunRecordFileError {
    /// Render only the human message; control uses the typed kind.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0.message)
    }
}

impl std::error::Error for RunRecordFileError {
    /// Expose the first retained persistence or native I/O cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Some(source) = self.0.source.as_ref() {
            return Some(source);
        }
        self.0
            .io_source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}
