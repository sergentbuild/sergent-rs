# Observability Specification

When a Sergent Run ends, its caller needs more than "it worked" or "it failed."
The caller needs to know which Scene revision the Run observed, which steps it
reached, what evidence each step produced, and how the Run ended. The
**Run Record** keeps these facts in execution order. Think of it as a ledger:
the runtime appends to it and never deletes or mutates an element, so it holds
faithful data. The analogy does not prescribe a financial-ledger data model. In
a programming context, "Run Record Ledger" names exactly its sequence field of
Step Records, which the [Run Record specification](run-record-spec.md) defines.

Just like valuable logs from production systems, the Run Record holds valuable
information for observability after the call returns.

This document defines observability for conforming implementations. It
specifies the data conventions, observer delivery, and progress data. The
[Run Record specification](run-record-spec.md) specifies the exact
Run Record structure, and the [Run Record file format](run-record-file-format.md) defines the
portable persistence format for these structures.

The Sergent Specification expects independent implementations (in different programming
languages) to preserve the same framework-owned record structure for equivalent Runs.

However, this specification also marks certain captured values as
implementation-native, meaning they may differ among implementations.

The [execution model](execution-model.md) defines the Run flow, stages, and
terminal statuses that the Run Record records. Observability exposes these
framework-owned facts through the `RunRecord`, progress snapshots, and
portable Run Record files. It is important to treat persisted records as evidence for inspection,
never as "runtime values". Neither the framework nor the application must ever parse them
back into the in-process objects.

## Ownership

Responsibility cleanly divides between the Sergent Runtime and the application.

The runtime is responsible for execution tracing that does not depend on an
application's domain. It provides a stable Run ID for each Run, one ordered
`RunRecord`, and structured terminal facts beside the Run Record. It guarantees
that the observer delivery never interferes with the runtime. It also offers an opt-in
Run Record harness.

On the other side, the application sets the surrounding policy. It chooses the
log directory, the log identity, retention, redaction, and the meaning and timing
of application and user events.

If applicable, the application should also decide how to durably persist its concrete Scene
data. The Sergent Framework never persists Scenes.

The `RunRecord` is the framework's only durable evidence.

The current design constraint is that one Sergent Run must use one model,
but different Runs may use different models. Therefore, the Run Record stores
the name of the chosen model, even if the Run fails before its first model call.
This records the configured model for a model-centered Run even when no call is reached.

In the case of an unhandled exception, such as a process-level failure that
"crashes" or short-circuits the execution before the runtime can build a Sergent
Result, there is no result and no Run Record. The application records such a failure
as application activity; it is never a framework Run end.

## Data conventions

These conventions define how to write every value in this document, in the
[Run Record specification](run-record-spec.md), and in the
[Run Record file format](run-record-file-format.md). They are normative.

### Identifiers

Framework identifiers match `^[a-z][a-z0-9]*_[0-9a-f]{32}$`. Run IDs use
prefix `run`, Operation IDs `op`, and the harness's default log ID `log`; Scene IDs use any
conforming prefix. An application-configured log ID follows the filename grammar in the Run
Record file format instead. Provider request IDs are provider-native text with no
framework grammar.

### Timestamps

Timestamps are UTC ISO-8601 with exactly six fractional digits and a literal
`Z` suffix, for example `2026-08-28T09:41:02.881500Z`.

### Durations and latency

Durations and latency are whole non-negative integer **milliseconds**; a
sub-millisecond span is `0`.

### Revisions

Revisions are non-negative integers.

### Token counts

Token counts form one object with string keys and non-negative integer
counts. The reference keys are `input` and `output`; `output` is the key that the
[Run Record specification](run-record-spec.md#model-call-records)'s token aggregation rule
sums. When the provider reports no usage, the object is null. It never contains
guessed partial counts.

### Types and fields

Every declared field of every typed record serializes on every export. An
absent optional value serializes as JSON `null`, never as an omitted key.
Only two areas omit keys: the three correlation keys in a Run Record file line
envelope, and captured projections whose capture rule makes a key
conditional, such as `images` in a message projection.

### Sergent Vocabularies (status, stages, etc.)

Sergent's closed vocabularies allow exactly the listed values: the statuses and stages in the
[execution model](execution-model.md#stages-and-status) and the progress snapshot section
below, and the step names, flows, commit kinds, and cancellation checkpoints in the
[Run Record specification](run-record-spec.md). The error `kind`
vocabulary remains open, with a reserved framework subset.

### Diagnostic names

Diagnostic name fields are implementation-native: a captured value's
`value_type`, the `type` key of degraded values, and the observer and
exception type names inside error metadata. Conforming implementations must
preserve their presence and placement, but their spelling need not match.

## Run Record specification

See [Run Record specification](run-record-spec.md)

## Observer delivery

**Observers** let other code watch a Run without controlling it. Configured
observers form ordered slots. Delivery follows the supplied order. A repeated
object creates a repeated slot. Every slot receives every callback even if an
earlier slot fails.

Delivery for one Run follows this exact sequence:

1. One progress per reached stage. The first is the `queued` progress, which the runtime
   delivers before the pipeline starts.
2. One terminal progress carrying the last reached stage and the terminal
   status. The stage does not advance at termination.
3. One terminal callback, the `finished` callback, carrying the Sergent Result.

The framework contains an ordinary callback exception, including a
cancellation-shaped exception directly raised by a callback. The exception
becomes a Run error of kind `observer_error` in the result's
`observer_errors`. The framework limits its message to 2048 characters; the message names
the callback and the failure. Its metadata contains exactly `callback` (`progress` or
`finished`), `observer_type`, `exception_type` (both implementation-native
qualified names), and `stage`.

Process-control exceptions, such as keyboard interruption and system exit
equivalents, remain outside containment and may escape. A commit that already
happened remains authoritative. Containment never changes the stage, status,
cancellation, Scene mutation, or Run Record. It also never reverses side effects
that an observer performed through captured references.

The framework attaches errors contained during terminal progress delivery to
the result before the terminal callback. Terminal-callback errors join the
same collection. Later terminal observers and the caller therefore see one
accumulated list.

## Progress snapshots

Progress provides a small, sanitized view of a Run while it advances. A
**progress snapshot** contains exactly five fields: `run_id` (string), `scene_id`
(string or null), `stage` (string), `status` (string), and `revision`
(integer). Status is `queued`, `running`, `success`, `failure`, or
`cancelled`.

Sanitization comes from structure. The snapshot contains only these facts, so
it cannot leak prompts, model output, or Scene content. `scene_id` is null and
`revision` is `0` until the Run binds Scene identity. The `started` snapshot
contains the bound identity.

## The portable Run Record file format

Persistence is optional and has a separate boundary. The opt-in Run Record
harness writes these structures to a JSON Lines file. The file contains
framework `run.start` and `run.end` boundary lines plus application and user events,
with the complete `RunRecord` inside each Run end. The
[Run Record file format](run-record-file-format.md) defines the line encoding,
line envelope, event rules, and file discipline.

## Sensitivity

The detail that makes the complete Run Record useful also makes it sensitive forensic
data. The result and every persisted Run end can contain prompts, model
output, model-visible Scene projections, the canonical `Proposal Schema` of each model call
with its application descriptions, proposal payloads, and Patch facts.

Progress is the sanitized view. Applications are responsible for the log
directory, access, retention, and redaction.
