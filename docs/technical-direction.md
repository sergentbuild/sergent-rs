# Sergent Rust Technical Direction

**Target:** The Sergent reference implementation in Rust 2024 Edition.

**Authority:** The Sergent Specification in `sergent/docs/`. Its [overview](../sergent/docs/KNOWLEDGE.md) is the reading entrance; the [framework](../sergent/docs/framework.md), [execution model](../sergent/docs/execution-model.md), [trust boundaries](../sergent/docs/trust-boundaries.md), [observability](../sergent/docs/observability.md), [Run Record](../sergent/docs/run-record-spec.md), and [Run Record file format](../sergent/docs/run-record-file-format.md) documents define the rules this direction builds on. This document records only the decisions that bind the Rust implementation beyond the specification: crate responsibilities, Rust type and trait design, construction rules, evidence bindings, and test policy. It does not restate specification rules; each rule links the document that defines the rule it depends on.

**Audience:** Anyone implementing, reviewing, or modifying Sergent Rust.

**Interpretation:** `MUST`, `MUST NOT`, `ONLY`, and `NEVER` are binding. If this guide conflicts with the specification, fix this guide; the specification wins.

## 1. Crates and Construction

### R01 - Keep the Four-Crate Typed Surface

- **Crates:** `sergent-rs-core` holds specification values, the three interfaces (`ModelClient`, `SceneActions`, `SergentRecipe`), canonical `ProposalSchema` machinery, and `OperationRegistry`. `sergent-rs-runtime` is responsible for execution, Scene authority, cancellation, observer delivery, Run Record construction, and the Run Record harness. `sergent-rs-providers` is responsible for concrete model transport, provider configuration, and the deterministic test client. The `sergent-rs` public API crate holds curated re-exports and default wiring only. The layering follows the specification's [implementer guidance](../sergent/docs/for-implementers.md); the crate cut is this repository's decision.
- **Dependency law:** Runtime depends only on core, including in tests. Providers depend only on core. Lower crates never re-export each other. Applications depend on the public API crate alone.
- **Type law:** Bind Recipe, Scene actions, model client, Scene, Intent proposal, Intent, and Target with generics and associated types. `ModelClient` uses static dispatch. Trait objects are limited to the closed heterogeneous Operation script and observer slots.
- **Naming:** The Rust `Sergent` type is a configured Sergent Instance backed by the Sergent Runtime; unformatted Sergent names the concept. The four crates are an implementation layering, distinct from the Sergent Vision's three application layers.
- **Demonstration:** Framework code and documentation MUST NOT invent an application domain to demonstrate the framework. Acceptance evidence uses domain-neutral tests in the crate responsible for each invariant. To demonstrate a usage pattern, you can reference an application's entrypoint in the [examples project](https://github.com/sergentbuild/sergent-rs-examples) via GitHub URLs.

### R02 - Capture Stable Configuration Once

- **Capability:** `ConfiguredRecipe::intent_only` and `ConfiguredRecipe::plan_capable` are the only capability constructors. The Plan-capable form consumes one completed `OperationRegistry`, so a maximum Operation count without a registry is unrepresentable. Registry capability plus the captured Intent proposal source (model-backed, or the runtime-provided `IntentProposalPassThrough`) expresses the Run Kind of the [execution model](../sergent/docs/execution-model.md).
- **Capture:** `Sergent::new` composes a configured Recipe, the application's Scene actions, and one model client. It derives the Intent proposal schema when the Intent mode is model-backed, captures the pass-through proposal value otherwise, and takes the Plan schema and Operation count bound from inside the consumed registry, so every reachable canonical schema is fixed in private fields before any provider call; a failed derivation is a `ConstructionError`. A run NEVER rereads Recipe configuration, replaces a registry, or rederives a schema.
- **Per-run inputs:** Scene source, MindBuf, `RunSettings`, cancellation, and observer slots are per-run data. `RunSettings` requires the caller's exact opaque model name at construction and holds independent Intent and Plan `ModelSettings`; phase settings may default, model selection may not.
- **Runs:** One `Sergent` value serves separate or concurrent invocations. Each run value is single-use and is never re-entered, resumed, or restarted.
- **Stop data:** Do not add a framework size budget for Stop terminal data until a living application establishes one.

### R03 - Give Every Fact One Authority

- **Authorities:** Run identity belongs to the run and `RunRecord`. Selected Target data belongs to `Target`. Operation identity belongs to `PlanStep`. Canonical structure belongs to `ProposalSchema`. Revision authority belongs to the Scene authority the run was given.
- **Construction authority:** Registry decode alone mints model-origin `PlanStep` values. Reached Recipe hooks derive `ExecutionPlan` and compile `Patch`. Runtime alone validates the Patch, dry-runs, and commits; application code MUST NOT bypass those transitions.
- **Target identity:** Runtime allocates the selected Target once and borrows that exact value in admissibility, whole-plan validation, dry-run, commit, Scene application, and rebase. Tests assert the allocation identity, never equal-looking data.
- **Rust direction:** Use private fields and constructors that the responsible module controls wherever unchecked construction would violate an invariant. A crate-local `#[cfg(test)]` factory may construct valid edge cases; it MUST NOT expose a production bypass. Cross-crate tests use the real boundary.

## 2. Trust Boundaries in Rust

### R04 - Map Each Boundary to Its Crate

- **Rule:** The [trust boundary specification](../sergent/docs/trust-boundaries.md) enumerates the five boundaries and the three-question method. Every validation or defensive branch in this workspace MUST belong to one boundary or establish a construction invariant, and MUST survive the three questions or be deleted.
- **Responsible crates:** Model output crosses in runtime through exact serde decodes: one typed Intent proposal decode, or the two-stage registry Plan decode. Human input and persistence load are the application's responsibility. Shared live state at commit is runtime's `SceneState`. External systems are the providers crate: transport, credentials, and process configuration.
- **Construction:** Schema dialect proof, registry build, and wiring checks report developer mistakes as `ConstructionError` or `RegistryError` before any provider call; they are not boundaries.
- **No schema validator:** The canonical schema constrains generation; the serde crossing admits the value. The dependency graph contains no JSON Schema validation library, and none may be added.

## 3. Canonical Proposal Schema

### R05 - Derive Structure and Decode From One Rust Type

- **Profile:** Derive `Deserialize` and `JsonSchema` on the same type, plus `Serialize` so the Run Record can capture it. Require `deny_unknown_fields` on every model-visible object. Pin `schemars` exactly at the workspace version, in applications too. Generate the JSON Schema 2020-12 form that describes deserialization and suppress its meta-schema declaration.
- **Allowed Rust forms:** Named-field structs, supported scalars, `Vec`, finite scalar unit enums, `Option`, unsigned `NonZero` integers, doc-comment descriptions, and inclusive `schemars` range bounds.
- **Rejected Rust forms:** Open maps, arbitrary JSON values, tuple-shaped model objects, recursion, non-closed objects, flattening, external references, unsupported schema keywords, Serde aliases, Serde or Schemars `with` and `schema_with` overrides, ambiguous untagged unions, and handwritten `JsonSchema` implementations. Construction rejects every structurally visible form; authoring properties that generic bounds cannot observe follow the enforcement rule below.
- **Option rule:** `Option<T>` is model-facing as a required property whose value is `T` or `null`. The omission Serde tolerates is not a second model-facing promise.
- **Authoring enforcement:** Add a compile-time check for forbidden authoring properties only when its complete production implementation is at most 100 lines. Otherwise document the forbidden properties and trust application authors. NEVER add a proc-macro crate, plugin system, source scanner, or other enforcement framework for this proof.
- **Prompts:** Prompt text follows the framework's [semantic rules](../sergent/docs/framework.md#semantic-rules-ride-the-schema); it never restates the response shape.

### R06 - Keep the Normalizer Closed

- **Conversions:** The normalizer performs ONLY these maintained conversions on the schemars output: remove emitter metadata `title`, `default`, and `format`; rewrite `const` to a one-value `enum`; rewrite nullable type unions to `anyOf`; wrap a described reference in `anyOf`; require every declared object property. It is not a repair facility, and no conversion may be added merely to make an unsupported type pass.
- **Proof:** One validator proves the [canonical dialect](../sergent/docs/framework.md#canonical-schema-dialect) for every derived schema and for the composed Plan envelope at construction. A failure names the proposal and the JSON pointer.

### R07 - Compose the Plan Envelope and Preserve Schema Identity

- **Envelope:** The continuing Plan proposal is one closed object with one required `operations` array carrying `minItems` 1 and the configured `maxItems`. Each branch derives from the exact registered Operation type with one fixed `call` discriminator. Definitions lift to root `$defs`; JSON-equal same-name definitions are shared, and unequal collisions fail construction. Branch order follows registration order.
- **Identity:** Store each canonical schema once behind `Arc<ProposalSchema>`. Runtime alone builds the request input from that allocation, the caller's model selection, and the phase settings; the Recipe only attaches application-authored messages to it and never receives the schema, so substitution is unrepresentable and this binding needs no separate unchanged-schema check. The resulting request is read-only to consumers.
- **Enforcement:** NEVER expose the captured schema, model selection, or phase settings to Recipe replacement; NEVER rederive, deep-copy, edit, replace, deep-validate, or weaken the schema. A provider adapter performs only its documented translation on an isolated copy.

## 4. Registry, Operations, and Scene Actions

### R08 - Fix Membership at Build and Decode in Two Stages

- **Registration:** Each `register::<Op>(call)` declares the non-empty fixed discriminator for that branch; the model-visible call is never inferred from a Rust type name. Membership is immutable after `OperationRegistry::build`. The same registry composes the schema branches and performs typed decode, so vocabulary cannot drift.
- **Decode:** First reject unknown envelope fields and enforce count bounds. For each entry, read `call`, select the registered branch, remove the discriminator, and strictly decode the operand struct. Do not use Serde internal tagging where it weakens `deny_unknown_fields`.
- **Closure:** No runtime plugin mutation, proc-macro crate, or build script may alter registry membership.

### R09 - Keep Operations Inert and Framework-Identified

- **Payload:** An Operation is a plain data value that carries its action operands by value only: no Scene references, provider handles, callbacks, runtime services, Target identity, or hidden execution authority. It NEVER exposes raw field mutation, arbitrary JSON patching, generic code execution, unrestricted filesystem access, or provider tools.
- **Isolation:** Every registered Operation provides an isolation-preserving `Clone` that isolates nested mutable values; shared immutable storage is allowed only when it exposes no mutation authority. Core keeps the erased cloning for Plan-to-Patch copies private. `Clone` is a promise, not proof: prove deep isolation with a focused test.
- **Identity:** Framework code alone mints `OperationId` after typed decode and stores it with the fixed `call` on `PlanStep`, never on the application Operation. A rebase replacement supplies behavior only and inherits its step's call and Operation ID.
- **Admissibility:** The admissibility hook is synchronous, read-only, and admissible by default. One shared runtime pass algorithm serves the initial pass and the rebase pass; each boundary projects the neutral rejection evidence into the error kind of that boundary.

### R10 - Keep Scene Actions Synchronous and Deterministic

- **Seam:** `SceneActions` is the only isolation seam. Do not require `Scene: Clone`. A value Scene may opt into `clone_scene_via_clone!`; deep-copy, non-`Clone`, and instrumented Scenes keep explicit clone methods.
- **Determinism:** Apply and verify are synchronous and MUST NOT call a model, perform network or filesystem I/O, read a clock, use randomness, accept human input, or suspend. `VerificationReport` is a core type and never repairs the after-Scene.
- **Patch:** The default compilation makes isolated copies of every validated Plan Operation in order and preserves every Operation ID; an override returns a typed conforming `Patch`. Patch validation checks exactly non-empty Operations, base Scene identity, and existence of the original Target; there is no defensive deep Plan-to-Patch equality pass. `OperationFault` preserves an application fault's open kind, message, and metadata.

## 5. Scene Authority, Commit, and Rebase

### R11 - Bind Commit Policy When Authority Is Created

- **Choice:** `SceneSource` selects a plain snapshot or shared `SceneState`. The plain form clones immediately and commits to the rehearsed isolated Scene with no live comparison; the [writer model](../sergent/docs/framework.md#writer-model-and-revision-policy) decides which form an application may use.
- **Allocation:** `SceneState` stores the Scene, its identity, the revision representation (external or embedded), and the strict-or-rebase stale policy in one allocation before any alias exists. Clones share that allocation. No conversion may replace policy while an alias to the mutable authority remains.
- **Exhaustion:** Advance revisions with checked arithmetic. `u64::MAX` yields the one `revision_exhausted` framework error, shared by commit and `SceneEditError`; NEVER wrap or saturate.
- **Critical section:** Commit holds a synchronous lock only inside the deterministic tail, contains no provider await, and ignores cancellation once entered. Prepare a candidate that cannot exist unless every proof passed, then install Scene and identity together through one seam.

### R12 - Rebase Inside Runtime, Model-Free

- **Seam:** The rebase seam receives the original validated Patch, base Scene, current Scene, original Intent, and the exact original Target, and returns a replacement Patch plus one metadata mapping. Runtime NEVER accepts a replacement Target.
- **Second pass:** Runtime performs every check of the [rebase sequence](../sergent/docs/execution-model.md#patch-rebase-on-shared-live-state): envelope and Target validation, admissibility against fresh copies of the current Scene, dry-run on one evolving current copy, then installation.
- **Summary:** One core `PatchSummary` builder is the single source of the representation. A conflict before a replacement embeds the original summary; a rejection after a replacement embeds the rejected rebased summary; a successful rebase never rewrites Patch-step evidence. Metadata shapes follow the [Run errors](../sergent/docs/run-record-spec.md#run-errors) rules.

## 6. Async Execution, Cancellation, and Observers

### R13 - Keep Only Model Invocation Async

- **Boundary:** `ModelClient` uses static `M: ModelClient` dispatch with a return-position future. Recipe, Scene actions, admissibility, Patch compilation, dry-run, rebase, and commit remain synchronous. After the last provider await, admissibility through commit is one synchronous deterministic tail.
- **Cancellation:** `CancelToken` is cooperative. Race every in-flight provider future against it. `RunHandle::cancel` trips the token and NEVER aborts the task. Dropping an in-flight provider future adds no attempt evidence. The runtime has no whole-run timer: an application decides its deadline, retains the pending run, requests cancellation, and awaits settlement.
- **Checkpoints:** The checkpoints and their evidence names follow the [cancellation record](../sergent/docs/run-record-spec.md#outcome-terminal-record-and-cancellation).

### R14 - Observers Borrow, Never Control

- **Slots:** `RunObserver<Scene>` is object-safe. Slots receive a borrowed `ProgressSnapshot` during the run and a borrowed `SergentResult<Scene>` at terminal delivery, but no mutation authority, Run Record builder, or cancellation control. Awaited runs borrow observer slots; started runs hold boxed slots.
- **Progress:** `ProgressSnapshot` serializes exactly the five fields of the [progress snapshot](../sergent/docs/observability.md#progress-snapshots); richer live inspection stays outside it.
- **Panics:** A panic escapes; it never becomes an ordinary observer error, and a commit completed before it remains authoritative.

## 7. Run Record, Result, and Persistence

### R15 - Bind the Evidence Types

- **Home:** `RunRecord`, `RunOutcome`, `RunTerminal`, `RunError`, `CapturedValue`, `PatchSummary`, and `SergentResult` are inert core values with the shapes of the [Run Record specification](../sergent/docs/run-record-spec.md). `RunRecordHeader` and `RunRecordCompletion` are runtime-facing construction values; serialized shape stays independent of construction types.
- **Coherence:** Expected run failures live inside `SergentResult`, never in an outer public `Result`. `RunOutcome` variants make success-with-error and failure-without-error unconstructible.
- **Errors:** Error kinds are open text with a reserved framework subset. Core provides exact constructors for every living reserved shape; runtime control NEVER matches message text. `internal_error` has no producer in this binding because panics escape, so it has no public convenience constructor; add its complete metadata only with a living contained ordinary-exception path.
- **Timing:** Millisecond precision from the nearest monotonic clock is sufficient. A response-backed call's latency spans the complete provider invocation as the provider reference defines. A pre-Unix-epoch host clock is a process-level environment failure and MUST NOT be encoded as epoch zero.

### R16 - Keep the Harness Narrow

- **Harness:** `JsonlRunRecordWriter` is the only Run Record harness. Creation accepts an application-selected directory (created when absent), a validated application name, and an optional validated log identity; runtime mints the default `log_...` identity, creates the file exclusively, and returns the writer with its path. Envelopes, encoding, and file discipline follow the [Run Record file format](../sergent/docs/run-record-file-format.md).
- **Direct events:** Direct-event values use a conversion graph that belongs to runtime, NEVER `CapturedValue`. Conversion rejects true cycles, non-finite numbers, and conversion failures before writing and allows shared acyclic nodes.
- **Permissions:** The file mode that admits the creating user alone is guaranteed only on Unix. On Windows the file inherits directory ACLs and the application selects a suitably secured directory; do not add runtime DACL machinery.
- **One-way:** Neither runtime nor application reconstructs in-process runtime objects from Run Record files. Version markers stay at v1: a breaking shape change replaces v1 in place, with no v2 marker, compatibility reader, or migration.

## 8. Provider Boundary

### R17 - Keep the Provider Seam Structural

- **Seam:** `LlmClient` implements `ModelClient`. It receives one immutable `ModelRequest` and produces call evidence plus one `ParsedJsonObject`, or a structured `ModelError` carrying every reached fact. Adapters receive only the canonical schema, messages, settings, and resolved configuration.
- **Configuration:** `provider/model` is opaque caller data. `LlmClient::new` discovers credentials and endpoints per invocation from the process environment; `LlmClient::configured` uses one immutable `ProviderConfig` instead.
- **Reference:** The [provider reference](../sergent-rs-providers/docs/providers.md) is the sole authority for adapters, endpoints, credentials, settings translation, schema lowering, envelope admission, transport configuration, retryability, and attempt evidence. It MUST stay aligned with the implementation, and this document does not restate it.
- **No semantic retry:** The transport's bounded retry follows the [mitigation rule](../sergent/docs/execution-model.md#step-failure-and-mitigation). Typed decode, semantic validation, admissibility, Plan legality, dry-run, stale work, and merge conflict NEVER trigger a provider retry.

## 9. Tests and Acceptance

### R18 - Test Every Invariant at Its Home

- **Schema:** Prove sanctioned forms and construction failure for every structurally detectable rejected shape, definition collision, discriminator error, and count bound in core.
- **Boundary:** Reject malformed or framed JSON, unknown fields, echoed IDs, unknown calls, empty Plans, and excessive Operation counts through the real crossings.
- **Execution:** Cover model-backed Intent, pass-through, Stop, every failure stage, cancellation at every checkpoint, dry-run isolation, successful commit, stale failure, and deterministic rebase conflict in runtime with private fakes.
- **Evidence:** Prove exact Target preservation, isolated Plan-to-Patch copies, Operation ID correlation, complete failed-crossing call evidence, observer isolation, and Run Record closure.
- **Concurrency:** Use deterministic gates and parked fakes, NEVER sleeps, virtual time, sockets, or local servers.
- **Conformance:** The specification is the oracle. No implementation, including this one, is authoritative over it.

### R19 - Never Manufacture Impossible Values or Speculative Surface

- **Inputs:** Malformed data enters through real boundaries. Invalid construction uses real bad wiring. Valid typed edge cases use crate-local test factories.
- **Deletion:** Delete an unreachable guard together with the test that exists only to feed it.
- **Surface:** Every optional method, fallback, compatibility path, and extension point MUST name the live wiring and execution step that reaches it. Delete unused public types, dynamic plugin systems, future-only abstractions, duplicated authorities, and fallback shapes with no living producer.

### R20 - Pass the Gate

- **Rule:** A change is complete only when `just lint` and `just test` pass on the pinned toolchain, as the [verification policy](KNOWLEDGE.md#hermetic-verification) requires. Tests use no live provider, real credential, network, process environment mutation, or sleep-based concurrency.
