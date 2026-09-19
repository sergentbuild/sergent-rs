# Execution Model Specification

This document traces one Sergent Run from its trigger (Begin) to its Sergent Result (End).
It defines the ordered flow, stages, terminal behavior, and commit behavior.
The [terminology document](terminology.md) formally defines the vocabulary used here, and the [framework specification](framework.md) explains how the pieces work together in the big picture.
A builder new to Sergent should acquire a good understanding of the [overview](KNOWLEDGE.md) and the framework specification first, then use that learning to unlock this document.

## A Sergent Run: Async and Transactional

Think of a Sergent Run as an asynchronous function call with transactional Scene effects.
Its data-safety guarantees are inspired by database transactions:
at commit, all Scene effects succeed together. Otherwise, the runtime discards
them and leaves the Scene unchanged. There is no partial Scene mutation to
clean up later.

## The Sergent Run flow

One Run moves through the following steps.

```text
user activity
-> (a condition or a user action that triggers) a Sergent Run
-> MindBuf update and bounded Scene context
-> select and create Target object; if none, terminal failure with no mutation, then return
-> obtain a typed Intent Proposal, which is one of:
     an app-defined object holding the model output         [model call]
     a runtime-provided deterministic variant (no model call)
-> derive and validate the Intent
-> Intent flow chooses Continue or Stop
-> if Stop: terminal success with no mutation, then return
-> if Continue: the model proposes Plan Proposal            [model call]
-> derive the Execution Plan from the Plan Proposal
-> initial admissibility pass: check each Operation against the stable base context
-> the Recipe validates the Operation order and the semantics of the whole Execution Plan
-> compile the validated Execution Plan into a Patch
-> Patch validation: check the Patch envelope and the bounded Target
-> dry-run: simulate the Patch on a scratch copy of the Scene, then verify the resulting Scene
-> commit: apply the Patch for real
-> close the Run Record and build the Sergent Result
-> return to the caller
```

The `[model call]` tag marks the only two steps that may call the model. Every
other step is deterministic.

Sergent is built around the model: a Run that does real work calls the model at
least once. In the typical case, a Run makes one or two model calls:

- Two calls, when the model proposes both the Intent and the Execution Plan.
- One call, when the runtime supplies the Intent Proposal itself (the deterministic
  pass-through variant in the flow above), the Recipe derives a **Continue** Intent from
  it, and the model proposes only the Execution Plan.
- One call, when the model proposes an Intent that decides to **Stop**, so the Run
  needs no Execution Plan.

A Run makes no model call only when it ends before reaching the first one: Target
selection finds no Target, a cancellation stops it before the first call, or its
pass-through Intent decides to **Stop**. That last outcome is technically possible but
offers no practical value. Pass-through is intended to fast-forward to model-backed
Execution Planning. These are early exits from a model-centered Run.

A Run can use both phases, or it can be Intent-Only or Execution-Plan-Only. This
application-configured choice is the Run Kind.

Intent-Only Runs only allow the model to express ideas and information but not Scene
modifications: the Recipe has no Operation Registry, so only a **Stop** Intent succeeds, and a
validated **Continue** Intent fails the Run before any Execution Plan call. The use cases
include reviews, planning, ideation, and workflow decision.

Execution-Plan-Only Runs skip the model-backed Intent phase (the runtime supplies the
deterministic pass-through Intent Proposal) and ask the model to directly propose concrete
Scene modifications. The use cases are typically straightforward Scene
actions that do not benefit from dedicated assessment and analysis effort (i.e., the Intent
phase). Note, the Intent phase is Sergent's counterpart of the "Plan Mode" of today's
coding agents; the Execution Plan is unrelated to "Plan Mode". The assessment and
analysis effort (which costs more tokens) is best saved for more complex Scene modifications.

The next sections walk through each step. The edge-case terminal situations and
the concurrency rules come afterward.

## A step-by-step walkthrough

### Observation assembly

The Run first refreshes the application-curated MindBuf (see its definition in the
[terminology document](./terminology.md)). It then combines the
MindBuf with the bounded Scene context to form the Observation.

The application configures the Run Kind and Target selection policy before the
Run. These choices control execution; they are not observations and do not
enter through the MindBuf.

### Proposal handling during execution

Both stages that may call the model follow the framework's
[model-only-proposes rule](framework.md#the-model-only-proposes). Provider
output never directly mutates the live Scene. The runtime first decodes that
output as typed proposal data. It then derives and validates the corresponding
Intent or Execution Plan objects.

Only a validated Execution Plan may execute (analogy to a small programming script).

Every model-backed request carries the canonical `Proposal Schema` from the
[framework specification](framework.md#the-canonical-proposal-schema). The
schema constrains generation and improves the model output's type-conformance.
It is important to remember that any provider output alone remains untrusted.

Prompt composition follows the framework specification's rules for model-visible
context ([semantic rules ride the schema](framework.md#semantic-rules-ride-the-schema)).

### Intent resolution

Intent answers the Run's first question: should the Run **Stop**, or should it
**Continue** toward a mutation? Intent derivation always receives a typed Intent
Proposal. The mode determines where that proposal comes from:

- For model-backed Intent, the application defines the domain-specific
  Intent Proposal type, and the model supplies its data.
- For deterministic Intent, such as pass-through, the runtime implementation
  supplies a generic Intent Proposal variant and makes no model provider call.

**Runtime-provided variants remain proposals**. In both cases, the Intent Proposal passes through the same Recipe derivation rule and Intent validation boundary, and it never gains mutation
authority.

Note: a conforming framework may provide a minimal **Continue** Intent value.
Applications directly use it or extend it with decision data. That decision data is what
the Execution Plan request and Execution Plan derivation consume.

A validated **Stop** Intent successfully ends the Run at the Intent stage. The Run
makes no Execution Plan call and creates no Patch. The Scene stays unchanged, and
terminal metadata stays bounded.

Only a validated **Continue** Intent reaches the Execution Plan phase. Therefore, every Run
that produces a mutation **has both a validated Intent and a Plan Proposal.**

### The Execution Plan phase and the Operation Registry

In the Execution Plan phase, the model proposes the Plan Proposal. Its Operation
data uses the Programming Interface's Operation vocabulary. The Execution Plan MUST NEVER
directly access the Scene data model (it is similar to how a script using the Python
binding of a C library is forbidden to directly touch the internal data structures).

The framework's [Operation Registry and Plan Proposal envelope
section](framework.md#operation-registry-and-the-plan-proposal-envelope) defines the
closed Operation set, schema and envelope derivation, generic registry checks,
typed decoding, and Operation-count bounds. The envelope contains at least one
Operation and respects the optional configured maximum. During the Run, the
framework uses the captured registry to decode the proposal. Application code
defines semantic preconditions that require Scene, Intent, or Target
knowledge.

Each decoded Operation must pass the admissibility check invoked by the
runtime. The derived Execution Plan must pass the Recipe's validation of the
Operation order and the whole Execution Plan. The resulting Patch must pass
execution safety before it can become a committed mutation.

Under the framework's [registry rule](framework.md#operation-registry-and-the-plan-proposal-envelope),
a Recipe without a registry is Intent-Only. It has no Execution Plan schema and no Execution
Plan request. A derived **Continue** Intent without a registry is an application mistake: the
runtime fails the Run before any Execution Plan provider call. A pass-through Intent Proposal
has no Intent Proposal schema and no Intent Proposal request.

Runtime bookkeeping identity, the Run ID and the Operation IDs that the framework assigns under
its [Operation identity](framework.md#operation-identity) rule, lives outside the context
visible to the model and MUST NOT appear in the canonical `Proposal Schema`
(see the framework's [canonical schema section](framework.md#the-canonical-proposal-schema)). At the typed
crossing, the framework rejects both an echoed Operation ID and a top-level Run ID as unknown
fields.

### The Target during execution

Under the framework's [Target ownership](framework.md#target-ownership) rule,
the Target is the runtime's authoritative handle for the execution subject.
Target selection establishes which Scene portion, resource, entity, or other
application-defined subject the Run is allowed to evaluate and modify. It also
provides the Target-specific execution context needed to interpret domain
rules and safely apply Operations. Target selection is therefore part of
execution setup, not an incidental value reconstructed by individual
Operations.

Only the Target chosen at Run start carries the selected Target identity and
Target-specific execution context. These values are not Operation operands, and the
application MUST NOT copy them into Intent, Execution Plan, or Operation merely for
deterministic application. Every consumer respects the Target as both read-only and ephemeral. A derived domain fact may remain in Intent or Execution Plan only when it has decision meaning independent of the Target.

The runtime and the application use the Target to:
- provide the subject and context required to validate Intent and the
  Execution Plan;
- let Operation Admissibility checks evaluate Target-specific preconditions;
- resolve Target-relative references and business rules;
- supply the subject and context needed to simulate and apply Scene effects; and
- preserve a consistent execution subject across validation, dry-run, and
  commit.

The Target itself does not grant an Operation permission to bypass validation,
choose a different subject, directly mutate the Scene, or change the Run's
execution context. Operations express requested actions; the Target supplies
the stable subject and context against which the runtime interprets those actions.

The runtime passes that same Target to Execution Plan validation, dry-run, commit,
the Scene's apply step, and each Operation. It never reselects or reconstructs the
Target during execution. If Target-specific state changes during execution,
the runtime follows the applicable Scene and revision rules rather than
silently replacing the Target or changing the Run's subject.

### Operation Admissibility flow

**Operation Admissibility** determines whether one typed Operation is compatible
with the Scene, the validated Intent, and the selected Target. An
**admissibility pass** is one ordered traversal that individually checks the candidate
Operations. The runtime invokes each validator once per visited
Operation and stops the pass at the first rejection.

Within each pass, the runtime uses one stable, bounded context. Each validator
receives a fresh, isolated copy of the same Scene snapshot for that pass, the
original validated Intent, and the exact Target selected at Run start. All
application-defined fields remain intact. Earlier Operations never create an
evolving validation Scene or affect the context seen by later validators.

A validator MUST be deterministic and read-only. It may reject its Operation.
It MUST NOT mutate the Scene, Intent, Target, Operation, or Execution Plan. It
MUST NOT filter or replace an Operation, bind runtime facts, or apply an
Operation. An Operation with no additional per-Operation precondition is
admissible by default and needs no custom validator. The runtime still checks
every Operation it visits.

The initial pass follows typed Plan Proposal decoding and Execution Plan
derivation, before the Recipe's Execution Plan validation. It visits the
Operations in Execution Plan order against the Run's base Scene. If all pass,
the runtime proceeds to Recipe validation and the remaining execution-safety
steps below.

One successful pass establishes compatibility with one Scene snapshot. If
shared live state rebases a stale Patch onto a changed Scene, the runtime must
perform another pass against that current Scene. Even unchanged Operations
need this check because their preconditions may no longer hold. This is
revalidation for a changed Scene, never an iterative repair of rejected
Operations or a model retry. The
[Patch rebase sequence](#patch-rebase-on-shared-live-state) defines the second
pass's placement and failure behavior.

If the initial pass rejects an Operation, the Run ends with status `failure`
at stage `execution_plan` and error kind `validation_error`. The structured
error identifies the Operation index, call discriminator, and authoritative
Operation ID. The Run's base Scene remains unchanged; Patch compilation,
dry-run, and commit do not occur. An unexpected ordinary exception becomes an
`internal_error`. Process-control exceptions escape containment, as the
[observability specification](observability.md#observer-delivery) defines. Operation
Admissibility does not introduce a model retry.

### Verification, dry-run, and commit

A successful admissibility pass does not authorize mutation or establish the
legality of the whole Execution Plan. The remaining safeguards have separate
responsibilities:

- The Recipe checks Operation ordering, cross-step interactions, budgets, and
  whole-Execution-Plan legality.
- Patch validation checks the Patch envelope and the bounded Target.
- Operation application and the Scene's Target-aware apply step keep their own executable
  checks, such as an Operation rejecting an operand that makes its own mutation undefined.
- Dry-run is the only framework phase that applies the full Patch, in order,
  to one isolated and evolving Scene copy.
- Application-defined deterministic Scene verification checks complete
  before-and-after domain invariants that a generic runtime cannot know and returns a
  Verification Report; any issue in the report fails the dry-run.
- Commit authority controls final mutation and revision policy. Only a
  successful dry-run may reach the step that commits the Patch's effects.

The runtime performs Patch validation, dry-run, and commit through the plain
or shared live Scene authority selected by the application.

## Scene Actions in Operations and Execution Plan

Operations in Execution Plan use the deterministic Scene Actions defined in the
[terminology document](./terminology.md). The Scene business state and
runtime-facing actions may be the same object or separate objects; the flow
does not change. The application receives the exact selected Target during
apply.

## Cancellation and the Sergent Result

When a Run reaches a terminal outcome and can return an in-process result, the
runtime closes the `RunRecord` and builds the Sergent Result. Alongside the
Run Record, the result presents the terminal Scene, the last reached stage, the terminal
message and metadata, and the contained observer delivery errors. The `RunRecord` remains the
framework's only durable evidence.

Cancellation is a first-class runtime outcome. When cancellation reaches the
runtime before the deterministic commit boundary, the returned result has
terminal status `cancelled`, a cancellation-shaped Run error, a cancelled
`RunRecord`, and the unchanged input Scene.

The runtime contains unexpected runtime exceptions as structured failures. It
presents the error message in both the foreground result and the `RunRecord`.
A process-level failure that prevents result creation cannot produce a
framework Run end with a missing Run Record.

## Stages and Status

The runtime reports these progress stages, which mirror the Run flow:

```text
queued -> started -> intent_call -> intent -> plan_call -> execution_plan -> patch -> dry_run -> commit
```

The `intent_call` stage appears only for model-backed Intent. A pass-through
Intent Run advances from `started` directly to `intent` and validates the
Intent. It then uses the same `plan_call` and deterministic stages as any other
Run.

A **Stop** Intent Run succeeds at `intent`. Its `RunRecord` records
`revision_after` equal to `revision_before` and preserves provider-call facts
gathered before the **Stop** decision. A pass-through **Stop** Run makes no model call, so it
has no provider-call facts.

Terminal status is separate from stage: `success`, `failure`, or `cancelled`.
If Target selection returns no Target, the Run terminates with `failure` at
`started`, before either proposal phase, and leaves the Scene unchanged. Its Run error is the
no-target error that the Recipe declares.

## Run Record ownership and observer delivery

The runtime is responsible for validation, Patch application, any selected
live-state stale comparison, and the application-agnostic `RunRecord`
for a Run. The runtime and application share responsibility for durable
persistence. The runtime returns the terminal Scene, the complete Run Record, the terminal
convenience facts, and the observer delivery errors. The application decides whether and
where to persist the Run Record beside its Scene data.

Observer delivery MUST NOT control execution. An ordinary observer callback
failure never changes status, stage, mutation, cancellation, or commit
behavior. One failing observer does not block later observers. The returned
result names each contained delivery failure.

Delivery errors stay beside the Run Record, never inside it, because a terminal
observer can fail only after the Run Record has closed. The [observability
specification](observability.md) and [Run Record specification](run-record-spec.md) define the full delivery contract, the
`RunRecord` structure, capture rules, and progress snapshot fields. The
[Run Record file format](run-record-file-format.md) defines the portable persistence format.

## Run identity through execution

The stable [Run ID](./terminology.md#run-id) remains unchanged through live
progress, the `RunRecord`, Run Record file boundaries, and committed changes. Within
the Run Record, Patch-step Operation IDs and trace rows correlate deterministic
action evidence under that authoritative Run ID.

## Progress tracking

An async caller should be able to start a Run and inspect its progress without
awaiting completion. A sanitized progress snapshot is one way to do this. Each
implementation chooses the mechanism, but the snapshot contract is fixed.

Progress snapshots expose only sanitized Run metadata. The
[observability specification](observability.md#progress-snapshots) fixes the exact fields: Run
ID, Scene ID, stage, status, and Scene revision. The portable
[Run Record file format](run-record-file-format.md) embeds the snapshot verbatim, so
an implementation must not add or remove fields. Implementations may provide
richer live-inspection facts outside the snapshot. Progress never exposes
prompts, Scene content, or raw model responses.

## Construction-time capture

The runtime fixes its model-facing structure before a Run begins. At
construction, it captures the exact Intent Proposal type, optional registry,
optional maximum, and every reachable schema.

The Recipe's request builder receives the captured canonical schema. The model
request it returns MUST carry exactly that schema, unchanged. The builder never
substitutes a re-derivation, a variant, or a weakened form. Before request
capture or provider selection, the runtime verifies that the request's schema
is the captured one.

Captured schemas are trusted application data. The runtime adds no recursive
immutability or deep snapshot defense. Each implementation defines the
language-binding rule for the concrete unchanged-schema check. The runtime
does not reread Recipe configuration during a Run.

## Step failure and mitigation

If any step before commit fails, the whole execution ends. Commit runs
**only** after every previous step succeeds. Failure criteria depend on the
step and must account for network, IO, and other external risks.

A step may retry or use another mitigation only if the mitigation preserves
the step's contract. A step's contract is its purpose and responsibility
together with its input and output: after the mitigation, the step still
receives the same input, still produces the same kind of output, and still
keeps every rule it must enforce. Mitigation MUST NOT weaken a model request.
A retry keeps the same messages, the same canonical `Proposal Schema`, the
same provider translation, and the same native schema field. It never
switches to prompt-only structure, generic JSON-object framing, or a reduced
Operation set.

## Revision, writer model, and commit behavior

Revision policy follows the [writer model](framework.md#writer-model-and-revision-policy) in the
framework specification. Every
Run captures a base revision, which binds the Patch to the snapshot that the
Run observed. This revision appears in progress. The `RunRecord` records
the Scene revision before and after the Run. These Run facts do not compare
against unrelated live changes.

Passing a plain Scene creates a snapshot clone. The Run cannot observe
foreground changes after it starts. Commit applies to the isolated snapshot
without a current-state comparison. A serial writer therefore serializes whole Runs,
from Observation through commit: one Run, one writer. A serial or turn-based application
excludes other Runs and user writes to the same authority throughout that interval.
Serializing only commit writes does not make overlapping Runs serial.

Passing shared live state selects live stale protection. At commit, the
runtime compares the Run's base revision with current state. Strict revision
equality is the default: if the base revision is no longer current, the Run fails
without mutation. For concurrent Runs sharing the same Scene, the application chooses
this strict rejection or the application-defined deterministic rebase described below.

## Patch rebase on shared live state

Applications that can deterministically preserve overlapping work may provide
application-defined Patch rebase on shared live state.

The runtime first validates and dry-runs the original Patch against the base
snapshot. It asks the Scene to rebase only when the live revision has advanced.
When the Scene returns a rebased Patch, the runtime validates its envelope and
Target. It then performs a second admissibility pass in rebased Patch order,
including unchanged Operations. This pass follows the
[Operation Admissibility rules](#operation-admissibility-flow) using the current
Scene snapshot, the original validated Intent, and the exact original Target.
The runtime never reselects the Target. The pass must accept every returned
Operation before execution can continue.

A successful pass proceeds to dry-run against current state. A rejection
produces a structured `merge_conflict` at the commit stage. It leaves the
current Scene unchanged and does not cause a model call. Commit still uses
revision-checked authority. The framework reports merge conflicts as possible rebase errors;
the application decides how to resolve them. The runtime supplies no domain conflict resolver
and never asks the model to resolve a conflict during commit.

## Revision representation

Revision representation is independent of stale protection. The revision may
exist outside the Scene data: shared live state then advances it beside that
data. An application may instead declare that the Scene data embeds the
revision. When the application selects embedded identity consistency, the
resulting Scene must keep the same Scene ID and advance the revision of the Scene base
actually committed by exactly one. Otherwise, the runtime rejects the commit.
After rebase, this is the current Scene's revision, not the Run's original observed revision.
The Run Record preserves that original observation.
The mere presence of a revision field selects neither shared live state nor embedded consistency.

## Per-call sizing

Each model-calling step should set output-token, timeout, and thinking-effort
bounds that match the approximate size and latency of its codified output. A
small codified Intent needs tighter settings; a codified Execution Plan
needs larger settings. Per-request output-token, timeout, and thinking-effort
controls provide this budget. The application builder is responsible for choosing
values suitable for the domain. The framework provides the controls but does
not enforce whether those values are suitable.

## Async execution

A production-ready runtime should offer an asynchronous form of the Run built
on the language's concurrency mechanism. The async form lets a caller start a
Run and keep serving users while the Run waits on the model. Model calls are
the Run's natural suspension points. The deterministic steps between them need
no suspension.

The async form changes scheduling only. It does not change the Run flow, the
validation chain, or the commit authority defined above.

## Concurrency

Async runtimes may concurrently schedule many Runs. They isolate disjoint
authority at its real conflict boundary. When overlapping writers can harm the
same authoritative data, shared live state holds the current Scene under a
lock. It then applies the strict or deterministic-rebase policy selected
above. The runtime must not hold the lock across a model await.
