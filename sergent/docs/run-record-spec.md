# Run Record Specification

## The Run Record

Every **Run** that produces an in-process result closes with exactly one `RunRecord`. A
process-level failure that prevents the Sergent Result yields none, as the
[observability specification](observability.md#ownership) explains.

This `RunRecord` object is the runtime's write-only evidence, not an active runtime value.
It never participates in or interferes with the runtime's execution flow. It quietly
records what happened.

### Anatomy of the RunRecord object

The `RunRecord` object holds an ordered list of Step Records that captures the Run's execution
evidence: the **Run Record Ledger**. Records appear in execution order, and that order is preserved
throughout the Run Record Ledger.
Because each record's position conveys its sequence, records do not contain an index or sequence field.

The `RunRecord`'s top-level fields are:

- `run_id` (string): the framework Run ID.
- `model_name` (string): the caller's full `provider/model` selection.
- `timing` (time span).
- `scene` (Scene transition or null): null only before the Run has begun.
- `steps` (array of Step Records, in execution order): the **Run Record Ledger**.
- `outcome` (Run outcome).
- `cancellation` (cancellation record or null): non-null only on a cancelled
  Run.

A closed `RunRecord` contains finalized evidence. Every Step Record and the Run
outcome has a terminal status (`success`, `failure`, or `cancelled`). Before
closing the Run Record, the Sergent Runtime finalizes any open Step Record with
the Run's terminal status and, when applicable, the corresponding error.

The Run Record harness persists this closed object at `run.end`. At `run.start`,
it persists the `started` progress snapshot, which may carry `running`. See the
[Run Record file format](run-record-file-format.md) for both framework boundaries.

### Time span

- `started_at` (timestamp).
- `finished_at` (timestamp or null).
- `duration_ms` (integer or null).

A closed time span answers two questions: when did the work happen, and how
long did it take? `finished_at` and `duration_ms` are either both null or both
present. The Run and every step each contain one time span.

### Scene transition

- `scene_id` (string): the framework Scene ID.
- `revision_before` (integer): the revision the Run observed.
- `revision_after` (integer or null).

This transition identifies the Scene state that the Run observed and whether
the Run advanced it. A committed Run records the advanced revision. A
**Stop** Intent success records `revision_after` equal to `revision_before`
because no mutation occurred and the revision did not advance. A failed or
cancelled Run records null.

### Step Records

The Run Record groups a Run into five larger pieces of work. Exactly five step
names exist, in this relative order: `process_input`, `intent`,
`execution_plan`, `patch`, `commit`. The Run Record Ledger includes only steps that
actually start.

Steps cover more work than progress stages, so the two do not correspond
one-to-one: there are nine stages (see the
[execution model](execution-model.md#stages-and-status)) and five steps. The `queued` stage
precedes every step, and the `started` stage's work is the `process_input` step. The
`dry_run` stage has no separate step; its evidence appears on the `patch` step. The
`intent_call` and `plan_call` stages expose the model calls nested inside the `intent` and
`execution_plan` steps.

Step fields:

- `name` (string): one of the five step names.
- `status` (string): `running`, `success`, `failure`, or `cancelled`.
- `timing` (time span).
- `input` (captured value or null): only `process_input` records an input.
- `output` (captured value or null): the step's named evidence, below.
- `error` (Run error or null).
- `model_call` (model call record or null): non-null only on a model-backed
  `intent` step and on the `execution_plan` step.

If a step remains open when the Run finishes, it closes with the Run's
terminal status and error. This rule attributes a failure or cancellation to
the step where it happened without inventing a synthetic step.

### Step evidence

A step gathers evidence as it advances. Its `output` is one captured value
that wraps one merged object. The object accumulates named facts. Each
addition captures the entire object again. Therefore, a capture failure during
a merge replaces that step's whole merged output, including its earlier facts, with the
capture error; this is the one place where a capture failure displaces earlier evidence. Each
step uses these named facts:

- `process_input` input: `{"observation": <rendered MindBuf text>}`. The key name is fixed,
  and it holds only the MindBuf channel of the Observation; the Scene channel appears as the
  Scene transition, never as content. Its output
  contains `selected_target`, which is the selected Target's data or null when
  the Run found no Target. This field is the selected Target's only location
  in the Run Record. Intent, Execution Plan, Patch, and Operation captures do not copy it.
- `intent` output: `derived_intent` (the captured application Intent) and
  `flow` (`continue` or `stop`).
- `execution_plan` output: `derived_execution_plan`, exactly
  `{"base": <scene identity>, "steps": [<operation data>...]}`. This `steps` key is the
  Execution Plan's own field name for its ordered Operations and is unrelated to the
  top-level `steps` array of Step Records; the Patch summary names the same Operations
  `operations`. The Execution Plan has no ID of its own. Framework Operation IDs never
  serialize inside Operation data.
- `patch` output: `compiled_patch` (the Patch summary, below) and `dry_run`
  as `{"after_identity": <scene identity>}`; when Patch validation fails, also
  `patch_validation` as `{"status": "failure"}`.
- `commit` output: `commit_kind` (`plain` for a snapshot Scene; `exact` or
  `rebased` for shared live state) and `metadata`. The metadata is the
  Scene-supplied rebase mapping. The runtime never extends it, and it is empty
  when there was no rebase.

A Scene identity is `{"scene_id": <string>, "revision": <integer>}`.

The Patch summary is one object:

- `base` (Scene identity): the Patch's base.
- `operation_count` (integer).
- `operation_ids` (array of strings): each Operation's framework ID.
- `operation_call_names` (array of strings): each Operation's discriminator.
- `operation_trace_ids` (array of strings): each trace row's Operation ID. Patch compilation
  produces one trace row per Operation, exactly `{"op_id": <string>}`, so a reader can match
  each Operation to its effect.
- `operations` (array of captured values): the full Operation data, with
  concrete-type fields preserved.

The four arrays describe the same Operations in the same positions. They
align by position. On a valid Patch, `operation_ids` equals
`operation_trace_ids`. A merge-conflict error embeds the same Patch summary under the `patch`
key in its metadata.

### Captured values

Application-shaped values do not always cleanly fit into JSON. A captured
value is a best-effort projection envelope. It records either the projected
data or the capture failure that took its place:

- `value` (any JSON value or null): the projected data.
- `value_type` (string): the implementation-native concrete type name of the
  captured value; diagnostic only.
- `error` (Run error or null): a capture failure recorded in place.
- `status` (string): `captured` when `error` is null, else `capture_error`.
  The serializer derives it from `error`; the record never stores it independently.

A capture failure never fails a Run, and it never blanks a separately captured value (the
terminal message and metadata, for example, are captured independently). The
envelope records the conversion failure in place. The error kind is
`capture_error`, its message names the failure, its metadata is
`{"value_type": <name>}`, and `value` is null.

The conversion rules are exact for framework-owned projection shapes. Typed
application- and provider-owned values project to JSON while preserving fields
from their concrete types. Fields for which this specification defines no
schema remain implementation-native and need not use the same names across
language bindings. An image part captures only
`{"media_type": <string>, "bytes": <decoded byte count>}`; its payload data
never enters the Run Record. A message captures `role` and `content`. It adds the
`images` key only when the message carries at least one image, so a text-only
message omits the key. A model request captures exactly
`{"model_name", "messages", "model_settings"}`. It excludes the `Proposal
Schema` because the enclosing model call record contains it. The
`model_settings` value preserves the implementation's native settings shape;
its member names and units are not portable record fields. A value whose type none of these
rules covers degrades to `{"type": <native type name>, "repr": <native rendering>}`; this
degradation is a successful capture, not a capture failure. A capture failure arises only when
a conversion itself fails.

### Model call records

The Run Record nests each model call inside the step that made the call. Each
model call record contains:

- `proposal_schema` (object): the complete canonical `Proposal Schema`,
  `{"name": <string>, "json_schema": <object>}`. The schema's own `name` is
  its identity. No separate schema-name field or second delivered or
  wire-schema copy exists. Endpoint identity plus SDK version identify the
  documented adapter translation
  ([framework specification](framework.md#the-canonical-proposal-schema)).
- `model_name` (string): the full selection carried by the request.
- `identity` (object or null): the resolved endpoint,
  `{"provider", "model", "sdk_package", "sdk_version"}`; `provider` is the
  selection prefix before the first `/`, `model` the unchanged
  provider-native remainder; the SDK fields are strings or null.
- `payloads` (object): `request` (captured value, present from call start),
  `raw_response` (string or null), `parsed_json` (object or null), and
  `parsed_proposal` (captured value or null, present only when a typed
  proposal crossed).
- `usage` (object or null): `latency_ms` (integer), `tokens` (object or
  null), `request_id` (string or null). These three fields appear only in `usage`; an
  attempt row carries its own timing, status, retryable flag, and error, never these three.
- `attempts` (array): completed provider tries, each
  `{"timing": <closed time span>, "status": "success"|"failure",
  "retryable": <boolean or null>, "error": <run error or null>}`.

The record grows as the call proceeds. Opening the record adds the schema,
model name, and request capture. Completing it adds identity, response
payloads, usage, and attempts.

A schema-validation failure at the typed proposal crossing still leaves a
complete call record with identity, usage, raw text, parsed JSON, and
attempts. Its `parsed_proposal` is null. The enclosing step fails with error
kind `schema_validation_failed`, and the terminal stage remains the call
stage.

Failure evidence has one source. If a transport error contains a partial
response, that response contributes identity, attempts, raw text, parsed JSON when the parse
succeeded, and usage; `parsed_proposal` is null because no typed proposal crossed. A
transport failure without a response directly contributes identity and
attempts. Its response payloads and usage are null.

An interrupted provider await adds no synthetic attempt row. It detaches the
in-flight call's response facts from the record and leaves only the request
capture. Attempts attached to a response that returned and was recorded
before a later cancellation remain.

A token total is known only when all its parts are known (see the
[token counts convention](observability.md#token-counts)). Aggregation over a
record is complete only when every call reports output usage. Otherwise, the
aggregate is unknown rather than zero. A partial sum never appears as a total.

### Outcome, terminal record, and cancellation

The Run outcome:

- `status` (string): `running`, `success`, `failure`, or `cancelled`.
- `error` (Run error or null).
- `terminal` (terminal record or null).

These three fields must agree. The `running` and `success` statuses carry no
error. The `failure` and `cancelled` statuses require one. `terminal` may be
present only when the status is `success`.

The terminal record carries the Run's terminal success facts: `message`
(captured value) and `metadata` (captured value). The framework independently
captures them, so a failure to capture one cannot destroy the other. The
message is bounded human-readable text. Metadata is a structured success payload from
the application. The framework carries it without interpretation.

A **Stop** Intent may return planning or decision data to its enclosing workflow.
A live commit may return Scene-supplied rebase consequences. The application
defines the mapping's meaning and typed decoding. The terminal record is
present only when either fact exists. A committed success without either fact
records null. This case includes every plain commit and a live commit with an
empty mapping.

The cancellation record: `requested_at` (timestamp) and `checkpoint` (string
or null) naming where cancellation took effect when known: `before_intent`,
`after_intent_validation`, `before_dry_run`, `before_commit`, or
`task_cancelled` for an interrupted await.

### Run errors

A Run error is `{"kind": <string>, "message": <string>,
"metadata": <object>}`. Its message is human-readable text, not a structured fact.
Control decisions read `kind` and `metadata` instead. The kind vocabulary is
open: transport kinds and application kinds pass through verbatim. The
framework copies those errors and their metadata in isolation so Run Record evidence cannot
alias live application state. The framework reserves and creates these kinds; all of them are
Run Record evidence except `observer_error`, which appears only beside the Run Record in the
Sergent Result:

- `capture_error`: metadata `{"value_type": <name>}`.
- `observer_error`: metadata `{"callback", "observer_type",
  "exception_type", "stage"}` (see [observer delivery](./observability.md#observer-delivery)
  in the observability specification).
- `cancelled`: the cancellation outcome error; metadata is empty.
- `validation_error`: an admissibility rejection carries
  `{"index": <integer>, "call": <string>, "operation_id": <string>}`; a
  dry-run verification failure carries
  `{"verification_issues": [<issue>...]}`; other invalid-value failures carry
  their own facts.
- `schema_validation_failed`: a proposal that failed the typed crossing.
- `merge_conflict`: metadata always contains `base_revision`,
  `current_live_revision`, the rejected Patch summary under `patch`, and `scene_metadata`.
  For a Scene-declared conflict, `scene_metadata` contains the Scene-supplied conflict facts.
  For a rebased Patch that failed its second admissibility pass, Patch validation, or dry-run,
  `scene_metadata` contains the Scene-supplied rebase mapping. This failure also adds
  `validation_error`, exactly `{"kind", "message", "metadata"}`. Its `metadata` contains
  the failing check's facts, isolated from the application mapping and framework fields.
  The nested object describes a check failure; its `kind` is `operation_admissibility` for
  an admissibility rejection, and otherwise the implementation-native name of the rejecting
  check. These are check names, not Run error kinds. The framework reports the conflict;
  the application determines how to resolve it.
- `stale_patch`: metadata carries `base_revision` and `current_revision`. Both this key and
  `current_live_revision` above name the live revision at the time of the check; the two key
  names are fixed.
- `patch_validation`: a Patch envelope defect; metadata is empty except for
  an embedded-identity rejection, which carries `identity_issues`,
  `expected_identity`, and `actual_identity`.
- `internal_error`: an unexpected ordinary exception. Its metadata contains
  `exception_type` (native name), `traceback` (a bounded native rendering of
  at most 20 frames that keeps at most the final 8000 characters),
  `cause_chain` (an array of `{"exception_type", "message"}` that follows the
  native cause relation and guards against cycles), and `current_step` (the
  open step name or null).

A transport error's metadata records `{"retryable": <boolean>}`.

### Run shapes

The fields above contain different values depending on where a Run ends, but
their structure does not change. These reference outlines show the main Run
shapes:

- No Target (terminal failure at `started`, as the
  [execution model](execution-model.md#stages-and-status) defines): one
  `process_input` step, status `failure`, `selected_target` null, the Recipe's
  no-target error, `revision_after` null.
- Pass-through **Stop** (technically possible but without practical value): steps
  `process_input` and `intent`, no model call, `flow` `stop`, success with a terminal record
  when the **Stop** Intent supplied terminal facts, and equal revisions. Pass-through is
  intended to fast-forward to model-backed Execution Planning, not end a Run before it.
- Full **Continue** Run: five steps, call records on `intent` (model-backed only)
  and `execution_plan`, success, an advanced revision, and a terminal record
  that follows the rebase-mapping rule above.
- Model failure: the failing step keeps its call record per the rules above
  and carries the error; later steps do not exist.
- Cancelled: any open step closes `cancelled`, the runtime populates the cancellation
  record, `revision_after` null.

## The Sergent Result structure

The Run Record is durable evidence. The [Sergent
Result](./terminology.md#sergent-result) is the in-process value that returns this
evidence and useful terminal facts to the caller. This document defines its
exact observable structure.

The framework never persists the Sergent Result. The Run Record harness persists the
Run Record at Run end, the `started` progress snapshot at Run start, and the application and
user events; the result itself stays in process. The Sergent Result contains:

- `scene`: the exact terminal Scene;
- `run_record`: the closed Run Record;
- `stage`: the last reached stage (one of the nine stages);
- `terminal_message` (string or null) and `terminal_metadata` (object):
  convenience facts; on success they duplicate the outcome's terminal facts,
  and on failure or cancellation they duplicate the outcome error's message
  and metadata;
- `observer_errors` (array of Run errors): contained delivery failures.

Status, error, and Scene identity come from the Run Record. The derived identity
uses the committed revision when one exists.

Observer errors remain beside the Run Record, never inside it. A terminal observer
can fail only after the Run Record closes. Recording that failure in the same
Run Record would create a recursive persistence requirement.
