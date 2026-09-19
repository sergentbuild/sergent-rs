//! The MindBuf observation channel supplied to each run.
//! @sergent/docs/framework.md
//!
//! MindBuf is the required observation input carrying recent information the
//! committed Scene cannot hold (recent human actions, outcomes of earlier Runs,
//! and non-blocking errors or rejected attempts). The application curates and
//! compresses it into context-ready text. MindBuf carries facts, never Run
//! Kind or Target selection control.

/// The observation seam: render context-ready out-of-Scene text.
/// @sergent/docs/framework.md
pub trait MindBuf {
    /// Return context-ready observation text without mutation.
    /// @sergent/docs/framework.md
    fn export(&self) -> String;
}
