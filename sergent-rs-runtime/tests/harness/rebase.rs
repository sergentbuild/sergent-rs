//! The `SceneRebase` fake: `TestRebaser` either rebinds the same operations onto
//! the current identity or always reports a merge conflict, selected by
//! `RebaseMode`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Map;
use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::plan::Patch;
use sergent_rs_runtime::scene_state::{RebaseContext, RebaseOutcome, SceneRebase};

use super::scene::{Doc, DocIntent, Spot};

#[derive(Clone, Copy)]
pub enum RebaseMode {
    /// Rebind the same operations onto the current identity.
    AcceptSameOps,
    /// Always report a merge conflict.
    Conflict,
}

pub struct TestRebaser {
    pub mode: RebaseMode,
    pub calls: Arc<AtomicUsize>,
}

impl TestRebaser {
    pub fn new(mode: RebaseMode) -> Self {
        Self {
            mode,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl SceneRebase<Doc, DocIntent, Spot> for TestRebaser {
    fn rebase(
        &self,
        context: RebaseContext<'_, Doc, DocIntent, Spot>,
    ) -> RebaseOutcome<Doc, DocIntent, Spot> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            RebaseMode::Conflict => {
                RebaseOutcome::Conflict(RunError::of(ErrorKind::MergeConflict, "cannot merge"))
            }
            RebaseMode::AcceptSameOps => {
                let steps = context
                    .original_patch()
                    .steps()
                    .iter()
                    .map(|step| step.isolated_copy())
                    .collect();
                RebaseOutcome::Rebased {
                    patch: Patch::for_rebase(context.current_identity().clone(), steps),
                    metadata: Map::new(),
                }
            }
        }
    }
}
