# sergent-rs Knowledge

This is the project-level knowledge of the sergent-rs workspace, the Rust
reference implementation of the Sergent Specification. It explains why the
workspace exists, how the whole system works, which decisions shape it, and
where to study each part in depth.

New to Sergent? Read the
[specification overview](../sergent/docs/KNOWLEDGE.md) first. It takes a few
minutes, and this document builds on its vocabulary.

## Purpose

The Sergent Specification is a rulebook for agentic software in which the user
stays in charge: a model only proposes a change, and deterministic code
validates, rehearses, and commits it. The rulebook is language-agnostic and
contains no code. This workspace makes it concrete in Rust, for two audiences.
An implementer in another language sees one complete, conforming answer to
every design question the specification raises. A Rust developer gets a
framework that is ready to use for building Sergentic applications.

The specification is authoritative over this implementation. Three habits keep
that promise honest:

- The specification is the only source of framework rules. The documents in
  this workspace never restate those rules; they cite the specification and
  explain only what Rust adds. When the implementation and the specification
  disagree, the implementation is wrong.
- A tracked copy of the specification lives in [sergent/](../sergent/README.md).
  Its upstream home is the Sergent reference project, and the author's refresh
  from that project is the only writer of the in-tree copy. Never edit the copy
  by hand.
- The specification's [terminology](../sergent/docs/terminology.md) is the
  naming authority. A public Rust item that maps to a framework concept carries
  the specification's name, and its doc comment points at the specification
  document that defines it, in the textual form `@sergent/docs/framework.md`,
  a path from the workspace root.

Materializing the specification means giving each concept exactly one home in
Rust. A specification value becomes a type whose private fields and
constructors make an invalid state unrepresentable. An application
responsibility becomes a trait. A rule about order becomes a sequence of typed
stages that cannot be entered out of order. A trust boundary becomes one strict
decode. The next section shows where each home is.

## Feynman Explanation

The specification tells the story of one bounded Run: observe a Scene and a
MindBuf, select one Target, resolve a typed Intent, let the model propose an
Execution Plan, then validate, rehearse, and commit a Patch while closing a Run
Record. The [specification overview](../sergent/docs/KNOWLEDGE.md) tells that
story in full. This workspace answers a different question: where does each
part of the story live in Rust, and why is the stack cut into four crates?

The core crate is the vocabulary. It holds the typed values the specification
names and the three interfaces an application implements or consumes:
`SergentRecipe` is the application's policy, `SceneActions` is the
deterministic Scene seam, and `ModelClient` is the only asynchronous seam. Core
also holds the two pieces of machinery that make the model-output boundary
typed. Canonical `ProposalSchema` derivation turns one Rust proposal type into
the closed model-facing schema and proves the dialect at construction.
`OperationRegistry` binds each Operation type to its fixed call discriminator
and uses the same declarations to compose the Plan schema and to decode model
output. Core has no engine and performs no I/O.

The runtime crate is the engine and the only mutation authority. The Rust
`Sergent` type is a configured Sergent Instance: it captures the configured
Recipe, the Scene actions, the model client, and every reachable schema once,
then serves independent bounded Runs. A Run is a sequence of typed stage
transitions; each stage value holds exactly the facts proved so far, and
advancing consumes the previous stage. The model is awaited at most twice.
Everything after the last await is one synchronous deterministic tail:
admissibility, whole-plan validation, Patch compilation, dry-run, and commit.

Commit goes through the Scene authority the application chose for that Run: a
plain snapshot, or shared `SceneState` when several writers can touch the same
Scene. Every returning Run closes one `RunRecord` and returns one
`SergentResult`; the opt-in `JsonlRunRecordWriter` harness persists Run Records
in the portable file format.

The providers crate is the external-systems boundary and nothing more.
`LlmClient` turns one immutable `ModelRequest` into one parsed JSON object plus
sanitized call evidence, or a structured error carrying every reached fact.
Adapters translate to each provider's native schema-constrained facility and
never see proposal types, the registry, or the Scene. One private HTTP seam
serves production and the scripted in-memory client, so provider behavior is
proven without sockets.

The public API crate, `sergent-rs`, is the application's single import. It
re-exports the curated surface and wires the defaults; it adds no behavior. An
application composes a Sergent Instance in two visible steps: it chooses the
Recipe capability with `ConfiguredRecipe`, Intent-only or Plan-capable with one
closed registry, then calls `Sergent::new` with its Scene actions and a model
client. The application keeps everything the specification leaves to it: domain
types, Recipe policy, orchestration across Runs, deadlines, the writer model,
and persistence.

The trust model is the specification's
[five boundaries](../sergent/docs/trust-boundaries.md#the-five-boundaries)
mapped to crates. The runtime crate is responsible for the model-output
crossing, which is an exact serde decode, and for shared live state at commit.
The providers crate is responsible for external systems. Applications are
responsible for human input and persistence load. Everywhere else a typed value
is trusted, because construction is the strongest validation.

## The Biggest Technical Decisions

The [technical direction](technical-direction.md) records these decisions in
binding, rule-by-rule form. This section tells them as one story and gives the
reason behind each.

### Four crates, one direction

The stack follows the layering that the specification's
[implementer guidance](../sergent/docs/for-implementers.md) recommends: values
and interfaces, then the engine, then concrete transport, then the
application-facing layer. The dependency graph is acyclic and one-directional.
The runtime crate and the providers crate each depend on the core crate alone,
tests included, so the engine can never lean on a concrete provider; runtime
tests bring private fakes instead. The public API crate depends on all three
and adds curated re-exports and default wiring only. Lower crates never
re-export each other. An application depends on the public API crate alone and
never imports a framework crate directly. These four implementation layers are
distinct from the three application layers of the Sergent Vision.

### Types carry the trust

Generics and associated types bind the Recipe, the Scene actions, the model
client, and the application's Scene, Intent proposal, Intent, and Target types,
so framework code carries application values without inspecting them. Dispatch
is static. Trait objects appear in two places only, where the members truly
differ in type: the closed Operation script of one plan, and the observer
slots.

Private fields and constructors make an invalid value unrepresentable, so a
value that exists is a value that was proved. Every validation in the workspace
belongs to one of the five boundaries or establishes a construction invariant.
A check that names no living producer of the bad shape is deleted, together
with the test that fed it.

### One Rust type gives both the schema and the decode

A model-visible type derives `Deserialize`, `JsonSchema`, and `Serialize` from
one definition. Serde admits the model output, schemars generates the structure
sent to the model, and serialization lets the Run Record capture the value. The
two sides cannot drift, because they come from the same type.

A small closed normalizer converts the schemars output into the specification's
[canonical schema dialect](../sergent/docs/framework.md#canonical-schema-dialect),
and one validator proves the dialect at construction, before any provider call.
The schema constrains generation; the strict serde decode admits the value. The
dependency graph therefore holds no JSON Schema validation library. `schemars`
is pinned to one exact version, in applications too, because its emitted
structure may change without a major release.

The Operation registry applies the same idea to plans. Each registration
declares one explicit call discriminator for one Operation type, never inferred
from a Rust type name. The same registry composes the Plan schema and performs
the two-stage typed decode, so the vocabulary the model sees and the vocabulary
the decoder accepts are one fact. Framework code alone mints Operation
identity, after the typed decode.

### Configuration is captured once, and a Run is single-use

`ConfiguredRecipe::intent_only` and `ConfiguredRecipe::plan_capable` are the
only capability constructors. The Plan-capable form consumes one completed
registry, which carries the Operation count bound, so a bound without a
registry cannot be expressed. `Sergent::new` captures the configured Recipe,
the Scene actions, one model client, and every reachable canonical schema
before any provider call; a failed derivation is a construction error.

One `Sergent` value then serves separate or concurrent Runs, and a Run never
rereads configuration. Scene source, MindBuf, `RunSettings`, cancellation, and
observers are per-Run inputs. `RunSettings` requires the caller's exact model
name, because no framework code may choose a model: the workspace has no model
catalog, default model, pricing, or capability matrix.

### Only the model call is async

`ModelClient` is the single asynchronous interface, and tokio drives it. A Run
makes at most two model calls. Recipe hooks, Scene actions, admissibility,
Patch compilation, dry-run, rebase, and commit are synchronous. Everything
after the last provider await is therefore one deterministic tail, and the
live-state lock is never held across an await.

Cancellation is cooperative. A token is raced against each in-flight provider
future and checked at fixed checkpoints, and nothing interrupts a commit once
it begins. The runtime has no whole-Run timer: the application decides its
deadline, requests cancellation, and awaits settlement. A panic is process
control and escapes.

### Commit policy is bound when authority is created

An application first answers the specification's
[writer model](../sergent/docs/framework.md#writer-model-and-revision-policy)
question, then picks a `SceneSource`. The plain form clones the Scene at once
and commits to the rehearsed copy with no live comparison. Shared `SceneState`
stores the Scene, its identity, the place where the revision is stored, and the
strict-or-rebase stale policy in one allocation before any alias exists, so no
alias can carry a different policy.

Commit prepares a candidate that cannot exist unless every proof passed, then
installs Scene and identity together through one seam.
[Rebase](../sergent/docs/execution-model.md#patch-rebase-on-shared-live-state)
runs inside the runtime and never calls a model. Revisions advance with checked
arithmetic; exhaustion is one structured error, never a wrap.

### Evidence is inert and flows one way

`RunRecord`, `SergentResult`, and their parts are inert core values with the
shapes of the [Run Record specification](../sergent/docs/run-record-spec.md).
Expected Run failures live inside the result, never in an outer `Result`, and
the outcome type makes success-with-error unconstructible. Error kinds are open
text with a reserved framework subset; control flow reads kinds and metadata,
never message text.

Evidence states only what a Run reached. An aggregate is reported only when
every part is known, and nothing is invented for work that was interrupted.
`JsonlRunRecordWriter` is the only Run Record harness. Neither the framework
nor an application parses a Run Record file back into runtime objects.
Serialized format markers stay at v1; a breaking shape change replaces v1 in
place, with no compatibility reader and no migration.

### The provider seam is structural

`LlmClient` implements `ModelClient` over reqwest. It receives one immutable
request and returns one parsed JSON object with call evidence, or a structured
error with every reached fact. Adapters are pure translators that see only the
canonical schema, messages, settings, and resolved configuration.

The transport retries a narrow class of transient faults with an identical
request. Typed decode, validation, and commit failures never trigger a provider
retry, as the specification's
[mitigation rule](../sergent/docs/execution-model.md#step-failure-and-mitigation)
requires. The [provider reference](../sergent-rs-providers/docs/providers.md)
is the one document that lists the supported adapters and states their exact
behavior.

### A small, deliberate dependency set

The toolchain is stable Rust 1.96.0 with the Rust 2024 edition, pinned by
`rust-toolchain.toml`; rustup installs it on first use. The dependency set
stays small, and each crate declares what it uses: serde and serde_json for
typed decoding and evidence, schemars for schema derivation, tokio for async
execution and synchronization, reqwest with the default TLS backend for HTTP,
thiserror for error types, and uuid for framework identifiers.

The adapters encode native requests themselves over reqwest. Redirects,
protocol retries, and system proxies are disabled, so one recorded attempt is
exactly one wire request.

### Hermetic verification

Tests never contact a live provider, read a real credential, or open a socket.
Concurrency is proven with deterministic gates and parked fakes, never sleeps.
Provider behavior is proven against a scripted in-memory HTTP seam fed sentinel
credentials through fixed read-only configuration, so the same scripted inputs
produce the same execution sequence on Windows, macOS, and Linux. Application
tests script model outcomes with the deterministic `testing::StaticLlmClient`
that the public API crate exposes. Malformed data enters a test through the
real boundary; a test never hand-builds an impossible value to keep a guard
alive.

The gates run on the developer's machine; the project uses no hosted CI. Run
`just test` and `just lint` from the workspace root; both must pass before a
change is finished. `just fmt` formats the Rust code in place.

## Navigation Map

### The specification

Start with the [overview](../sergent/docs/KNOWLEDGE.md); it explains one Run
and orders the specification documents.

- Before changing framework behavior, read the
  [framework](../sergent/docs/framework.md),
  [execution model](../sergent/docs/execution-model.md), and
  [trust boundaries](../sergent/docs/trust-boundaries.md) documents.
- Before changing evidence or persistence behavior, read the
  [observability](../sergent/docs/observability.md),
  [Run Record](../sergent/docs/run-record-spec.md), and
  [Run Record file format](../sergent/docs/run-record-file-format.md)
  documents.
- Before naming a public item, read the
  [terminology](../sergent/docs/terminology.md) document.
- Application authors also benefit from two advisory guides:
  [reliability best practices](../sergent/docs/reliability-best-practices.md)
  and [prompt engineering](../sergent/docs/prompt-engineering.md).

### The technical direction

The [technical direction](technical-direction.md) is the binding record of the
decisions that reach beyond the specification: crate responsibilities, type and
trait design, construction rules, evidence bindings, and test policy. It never
restates the specification; each rule links the specification document it
builds on.

### The crates

Each crate keeps a README for its mission and a knowledge file for its design.

- sergent-rs-core: [README](../sergent-rs-core/README.md) and
  [knowledge](../sergent-rs-core/docs/KNOWLEDGE.md). Specification values, the
  three interfaces, the canonical Proposal Schema machinery with its Rust
  authoring profile, and the Operation registry.
- sergent-rs-runtime: [README](../sergent-rs-runtime/README.md) and
  [knowledge](../sergent-rs-runtime/docs/KNOWLEDGE.md). The Run pipeline, the
  deterministic tail, Scene authority, cancellation, observer delivery, Run
  Record construction, and the Run Record harness.
- sergent-rs-providers: [README](../sergent-rs-providers/README.md),
  [knowledge](../sergent-rs-providers/docs/KNOWLEDGE.md), and the normative
  [provider reference](../sergent-rs-providers/docs/providers.md) for adapters,
  endpoints, credentials, settings translation, retryability, and attempt
  evidence.
- sergent-rs: [README](../sergent-rs/README.md) and
  [knowledge](../sergent-rs/docs/KNOWLEDGE.md). The public API crate: the
  building pattern of a Sergentic application, the curated surface, and the
  deterministic test client.

### Example applications

Complete Sergentic applications built on this stack live in the separate
sergent-rs-examples repository. A document or comment in this repository
mentions one only to explain a usage pattern, and writes a plain-text notation
instead of a link: `sergent-rs-examples:{example}` names an application (for
example `sergent-rs-examples:ghoul`), and `sergent-rs-examples:{filename}`
names a file in that repository. Never write a relative link into that
repository, and never describe it as part of this workspace.

A new builder starts with `sergent-rs-examples:docs/learning-path.md`, which
orders the concepts and the applications. The `sergent-rs`
[knowledge file](../sergent-rs/docs/KNOWLEDGE.md) names the usage pattern each
application demonstrates.
