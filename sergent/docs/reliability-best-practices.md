# Reliability Best Practices

This document offers guidance, not specification. It collects practices from
conforming implementations and the proving applications built on them: the example
applications that accompany the reference implementations and prove the specification in
practice. It adds no conformance requirements.

The [framework specification](framework.md),
[terminology](terminology.md),
[execution model](execution-model.md),
[trust boundaries](trust-boundaries.md),
[observability](observability.md), [Run Record specification](run-record-spec.md), and
[Run Record file format](run-record-file-format.md) define the normative rules. When this guide
restates a rule, it links to the document that defines it. All other guidance is advisory,
whether it says "should" or "may" or gives a plain instruction.

This guide follows a failure from start to finish. It explains what can fail
during a model call, which layer handles each problem, and how an application
can turn a contained failure into recovered work.

## Expected failure modes

Start where uncertainty enters. The [trust boundary
specification](trust-boundaries.md) defines five boundaries. The model-output
boundary admits untrusted proposal data into the Run pipeline. Sometimes that
data is unusable. A conforming implementation should treat these failure
classes as normal operating conditions:

- Transient transport faults occur before any usable text exists. They include
  timeouts, connection failures, rate limiting, and provider server errors. An
  identical second attempt often succeeds.
- Malformed model text cannot be parsed as exactly one JSON object. Examples
  include text around the payload, several objects, or no object at all.
- Provider refusals contain a refusal instead of proposal data.
- Truncated envelopes stop in the middle of an object because the call reached its per-call
  output-token cap. An undersized cap usually causes this failure, so it is a configuration
  fault more often than a model fault; see the sizing section.
- Schema-violating payloads parse as one JSON object but fail the typed crossing
  into an Intent Proposal or Plan Proposal. Examples include a missing field, a
  wrong type, an unknown Operation discriminator, or an out-of-bounds count.
- Semantic rejections occur after the runtime derives a typed Intent or Execution Plan
  from the decoded proposal. Intent validation, Operation Admissibility, or Recipe validation
  then rejects it against the Scene, Intent, or Target.

The frequency of malformed text and schema violations depends on model
capability. The canonical `Proposal Schema` uses the provider's native
schema-constrained output facility, which greatly improves conformance with a
capable model. Low-capability models still violate the schema. This leads to
two important conclusions:

- Schema violation is an expected, contained failure class. An implementation
  that cannot contain a failed typed crossing is incomplete
  ([Run Record specification](run-record-spec.md#model-call-records)); the failure is not
  simply bad luck.
- A capable model that rarely fails does not prove the implementation is
  robust. Fakes prove robustness, as the last section explains.

The application chooses a model with enough capability for its domain. The
framework contains failures and records evidence; it does not compensate for
an underpowered model.

## Mitigation responsibilities

The first reliability question is not "can we retry?" but "which layer handles
the problem?" This guide assigns each mitigation to exactly one layer: the application, plus
the execution engine and model transport layers of the recommended implementation layering
in the [for-implementers](./for-implementers.md) document:

```text
application       Run-level retry policy: fresh, full-strength Runs
execution engine  retries nothing; contains failures; Scene unchanged
model transport   bounded retry, transient faults only, identical request
```

These layers perform different jobs. The transport retries a narrow class of
faults. The execution engine contains the outcome. The application decides
whether to try the whole Run again. The [execution
model](execution-model.md) permits mitigation within a step only when the
mitigation preserves that step's contract: the step's purpose, responsibility,
input, and output, as the execution model explains. This guide recommends
automatic in-call retry at one place only: the transport. Run-level retry, which starts a
fresh Run, is a separate application decision described below.

Mixing these responsibilities causes predictable trouble. Hidden retries make
attempt counts unknowable. Runtime loops can keep calling a model that will not
conform. Applications can also duplicate containment that the runtime already
provides.

### Model transport and provider adapters

The model transport is the only layer that should automatically retry within a call. It
should retry only transient transport faults: timeouts, connection failures,
rate limiting, and provider server errors. Keep this behavior small and
predictable:

- Use a small fixed attempt bound. Two attempts total is a proven choice. One
  retry absorbs a transient failure. Longer in-call loops keep a Run open
  against a failing provider; additional attempts belong to Run-level policy.
- Send an identical second request. Mitigation must not weaken a model request
  ([execution model](execution-model.md)). The retry preserves the messages,
  canonical `Proposal Schema`, provider translation, and native schema field. It
  never switches to prompt-only structure, generic JSON-object framing, or a
  reduced Operation set. If an adapter cannot express a request through the
  provider's native facility, it should fail closed before making any provider
  call, unless the application deliberately uses that model without the `Proposal Schema`,
  as the [framework specification](framework.md#canonical-schema-dialect) allows. Silently
  degrading the request would weaken it.
- Disable hidden retries inside the provider SDK. This keeps the attempt count
  bounded and makes returned attempt evidence explicit. When the model call returns its
  evidence, each completed provider attempt appears as one attempt row with its timing,
  status, and retryable flag. An interrupted await retains no attempt facts from that
  in-flight call, even if an earlier attempt completed. Attempts from a response
  already returned and recorded remain ([Run Record specification](run-record-spec.md#model-call-records)).

The transport should also define a small, stable taxonomy of error kinds.
Examples include timeout, connection failure, rate limited, provider
unavailable, provider error, invalid request payload, invalid model text, model
not found, and missing credentials. Each kind has a fixed retryability.

Stable kinds matter because the Run Record records transport kinds verbatim. The
error-kind vocabulary is open and includes a reserved framework subset.
Run-level policy later branches on these kinds. The retryable flag has one
narrow meaning: whether an identical in-call attempt may follow. Run-level
retry is a separate application decision based on the error kind.

Extraction is not mitigation. The transport extracts response text and applies
one strict parse to one JSON object, with no repair or salvage
([trust boundaries](trust-boundaries.md)). Provider-native response-envelope
extraction may locate the response text. After extraction, however, code-fence
stripping, bracket hunting, truncation repair, and best-effort recovery of a
partial object are all forms of repair.

Malformed text, refusals, and truncated envelopes therefore fail the call
under a non-retryable kind, while the raw text remains available as evidence.
The application decides whether to spend another full-strength Run on unusable
text. That decision uses a budget and added observation; it is not an automatic
transport response. For a truncated envelope, the application first checks the per-call cap,
because an identical fresh Run truncates again.

The transport does not decide whether a parsed object conforms to the schema.
The runtime performs that check at the typed proposal crossing. That crossing
is the model-output boundary, not the external-system boundary.

### Execution engine

Once a call enters the execution engine, reliability changes from retry to
containment. The runtime should never retry a transport fault, failed parse,
schema violation, or semantic rejection. The execution model already states
that Operation Admissibility introduces no model retry
([execution model](execution-model.md)). This guide recommends the same posture
throughout the engine.

Containment gives the caller an honest result and keeps the Scene safe:

- Every ordinary failure ends as a structured Sergent Result. It records the
  terminal status `failure`, the stage where the failure occurred, and a Run
  error whose kind and metadata serve as control facts.
- Every pre-commit failure and cancellation leaves the Scene unchanged. No
  Patch reaches commit unless every earlier step succeeds
  ([execution model](execution-model.md#step-failure-and-mitigation)).
- A failed model call records all available evidence in the `RunRecord`
  ([Run Record specification](run-record-spec.md)). When the failure includes a provider
  response, the Run Record keeps the request capture, raw response text, parsed
  JSON when parsing succeeded, attempt rows, and any reported usage. A
  response-less transport failure keeps the request capture and attempt rows,
  with null response payloads and null usage. A proposal that fails the typed
  crossing fails its step with the reserved kind `schema_validation_failed`,
  leaves `parsed_proposal` null, and keeps the terminal stage at the call stage:
  `intent_call` or `plan_call`.

The unchanged Scene makes fresh Run-level retry safe with respect to Scene
state. A failed Run may still incur model spend, produce evidence in the
Run Record and the Run Record file, and invoke observers, but it commits no Scene mutation. The
application should still bound spending, observer work, and attempts. The
runtime therefore needs no retry machinery.

### Application

After the transport bounds its retry and the runtime contains the result, the
application makes every remaining reliability decision:

- which model is capable enough for the domain;
- whether to retry a failed Run, how to retry it, and how many attempts to
  allow;
- which per-call sizing values to use;
- what the next attempt observes about the failure;
- whether and how to persist the Run Record;
- what to show the user.

The Recipe forms part of this application policy. It supplies semantic
validation, whose rejections appear as `validation_error`. The following
sections describe the application's options.

## Run-level retry postures

An application that retries should start a fresh, full-strength Run, never a
degraded one. The new Run uses the same Recipe, the same canonical schema, and
the same or stronger settings. Raising an undersized per-call cap strengthens
the next attempt; see the sizing section. Apart from that correction, the
intended change is added observation, as the next section describes.

The execution model's [never-weaken rule](execution-model.md#step-failure-and-mitigation)
governs mitigation within one Run. This guide recommends applying the same principle to
fresh Runs.

Retry policy should inspect the structured error kind and metadata, never
message text. A message is human-readable text, not a structured control fact
([Run Record specification](run-record-spec.md)). Classifiers should fail closed: an
unrecognized kind is not retried.

Proving applications use three practical postures:

- Automatic bounded retry first classifies the terminal kind by attribution.
  A failure is model-attributable when the runtime or transport rejected output that the
  model produced: the transport kind for invalid model text,
  `schema_validation_failed` at the typed crossing, or a semantic
  `validation_error`. Before treating invalid model text as model-attributable, rule out
  truncation from an undersized cap, which is a configuration fault that a larger cap fixes.
  Start a fresh Run only for these kinds and only within an
  explicit attempt budget. Failed attempts consume that budget. This rule lets
  a persistently failing Target terminate instead of loop. Abort on every other
  kind: transport kinds that identify infrastructure faults, which already
  received any worthwhile transport retry, plus `cancelled` and
  `internal_error`.
- User-gated retry shows the structured failure and lets the user start the
  fresh Run. Preserve the user's submitted input so the retry is a
  resubmission rather than re-entry.
- No retry is also valid. A bounded serial loop may treat any failed round as
  fatal. A fan-out may isolate each parallel Run's failure in place while
  sibling Runs continue.

All three postures conform to the specification. Each places the decision
above the runtime and reads structured error kinds, and a posture that retries restarts a
complete Run instead of resuming a partially completed one.

## Failure feedback into the next attempt

A fresh Run starts with a clean execution path. If one failure fact would
change the next decision, provide that fact as curated observation rather than
replaying raw output. Record a rejected attempt in the MindBuf, or in the Scene
when the domain keeps such facts there, only if the fact helps the next
decision. "This exact move was rejected; avoid it" can change the next
proposal. A stack trace cannot.

- Expose only the current unresolved failure, not an accumulating history.
- Clear or age out the fact after a successful attempt.
- Keep durable failure history in the Run Record, not in model context.

Raw failed output dilutes the request, anchors the model on its mistake, and repeats
evidence already recorded in the Run Record.

## Sizing and fail-fast construction

One common "model failure" begins in configuration. An undersized per-call
output-token cap truncates the payload in the middle of an object. The failure
then appears as an incomplete envelope or a strict-parse failure.

The [execution model](execution-model.md#per-call-sizing) recommends the sizing rule: each
model-calling step should bound its output tokens, timeout, and thinking effort according to
the approximate size and latency of its codified output. The application builder chooses the
values; the framework provides the controls but does not enforce them. Tight settings suit a
small codified Intent. An Execution Plan needs larger settings. In practice:

- Measure real payload sizes for each workload before trusting a cap.
- Treat framework defaults as safety fallbacks, not tuned values.
- When failures resemble truncation, check the cap before blaming the model.

Anything that can fail before model spend should do so. Construction-time and
wiring-time checks are construction, not a new boundary
([trust boundaries](trust-boundaries.md)). Derive schemas and construct
registries once, before a Run. If the implementation cannot prove a schema derivation input
inside the canonical dialect, construction fails
([framework specification](framework.md#canonical-schema-dialect)).

Missing credentials should fail before any provider attempt. A locally hosted
model setup may also check model existence before a Run. This produces a clear,
non-retryable kind such as model not found instead of a confusing transport
failure during the Run.

## Diagnosis from the Run Record

Model-attributable failures are nondeterministic. They can resist on-demand
reproduction, disappear with capable models, and reappear under load or with a
weaker model. Diagnose them from recorded evidence, not repeated Runs.

A failed call keeps every available fact. A persisted Run Record can therefore
directly answer the important questions: what was requested, what the provider
returned, which attempt failed under which kind, and where the Run stopped.

Applications that require reliable diagnosis should persist Run Records in the
portable [Run Record file format](run-record-file-format.md) and use them as the primary
incident artifact. A persisted complete Run Record is sensitive because it may contain
prompts, model output, and Scene projections. The application sets directory,
access, retention, and redaction policy
([observability](observability.md)).

## Proving reliability with fakes

You can test containment without a live provider. Use the fake closest to the
behavior under test. Place each assertion at the injection point that can prove
it:

- A provider-shaped fake behind the model-client interface replaces the
  transport and proves runtime and application containment. It should emit
  schema-violating payloads on demand, which produce
  `schema_validation_failed`; schema-conforming but semantically illegal
  proposals, which produce `validation_error`; and structured
  transport-shaped failures. Tests at this point verify that terminal status
  and stage match the injected failure, that error kind and metadata provide
  the facts used by Run-level policy, and that every pre-commit failure and
  cancellation leaves the Scene unchanged.
- A fake provider endpoint beneath the real transport proves transport
  behavior. It should emit malformed text, refusal envelopes, truncated
  envelopes, and transient-fault-then-success sequences. Tests at this point
  verify strict-parse failure for unusable text, each kind's fixed
  retryability, and the exact bounded attempt count when a transient fault is
  followed by success. Attempt rows provide the evidence.

Tests should never contact live providers, and test design should never weaken
the reliability machinery. A fake that emits only conforming payloads proves as
little as a capable live model.
