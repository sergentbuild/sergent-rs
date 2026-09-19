# sergent-rs-runtime

The execution engine of the Sergent Rust reference implementation: the crate
that turns a model's suggestion into one safe, fully evidenced attempt to
change application state.

## Introduction

Sergent is a concept for agentic software in which a model only proposes a
change and deterministic code decides what actually happens. The
[Sergent Specification](../sergent/docs/KNOWLEDGE.md) records that concept as
a rulebook, and this crate implements its
[execution model](../sergent/docs/execution-model.md) in Rust.

A model is good at choosing an action, and deterministic code is what keeps
state correct. Sergent settles that division with one rule:
[the model only proposes](../sergent/docs/framework.md#the-model-only-proposes).
This crate is where that rule becomes running code. It sequences a Run, admits
model output through typed crossings, rehearses every effect on an isolated
copy of the Scene, and holds the sole authority to commit.

The unformatted word Sergent names the concept. The Rust `Sergent` type is a
configured Sergent Instance backed by the Sergent Runtime: one value that
captures its configuration once and then drives many independent Runs.

This crate sits at the center of the workspace. It depends only on
`sergent-rs-core`, never on a provider implementation, and applications reach
it through the `sergent-rs` crate rather than importing it directly.

## A Run in the Rust Implementation

A Run is an asynchronous call with transactional Scene effects. Two entries
start one. `Sergent::run` awaits a single Run. `Sergent::start` schedules the
same pipeline on a task and hands back a `RunHandle` offering progress,
cancellation, and the awaited result.

Every Run has one shape: three asynchronous phases, then one synchronous tail.

- Process input captures the Scene the Run observes and selects the exact
  Target it may change.
- Intent resolution obtains a typed Intent proposal, derives the Intent, and
  validates it. A Stop Intent ends the Run here, successfully, with no change.
- The Execution Plan phase asks the model for a Plan proposal, decodes it
  through the registered Operation vocabulary, and derives the Execution Plan.

At most two of those phases call a model, and both treat the answer as data to
decode rather than as an instruction to carry out. The synchronous tail then
checks each Operation for admissibility, lets the Recipe judge the whole
script, compiles a Patch, validates its envelope, rehearses the complete Patch
on an isolated Scene copy, and commits. Nothing in the tail awaits, which
makes a commit against shared live state atomic by construction rather than by
discipline.

The application picks the Scene authority for each Run: a plain snapshot it
alone writes, or shared live state that compares revisions under a lock and
either rejects stale work or asks the application to rebase it
deterministically. Cancellation is cooperative and observed at fixed
checkpoints before commit.

Each Run returns one `SergentResult` carrying the terminal Scene, the last
reached stage, the terminal facts, any contained observer failures, and the
closed `RunRecord` that holds the complete evidence.

Domain meaning stays outside this crate. The Recipe explains what a proposal
means, Scene actions perform and verify the change, and the model client
handles one transport call. The runtime sequences them and decides what may
become state.

## Navigation Map

Read the [runtime knowledge file](docs/KNOWLEDGE.md) for the complete study:
the end-to-end walkthrough, the internal structure and its dependencies, and
the side-effect and reliability rules.

The specification is the authority over everything here. Read it for a rule,
then read the knowledge file for the Rust design that carries the rule.

- [Framework](../sergent/docs/framework.md): the vocabulary, the canonical
  Proposal Schema, admissibility, and the writer model.
- [Execution model](../sergent/docs/execution-model.md): the ordered Run flow,
  stages, cancellation, concurrency, and the rebase sequence.
- [Trust boundaries](../sergent/docs/trust-boundaries.md): the five places a
  value may enter a Run, and the method that deletes checks guarding nothing.
- [Observability](../sergent/docs/observability.md): observer delivery and the
  progress snapshot fields.
- [Run Record](../sergent/docs/run-record-spec.md): the record, the Step
  Record, and the result structures.
