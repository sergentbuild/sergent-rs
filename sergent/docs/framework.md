# The Sergent Framework

This document is the normative rulebook at the center of the **Sergent Specification**. It defines the **Sergent Vision**, the programming mindsets, the framework rules, the canonical schema dialect, and the writer model and revision policy; the [terminology document](./terminology.md) defines the vocabulary. Readers should build a good understanding of these rules before continuing to the next subject.

## Sergent Vision

The **Sergent Vision** begins with one design choice: build an agentic application as three separate layers that communicate with one another. Software built this way with the Sergent Runtime is Sergentic software, so the vision is also called the Sergentic software design pattern. From top to bottom, the layers are:

The user behavior layer. This layer is the app's user interface and logic. It gives a user the software interface needed to complete concrete tasks. The user sets the direction.

The agentic layer. This layer is the copilot, aka the agentic decision-making layer. It fully respects the user direction and helps with the user's tasks. It observes the user's work and uses inference to decide which companion actions would help.

The algorithmic layer. This layer uses traditional software architecture and it could be an existing software system. It respects the classic GOF software design patterns and must enforce determinism. It provides a Programming Interface to the agentic layer.

The three layers form the execution path:

1. The application keeps its domain data in a bounded, observable state
   called the **Scene**.
2. A Run starts. It observes the **Scene** plus the **MindBuf**, a small buffer of
   recent context outside the Scene.
3. The model proposes but never executes. Each Run has at most two proposals:
   first an **Intent Proposal**, then a **Plan Proposal** that carries typed **Operation**
   data over the **Programming Interface**.
4. The deterministic runtime derives and validates the **Intent** and
   **Execution Plan**, compiles the validated **Execution Plan** into a **Patch**, rehearses it in an isolated scratch copy, and only then **Commits** or **Rejects** the change.
5. The Run closes with its terminal result and a complete **Run Record**.

The Sergent Specification regulates the responsibilities and objectives of
the agentic and algorithmic layers. For implementers, it provides both the
vocabulary and the execution model.

## The Programming mindsets

A successful programming technology invents a mindset: a way of thinking and designing the
software system to be conforming and efficient.

The **Sergent Vision** invents two mindsets:

- the Scene mindset
- the agentic-algorithmic mindset

### The Scene mindset

Before looking inside a Run, consider the state that it acts on. Sergent calls
this state "a Scene of objects". User activities and committed Sergent Patches
change the object states. Deterministic Scene APIs query and manipulate
them, such as creating and removing objects.

Every activity on a Scene is meaningful, concrete, and object-driven. The
whole Scene can be observed at any moment. As a whole, it may express a
higher meaning, a goal and objective, or the direction of changes.

For example, a Sergent Scene may be:

- a text document
- a digital image
- a virtual construction site
- a folder of files represented through a bounded, deterministic Scene API
- a network of compute servers behind a deterministic Scene API
- an ongoing video game represented through patchable game-state objects
- ...(many other bounded, observable object models)

### The agentic-algorithmic mindset

The agentic-algorithmic mindset concerns the processes that determine: "what decisions the model should make" and "how these decisions translate into concrete actions."

The application builder decides what the algorithmic layer can do. This is the place where the
classic software architectures rule. This layer can also be an existing
software code base where the builders want to add agentic decision-making
on top. Everything this layer exposes to the agentic layer must be deterministic.

In order to complete the algorithmic layer so it supports the agentic layer
sitting on top, the application builder must design the Programming Interface on this layer.
This is technically similar to building a scripting binding for a rigid and
opaque software core. This binding empowers high-level business logic to be built in terms
of this scripting binding rather than changing the software core.
Think of a Python binding for a rigid and opaque C data structure, such as a
database. The scripting binding must offer enough functionality and flexibility while
preventing programmers from taking invalid actions; similarly, in our case the Programming
Interface on the algorithmic layer is both an empowerment and a constraint.

An **Operation** is the unit of this Programming Interface. An Operation never stands on its
own; it exists only inside an Execution Plan, like a statement inside a script.

A well-designed Programming Interface should be selective, controlled, concise, and task-driven.
The available actions define the functional power of the agentic layer.

The agentic layer concerns the execution flow and the decision-making process.
It expresses the execution flow in terms of classic software control structures: fan-out, fan-in,
looping and branching. It expresses the decision-making process in terms of the **Recipe**, a
policy object that exposes the programmability (i.e. the **Operations**) to the model.

Therefore, the application builder should see this mental picture:

```text
--------------------------------------------
 agentic layer     | execution flow, Recipe
--------------------------------------------
 algorithmic layer | Programming Interface
                     software core
```

## Framework rules

The [terminology document](./terminology.md) formally introduces the technical definitions of the individual pieces in the framework rules. This section defines how these pieces work together. Every conforming implementation must follow these rules.

### A Run has at most two proposal phases

#### The first phase

The first phase resolves Intent:

- In a model-backed Intent phase, the application defines the domain-specific
  Intent Proposal type, and the model supplies its data.
- When the Scene state and the selected Target already determine the
  high-level task, a runtime implementation may instead supply a generic
  deterministic proposal variant, such as pass-through.
- In both cases the runtime derives the Intent through the rule that the Recipe supplies, then validates it.

Pass-through is intended to fast-forward to model-backed Execution Planning. Deriving a
**Stop** Intent from it is technically possible, but offers no practical value: a Sergent Run
is built around the model, and pass-through exists to reach its Execution Plan call.

The resulting Intent flow determines the runtime's next action. **Continue** asks
for a Plan Proposal. **Stop** returns terminal success because no mutation is
needed.

The standard flow vocabulary is limited to **Continue** and **Stop**. The application
manages more complex execution flows, such as looping and fan-out using ordinary
software control structures. For example, an application
can orchestrate fan-out by starting separate concurrent Runs; fan-out is not currently
an Intent flow. An implementation MUST NOT reserve another standard flow merely
for anticipated use. A future standard flow can enter the specification when it
has a concrete runtime consumer and defined execution, terminal, and Run Record
behavior.

#### The second phase

The second phase only runs after the Intent decides to **Continue**. In this phase, the model
proposes the Plan Proposal: typed Operation data in terms of the algorithmic layer's
Programming Interface. The word "derive" then carries two meanings that builders must keep apart:

- On the type level, the Recipe derives the Execution Plan: as the application's
  policy object, it appoints the application-defined ExecutionPlan type and supplies
  the rule that turns a Plan Proposal into it (see the mechanical defaults below).
- During Sergent Runtime execution, the runtime derives the Execution Plan from
  the Plan Proposal through the Recipe's rule, by handing the decoded Plan Proposal to that rule.

Next, the runtime checks the admissibility of each Operation, and the Recipe
validates the Operation's order and the legality of the whole Execution Plan. The
runtime then compiles the Patch, dry-runs it, and either commits or rejects it.

The Recipe object defines the semantic hooks (abstract or virtual methods) that the application
code implements: Intent request construction, Intent derivation, Intent and Execution Plan
validation, and Execution Plan request construction. The Recipe also declares the no-target error
that the runtime returns when Target selection finds nothing. Exactly two Recipe tasks have
mechanical framework defaults: deriving the Plan Proposal envelope into the
Execution Plan and ordinary Patch compilation. A conforming implementation
may express Recipe defaults as default trait methods, an abstract base with
virtual defaults, or an interface plus an embeddable base.

### The model only proposes

The boundary is simple: the model never executes or mutates state. The
validated Execution Plan carries actions over the Scene API. Provider
tool-calling and function-calling never carry them. The model returns
structured proposal data and takes advantage of the output schema feature when it is
available. Only the deterministic runtime may derive, validate, and execute that data.

Each Operation is a high-level call over the deterministic algorithmic-level Scene
API. The runtime compiles the validated Execution Plan into a Patch and first dry-runs that
Patch in an isolated simulation. It then applies the change to the real Scene or rejects it.

### The canonical Proposal Schema

A `Proposal Schema` is the model-facing output contract for a proposal request. Its
purpose is to use the structured-output or schema-constrained generation
facilities offered by frontier models. These facilities help the model produce
more accurate JSON with the required shape, reducing malformed or structurally
ambiguous output before it reaches the runtime.

A Run can have at most two proposal objects: an **Intent Proposal** and, when
the Intent continues, a **Plan Proposal**. They are different typed proposal
objects, but they use the same `Proposal Schema` abstraction because the
abstraction describes the request boundary, not one particular proposal kind.
Each model-backed phase constructs one schema for the proposal expected in that
phase:

- For Intent, the schema derives from the application-defined Intent Proposal
  type.
- For the Execution Plan, the schema derives from a closed envelope composed from the exact
  registered Operation types, with a non-empty Operation list and the
  configured maximum when one exists.

Thus, there is one schema concept and one schema carried by each request, while
the schema's concrete structure varies with the phase. This design avoids
having separate schema mechanisms for two uses of the same model-output
boundary, while still ensuring that the runtime never decodes an Intent Proposal or a
Plan Proposal against the wrong definition.

The exact typed proposal definition that later receives the provider output is
the authority for that phase's schema. The schema is the only model-facing
structural representation. The Operation discriminator, the fixed-value field that names
which registered Operation a call invokes, is required. Runtime bookkeeping identity (the Run
ID and the Operation IDs) lives outside the model-visible context and MUST NOT appear in the
schema. Hand-written structural text, response objects, schema examples, and parallel
convention renderings do not conform.

The schema travels with the model request and uses the provider's native
schema-constrained output facility. An implementation that wraps providers
behind adapters lets an adapter perform only the documented translation that
this facility requires. The adapter never receives proposal classes, Operation
classes, or the Operation Registry.

Schema-constrained generation does not replace validation. Provider text must
still pass through one strict object parse, exact typed proposal decoding,
application semantic validation, dry-run, and revision-checked commit.

### Operation identity

The framework assigns one Operation ID (`op_id`) to each Operation. This ID
supports deterministic Patch and trace matching. Patch compilation makes an
isolated copy of every validated Operation of the Execution Plan, including its
nested mutable values, and preserves the ID.

An Operation is unaware of the Run, and never carries Run identity - it is similar
to how a statement in a "programming script" does not know the identity of the script.

On the higher level, the enclosing Run and its Run Record contain the authoritative Run ID that correlates the Execution Plan and Patch evidence.

### Operation Registry and the Plan Proposal envelope

If a Run is not Intent-Only, then this Run needs a closed set of available actions.
The application defines this action set as the **Operation Registry**: the closed Operation set
that the model may use. It is similar to the grammar in a scripting language.

During registry construction, the framework derives the canonical
schema branches and composes the Execution Plan's `Proposal Schema`. The framework uses
the same registry for typed decoding.

The Plan Proposal envelope contains one Operation list. The list must contain
at least one Operation and may have an application-configured maximum. The
minimum is one because a continuing Execution Plan must compile to a non-empty
Patch. A Recipe without a registry is Intent-Only and cannot continue to an
Execution Plan call. Configuring a maximum without a registry is invalid construction.

Validation responsibility divides between two levels: the framework level and the application level.

On the framework level, the framework performs schema derivation and generic registry checks, such as checks for duplicate discriminators and unknown calls.

On the application level, the app defines semantic Operation preconditions, such as whether a call is meaningful for the current Scene and Intent.

### Semantic rules ride the schema

In the programming universe, structures alone do not express every business logic rule, and a well-formed structure may contain incorrect business logic data.

Therefore, the Operation definitions must include semantic rules that the type system alone cannot carry, such as validator-enforced invariants and Scene preconditions.

In some implementations, the language and its ecosystem let the Operation definitions
express these rules as doc comments or field descriptions.

Derivation carries the rules into the canonical schema when the
[canonical schema dialect](#canonical-schema-dialect) can express them.

In other implementations where the programming language or the ecosystem lacks such
descriptive type-level tooling, the implementer can define an accompanying data structure
to specify the semantic rules.

It is important to ensure semantic rules cooperate with prompt engineering and not to
duplicate the requirements or introduce conflicts. The application's prompt text retains the
task meaning, Scene, Target, Intent, MindBuf, and semantic rules that the schema alone does not
prove. The prompt text does not restate the structural output policy.

### Target ownership

One selected **Target** follows the Run from start to finish. It contains the
authoritative selected Target identity and Target-specific execution
context. Intent, Execution Plan, and Operation schemas respect the Target
as both read-only (never to be modified) and ephemeral (bounded lifetime and only living in a Run).
The Operation schemas only describe the
action operands chosen for the call. The runtime interprets these operands
relative to the selected Target, and an Operation whose operands do not fit
that Target fails the admissibility validation.

The runtime supplies the same selected Target to the Scene's apply step and to every Operation. The runtime never reselects or reconstructs the Target. An Intent or Execution Plan may retain a derived
domain fact only when that fact has decision meaning independent of the
selected Target. For example, in a digital content production application, the Intent may keep the observation that the Target is "red" because the recoloring decision depends on it, but never the
Target's identity, which the Target already supplies.

### Operation Admissibility and Validation

**Operation Admissibility** answers one narrow question: is this single Operation
compatible with the Scene, the validated Intent, and the selected Target? An
**admissibility pass** visits the candidate Operations in order, independently checking
each against one fixed Scene snapshot and stopping at the first
rejection. These checks are deterministic and read-only; they do not apply
Operations or check their combined effects.

The initial pass checks the Execution Plan against the Run's base Scene. If
shared live state rebases a stale Patch, the runtime performs another pass over
the rebased Operations against the current Scene, retaining the original Intent
and Target. Even an unchanged Operation may become inadmissible when the Scene
changes. The additional pass checks that new context; it does not repair a
rejected Operation or ask the model to try again.

A successful pass does not authorize mutation. The Recipe checks ordering and
whole-Execution-Plan legality; Patch validation, dry-run, Scene verification,
and commit authority provide the remaining safeguards. The
[execution model's Operation Admissibility flow](execution-model.md#operation-admissibility-flow)
defines the complete pass rules and failure behavior, and its
[Patch rebase sequence](execution-model.md#patch-rebase-on-shared-live-state) places the second pass.

## Canonical schema dialect

The canonical `Proposal Schema` uses one closed, language-agnostic JSON Schema
dialect. In this context, "closed" means that a conforming schema admits only:

- a top-level object, never a root `anyOf` or a root reference;
- root-level `$defs` and acyclic local references of the exact form
  `#/$defs/...`, where every reference resolves and a reference node
  carries no ordinary sibling keyword;
- `type` values `object`, `array`, `string`, `integer`, `number`,
  `boolean`, and `null`;
- `properties`, `required`, and `additionalProperties: false`;
- `items`, `minItems`, and `maxItems`, with `items` present on every
  array;
- inclusive `minimum` and `maximum`;
- string and scalar `enum` values;
- `anyOf`; and
- `description`.

These limits apply throughout the structure. Every inline or defined object
is closed with `additionalProperties: false`. Its `required` list includes
every declared property exactly once. Dynamic maps, open objects, unknown
keywords, and external or recursive references do not conform.

An implementation must prove conformance during construction, before any
provider call. If it cannot prove a derivation input's structural meaning
inside this dialect, the input is outside the dialect and MUST fail
construction. The implementation never weakens, repairs, or silently accepts
the input.

Each implementation's documentation defines how its language's type system
maps onto this dialect, including which native type forms it rejects as
out-of-dialect.

Remember, the `Proposal Schema` exists because of today's frontier LLMs and their
structured-output or schema-constrained generation facilities. If future LLM
generations introduce new output schema enforcers, it may change or be redundant.

The application builder is responsible for understanding each model's
structured-output feature and its requirements, because the providers differ.
If a model clearly does not support structured output, or its documentation
does not mention such a feature, the `Proposal Schema` has no influence on that
model's generation, and an implementation should not use it with that model.
This is an implementation choice that the specification does not govern. Think
of it as the POSIX standard versus each operating system's own support for it.

Likewise, the provider adapter is a suggested strategy for offering one unified
model client interface across providers, not a rule of this specification. Think
of it as the ANSI C standard library versus each platform's own libc.

## Writer model and revision policy

Every Run observes a Scene revision. The revision identifies the version that
the Run saw, and it appears in progress and in the Run Record. This fact
records what the Run observed. It does not, by itself, mean that the
application needs stale-write protection.

The application first answers the most important question: **who can write?**

It must select **a writer model** before it selects **a commit policy**:

- With **one serial writer**, whole Runs follow one application-controlled sequence:
  one Run, one writer. From Observation through commit, the application excludes another
  Run or user write to the same authority. A FIFO queue of whole Runs can provide this
  exclusion; a queue of completed writes cannot. A plain Scene snapshot is sufficient,
  and no stale comparison is needed for integrity.
- **Turn-based writers** (user->AI->user->AI->user...) can use the same policy when the
  application state machine excludes other writes throughout each Run.
- For **concurrent writers** over **disjoint data**, the application divides authority at
  the actual conflict boundary instead of sharing one needlessly strict
  revision. This is also known as "one-writer-one-chunk". The application, not
  the Sergent Runtime, is entirely responsible for the disjoint data guarantee.
- **Concurrent writers** that can affect **the same authority** need shared live
  state and stale-write handling when accepting both without comparison could
  lose or corrupt meaningful work. These Runs remain concurrent even if their commits
  are serialized. The application chooses **strict stale rejection** or
  **an application-defined deterministic Patch rebase** onto the current Scene.

| writer model  | data ownership | commit policy                                 |
| ------------- | -------------- | --------------------------------------------- |
| serial writer | any            | whole Runs are serial                         |
| turn-based    | any            | whole Runs alternate with user writes         |
| concurrent    | disjoint       | divided authority (one-writer-one-chunk)       |
| concurrent    | shared         | application chooses strict rejection or rebase |

The serial writer model is different from the concurrent model used by
examples such as Ghoul (one [reference example app](./KNOWLEDGE.md#example-applications-quick-tour)).

In Ghoul (the 2d game level editor example), the user and the AI may edit together while separate concurrent Runs are in progress. Each Run plans against the revision it observed, but
commit authority remains the current shared live Scene. If the Scene has not
changed, the Patch commits normally. If the user or another Run has advanced
the Scene, strict equality rejects the stale Patch unless the Scene defines
a deterministic rebase. Rebase applies the Patch's intended change to the
current Scene, rather than allowing the stale Patch to overwrite it.

A plain Scene is a snapshot input, so it cannot observe unrelated changes
after a Run starts. Shared live state retains current authority and compares
the Patch base revision before commit.

After an application chooses shared live state, strict revision equality is
the default. A Run that planned against revision N must not overwrite a later
revision. A Scene-defined deterministic rebase is the explicit alternative.
The runtime first validates and dry-runs the original Patch. When its base
revision is stale, the runtime may invoke the Scene's rebase mechanism,
then validates and dry-runs the rebased Patch against the current Scene.
It commits only the Patch that passes those checks through revision-checked
authority. The framework reports a merge conflict as a possible rebase error. The
application decides how to resolve it; the runtime never resolves domain conflicts or
asks the model to resolve them during commit.

Revision representation is a separate decision from the writer model and the commit
policy. It answers a different question: **where is the revision stored, and how
does the runtime verify that the Scene and its revision belong together?** The
revision still identifies the version observed by a Run in either representation.

- **External revision ownership:** the live Scene stores its domain data, while
  the commit authority stores the current revision beside it. For example, a
  text-document Scene might contain the document objects, while a live wrapper
  separately maintains `revision = 42`. A successful commit updates the
  document and the wrapper's revision as one authority-controlled operation, so
  no embedded identity check applies to the Scene data. This is useful when the
  domain Scene format should remain unchanged, or when one authority is already
  responsible for versioning.

- **Embedded revision ownership:** the Scene representation itself carries the
  revision, alongside its domain data. For example, a serialized document Scene
  might contain `scene_id: "doc_0123456789abcdef0123456789abcdef"`, `revision: 42`, and the
  document objects. The resulting Scene must keep the same `scene_id` and advance the revision
  of the Scene base actually committed by exactly one. Committing against `42` produces `43`;
  rebasing onto the current Scene at `43` produces `44`. The runtime rejects any other
  resulting Scene identity. The Run Record retains the original observed revision separately.
  This is useful when Scenes are copied, persisted, transmitted, or independently inspected
  and must carry their version identity with them.

These choices do not determine whether the runtime rejects stale work. A serial writer
can use either representation without stale comparison. Conversely, the
concurrent shared-live-state model described by the Ghoul example can use
external or embedded revisions; it still needs strict revision equality by
default, or an explicitly defined deterministic rebase, because separate Runs
may plan against different versions. A plain Scene snapshot may also contain an
embedded revision, but embedding that fact does not make the snapshot observe
later live changes.

Selecting embedded revision ownership therefore does not enable stale
protection, and selecting shared live state does not require an embedded
revision. The three decisions are independent:

- the writer model says which state has commit authority;
- the commit policy says whether an observed revision must still be current;
- revision representation says where that revision fact is stored and how the
  runtime checks its consistency.
