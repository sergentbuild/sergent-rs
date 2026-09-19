# sergent-rs-core

`sergent-rs-core` is the foundation crate of the Sergent Rust implementation.
It defines the typed vocabulary and the three interfaces that every other layer
exchanges, and it runs nothing itself: no Run, no mutation, no input or output.

## Purpose and Role

Sergent describes agentic software in which a model proposes and deterministic
code decides. The [specification overview](../sergent/docs/KNOWLEDGE.md)
introduces that idea and its vocabulary. This crate expresses the boundary
between model reasoning and deterministic application behavior as Rust types,
so the other layers exchange values without inspecting each other.

Core supplies five cohesive parts.

- Specification values for Scene identity, Target, MindBuf, Intent, Operations,
  Execution Plans, Patches, model requests, errors, and timing, plus the closed
  Run vocabulary of stages, statuses, and Step Record names.
- The three framework interfaces: `ModelClient`, `SceneActions`, and
  `SergentRecipe`. An application's Scene, Intent, Target, and proposal types
  stay statically associated with them and opaque to framework code.
- Canonical Proposal Schema derivation, with a closed normalizer and a
  construction-time proof of the schema dialect the specification fixes.
- The Operation registry, which binds one explicit call discriminator to each
  application Operation and is the single authority for both Plan proposal
  schema composition and typed Plan decoding.
- Inert evidence: the facts of one model call, the closed Run Record, and the
  result value a Run returns.

Three crates build on this foundation. `sergent-rs-runtime` drives the three
interfaces through one Run and adds mutation, cancellation, progress, observer
delivery, and Run Record construction. `sergent-rs-providers` implements
`ModelClient` as real transport to a model endpoint. `sergent-rs`, the public
API crate, presents the curated surface and the default wiring that
applications use. A Sergentic application depends on `sergent-rs` alone and
supplies the concrete Scene, MindBuf, Target, Intent, Operation set, Recipe
policy, semantic rules, writer model, and persistence policy. These four crates
are implementation layers of this repository, distinct from the three
application layers of the
[Sergent Vision](../sergent/docs/framework.md#sergent-vision) that each
application arranges for itself.

## Design Philosophy

Four decisions shape every type in this crate.

**Construction is the validation.** Private fields and constructors that prove
their invariants mean a value that exists is a value already checked. Schema
derivation, registry composition, identifier parsing, and image admission fail
at the line that made the mistake, before any provider call.

**Only a model call suspends.** `ModelClient` is the single asynchronous
interface. Recipe policy, Scene actions, Operation behavior, schema proof, and
every value constructor are synchronous, which keeps most of the crate free of
concurrency.

**Applications keep their types.** Generic parameters and associated types
carry the application's Scene, Intent, Target, and proposal types through
framework code untouched, so no dynamic escape hatch is needed to move them.

**Nothing here observes the world.** Core reads no clock, touches no
filesystem, and opens no connection; a value carrying a timestamp receives it
from the caller. Every side effect stays in the layer responsible for it.

A change belongs in core when it defines a stable cross-layer type, one of the
three interfaces, canonical proposal structure, closed Operation registration
and decoding, or inert evidence that several layers share. An execution engine,
live Scene authority, provider adapters, credential handling, network or
filesystem behavior, application orchestration, and persistence policy belong
to the layers above. Core defines the interfaces they use, and nothing more.

## Navigation Map

Read [docs/KNOWLEDGE.md](docs/KNOWLEDGE.md) next. It covers the technical
design, the Rust authoring profile for model-visible types, the registry and
decode rules, the evidence values, and the testing practice of this crate.

The specification is authoritative over everything here: read it for framework
meaning, and the knowledge file for the Rust binding.

- [Framework](../sergent/docs/framework.md): the rules and schema dialect.
- [Execution model](../sergent/docs/execution-model.md): the Run flow.
- [Trust boundaries](../sergent/docs/trust-boundaries.md): the five crossings.
- [Observability](../sergent/docs/observability.md): data conventions.
- [Run Record specification](../sergent/docs/run-record-spec.md): the evidence
  and result structure.
