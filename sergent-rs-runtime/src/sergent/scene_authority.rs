//! Private per-run Scene authority captured from the public Scene source.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use sergent_rs_core::error::RunError;
use sergent_rs_core::plan::Patch;
use sergent_rs_core::run_record::RunTerminal;
use sergent_rs_core::scene::{SceneActions, SceneIdentity};
use sergent_rs_core::target::Target;

use crate::commit::{CommitOk, commit_live, commit_plain};
use crate::scene_state::{LiveCommitInput, SceneRebase, SceneSource, SceneState, advance_identity};

/// Owns the isolated base Scene and selected commit authority for one run.
pub(super) struct RunScene<S, P> {
    base_scene: S,
    base_identity: SceneIdentity,
    authority: CommitAuthority<S, P>,
}

/// The Scene authority selected by the application for this run.
enum CommitAuthority<S, P> {
    Plain,
    Live(SceneState<S, P>),
}

impl<S, P> RunScene<S, P> {
    /// Captures one isolated base Scene together with its matching commit authority.
    pub(super) fn capture<A>(source: SceneSource<S, P>, actions: &A) -> Self
    where
        A: SceneActions<Scene = S>,
    {
        match source {
            SceneSource::Plain(scene) => {
                let base_scene = actions.clone_scene(&scene);
                let base_identity = actions.identity(&base_scene);
                Self {
                    base_scene,
                    base_identity,
                    authority: CommitAuthority::Plain,
                }
            }
            SceneSource::Live(state) => {
                let (base_scene, base_identity) = state.snapshot(actions);
                Self {
                    base_scene,
                    base_identity,
                    authority: CommitAuthority::Live(state),
                }
            }
        }
    }

    /// Borrows the isolated Scene snapshot observed by the run.
    pub(super) fn base_scene(&self) -> &S {
        &self.base_scene
    }

    /// Borrows the identity paired with the isolated base Scene.
    pub(super) fn base_identity(&self) -> &SceneIdentity {
        &self.base_identity
    }

    /// Consumes the authority and returns the unchanged isolated base Scene.
    pub(super) fn into_base_scene(self) -> S {
        self.base_scene
    }

    /// Derives embedded-identity dry-run enforcement from the selected authority.
    pub(super) fn expected_identity(&self) -> Result<Option<SceneIdentity>, RunError> {
        match &self.authority {
            CommitAuthority::Live(state) if state.is_embedded() => {
                advance_identity(&self.base_identity).map(Some)
            }
            CommitAuthority::Plain | CommitAuthority::Live(_) => Ok(None),
        }
    }

    /// Commits the proven after-Scene through the authority selected at capture.
    pub(super) fn commit<I, T, A>(
        &self,
        patch: &Patch<S, I, T>,
        intent: &I,
        target: &T,
        dry: S,
        actions: &A,
    ) -> Result<CommitOk<S>, RunError>
    where
        T: Target,
        A: SceneActions<Scene = S, Intent = I, Target = T>,
        P: SceneRebase<S, I, T>,
    {
        match &self.authority {
            CommitAuthority::Plain => {
                let revision_after = actions.identity(&dry).revision;
                Ok(commit_plain(dry, revision_after))
            }
            CommitAuthority::Live(state) => commit_live(
                state,
                actions,
                LiveCommitInput {
                    base_scene: &self.base_scene,
                    patch,
                    base_identity: &self.base_identity,
                    intent,
                    target,
                },
                dry,
            ),
        }
    }

    /// Project non-empty successful rebase consequences into terminal data.
    pub(super) fn terminal(&self, committed: &CommitOk<S>) -> Option<RunTerminal> {
        RunTerminal::capture(None, committed.metadata.clone())
    }
}
