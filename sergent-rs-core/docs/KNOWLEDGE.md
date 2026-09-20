# sergent-rs-core Knowledge

Core is the bottom layer of the Sergent Rust implementation. It defines the
typed vocabulary that the runtime, the provider transport, and applications
exchange, and it uses construction to make invalid framework states
unrepresentable. It executes no Run, mutates nothing, and performs no input or
output. The [crate README](../README.md) gives the short tour; this file is the
technical study.

The specification is authoritative over everything here. Read the
[framework](../../sergent/docs/framework.md) for the Scene mindset, Operation
identity, admissibility, and the canonical schema dialect; the
[execution model](../../sergent/docs/execution-model.md) for the Run flow and
its stages; [trust boundaries](../../sergent/docs/trust-boundaries.md) for the
five places where a value arrives untrusted;
[observability](../../sergent/docs/observability.md) for data conventions; and
the [Run Record specification](../../sergent/docs/run-record-spec.md) for the
evidence and result structure. This file explains the Rust binding of those
rules.

## Technical Design and Key Decisions

### The typed spine

Scene, MindBuf, Target, Intent, Intent proposal, and Operations are application
types. Core binds them through generic parameters and associated types, so
framework code carries them without inspecting them. Every framework value that
travels with them, such as a Plan proposal, an `ExecutionPlan`, or a `Patch`,
is generic over the same Scene, Intent, and Target triple. Mixing a value from
one application with the types of another is a compile error, not a runtime
surprise.

One trait object survives this rule: a private erased Operation, which exists
because a single Operation script holds several concrete Operation types. It
stays crate-private, so an application implements domain behavior only and
never meets the erased form.

### One asynchronous seam

`ModelClient` is the crate's only async item. It uses static dispatch through a
return-position future that is `Send`, so a multi-threaded executor can drive a
Run. Recipe policy, Scene actions, Operation behavior, schema proof, and every
constructor are synchronous. The `Operation` interface requires `Send + Sync`,
which lets an `ExecutionPlan` and a `Patch` cross an await point inside the
runtime.

### Construction is the validation

Private fields and constructors that prove their invariants mean a value that
exists is a value already checked. Schema derivation, registry composition,
identifier parsing, and image admission fail at the line that made the mistake,
before any provider call. No setter can break an invariant that construction
has established.

The [trust boundary specification](../../sergent/docs/trust-boundaries.md)
names the crossings; core supplies the types that cross them and the invariants
a crossing establishes. Once a crossing produces a typed value, nothing parses
it again, probes its shape, or revalidates it.

### One fact, one place

Every fact has a single home, and the values around it read that home instead
of keeping a copy that could disagree.

- The selected Target holds the target identity and any target-specific
  context. Intent, Execution Plan, Operation, and Patch never copy it. A target
  identifier converts from a Scene identifier for the whole-Scene case.
- A Plan step holds the framework-minted Operation identity and the fixed call
  discriminator; the application Operation beside it carries action operands
  only. Isolated copies and rebase replacements inherit both framework facts,
  so a replacement supplies behavior and mints nothing.
- The canonical schema holds the model-visible structure and lives behind an
  `Arc`. The framework request input consumes itself when the Recipe's messages
  arrive, so the exact schema allocation reaches the provider uncopied and
  substitution is unrepresentable.
- The Run outcome carries terminal status, terminal data, and terminal error as
  one coherent enum, which makes success-with-error and failure-without-error
  unconstructible. The result value projects those facts from the Run Record it
  holds rather than storing rival copies, and keeps observer delivery failures
  beside that record.
- One call usage value holds the latency, token counts, and request identity of
  a single model call. An attempt row holds only its timing, its verdict, and
  its retryability.

### The three interfaces

`ModelClient` takes one immutable request and returns either a parsed JSON
object beside call evidence, or a structured failure carrying whatever evidence
the call reached. A provider sees prompt messages, the caller's model
selection, the request settings, and the canonical schema. It never sees
proposal types, the registry, Scene, MindBuf, Intent, Target, or Patch, which
is what keeps transport replaceable.

`SceneActions` is the deterministic authority over one Scene type: identity,
isolated copy, Target selection, Target existence, Intent- and Target-aware
apply, and whole-Scene verification. It deliberately does not require the Scene
to implement `Clone`, because a Scene may need deep-copy or instrumented
copying; a value Scene whose ordinary `Clone` already isolates every nested
mutable value opts in through the `clone_scene_via_clone!` macro. Verification
returns a report that is either accepted or carries at least one issue, and it
never repairs the after-Scene.

`SergentRecipe` holds application meaning: the messages for each model-backed
phase, Intent derivation, Intent validation, whole-plan validation, and the
single error fact for a Run that selects no Target. Exactly two hooks carry
mechanical framework defaults, as the
[framework rules](../../sergent/docs/framework.md#a-run-has-at-most-two-proposal-phases)
require: binding a decoded Plan proposal to the observed Scene identity, and
compiling isolated Patch copies. A Recipe that derives Intent without a
provider call returns a locally constructed pass-through proposal instead of
implementing the Intent message hook. Each message hook defaults to the error
kind `recipe_contract_error`, so a Recipe implements only the hooks its Run
Kind reaches, and a missing one surfaces as a wiring mistake rather than as an
empty prompt. Registry capability, model selection, and
request settings belong to runtime construction, never to the Recipe, so a
Recipe cannot quietly change what the model is asked or which model answers.

## How Core Materializes the Specification

### The canonical Proposal Schema and the Rust authoring profile

One Rust definition drives both the typed decode and the model-visible
structure. An Intent proposal type and every registered Operation derive
`Deserialize`, `JsonSchema`, and `Serialize` on that same definition:
deserialization admits the model output, the schema derive generates the
structure sent to the model, and serialization lets the Run Record capture the
concrete value. Every model-visible object carries `deny_unknown_fields`.

Generation is pinned. The workspace fixes `schemars` at one exact version,
never a range, because its emitted structure may change without a major
release, and the normalizer and validator described below are written against
that exact output. Every crate that derives a model-visible type inherits the
same pin, and an application must match it. Derivation asks for JSON Schema
2020-12 under the deserialization view of the type and suppresses the
meta-schema declaration.

The [canonical schema dialect](../../sergent/docs/framework.md#canonical-schema-dialect)
is specification-defined and closed. This crate defines how Rust forms map onto
it, which the specification asks each implementation to publish.

In profile: named-field structs, the string, boolean, integer, and float
scalars, vectors, fixed unit enums, optional values, unsigned non-zero
integers, doc comments as descriptions, and inclusive range bounds. A unit enum
becomes a root definition holding a string enum, referenced from its field. An
optional field is model-visible as a required property whose value is the inner
type or null; Serde also accepting an omitted key is not a second model-facing
promise.

Rejected during construction, located by proposal name and JSON pointer: open
maps, arbitrary JSON values, tuple-shaped fields, recursive types, a struct
without `deny_unknown_fields` because its emitted object is not closed, signed
non-zero integers, and any authored refinement the dialect cannot express, such
as a string pattern, a minimum length, a multiple-of bound, or a uniqueness
constraint.

Some authoring mistakes are invisible both to a generic bound and to the
emitted schema: Serde aliases, Serde or Schemars type overrides, ambiguous
untagged unions, and a hand-written schema implementation. No proc-macro crate,
source scanner, or plugin system exists to catch them, and none may be added:
an enforcement framework would cost more than the mistakes it prevents.
Application authors honor that part of the profile themselves.

Derivation runs one deliberately small normalizer before proving the dialect.
The closed conversion list drops non-authoritative emitter metadata (`title`,
`default`, and `format`), rewrites a constant into a one-value enum, splits a
nullable type union into constrained non-null and null branches, isolates a
described reference inside an `anyOf`, and requires every declared property. It
repairs nothing else. An authored keyword the dialect cannot express survives
normalization, so the validator rejects it loudly instead of silently weakening
the constraint. The proved schema then constrains generation; typed decoding,
not a second schema pass, admits what the model returns.

### The Operation registry and the Plan proposal envelope

An application Operation carries action operands and its domain hooks, never a
Scene reference, a provider handle, a callback, target identity, or execution
authority. Its `Clone` must isolate nested mutable values, because core clones
through it when copying a validated Execution Plan into a Patch. Core keeps the
erased cloning machinery private, so applications implement domain behavior
only.

The registry is the single authority for the Plan vocabulary. Each registration
binds one non-empty explicit call discriminator to one exact Operation type,
and that same declaration composes the branch of the canonical Plan envelope
and decodes it. Schema membership and decoder membership therefore cannot
drift. The discriminator is never inferred from a Rust type name, and an
Operation struct that declares a `call` field itself is rejected. Branch
definitions lift to the root of the envelope, equal definitions of the same
name are shared, and an unequal collision fails construction. The envelope is a
closed object with one non-empty Operation array carrying the configured
maximum when one exists; a maximum without a registry is unrepresentable,
because the builder accepts it only while finishing a registry.

Decode is two staged checks at the model-output crossing, the first of the
[five boundaries](../../sergent/docs/trust-boundaries.md#the-five-boundaries).
The envelope stage rejects an unknown field, a missing or non-array Operation
list, an empty list, and a count above the maximum. The entry stage selects the
registered branch by its call, removes that discriminator, and strictly decodes
the operand struct, so an unknown field, a wrong scalar, a missing field, or an
echoed bookkeeping identifier fails there. Only a successful typed decode mints
an Operation identity. Diagnostics built from model output are escaped and
bounded before they reach a human-readable message; the full raw text stays in
the sensitive call evidence.

For a complete in-profile Operation set and its registry wiring, study
the [gomoku app](https://raw.githubusercontent.com/sergentbuild/sergent-rs-examples/refs/heads/main/gomoku/README.md).

### Identifiers, requests, and per-call sizing

Identifiers are opaque newtypes spelled as a lowercase prefix plus 32 lowercase
hex characters, and the grammar matters across implementations because
persisted application artifacts carry identifiers another implementation must
load. The Run identifier fixes its prefix, Scene and Target identifiers accept
an application prefix, and an Operation identifier can be minted only inside
the framework decode described above.

Runtime combines the caller's model selection and settings with the captured
schema, then attaches only the messages the Recipe returned. Request settings
carry non-zero output-token and timeout bounds plus an explicit thinking
effort, and they default while the model selection never does. The timeout
bounds one provider request rather than the whole Run, so an application keeps
any finer deadline and decides how it maps into settings. The specification's
[per-call sizing](../../sergent/docs/execution-model.md#per-call-sizing) rules
govern the meaning of these bounds.

Prompt images are bounded PNG values admitted at construction, and the
admission bounds its decoding work instead of decoding a rejected input in
full. Transport reads the payload; serialization exposes only the media type
and the decoded byte count, which keeps image bytes out of evidence.

### Run evidence and the Sergent Result

The captured value is the one best-effort evidence envelope for
application-shaped data. It always emits the projected value, the
implementation-native type name, any local capture failure, and a status
derived from that failure, as
[captured values](../../sergent/docs/run-record-spec.md#captured-values)
require. Intent, Target, typed proposals, terminal facts, and concrete
Operations all use it. Operation capture stays behind the private erased seam,
so each Operation is captured independently and one failed projection cannot
blank its neighbors.

Run evidence values are inert and take their shapes from the
[Run Record specification](../../sergent/docs/run-record-spec.md); this crate
supplies their constructors and nothing else about them. One Patch summary
builder holds the compiled Patch representation that the Patch step and the
merge-conflict constructors embed. Each reserved error shape has exactly one
constructor, so Scene-supplied facts, check facts, and framework fields never
share a namespace.

Application defenses raised while applying an Operation return a fault carrying
one open application error, and the runtime preserves its kind, message, and
metadata unchanged. Core supplies an exact constructor for every framework
error shape that has a live producer: admissibility, verification, Patch
validation, stale work, merge conflict, revision exhaustion, cancellation, and
observer metadata. There is deliberately no partial internal-error constructor,
because panics escape rather than becoming contained errors, so nothing in this
binding produces one.

Run Record values check their coherence at construction: step order, agreement
between the final step and the outcome, the Scene transition, cancellation
evidence, and the reported terminal stage. These checks are assertions rather
than returned errors, because the runtime is their only producer and a
violation is a framework programming mistake, not runtime input.

Evidence helpers preserve uncertainty instead of manufacturing numbers: output
token aggregation reports a complete total, an incomplete result, or an
overflow as three distinct values. Timing values are inert because core reads
no clock; the runtime supplies every timestamp and duration, while core fixes
their exact serialized form.

## Coding, Testing, and Code Governance

### What belongs here

Add code to this crate when it defines a stable cross-layer type, one of the
three interfaces, canonical proposal structure, closed Operation registration
and decoding, or inert evidence that several layers share. Keep the crate
inert: no clock, no filesystem, no network, and no async item other than the
`ModelClient` seam. An execution engine, live Scene authority, provider
adapters, credential handling, application orchestration, and persistence
policy belong to the layers above. Core depends on no other crate of this
workspace, and the dependency graph points one way, from an application down
to this crate, which is what keeps the vocabulary shared and cycle-free.

Add no speculative surface. Every optional method, fallback, and extension
point must name the live wiring and execution step that reaches it, and an
unreachable guard leaves together with the test that exists only to feed it.

### Testing practice

Prove each invariant where it is established, and prove it there only.

- Prove schema authoring through real derived types and real registry
  construction. Cover each supported form and every structurally detectable
  rejection, including definition collisions and count bounds.
- Feed malformed model values through registry decode. Do not hand-build an
  impossible Plan step, schema, Run Record, or identifier merely to keep an
  unreachable guard alive.
- For Operations, prove deterministic application, independent admissibility,
  isolation-preserving clone behavior, and preservation of the call and the
  Operation identity across Execution Plan, Patch, and rebase replacement.
- For evidence changes, prove construction coherence among step order, terminal
  outcome, cancellation, terminal stage, and Scene revision, and prove that a
  completed call keeps its evidence when a later crossing rejects the proposal.
- Prove exact serialized shapes where the shape itself is the promise: the
  closed vocabulary strings, the identifier grammar, timestamps, and the flat
  record fields.

Core tests are hermetic by construction: no provider, no credential, no
network, no filesystem, no process-environment mutation, and no sleep-based
timing.

Run `just test` and `just lint` from the workspace root; both must pass before
a change is finished. `just fmt` formats the Rust code in place.
