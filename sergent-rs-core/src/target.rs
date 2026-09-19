//! The bounded Target selected for one run. @sergent/docs/framework.md
//!
//! A `Target` is the sole owner of its selected identity and any app-owned
//! execution context (for example mode data beside the target id). The
//! framework carries a Target opaquely through apply, admissibility, and the
//! Recipe hooks; the one fact it reads is the selected identity, so a run can
//! surface it in a progress snapshot.

use serde::Serialize;

use crate::ids::TargetId;

/// The selected-target seam: an app-owned value whose selected identity the
/// framework can read. @sergent/docs/framework.md
pub trait Target: Serialize {
    /// The stable identity of this selected target. @sergent/docs/framework.md
    fn target_id(&self) -> &TargetId;
}
