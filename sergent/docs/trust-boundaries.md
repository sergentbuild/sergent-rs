# Trust Boundary Specification

The [execution model](./execution-model.md) defines the validation chain inside one Run. This
document defines the trust model for that chain. It explains where a value
first enters as untrusted, who checks it, and when the rest of the system can
rely on its type.

A check adds safety when it stands between an untrusted producer and the typed
value the Run will use. Think of that check as an admission checkpoint. Once
the checkpoint has produced the typed value, **repeating the same check guards
no new risk**. It only asks the same fact to prove itself again.

> Do not abuse defensive programming. Validate well, and trust your (validated) data.

This specification therefore gives a closed list of **trust boundaries**: the places where a
value enters the Run from an untrusted producer. A defensive check can look reasonable where it
appears, so a general instruction to "avoid unnecessary validation" cannot be enforced. A
closed boundary list can. Any validation, parse, conversion round trip, or defensive branch
that admits runtime input outside the boundaries below violates the specification. It does not
add robustness. The execution-safety stages (Patch validation, dry-run, Scene verification, and
commit) admit no input; they guard the effects of already-typed values, and the first boundary
below places them.

## The trust rule

A value is untrusted only while it **crosses one of the enumerated
boundaries.**

At a boundary, parse and validate the value once, and fail closed.
A failure at this boundary must carry a clear reason; a success at this boundary must
produce the typed value. If validation fails, stop the crossing there.

**After the crossing, the typed value is trusted everywhere.** From that
point onward:

- No re-validation.
- No defensive re-parsing.
- No typed-to-untyped-to-typed round trips.
- No fallbacks for shapes that no living producer can create.

Values constructed from within the framework or application code **start as
trusted values by default** because they did not arrive from an outside producer in an unknown shape.

> **Construction is the strongest validation there is.**

Fail-fast checks during construction or wiring are also construction, not a
boundary. Operation Registry construction and provider-adapter wiring are
examples. These checks report a developer mistake at the line that made it.
They do not create a new runtime input boundary.

> **Serialization must be outbound and one-way.**

Canonical `Proposal Schema` derivation, progress snapshots, and Run Record persistence produce output from typed values. An outbound projection never permits the same process to re-parse that value.

Construction-time schema conformance and the unchanged-schema check on a request builder
validate the developer's wiring. They run when the runtime is constructed and when a
request builder returns, and they report a developer mistake instead of admitting runtime
input. **They are not runtime input boundaries.**

## The five boundaries

Within a single Sergent Run, validation and parsing that admit runtime input may happen only at these five boundaries. This rule applies to the Run's execution path; **it does not prohibit additional parsing or validation in the algorithmic layer** that handles domain requests, which is outside the scope of this specification. One component is responsible for each check. When a value crosses a boundary in the Run, it becomes typed, and that same execution path does not check it again.

### First, the model output

This is the only untrusted input admitted by the Run
pipeline. The Run crosses it at most twice (model output -> Intent Proposal -> Intent,
and model output -> Plan Proposal -> Execution Plan), and only when it reaches the
corresponding model phase. A Sergent Run is built around the model; a Run that reaches neither
phase has ended early, as the [execution model](./execution-model.md) explains.

The first crossing is the model-backed Intent Proposal: the runtime derives the Intent
through the Recipe, then validates it. The second is the Plan Proposal after a **Continue**
Intent: the runtime checks the envelope shape and count bounds, decodes typed Operations,
checks each Operation's admissibility, and then runs the Recipe's validation of the
Operation order and the legality of the whole Execution Plan.

Responsibilities inside this chain follow the execution model. The
framework defines the schema shape and registry vocabulary. The application
defines semantic preconditions and Execution Plan legality. Each link runs once. The
canonical `Proposal Schema` sent to the model and the exact typed crossing
come from the same proposal types, so the two sides cannot drift.

Never accept provider schema adherence as proof, and never run the schema
again as a validator. Never accept framework bookkeeping identity from this
boundary; the framework assigns that identity.

After this chain, no component re-parses the typed Operations. The later
execution-safety stages guard different risks, and none of them admits new input. Patch
validation checks the envelope returned by the Recipe's Patch compilation, for which the
framework provides a default that an application may override. Dry-run rehearses effects on
an isolated copy, and Scene verification checks the resulting Scene's domain invariants. Commit
guards revision authority.

### Second, user input

The application is responsible for commands, arguments, free
text, and files that a user provides. It validates them at the application
entry that receives them, then stores typed state. When one domain law
guards both the user path and the model path, define it once and have both
paths call it.

### Third, persistence load

Data read back from storage crosses on read, and only
where a living reader exists. In a Sergent Run, this boundary matters when durable
application state enters the Run. For example, the application may load a document, a
checkpoint, or the saved result of earlier application work that becomes the Scene. Such state
is application-owned; a Run Record is inspection evidence and never enters a Run. The loader
is the component that turns the persisted representation into the typed value used by the
Run. It checks the representation once, reports a clear failure, and passes the typed value
inward without asking every consumer to inspect it again.

A Run may also load state produced by an earlier Run, an older application
version, or another still-supported writer. That is a real producer and can
justify a narrowly documented compatibility path at this boundary. By
contrast, data written earlier in the same Run and already held as a typed
value does not cross the boundary again merely because it is passed between
steps. Nor does a startup loader demonstrate this boundary for a Run unless the
loaded value actually enters the Run's execution path.

At write time, the write side is responsible for its refusals, such as
encodability and naming. A format nobody reads back needs no loader.
A loader that tolerates a legacy shape must name a living producer of that shape; the
application deletes the tolerance with the migration that motivated it.

### Fourth, shared live state at commit

Here a validated Patch meets a Scene
revision it did not observe. The crossing covers revision-checked commit,
stale failure, and the optional rebase, in which the Scene returns a rebased Patch and the
runtime performs the second admissibility pass. This is the framework's one sanctioned
re-validation.
It runs again because the Scene changed, not because the earlier types are
in doubt.

### Fifth, external systems

This boundary is responsible for concrete model transport
and process configuration. The transport encodes the canonical schema
through the provider's native facility, retains provider responses, and
extracts the text through one strict parse to one JSON object. It performs
no repair or salvage. Credential and environment discovery also belong at
this boundary.

The parsed object then crosses the model-output boundary inside the
runtime. Typed proposal validation belongs to the first boundary, the model output, not here.
The transport trusts official provider SDKs per their documented API and does not re-prove
their promises.

### Everywhere else, values are trusted

Validate one fact once, at its boundary, for the one failure that the check exists to
report. Do not validate it once per function that touches it; that is the "overly defensive
programming" anti-pattern.

Application-defined validation steps may remain empty. Intent validation, Execution Plan
validation, Operation Admissibility, and Scene verification enforce independent application
invariants. For example, an application may require an Intent to reference an existing
account, an Execution Plan to contain Operations in a permitted order, an Operation to
satisfy a precondition of the selected Target, or a resulting Scene to satisfy a domain
invariant. When construction or an upstream boundary already guarantees one of these
invariants, the application correctly leaves that validation step empty.

A framework may opaquely carry application values through the model client
interface, the Scene Actions, and the Recipe hooks. Opacity does not make a
value untrusted. The code responsible for the value validates it once at
derivation, and the framework passes it through without inspection. When such a value
re-enters application code through a deliberately generic hook, the application decodes it
once, fails closed, and carries the typed value inward. The value keeps its identity through
the hook; this decode restores its static type once and is not a typed-to-untyped-to-typed
round trip.

An optional interface method implementation must identify the wiring and the
flow step that reach it. Otherwise, it must not exist.

## The method to discard redundant validation

Use this three-question method before adding or keeping a validation, conversion, or defensive branch.

The check must answer all three questions:

Q1. Can this check fail? Name a living producer of the failing value. If
none exists, delete the check. If one exists and this check is the boundary's own admission
check, keep it. If the boundary already admitted the value, ask why it let that value
through, and fix the boundary instead of adding a second checkpoint.

Q2. Is the upstream producer already inside the trust boundary? A value from
the same package, or from a trusted layer below it, is trusted. Resolve
doubt about an internal flow by reading that flow, not by adding defensive
code.

Q3. What is lost if the check is removed? Name the user-visible or
runtime-visible failure it prevents. A check whose removal changes
nothing observable is ceremony.

If a check cannot plainly answer all three, it violates this specification.

Similarly, do not write a useless validation test that exists only for "TDD
ceremony". If a validation test exists, use the same three-question method to
decide whether it guards against a possible risk.
