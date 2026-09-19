//! Intent flow control and stop terminal facts. @sergent/docs/framework.md

use serde::Serialize;
use serde_json::{Map, Value};

/// The control decision a validated Intent owns. Absent flow means continue.
/// @sergent/docs/framework.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentFlow {
    /// Ask for a Plan proposal and continue the run.
    Continue,
    /// End the run with terminal success and no mutation.
    Stop,
}

/// A typed statement of what the Sergent Instance wants to do; it also owns
/// the next control decision. Applications implement this on their Intent types.
/// @sergent/docs/framework.md
pub trait Intent: Serialize {
    /// The control decision this Intent carries; continue by default.
    /// @sergent/docs/framework.md
    fn flow(&self) -> IntentFlow {
        IntentFlow::Continue
    }

    /// An optional terminal message exposed by a stop Intent.
    /// @sergent/docs/framework.md
    fn terminal_message(&self) -> Option<String> {
        None
    }

    /// Bounded terminal metadata exposed by a stop Intent.
    /// @sergent/docs/framework.md
    fn terminal_metadata(&self) -> Map<String, Value> {
        Map::new()
    }
}
