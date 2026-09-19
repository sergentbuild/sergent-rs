//! Scene authority: the plain-snapshot source and the shared live `SceneState`
//! with its runtime-owned deterministic rebase seam.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! A plain scene is a snapshot input for one run; commit applies to the
//! isolated snapshot with no current-state comparison. A `SceneState`
//! is the shared live authority: it holds the current scene and an advancing
//! identity behind a lock and applies strict revision equality by default.
//! The lock is a synchronous `std::sync::Mutex`; the deterministic
//! commit tail never awaits, so the lock is never held across a model await.

use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Map, Value};
use sergent_rs_core::error::RunError;
use sergent_rs_core::plan::Patch;
use sergent_rs_core::scene::{SceneActions, SceneIdentity, identity_transition_issues};
use sergent_rs_core::target::Target;

/// The deterministic outcome of an application-owned patch rebase.
/// @sergent/docs/framework.md
pub enum RebaseOutcome<S, I, T: Target> {
    /// A deterministically merged Patch and its Scene-supplied consequences.
    Rebased {
        /// The replacement Patch bound to current Scene authority.
        patch: Patch<S, I, T>,
        /// Exact application metadata for a successful rebased commit.
        metadata: Map<String, Value>,
    },
    /// The overlap could not be merged; a structured merge conflict.
    Conflict(RunError),
}

/// The already validated original run facts borrowed by shared commit authority.
pub(crate) struct LiveCommitInput<'a, S, I, T: Target> {
    pub(crate) base_scene: &'a S,
    pub(crate) patch: &'a Patch<S, I, T>,
    pub(crate) base_identity: &'a SceneIdentity,
    pub(crate) intent: &'a I,
    pub(crate) target: &'a T,
}

/// The immutable inputs to one stale shared-state rebase decision.
///
/// The context preserves the run's isolated base Scene, the current live
/// Scene and identity, the original validated Patch (including its base
/// identity), the original validated Intent, and the exact Target selected at
/// run start. @sergent/docs/framework.md
pub struct RebaseContext<'a, S, I, T: Target> {
    base_scene: &'a S,
    current_scene: &'a S,
    current_identity: &'a SceneIdentity,
    original_patch: &'a Patch<S, I, T>,
    original_intent: &'a I,
    original_target: &'a T,
}

impl<S, I, T: Target> Copy for RebaseContext<'_, S, I, T> {}

impl<S, I, T: Target> Clone for RebaseContext<'_, S, I, T> {
    /// Copy the same borrowed authority facts without cloning their values.
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, S, I, T: Target> RebaseContext<'a, S, I, T> {
    /// Binds original run facts and current live authority for one stale rebase decision.
    pub(crate) fn new(original: &LiveCommitInput<'a, S, I, T>, current: &'a LiveInner<S>) -> Self {
        Self {
            base_scene: original.base_scene,
            current_scene: &current.scene,
            current_identity: &current.identity,
            original_patch: original.patch,
            original_intent: original.intent,
            original_target: original.target,
        }
    }

    /// Borrow the isolated Scene snapshot the run originally observed.
    pub fn base_scene(&self) -> &'a S {
        self.base_scene
    }

    /// Borrow the current authoritative Scene under the commit lock.
    pub fn current_scene(&self) -> &'a S {
        self.current_scene
    }

    /// Borrow the current authoritative Scene identity.
    pub fn current_identity(&self) -> &'a SceneIdentity {
        self.current_identity
    }

    /// Borrow the original validated Patch, including its base identity.
    pub fn original_patch(&self) -> &'a Patch<S, I, T> {
        self.original_patch
    }

    /// Borrow the original validated Intent.
    pub fn original_intent(&self) -> &'a I {
        self.original_intent
    }

    /// Borrow the exact Target selected once at run start.
    pub fn original_target(&self) -> &'a T {
        self.original_target
    }
}

/// A scene-owned deterministic patch rebase, the explicit alternative to strict
/// revision equality on shared live state. The runtime still owns
/// validation, dry-run, and revision-checked commit; the scene owns only the
/// merge decision. @sergent/docs/framework.md
pub trait SceneRebase<S, I, T: Target>: Send + Sync {
    /// Merge a stale patch against the current scene, or report a conflict.
    fn rebase(&self, context: RebaseContext<'_, S, I, T>) -> RebaseOutcome<S, I, T>;
}

/// The default shared-state policy: reject every stale Patch unchanged.
/// @sergent/docs/framework.md
#[derive(Clone, Copy, Debug, Default)]
pub struct StrictRevision;

impl StrictRevision {
    /// Builds stale-Patch evidence naming both the original and current revisions.
    pub(crate) fn stale_error<S, I, T: Target>(context: &RebaseContext<'_, S, I, T>) -> RunError {
        RunError::stale_patch(
            "patch base revision is stale",
            context.original_patch().base().revision,
            context.current_identity().revision,
        )
    }
}

impl<S, I, T: Target> SceneRebase<S, I, T> for StrictRevision {
    /// Rejects stale work unchanged without applying or rewriting its Patch.
    fn rebase(&self, context: RebaseContext<'_, S, I, T>) -> RebaseOutcome<S, I, T> {
        RebaseOutcome::Conflict(Self::stale_error(&context))
    }
}

pub(crate) enum StalePolicy<P> {
    Strict,
    Rebase(P),
}

#[derive(Clone, Copy)]
pub(crate) enum RevisionOwnership {
    External,
    Embedded,
}

impl RevisionOwnership {
    /// Reports whether commits and human edits must enforce Scene-owned identity transitions.
    pub(crate) fn is_embedded(self) -> bool {
        matches!(self, Self::Embedded)
    }
}

/// Holds the Scene and matching revision authority mutated together under the live-state lock.
pub(crate) struct LiveInner<S> {
    pub(crate) scene: S,
    pub(crate) identity: SceneIdentity,
}

/// One cohesive live allocation containing mutation authority and immutable policy.
pub(crate) struct LiveAuthority<S, P> {
    inner: Mutex<LiveInner<S>>,
    policy: StalePolicy<P>,
    revision_ownership: RevisionOwnership,
}

/// A rejected application-owned edit against shared live Scene authority.
/// @sergent/docs/framework.md @sergent/docs/trust-boundaries.md
#[derive(Debug, PartialEq, Eq)]
pub enum SceneEditError<E> {
    /// The caller observed a revision that is no longer current. The edit
    /// function was not invoked.
    Stale {
        /// The revision supplied by the caller.
        expected_revision: u64,
        /// The revision held by live Scene authority.
        current_revision: u64,
    },
    /// The application edit rejected its domain input. The original error is
    /// preserved and the live Scene remains unchanged.
    Rejected(E),
    /// Revision authority is already at `u64::MAX`, so the edit function was
    /// not invoked and the Scene remains unchanged.
    RevisionExhausted(RunError),
    /// An embedded-identity Scene edit changed its id or failed to advance its
    /// embedded revision by exactly one.
    EmbeddedIdentity {
        /// The embedded identity consistency issues.
        issues: Vec<String>,
    },
}

impl<E: std::fmt::Display> std::fmt::Display for SceneEditError<E> {
    /// Renders each typed edit rejection without changing its structured evidence.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stale {
                expected_revision,
                current_revision,
            } => write!(
                formatter,
                "scene edit expected revision {expected_revision} but current revision is \
                 {current_revision}"
            ),
            Self::Rejected(error) => write!(formatter, "scene edit rejected: {error}"),
            Self::RevisionExhausted(error) => {
                write!(formatter, "{}: {}", error.kind, error.message)
            }
            Self::EmbeddedIdentity { issues } => write!(
                formatter,
                "scene edit violates embedded identity: {}",
                issues.join(", ")
            ),
        }
    }
}

impl<E> std::error::Error for SceneEditError<E> where E: std::error::Error + 'static {}

/// Shared live Scene, revision, immutable stale policy, and revision ownership.
/// Runs remain concurrent outside the atomic commit lock. Clones share one
/// allocation whose policy cannot be replaced. @sergent/docs/execution-model.md
pub struct SceneState<S, P = StrictRevision> {
    authority: Arc<LiveAuthority<S, P>>,
}

impl<S, P> Clone for SceneState<S, P> {
    /// Shares the same locked Scene, revision authority, and immutable stale policy.
    fn clone(&self) -> Self {
        Self {
            authority: Arc::clone(&self.authority),
        }
    }
}

impl<S> SceneState<S, StrictRevision> {
    /// Wrap a scene and its current identity as shared live authority. External
    /// revision ownership is the default; the identity's revision advances
    /// beside the scene data on each commit.
    pub fn new(scene: S, identity: SceneIdentity) -> Self {
        Self::allocate(
            scene,
            identity,
            StalePolicy::Strict,
            RevisionOwnership::External,
        )
    }

    /// Wrap a Scene whose payload owns its identity. The initial authority is
    /// derived from that exact Scene through its actions, so embedded identity
    /// cannot begin inconsistent. @sergent/docs/framework.md
    pub fn embedded<A>(actions: &A, scene: S) -> Self
    where
        A: SceneActions<Scene = S>,
    {
        let identity = actions.identity(&scene);
        Self::allocate(
            scene,
            identity,
            StalePolicy::Strict,
            RevisionOwnership::Embedded,
        )
    }
}

impl<S, P> SceneState<S, P> {
    /// Construct externally revisioned live authority with one immutable rebaser.
    /// Clones created afterward share both this policy and the same Scene authority.
    /// @sergent/docs/framework.md
    pub fn rebasing(scene: S, identity: SceneIdentity, rebase: P) -> Self {
        Self::allocate(
            scene,
            identity,
            StalePolicy::Rebase(rebase),
            RevisionOwnership::External,
        )
    }

    /// Construct embedded-revision live authority with one immutable rebaser.
    /// The initial identity derives from the supplied Scene before aliases exist.
    /// @sergent/docs/framework.md
    pub fn embedded_rebasing<A>(actions: &A, scene: S, rebase: P) -> Self
    where
        A: SceneActions<Scene = S>,
    {
        let identity = actions.identity(&scene);
        Self::allocate(
            scene,
            identity,
            StalePolicy::Rebase(rebase),
            RevisionOwnership::Embedded,
        )
    }

    /// Creates the sole shared allocation before any handle can be cloned.
    fn allocate(
        scene: S,
        identity: SceneIdentity,
        policy: StalePolicy<P>,
        revision_ownership: RevisionOwnership,
    ) -> Self {
        Self {
            authority: Arc::new(LiveAuthority {
                inner: Mutex::new(LiveInner { scene, identity }),
                policy,
                revision_ownership,
            }),
        }
    }

    /// The identity currently held by live authority, read without cloning the
    /// Scene: the revision an application observes and supplies back as
    /// `try_edit`'s expected revision.
    pub fn identity(&self) -> SceneIdentity {
        self.lock().identity.clone()
    }

    /// Clone the current Scene and identity together under the live-state lock.
    /// @sergent/docs/execution-model.md
    pub fn snapshot<A>(&self, actions: &A) -> (S, SceneIdentity)
    where
        A: SceneActions<Scene = S>,
    {
        let guard = self.lock();
        (actions.clone_scene(&guard.scene), guard.identity.clone())
    }

    /// Atomically apply an application edit at the expected live revision.
    /// Stale or exhausted authority fails before invocation; rejection changes
    /// nothing. Success replaces the Scene and advances authority once.
    /// Embedded ownership checks the replacement identity; external ownership
    /// trusts it. Domain verification is not rerun.
    /// @sergent/docs/framework.md @sergent/docs/trust-boundaries.md
    pub fn try_edit<A, E>(
        &self,
        actions: &A,
        expected_revision: u64,
        edit: impl FnOnce(&S) -> Result<S, E>,
    ) -> Result<S, SceneEditError<E>>
    where
        A: SceneActions<Scene = S>,
    {
        let mut guard = self.lock();
        let current = guard.identity.clone();
        if current.revision != expected_revision {
            return Err(SceneEditError::Stale {
                expected_revision,
                current_revision: current.revision,
            });
        }
        let next_identity =
            advance_identity(&current).map_err(SceneEditError::RevisionExhausted)?;

        let edited = edit(&guard.scene).map_err(SceneEditError::Rejected)?;
        if self.is_embedded() {
            let edited_identity = actions.identity(&edited);
            let issues = identity_transition_issues(&next_identity, &edited_identity);
            if !issues.is_empty() {
                return Err(SceneEditError::EmbeddedIdentity { issues });
            }
        }
        let returned = actions.clone_scene(&edited);
        guard.scene = edited;
        guard.identity = next_identity;
        Ok(returned)
    }

    /// Acquires live Scene authority, recovering poisoned ownership for synchronous access.
    pub(crate) fn lock(&self) -> MutexGuard<'_, LiveInner<S>> {
        self.authority
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Borrows the immutable stale policy owned by this live allocation.
    pub(crate) fn policy(&self) -> &StalePolicy<P> {
        &self.authority.policy
    }

    /// Reports whether this allocation enforces Scene-embedded identity transitions.
    pub(crate) fn is_embedded(&self) -> bool {
        self.authority.revision_ownership.is_embedded()
    }
}

/// Advances revision authority once with checked arithmetic while preserving the Scene id.
pub(crate) fn advance_identity(current: &SceneIdentity) -> Result<SceneIdentity, RunError> {
    let revision = current.revision.checked_add(1).ok_or_else(|| {
        RunError::revision_exhausted(
            "shared Scene revision authority is exhausted",
            &current.scene_id,
            current.revision,
        )
    })?;
    Ok(SceneIdentity {
        scene_id: current.scene_id.clone(),
        revision,
    })
}

/// The scene authority a run commits through: a plain snapshot or shared live state.
///
/// Plain authority assumes the application excludes other writes from
/// Observation through commit. @sergent/docs/framework.md
pub enum SceneSource<S, P = StrictRevision> {
    /// A snapshot input owned by this run; commit needs no current comparison.
    Plain(S),
    /// Shared live state; commit compares the base revision under the lock.
    Live(SceneState<S, P>),
}

impl<S> SceneSource<S, StrictRevision> {
    /// Bind a plain Scene input to the default strict policy type.
    pub fn plain(scene: S) -> Self {
        Self::Plain(scene)
    }
}
