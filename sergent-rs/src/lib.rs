//! Batteries-included application-facing surface of the Sergent Rust reference
//! implementation. See @sergent-rs/docs/KNOWLEDGE.md.
//!
//! This is the one crate a Sergent application depends on. It assembles the
//! lower layers once so application code writes a Scene, its actions, a Recipe,
//! Operations, and a small app against a single curated surface, without
//! importing `sergent-rs-core`, `sergent-rs-runtime`, or `sergent-rs-providers`
//! directly. It adds no engine behavior, no transport, and no domain types: it
//! only wires and re-exports. Anything with real logic belongs in the runtime
//! (mechanism), the providers crate (transport), or the application (domain).
//! The exported [`Sergent`] type represents a configured Sergent Instance
//! backed by the Sergent Runtime; Sergent itself names the specification's
//! concept and vision.
//!
//! # The import rule
//!
//! Applications import from `sergent_rs` only. Every framework crate below is a
//! wiring-free layer that re-exports nothing; this crate is the assembly.
//!
//! # The curation rule
//!
//! Every re-export here is a name an application writing a recipe plausibly
//! touches. [`OperationRegistry`] configures [`ConfiguredRecipe`], so its
//! inherent `decode` method remains reachable through that re-export. Direct
//! application decode is not an endorsed workflow: the runtime owns the
//! model-output crossing, and the registry's `DecodeError` is not directly
//! re-exported here. Other framework-internal machinery, including the
//! embedded-identity helper and private engine modules, is deliberately absent.
//!
//! # Default wiring
//!
//! Constructing the default provider-backed runtime explicitly composes a
//! configured Recipe, application actions, and a provider client, so the
//! battery adds no wrapper that would hide the capability decision or client
//! construction.
//!
//! ```ignore
//! use sergent_rs::{ConfiguredRecipe, LlmClient, Sergent};
//!
//! // `recipe` and `actions` are the application's own two interface objects.
//! let configured = ConfiguredRecipe::plan_capable(recipe, registry);
//! let sergent = Sergent::new(configured, actions, LlmClient::new())?;
//! ```
//!
//! Applications that own configuration loading may instead pass immutable
//! [`ProviderConfig`] through [`LlmClient::configured`].
//!
//! `Sergent::new` returns `Result<_, ConstructionError>`; the caller still
//! supplies [`RunSettings`] containing the exact `provider/model` name on every
//! `run` or `start`. For hermetic tests, swap `LlmClient::new()` for
//! [`testing::StaticLlmClient`].

// --- core: framework identifiers (sergent_rs_core::ids) ---
pub use sergent_rs_core::ids::{IdError, OperationId, RunId, SceneId, TargetId};

// --- core: the structured run failure (sergent_rs_core::error) ---
pub use sergent_rs_core::error::{ErrorKind, RunError};

// --- core: the closed run vocabulary (sergent_rs_core::vocab) ---
pub use sergent_rs_core::vocab::{
    ProgressStatus, RunStepName, RunStepStatus, Stage, TerminalStatus,
};

// --- core: wall-clock evidence (sergent_rs_core::timing) ---
pub use sergent_rs_core::timing::{TimeSpan, Timestamp};

// --- core: the selected-target seam (sergent_rs_core::target) ---
pub use sergent_rs_core::target::Target;

// --- core: the scene-authority interface and identity (sergent_rs_core::scene) ---
pub use sergent_rs_core::clone_scene_via_clone;
pub use sergent_rs_core::scene::{SceneActions, SceneIdentity, VerificationReport};

// --- core: the MindBuf observation seam (sergent_rs_core::mindbuf) ---
pub use sergent_rs_core::mindbuf::MindBuf;

// --- core: Intent flow control (sergent_rs_core::intent) ---
pub use sergent_rs_core::intent::{Intent, IntentFlow};

// --- core: the Operation building block and its steps (sergent_rs_core::operation) ---
pub use sergent_rs_core::operation::{Inadmissible, Operation, OperationFault, PlanStep};

// --- core: the plan, patch, and proposal script types (sergent_rs_core::plan) ---
pub use sergent_rs_core::plan::{ExecutionPlan, Patch, PlanProposal};

// --- core: the canonical proposal schema (sergent_rs_core::proposal) ---
pub use sergent_rs_core::proposal::{ProposalSchema, SchemaError};

// --- core: the Operation registry, its builder, and its errors (sergent_rs_core::registry) ---
pub use sergent_rs_core::registry::{OperationRegistry, OperationRegistryBuilder, RegistryError};

// --- core: the application recipe interface (sergent_rs_core::recipe) ---
pub use sergent_rs_core::recipe::SergentRecipe;

// --- core: the model-call boundary and its evidence (sergent_rs_core::model) ---
pub use sergent_rs_core::model::{
    Attempt, CallUsage, ImageError, ImagePart, Message, MessageRole, ModelClient, ModelError,
    ModelIdentity, ModelRequest, ModelRequestCapture, ModelResponse, ModelSettings,
    ParsedJsonObject, ThinkingEffort, TokenCounts,
};

// --- core: the Run Record and the terminal result (sergent_rs_core::run_record) ---
pub use sergent_rs_core::run_record::{
    Cancellation, CancellationCheckpoint, CapturedValue, ModelCallPayloads, ModelCallRecord,
    OutputTokenTotal, PatchSummary, RunOutcome, RunRecord, RunStepRecord, RunTerminal,
    SceneTransition, SergentResult,
};

// --- runtime: the configured Sergent Instance and construction (sergent_rs_runtime::sergent) ---
pub use sergent_rs_runtime::sergent::{ConfiguredRecipe, ConstructionError, RunSettings, Sergent};

// --- runtime: the start-and-handle surface (sergent_rs_runtime::handle) ---
pub use sergent_rs_runtime::handle::RunHandle;

// --- runtime: cooperative cancellation (sergent_rs_runtime::cancel) ---
pub use sergent_rs_runtime::cancel::CancelToken;

// --- runtime: the observer seam (sergent_rs_runtime::observer) ---
pub use sergent_rs_runtime::observer::RunObserver;

// --- runtime: the sanitized progress snapshot (sergent_rs_runtime::progress) ---
pub use sergent_rs_runtime::progress::ProgressSnapshot;

// --- runtime: scene-authority sources and the rebase seam (sergent_rs_runtime::scene_state) ---
pub use sergent_rs_runtime::scene_state::{
    RebaseContext, RebaseOutcome, SceneEditError, SceneRebase, SceneSource, SceneState,
    StrictRevision,
};

// --- runtime: the opt-in Run Record harness (sergent_rs_runtime::run_record_file) ---
pub use sergent_rs_runtime::run_record_file::{
    JsonlRunRecordWriter, RunRecordApplicationName, RunRecordCorrelation, RunRecordEvent,
    RunRecordEventArray, RunRecordEventObject, RunRecordEventValue, RunRecordFileError,
    RunRecordFileErrorKind, RunRecordFileId,
};

// --- runtime: the minimal continue-flow Intent (sergent_rs_runtime::intents) ---
pub use sergent_rs_runtime::intents::IntentContinue;

// --- runtime: the Execution-Plan-Only Intent proposal (sergent_rs_runtime::intent_proposals) ---
pub use sergent_rs_runtime::intent_proposals::IntentProposalPassThrough;

// --- providers: concrete transport and immutable configuration (sergent_rs_providers) ---
pub use sergent_rs_providers::{CREDENTIAL_ENV_VARS, LlmClient, ProviderConfig};

/// Test-only wiring for application test suites.
///
/// Re-exports the provider-owned deterministic client and its closed outcome
/// script so an application drives full runs against exact model or transport
/// outcomes with no network, environment, or credential access.
pub mod testing {
    pub use sergent_rs_providers::testing::{StaticLlmClient, StaticLlmOutcome};
}
