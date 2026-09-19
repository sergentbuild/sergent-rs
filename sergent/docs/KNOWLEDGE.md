# Sergent Specification Overview

Start here if you are new to the Sergent Specification. This overview explains
the big picture first, then gives a reading guide for the complete
specification.

## Sergent and the Sergent Specification

**Sergent** is a technical concept and vision for building and operating agentic
applications. The **Sergent Specification** records that concept and vision as
a rulebook. The [terminology document](./terminology.md) provides
the formal technical vocabulary; this document provides a brief overview.

**Sergent** describes agentic software in which a user and AI agents work with
the same data while **the user by default stays in charge**.

The name combines (Ser)vant and A(gent). Think of good restaurant servants.
The servants watch the table, offer help at the right moment, and never grab
a guest's plate without permission. **Sergent** guides the AI agents to treat the
user's data with the same care.

**Sergent** follows one predictable execution path. It organizes work into bounded **Runs**.
Each **Run** takes one possible change from observation toward commit, similar to a database
transaction. During a **Run**:

- The runtime resolves a typed **Intent**. The **Intent** records one decision: either to **Stop**
  because no change is needed, or to **Continue** with concrete and meaningful actions.
- If the **Intent** chooses to **Continue**, the model proposes a description of small, typed
  **Operations** over the application data, from which the runtime derives the **Execution Plan**.
  (think of the **Execution Plan** as a small "programming script")
- The model (LLM) only proposes; it never executes. The runtime strictly decodes the
  model output, derives the typed **Intent** and **Execution Plan**, and validates those
  values using framework rules and app-owned rules. (think of that as checking whether
  the script is attempting to do bad things)
- The runtime simulates the validated **Execution Plan** on an isolated copy. It commits
  the change to the real data only after the simulation succeeds. (think of that as running
  the programming script on a scratch copy and catching any potential errors)

Simply put, whatever answer the model (LLM) returns is a **Proposal** that the runtime can
reject. If validation accepts the **Proposal**, the runtime "rehearses" the proposed changes
on a scratch copy of the designated data, and then decides whether to formally accept the
changes or reject them.

## The Three Layers

A Sergent application separates three architecture layers. From top to bottom, these layers are:

- In the **user behavior layer**, the user works through the user interface and
  application logic. The user sets the direction.
- In the **agentic layer**, the copilot observes the user's work and data, then uses a model to
  propose the actions exposed by the next layer.
- In the **algorithmic layer**, deterministic code manages the domain data and
  provides a collection of safe, well-defined actions. The specification names
  this collection of actions the **Programming Interface**.

In the agentic layer, the model proposes changes in terms of the **Programming Interface**, and
the runtime derives small **Execution Plans** (think of them as small "scripts") from those
proposals. The algorithmic layer executes only validated **Execution Plans** in two steps:
rehearse (on scratch copies) then commit. The user behavior layer keeps control of goals and
timing.

A **Run** performs this end-to-end layer-by-layer execution.

## The Story of One Run

One **Run** follows this path: Observe->Decide->Execution Planning->Validate->Rehearse, and
lastly, Commit the change when every check passes.

The Sergent Specification expects a **Run** to be non-blocking (async) by default.

### **Observe** a bounded context

The **Run** receives two observation channels:

The first channel is the **Scene** object.

The **Scene** supplies the bounded state the agent may inspect. It can represent
a text document, a game board, a folder of files, or another domain data
model behind **a deterministic API**. A **Scene** does not need to have spatiality
and can be purely abstract.

The second channel is the **MindBuf** (mind buffer) object.

The **MindBuf** supplies small, curated recent events that the
committed **Scene** cannot hold. Examples include recent user actions and the
outcomes of earlier Runs. The application may render the **MindBuf** as short facts for the
model (such as "the user moved the title to the top of the page" or "the previous Run's
proposal was rejected because the snippet to replace appeared twice").
The **MindBuf** carries observations only. It never carries control, such as the **Run Kind**
(the application's choice of which proposal phases the **Run** uses) or the **Target** selection.

Together, the **Scene** and **MindBuf** form the **Observation**.

Before the **Run** starts, the application configures how the **Run** selects the **Target**:
the bounded part of the Scene that this **Run** may affect. The **Run** resolves the **Target**
before the model sees anything. A **Target** can be a single object or a collection
of objects. It is the application's responsibility to ensure the **Target** is valid
and logical.

### **Decide** whether to **Continue**

The **Run** next resolves its typed statement of what the Sergent Instance wants to do, given
the **Scene**, **Target** and **MindBuf**.

This statement is the **Intent**. The runtime derives it from an **Intent Proposal** through the
**Recipe**, a policy object defined by the application (think of the **Recipe** as the brain of
the Sergent Instance). The runtime then validates the **Intent**.

An **Intent Proposal** has one of two sources:

- In the typical case, the model generates it, expressing its idea "I want to do ABC...". The
  runtime strictly decodes the raw JSON generated by the model into the typed **Intent Proposal**.
- In simple execution flows, the application skips this model call to save tokens. The runtime
  then supplies a deterministic **Intent Proposal** variant called **pass-through** without a
  model call. Its purpose is to fast-forward to **Execution Planning**, where the model proposes
  the changes. The runtime still derives the **Intent** through the **Recipe** and validates it.
  A **Stop** Intent is technically possible here, but offers no practical value: **pass-through**
  is intended to proceed to the model-backed work, not end the **Run** before it.

The idea-expressing **Intent Proposal** and its typed derivative **Intent** are modelled after
the "Plan Mode" in most coding agents. The "Plan Mode" allows the coding agent to firstly express
"what it wants to do systematically" and then use it as the guide for subsequent executions. But, if "Plan
Mode" is turned off, the coding agent directly carries on with execution - similar to the
**pass-through** proposal.

To avoid confusion, the **Intent** phase is Sergent's counterpart of "Plan Mode". Sergent's
**Execution Plan**, which the next section introduces, is unrelated to "Plan Mode"; it is the
typed list of **Operations** that the Run executes. The Sergent Specification does not have a
formal "Plan Mode".

It is important to highlight the difference:

- **Intent Proposal**: the typed object decoded from the raw JSON generated by the model (LLM),
  or the deterministic **pass-through** variant supplied by the runtime.
- **Intent**: the typed decision derived from the **Intent Proposal**. The runtime validates it.

**Intent** does not carry authority, meaning it only contains the expression of ideas, showing
the Sergent Instance's attempt to perform certain actions, not the actions themselves.

A validated **Intent** chooses one of two flows:

- **Stop**: the work is already complete. The Run succeeds without any subsequent calls or
  any changes to the **Scene**.
- **Continue**: work remains, so the Run moves to execution planning.

The Sergent Specification deliberately keeps the flow vocabulary small with only two verbs.
The design intention is that, if the applications want to design complex execution flows,
such as fan-out and fan-in, looping and branching, they can explicitly design them using the
language-specific control structures. This makes the execution flows more understandable.

### **Execution Planning**, **Rehearse**, and **Commit**

#### Execution Planning

If the **Intent** says **Continue**, then the runtime asks the model to create a **Plan Proposal**.

A **Plan Proposal** is the typed object decoded from the structured text the model generates. It
describes actions to perform - think of the model's text as a "programming script" (a Python
script, a Lua script, etc.) and the **Plan Proposal** as its parsed form.

The runtime derives an **Execution Plan** from this **Plan Proposal**. The **Execution Plan**
contains typed **Operation** calls into the algorithmic layer's **Programming Interface**. An
**Operation** never stands on its own; it exists only inside an **Execution Plan**, like a statement
inside a script. The runtime and the **Recipe** validate each **Operation** and the **Execution Plan**
as a whole.

This is technically similar to how a Python or Lua interpreter loads the programming script
in plain text form and creates the bytecode for each statement (which is a typed **Operation** in our case). Then, the programming script (now in bytecode form) is ready to be evaluated.

#### Rehearse and Commit

The runtime compiles the validated **Execution Plan** into a **Patch**: a concrete, ordered,
replayable sequence of changes.

It **dry-runs** the Patch on an isolated **Scene** copy (scratch copy); the dry-run is the
rehearsal. If the rehearsal succeeds, the runtime commits the **Patch** through the **Scene**
authority selected by the application.

When concurrent **Runs** share the same **Scene**, commit checks its revision. The application
chooses strict rejection of stale work or an application-defined deterministic **Patch** rebase
onto the current **Scene**.
The [framework specification](framework.md#writer-model-and-revision-policy) elaborates all the
writer models and commit policies.

### Run Record and observability

Every **Run** that produces an in-process result closes with a **Run Record**, the
ordered evidence of what happened. If any step before commit fails, or if the **Run**
is cancelled, the **Scene** stays unchanged and the **Run Record** records why.

For the sake of observability engineering, it is reasonable to preserve the **Run Record**
in the form of Run Record files.
This persistence is opt-in: the application decides whether to persist the Run Record.

When the application opts in, the Sergent Specification fixes the portable **Run Record** file
format (JSON Lines files). Every implementation follows the same JSON encoding constraints and
framework-owned record fields. Each Run-end line carries the complete **Run Record** without filtering.
The application is responsible for the surrounding policy: the file export directory, retention,
redaction, and access control. Beware that a complete **Run Record** can contain prompts, model
output, Scene data and sensitive information.

In the **Run Record** file, captured implementation-native values, including model settings,
preserve their implementation's native field names.

Note, even though the Sergent Specification advocates the same observability engineering practice
across languages, it does not require the **Run Record** files produced by each language to be
equal on the byte level.

The [observability specification](observability.md), the [Run Record specification](run-record-spec.md),
and the [Run Record file format](run-record-file-format.md) provide full coverage on this subject.

To highlight the practical value of Sergent, the next section gives a tour of the example applications
that accompany the reference implementations. Some of these examples exist in more than one
programming language, with similar technical designs.

## Example Applications Quick Tour

Here are some small, complete example apps that teach and prove Sergent.
Each example shows a unique design pattern with the same single-Run rulebook.

An example is an ordinary application built on a Sergent reference
implementation. This relationship resembles a TODO app built with TypeScript
and Next.js, an online shop built with Ruby on Rails, or an E-store
backend built with Python and FastAPI. The reference implementation provides
the stack; the example provides normal application code.

Note that these examples are not part of the specification; they are for demonstration
purposes.

These examples also provide a simple implementation benchmark: an implementation is feature
complete if it can build the following apps.

**Notes** is an interactive note-taking app with an on-demand copilot. The
copilot attaches short review notes to submitted text, and the app saves one
Markdown file. Related uses include text editing, meeting-note curation,
project planning, and creative storywriting.

**Gomoku** is a terminal board game in which the user duels the AI on a live
game board without persistence. Related uses include turn-based user-versus-AI
games and AI-versus-AI benchmarking.

**Ghoul** is a small 2D game editor that builds a dungeon-crawler level. It fans out
bounded region edits across one shared design Scene and deterministically
merges them. Related uses include digital content creation and game development.

**Harvester** is a non-interactive loop that generates an SVG image for a
subject supplied in a PNG image, then improves the result over serial rounds.
Related uses include workflows that need visual understanding, CAD, and
construction planning.

**Graph Builder** is a non-interactive batch application. It reads a product
specification and designs a DAG-based development flow through plan-review
cycles. The runtime remains responsible for one Run at a time; the application
composes the cycles around it. Related uses include project planning, product
design, and workflow design.

**Socrates** is an interactive app that interviews the user in rounds. It
refines and concretizes an initiative, then composes an actionable summary.
The app starts one Sergent Run per round, returns control to the user, and commits
the user answer later. User waiting time stays outside the Run. Related uses
include interview apps, story development, and product design.

**Writer-Producer** is a non-interactive adversarial pair. A writer proposes
complete synopsis versions from a private artistic brief. A producer uses a
separate private market brief to assess each version and request changes. The
loop ends on approval or when the turn budget set by the application runs out.
Related uses include review systems, story development, and planning apps.

**Coder** is a simple coding agent that completes Python source code inside a
secure wasm-based sandbox. Each round commits validated file Operations. A
deterministic sandboxed check observes the committed result and feeds the next
round. The rounds continue until the check passes or the round budget set by
the application runs out. Control then returns to a user gate, and only the
user ends the session; the model controls neither stop signal. Related uses
include coding agent and agentic coworker apps.

**Grammary** is a web app where the Sergent Runtime runs in the backend. It provides an
interactive writing experience where a Sergent Instance simultaneously polishes the user-typed
contents. It demonstrates the semi-real-time potential of Sergent. One can
borrow the technical design to create a user experience where the user continuously
works and the Sergent Instance provides timely and smooth updates to the user contents.

## Where to Go from Here

### The Complete Specification

The specification is partitioned into seven subjects. Continue through the subjects below
in order. Each document builds on the documents before it and on the terminology document,
which closes the list as the vocabulary reference that every other document uses.

1. [Framework specification](framework.md): the Sergent Vision, the two programming
   mindsets, the framework rules, the canonical schema dialect, and the
   writer model and revision policy.
2. [Execution model](execution-model.md): the end-to-end Run flow, stages and
   status, progress, operational concurrency, and commit behavior.
3. [Trust boundaries](trust-boundaries.md): the closed list of boundaries where runtime
   input is validated, where the execution-safety stages sit, and why typed values are
   trusted everywhere else.
4. [Observability](observability.md): the observability overview, covering ownership,
   data conventions, observer delivery, and progress snapshots.
5. [Run Record specification](run-record-spec.md): the Run Record
   structure, capture rules, Run errors, and the Sergent Result structure.
6. [Run Record file format](run-record-file-format.md): the portable persistence
   format for Run Record files shared by all implementations.
7. [Terminology](terminology.md): the list of terms we use in this spec and implementations.

### The Recommended Practices

Besides the specification documents, here are the recommended practices.
Understanding them is the key to writing efficient Sergentic applications.

1. [Reliability best practices](reliability-best-practices.md): a non-normative
   guideline on failure modes, mitigation responsibilities, and Run-level retry
   strategy.
2. [Prompt engineering](prompt-engineering.md): a non-normative guideline on
   composing model-visible text.
3. [For Implementers](for-implementers.md): a parting note written for the implementers
   wanting to develop the Sergent Framework in a given programming language.

## Start Building

These documents form a rulebook, not a code library. They are language
agnostic. An implementer can study them and create a conforming implementation
in any capable programming language.

Note, such a programming language should at least have the following capabilities:

- Support JSON serialization and deserialization
- Support asynchronous programming
- Provide a modern HTTP client

New builders should begin with learning and using a reference implementation.

Sergent provides reference implementations in Python, Rust, Go and TypeScript, with many more
reference implementations to come.

Acquire a good understanding of the chosen implementation's documentation and its example
applications first, then use that learning to build your own application.

### Designing a new implementation

If the builder is motivated to build a new implementation, clearly answer these questions
first before coding:

- How to structure the code base for modularity and responsibility segregation, especially the 3-layer architecture (user behavior layer, agentic layer, algorithmic layer)
- How to implement the model provider facade
- How to implement the Intent, Intent Proposal, Recipe, Plan Proposal and Execution Plan
- How to implement the runtime machinery and delegate the concrete logic to the app-owned Recipe
- How to implement the non-blocking/asynchronous Run
- How to implement the Run Record and observability tooling
- How to test end-to-end
