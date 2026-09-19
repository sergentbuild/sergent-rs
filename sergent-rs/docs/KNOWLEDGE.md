# sergent-rs Knowledge

This is the distilled knowledge of `sergent-rs`, the public API crate of the
Sergent Rust reference implementation. It explains why the crate exists, the
pattern you follow when you build an application on it, and where to study the
layers underneath.

## Introduction and Purpose

`sergent-rs` is the entrance and the public API for an application to incorporate
the power of the framework. It makes the capabilities of three lower crates reachable through
one import path while each capability stays in the crate that implements it.
This crate validates nothing, executes nothing, transports nothing, and
defines no domain type.

Your application defines the Scene and its Scene actions, the MindBuf, the
Target, the Intent proposal and the Intent, the Operations, and the Recipe
that carries its semantic rules. This crate is the one place to reach the
framework values those choices need.

The reason is reader economy. An application author learns one surface instead
of a layering that exists to keep framework responsibilities apart. The four
crates of the stack are implementation layers, and they are not the three
application layers that the
[Sergent Vision](../../sergent/docs/framework.md#sergent-vision) describes.

Two names deserve care from the first page. Sergent, unformatted, names the
concept and the vision. The exported Rust `Sergent` type is one configured
Sergent Instance backed by the Sergent Runtime: you build it once, then start
Runs with it.

## The Building Pattern and Architecture

### Two visible wiring steps

Composing a Sergent Instance takes two steps, and both stay in application
code.

First, bind the Recipe to a capability. `ConfiguredRecipe::intent_only`
declares registry absence: a Stop Intent succeeds, and a Continue Intent fails
the Run before any Plan call. `ConfiguredRecipe::plan_capable` instead
consumes one completed `OperationRegistry`, which fixes the Operation set, the
count bound, the canonical Plan structure, and the typed decode vocabulary in
a single move.

Second, compose that configured Recipe with the application's Scene actions
and one model client through `Sergent::new`.

Nothing wraps those two steps, and that absence is deliberate. A convenience
constructor would hide the capability decision or the client choice at the
exact moment the application must make it. Registry capability and the
Recipe's Intent proposal source together express the Run Kind that the
[execution model](../../sergent/docs/execution-model.md) defines.

`Sergent::new` reports bad wiring as a `ConstructionError` and makes no model
call, so fallible admission stays separate from any timed invocation. It
captures stable configuration once, and every invocation afterwards is one
bounded, single-use Run.

### Three configuration values, three lifetimes

- `ConfiguredRecipe` holds stable application wiring: the Recipe, its Intent
  proposal source, and optional Plan capability. Construction captures it
  once, and no Run rereads it.
- `RunSettings` carries caller data for one Run: the exact opaque
  `provider/model` name plus independent `ModelSettings` for the Intent and
  the Plan phase. It has no complete default, because model selection is
  mandatory, and a phase's settings matter only when the Run reaches that
  phase.
- `ProviderConfig` is an immutable snapshot of recognized credentials and
  endpoints. `LlmClient::new` discovers process values during invocation,
  while `LlmClient::configured` reads the supplied snapshot and nothing else.

Configuration precedence is an application decision. Build recognized values
explicitly, capture process values, and let a lower-precedence snapshot fill
what remains. Composition never mutates process state.

Model names stay exact caller-provided data. This crate keeps no model
catalog, no preferred model, and no normalization rule: the core crate defines
the generic request settings, and the providers crate performs the external
translation.

### Scene authority and the model client

The framework offers both Scene authority shapes and picks neither.
`SceneSource` carries either a plain Scene snapshot, which commits to the
rehearsed copy without comparing live state, or a `SceneState`, which holds
shared live authority with its stale policy fixed in the same allocation
before any alias to it exists. The
[writer model](../../sergent/docs/framework.md#writer-model-and-revision-policy)
decides which shape an application may use.

The model client is the second free choice. Pass the concrete `LlmClient` for
real provider calls, the deterministic client for hermetic tests, or an
application implementation of `ModelClient`.

### Model-visible types

Every model-visible application type derives `Deserialize` and `JsonSchema`
from one Rust definition, so the structure the model sees and the structure
the Run decodes cannot drift apart. An application pins `schemars` to the
exact version this workspace pins, because the canonical dialect validator is
written against that emitter's output shape. The
[canonical Proposal Schema](../../sergent/docs/framework.md#the-canonical-proposal-schema)
states what such a type may contain.

### Starting Runs and watching them

The application supplies the async executor. Building a client needs no
running executor, while starting a Run spawns a task and real provider work
performs network calls, so both need an active Tokio runtime with time and
I/O enabled.

Two entry points exist, and they differ in how they hold observer slots. An
awaited Run borrows observer slots for its duration. A started Run takes boxed
slots and hands back a `RunHandle<Scene>` that reports progress, requests
cancellation, and awaits the terminal result. `ProgressSnapshot` and the
object-safe `RunObserver<Scene>` carry live observation, and the
[observability specification](../../sergent/docs/observability.md) fixes the
snapshot fields and the delivery rules.

Cancellation is cooperative, and the framework runs no whole-Run timer. An
application that needs a deadline keeps the pending Run, requests cancellation
when its clock says so, and awaits settlement.

Everything around a Run stays in the application: human wait states, retries
across Runs, multi-run procedures, Scene adoption, and persistence policy. The
run handle and the opt-in Run Record harness serve that work without pulling
its policy into the framework.

### Evidence is for inspection

`SergentResult` and its closed `RunRecord` carry what one Run reached, with
captured values, model call payloads, Patch summaries, and the record children
all readable. The
[Run Record specification](../../sergent/docs/run-record-spec.md) defines that
structure, whose `steps` sequence alone is the Run Record Ledger. Read this
evidence, show it, and persist it; never parse it back into application
control flow. An Intent-Only Run that stops successfully returns its typed
decision as ordinary application state, and that value, not the record, drives
what happens next. This crate only makes the evidence reachable: it adds
neither a second projection of it nor an extra recorded field.

The Run Record harness is reachable in full: `JsonlRunRecordWriter` creation
plus the application and user event values it accepts. Those names stay with
the runtime crate, and this crate adds no filename, conversion, envelope, or
close helper around them.

### Testing an application without a provider

Application tests depend on `sergent-rs` alone, exactly like production code.
`sergent_rs::testing::StaticLlmClient` serves canned model objects through the
production parsing path, records every request, and touches no network,
process configuration, credential, or clock. Its simple constructor takes one
output per expected call, and its scripted constructor takes
`StaticLlmOutcome` values, which add a timeout and a rate-limited transport
failure. Exhausting the script is a harness error, never a fabricated provider
outcome. `CREDENTIAL_ENV_VARS` names the variables a hermetic suite strips.

A high-value application test walks the real path. Derive the model-visible
proposal and Operation types with `serde` and `schemars`, build the production
registry and configured Recipe, compose a `Sergent` through this crate, and
drive the typed pipeline to a terminal result. Park one started Run to prove
handle progress, cancellation, and boxed observer composition. Contain one
scripted transport failure and assert the exact structured evidence beside an
unchanged Scene. Inspect the recorded requests for the application's messages,
the model selection, and the settings of each reached phase, then assert the
terminal result, the final Scene, and the Run Record evidence.

Three habits waste effort: copying a complete generated schema into a test,
inspecting private framework state, and repeating a lower crate's test suite.
Test application semantic bounds at the actual request, and test mutation
policy at the final Scene and the result.

### Patterns the example applications demonstrate

Complete Sergentic applications live in a separate repository, and each one
shows a different way to use this crate:

- `sergent-rs-examples:cog` runs one Intent-Only assessment that stops with a
  typed decision under a deadline the application keeps.
- `sergent-rs-examples:gomoku` plays turn by turn on plain Scene authority
  with a pass-through Intent and one Plan call.
- `sergent-rs-examples:ghoul` fans out concurrent Runs over shared live Scene
  authority and merges them with deterministic rebase.
- `sergent-rs-examples:socrates` interviews a user across rounds and persists
  each accepted Scene before adopting it.

Read them for usage patterns; the specification stays the authority on
framework rules.

## The Structure of This Crate

The surface is a deliberate facade, not a mirror of every public name below
it. It gathers these groups:

- the three interfaces an application implements or hands over, with the typed
  values they exchange: Scene actions and Scene identity, the MindBuf
  observation seam, Target, Intent flow, Operations and their plan steps, the
  Execution Plan, and the Patch;
- the assembly names: Operation registration with its builder, Recipe
  capability binding, Sergent construction, and the construction failure;
- the per-Run inputs and controls: Scene sources with the rebase seam, run
  settings, the cancellation token, progress, the observer seam, and the run
  handle;
- the evidence values: the terminal result, the Run Record with its children,
  captured values, model call records, framework identifiers, timing values,
  and the closed vocabulary of stages and status;
- the provider wiring: the concrete model client, immutable provider
  configuration, and the credential variable list;
- the Run Record harness with its application and user event values; and
- the `testing` module.

### When a name may join

A name belongs here when an application that depends on `sergent-rs` alone has
a living reason to construct it, implement it, hand it to the framework, or
inspect it in a public result. Everything else stays below the facade.

Two cases show where the line falls. Machinery that only serves a crossing
inside the runtime or the providers crate stays out, and so do the Run Record
construction inputs that only the runtime builds, even though the core crate
exports them. The `OperationRegistry` arrives because it configures a Recipe,
not because an application should decode model output itself, and its decode
failure type is absent for exactly that reason.

### No wrappers

Prefer explicit re-exports and the constructors defined by the crate that
implements the type. A wrapper here earns its place only through real
application-facing assembly; one that forwards arguments hides a capability or
an authority choice and buys nothing. New behavior belongs in the crate
responsible for it: mechanism in the runtime crate, transport in the providers
crate, domain policy in the application.

### The `testing` module

Test-only wiring sits apart, in the `testing` module. It re-exports the
deterministic client of the providers crate and its closed outcome script, so
an application drives complete Runs against exact model or transport outcomes
with no network, environment, or credential access. The separate module marks
those names as test wiring rather than part of the production surface.

### Proving the surface

The tests in this crate check assembly, not framework behavior. They compile a
representative set of names spanning all three lower crates, assemble both
Recipe capability shapes, and drive the real pipeline with deterministic model
output. A generated inventory of every re-export proves nothing and does not
belong here.

Run `just test` and `just lint` from the workspace root; both must pass before
a change is finished.

## Navigation Map

Three crates sit below this one. Each carries a README and a knowledge file
beside its source; read them when you need more than the surface.

- `sergent-rs-core` defines the typed specification values and the three
  interfaces, the canonical Proposal Schema machinery, and the Operation
  registry. Study it to learn what a model-visible type may contain and what
  each evidence value holds.
- `sergent-rs-runtime` is the execution engine: Scene authority and commit,
  cancellation, observer delivery, Run Record construction, and the Run Record
  harness. Study it to learn how a Run proceeds and how rebase and revision
  policy behave.
- `sergent-rs-providers` performs the model transport: provider adapters,
  settings translation, credential discovery, and the deterministic test
  client. Study it to learn provider behavior, retryability, and attempt
  evidence.

The specification governs all of them:

- [Specification overview](../../sergent/docs/KNOWLEDGE.md) and
  [terminology](../../sergent/docs/terminology.md) open the rulebook.
- [Framework specification](../../sergent/docs/framework.md) states the
  Sergent Vision, the framework rules, the canonical schema dialect, and the
  writer model.
- [Execution model](../../sergent/docs/execution-model.md) states the Run
  flow, stages and status, cancellation, and commit behavior.
- [Trust boundaries](../../sergent/docs/trust-boundaries.md) lists the places
  where a Run admits outside input.
- [Observability](../../sergent/docs/observability.md),
  [Run Record specification](../../sergent/docs/run-record-spec.md), and
  [Run Record file format](../../sergent/docs/run-record-file-format.md) state
  the evidence and persistence rules.
