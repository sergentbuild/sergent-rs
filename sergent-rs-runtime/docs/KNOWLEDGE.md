# sergent-rs-runtime Knowledge

This crate is the execution engine. It sequences one Run, admits model output
through typed crossings, proves every effect before it lands, decides whether
the proven result becomes state, and closes the evidence. What follows
explains how, closely enough to rebuild the same engine from this page. The
unformatted word Sergent names the concept; the Rust `Sergent` type is a
configured Sergent Instance backed by the Sergent Runtime.

## The End-to-End Walkthrough

A model is good at choosing an action, and deterministic code is what keeps
state correct. So the model chooses proposal data, the Recipe says what that
data means, Scene actions prove its effects, and runtime authority decides
whether the proven result is committed. The engine drives those parties and
replaces none of them. The
[execution model](../../sergent/docs/execution-model.md) defines the ordered
flow implemented here.

### Configuration captured once

`ConfiguredRecipe` settles Operation registry capability before construction.
The Intent-Only form carries no registry and cannot reach a Plan. The
Plan-capable form consumes one completed `OperationRegistry` whose vocabulary,
decoder, Operation bound, and canonical Plan schema stay a single authority.
Because the Operation maximum lives inside that registry, a maximum without a
registry cannot be written down. Registry capability plus the Recipe's
captured Intent source expresses the Run Kind.

`Sergent::new` captures the configured Recipe, its no-target failure, the
Intent mode, the registry, the Scene actions, and the model client. A
pass-through Recipe supplies its deterministic proposal once; otherwise
construction derives the Intent proposal schema. Registry composition failures
belong to the core crate, so schema derivation is the only construction
failure this crate reports. Two minimal values make deterministic Intent
selection possible: `IntentProposalPassThrough`, an empty typed sentinel
signalling the absence of the Intent model call, and `IntentContinue`, a
minimal continue-flow Intent an application uses directly or wraps with extra
decision data. Neither gains mutation authority.

A Run borrows those captured facts and never rereads Recipe configuration, so
one instance drives repeated and concurrent Runs with no chance of stable
policy drifting between phases. The caller supplies the rest per Run: Scene
source, MindBuf, cancellation, observer slots, and `RunSettings`, which binds
the exact opaque model name with independent Intent and Plan phase settings
and has no complete default, because no framework code may pick a model for an
application.

The Recipe builds only application-authored messages. The runtime alone
combines them with the caller's model selection, the phase settings, and the
captured schema into the request. Since the Recipe never receives the schema,
substituting one is unrepresentable, and this binding needs no separate
unchanged-schema check.

### Two entries, one pipeline

`Sergent::run` awaits the pipeline with a borrowed MindBuf, a caller-held
`CancelToken`, and borrowed observer slots. `Sergent::start` schedules the
same pipeline on a task, taking the MindBuf by value with boxed observer
slots, and returns a `RunHandle` offering a synchronous progress snapshot, a
completion check, cancellation, and the awaited result. No synchronous twin of
the pipeline exists.

Each reached phase is a typed stage value holding exactly what it needs to
advance once or close once: the per-Run Scene authority, the exact Target, Run
Record construction, and delivery state. Advancing consumes the previous
value, so a later stage can neither rebuild an earlier one nor be entered
twice. The open Step Record holds the Run Record builder while it is open and
returns it on close, so an open step cannot coexist with a closed Run. At the
handoff into the deterministic tail the Target and the validated Intent are
boxed once; every later stage borrows that same allocation, which is how the
rule that a Run never reselects its Target holds by construction.

### The async front

Process input captures the Scene the Run observes, opens the Run Record from
the Run identity and the caller's model name, renders the MindBuf into the
Observation, and selects the Target once through the Scene actions. When
selection finds no Target, the Run fails at the started stage with the failure
the Recipe declared, leaving the Scene unchanged.

Intent resolution obtains a typed Intent proposal from the captured
pass-through value or from a model call, derives the Intent through the
Recipe, and validates it. A validated Stop Intent closes the Run successfully
at the Intent stage, with equal before and after revisions and the terminal
facts the Intent supplies. A Continue Intent under Intent-Only configuration
is a wiring mistake, so the Run fails there with the error kind
`recipe_contract_error` instead of reaching a provider.

The Execution Plan phase builds its messages, calls the model, decodes the
Plan proposal through the exact captured registry, and derives the Execution
Plan. It leaves its Step Record open and hands the derived plan to the tail.

### Model calls and the typed crossings

A Run performs at most two invocations, and one helper performs both. It opens
the model-call record from the immutable request before the provider future
starts, then races that future against cancellation with a biased select
preferring the cancellation branch. Three outcomes produce three truthful
evidence shapes: an interrupted await keeps request-only evidence and invents
no attempt row; a provider failure keeps the facts the transport reached,
preserving its open error kind and the retryability it reported; a completed
call carries its response facts exactly once.

The typed crossing follows, and it is the Run's only untrusted input under
[the model output boundary](../../sergent/docs/trust-boundaries.md#first-the-model-output).
An Intent proposal decodes strictly into the exact application proposal type.
A Plan proposal decodes through the exact captured registry, which names the
offending Operation index and call when it refuses. A rejection attaches the
completed call record without a parsed proposal and fails at the call stage,
so the evidence shows what the model said and that nothing was derived from
it. The canonical Proposal Schema constrains generation and is never rerun as
a validator.

### The deterministic tail

The tail runs with no await points, which alone makes a commit against shared
live state atomic by construction.

One admissibility function serves both passes. It visits Operations in script
order, hands each validator a fresh isolated clone of the same pass Scene
through the Scene actions seam, and stops at the first rejection. The initial
pass uses the Run's base Scene; the rebase pass uses the current Scene with
the original Intent and Target. Each caller projects a separate vocabulary
from the same neutral rejection: the initial failure reads as a validation
error at the Execution Plan stage, while the rebase failure appears as the
rejecting check nested inside a merge conflict.

Recipe validation of ordering and whole-script legality runs after the initial
pass. Patch compilation follows; its evidence is minted from the compiled
Patch and later extended, never rewritten. Envelope validation then guards the
result: at least one Operation, a base identity matching the observed Scene,
and a Target that still exists.

Dry-run is the only ordered simulation. It clones the Scene once, applies the
whole ordered script through the Scene actions, then asks verification to
judge the complete before-and-after transition. When the selected authority
declares that the Scene data carries the revision inside itself, the rehearsal
checks the resulting identity against the expected advanced identity. An
application fault keeps its exact kind, message, and metadata; a framework
rejection names the check that rejected it, which keeps rebase evidence
machine-readable. Runtime control reads structured values only, never text.

### Scene authority, commit, and rebase

`SceneSource` selects the authority before the Run starts, and the
[writer model](../../sergent/docs/framework.md#writer-model-and-revision-policy)
decides which form an application may use.

A plain Scene is cloned at capture, so later caller aliases cannot change what
the Run observed; commit returns the rehearsed Scene with the revision that
Scene reports and makes no live comparison. The application is responsible for
the exclusion that makes this safe. Turn-based application state is the usual
fit, demonstrated by the [gomoku app](https://raw.githubusercontent.com/sergentbuild/sergent-rs-examples/refs/heads/main/gomoku/README.md).

`SceneState` is the shared live authority. One allocation holds the locked
Scene with its matching identity, the immutable stale policy, and whether the
revision advances beside the Scene data or inside it. Its four constructors
settle both choices before any handle exists, and every clone shares that
whole allocation, so no alias can install a different policy while keeping the
same mutation authority. The default policy rejects stale work unchanged.
Applications read live state through `identity` and `snapshot` and write
through `try_edit`, which checks staleness and revision capacity before
invoking the edit, preserves a domain rejection unchanged, and checks embedded
identity before installing.

Commit takes the lock only inside the synchronous tail, so it is never held
across a model await. When the current identity still equals the Run's base,
the already rehearsed Scene becomes the exact candidate. Otherwise the policy
decides: strict authority returns stale evidence, while rebase-capable
authority checks revision capacity and then asks the Scene for a replacement
Patch. The runtime performs the entire safety sequence that follows, as the
[rebase sequence](../../sergent/docs/execution-model.md#patch-rebase-on-shared-live-state)
requires: envelope and Target validation against the current Scene,
preservation of the complete ordered Operation identity sequence, a renewed
admissibility pass, and a dry-run against the current Scene. Only then does a
candidate exist, and one installation seam assigns the Scene and its identity
together. A rebase never calls a model and never accepts a replacement Target,
and every rejection leaves live authority unchanged.

The [ghoul app](https://raw.githubusercontent.com/sergentbuild/sergent-rs-examples/refs/heads/main/ghoul/README.md) demonstrates this form.

Three evidence rules keep a rebase honest: a conflict the Scene declares
embeds the original compiled Patch summary; a returned Patch failing a later
check embeds that rejected summary instead and nests the rejecting check
beside the Scene-supplied mapping; a successful replacement never rewrites the
Patch step's original evidence. The committed result also names how it was
reached, as a plain, an exact, or a rebased commit. Revision arithmetic is
checked in one place, and exhaustion at the maximum produces one structured
error shared by commit and application edits, so nothing wraps or saturates.

### Closing the Run

The Run Record builder keeps open step construction outside the appended
sequence of Step Records, the Run Record Ledger. Closing an open step appends
exactly one finalized element, and no appended element or nested model call
changes afterward. Patch evidence accumulates its reached facts and is
captured again on each addition, which is why a closed step never needs
reopening.

Evidence accrues in the order the parties produce it: the rendered Observation
and the selected Target during process input, the Target's only place in the
record; the derived Intent and its flow; the derived Execution Plan; the
compiled Patch summary, the envelope marker, and the dry-run identity; then
the commit kind and the exact Scene-supplied metadata.

Closing seals the record, emits terminal progress, and assembles one
`SergentResult` holding the terminal Scene, the last reached stage, the
terminal facts, the contained observer failures, and the closed `RunRecord`.
Expected Run failures live inside that result rather than in an outer public
`Result`, following the
[Sergent Result structure](../../sergent/docs/run-record-spec.md#the-sergent-result-structure).

## Architecture and Dependencies

Responsibility inside the crate falls into six groups: the configured runtime
value with its per-Run pipeline; the Scene authority surface, holding the
per-Run source, shared live state, and the rebase seam; the execution-safety
helpers of admissibility, envelope validation, dry-run, and commit authority;
the evidence group of clock sampling, Run Record assembly, and model-error
projection; the observation group of progress, observer slots, and ordered
delivery; and the opt-in Run Record harness. Machinery shared by two
boundaries stays neutral, returning typed evidence that each boundary projects
into a separate vocabulary, as the one admissibility pass shows.

### Working with the core crate

The `sergent-rs-core` crate holds the typed values and the three interfaces
that describe a domain, plus the canonical Proposal Schema machinery, the
Operation registry, and the inert Run Record shapes. It performs no I/O and
reads no clock. This crate holds everything that happens over time:
sequencing, the typed crossings, execution safety, commit authority,
cancellation, delivery, Run Record assembly, and persistence.

One question settles where a new fact belongs. A value an application can hold
and reason about belongs to the core crate; a decision made while a Run is in
flight belongs here. Three consequences follow. Core is clock-free, so this
crate samples every timestamp a record carries. Core mints the exact error
constructors, so this crate selects among them and never reads error text to
steer control flow. Core supplies the default Patch compilation and the
default admissible answer, so an application needing neither writes neither.

### Dependency direction

This crate depends on `sergent-rs-core` and on nothing else in the workspace,
in its tests as well as in its production code. Concrete transport reaches a
Run only through the core `ModelClient` interface, which is why
`sergent-rs-providers` is absent even from the development dependencies. The
rule keeps the engine honest: were a provider reachable from a runtime test, a
provider behavior could quietly harden into an execution rule, leaving one
fact with two authorities. Runtime tests therefore keep private fakes.
Applications depend on `sergent-rs` and never import this crate directly.

The four crates of this workspace are implementation layers, distinct from the
three application layers of the
[Sergent Vision](../../sergent/docs/framework.md#sergent-vision). During a Run
this crate drives the agentic Recipe and the algorithmic Scene actions without
becoming either of them.

### External crates

- `tokio` supplies the async primitives a Run needs: spawning a started Run on
  a task, racing a provider future against cancellation, and the notification
  a cancel request wakes. Shared live state instead uses a synchronous mutex,
  because the deterministic tail never awaits.
- `serde` and `serde_json` carry the two typed crossings, the JSON evidence
  projections, and the exact Run Record file lines.
- `thiserror` types the persistence failures an application matches on.
- `uuid` mints the default Run Record file identity.
- `schemars`, pinned by the workspace, lets the pass-through proposal sentinel
  derive a schema exactly as an application-authored proposal type does.

## Side Effects and Reliability

### Cancellation

`CancelToken` is a cheap clonable record of the first request time paired with
a notification. First write wins, so repeated requests keep the original
evidence time. The awaited form rechecks the request state around arming the
notification, which makes it deterministic under a parked scheduler instead of
dependent on timing.

Four checkpoints are polled: after Target selection at the end of process
input, after Intent validation and before the flow gate, before dry-run, and
immediately before commit. Each in-flight provider invocation is raced
separately and records task cancellation. After the commit checkpoint nothing
interrupts installation, and a synchronous apply or verification is never
interrupted once it begins. `RunHandle::cancel` trips the same token; it
aborts no task and creates no resumable Run.

The runtime keeps no whole-Run timer, since per-request settings bound one
call only and an application decides the deadline. To obtain a closed
result once that deadline passes, keep the pending Run, request cancellation,
and await that same Run through settlement; dropping the work forfeits both
its result and its record.

A panicking task is process control. The result surface resumes the panic
rather than converting it into a Run error, and a commit that already happened
remains authoritative.

### Clock sampling

Core is inert and clock-free, so this crate samples every time fact. Each
sample pairs a wall-clock endpoint with a monotonic reading, and a closed span
reports the monotonic elapsed time, so a wall clock stepping backward cannot
yield a negative duration. A host clock before the Unix epoch is a
process-level environment failure, never an epoch-zero timestamp.

### Observation

One delivery value bundles the shared progress cell with the ordered observer
slots and collects contained failures beside the record. Closing the Run emits
terminal progress, builds the result carrying every progress failure, then
passes that accumulating result through the terminal slots, so later observers
and the caller see the growing collection, as
[observer delivery](../../sergent/docs/observability.md#observer-delivery)
requires.

`RunObserver` is object-safe with defaulted no-op callbacks, so an application
implements only the observation it wants. An ordinary returned error is
contained and named; a Rust panic inside a callback stays process control and
escapes. `ProgressSnapshot` is the exact portable five-field projection, and
its queued form precedes Scene binding. Its sanitization comes from structure
rather than filtering: it cannot hold a prompt, a model response, or Scene
content.

### Optional persistence

`JsonlRunRecordWriter` is the only Run Record harness, and it is opt-in: a Run
builds its `RunRecord` whether or not anything writes it down. The harness
joins a Run as an ordinary observer slot, so it gains no execution authority,
and its file sits behind a private seam, so deterministic tests script
persistence failures without touching a filesystem.
[Run Record persistence](run-record-harness.md) covers creation, the event
conversion graph, the writer guard, and the platform permission rules.

### Test policy

Runtime tests use private fakes for every interface they exercise. The crate
reaches no live service, no real credential, no network, no mutation of the
process environment, and no sleep-based timing. The shared test support
module groups reusable fakes by the interface each implements: Scene actions,
model clients, Operations, Recipes, rebasers, Scenes, and observers. A fake
that exists to probe one scenario stays in the test that needs it.

Concurrency is proven with a parked model client. It signals that a Run
entered the provider boundary, then waits on a release gate. Arrange every Run
at that boundary, then release them in the order that proves stale rejection,
deterministic rebase, cancellation, or progress. A timeout or a scheduler
delay proves no interleaving.

Each promise is proven at the party that makes it. Configuration tests prove
one-time capture and the exact request facts. Flow tests prove reached stages,
pass-through, Stop, the no-target end, and each validation rejection at its
stage. Model-output tests prove the crossing keeps complete call evidence with
no parsed proposal. Execution-failure and isolation tests prove clone
isolation, the first admissibility rejection, Patch evidence, and unchanged
state on failure. Live-state, rebase, and exhaustion tests prove atomic
snapshots and edits, strict rejection, the complete rebase safety sequence,
embedded identity, and failure precedence. Cancellation, handle, observer, and
Run Record file tests cover every checkpoint, delivery containment, record
closure, and persisted boundaries.

Malformed model data enters through the real crossing. Never hand-build an
impossible trusted value to keep a defensive branch alive; when a test needs
one, question the branch, following the
[three-question method](../../sergent/docs/trust-boundaries.md#the-method-to-discard-redundant-validation).

Run `just test` and `just lint` from the workspace root; both must pass before
a change is finished. `just fmt` formats the Rust code in place.
