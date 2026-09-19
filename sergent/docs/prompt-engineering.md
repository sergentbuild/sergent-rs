# Prompt Engineering Guideline

This document offers prompt-writing advice from the proving applications: the example
applications that accompany the reference implementations and prove the specification in
practice. It guides implementers and application authors, but it adds no conformance
requirements. The words "should" and "may" are advisory. The word "must"
appears only when this document repeats an existing specification rule.

The specification documents remain authoritative for those rules:

- The [framework specification](framework.md) defines the canonical
  `Proposal Schema` and the rules for Operation descriptions that feed the
  schema. `Proposal Schema` is the only structural representation shown to the
  model.
- The [terminology document](terminology.md) and the
  [execution model](execution-model.md) define the MindBuf as an observation
  channel that never carries control.
- The [execution model](execution-model.md) defines how to size settings for
  each model call.
- The [trust boundaries specification](trust-boundaries.md) defines where
  validation belongs.

## Start with the real problem: dilution

Prompt trouble often begins with duplication, not weak wording. A request
becomes diluted when it explains structure that the schema already provides.
The same thing happens when it surrounds one clear Scene projection with
summaries derived from that projection. Several versions of the same fact then
compete for the model's attention.

Most prompt improvement therefore comes from subtraction. Give each fact one
clear home. Include only text that can improve the model's next decision.

## Let the schema carry structure

Every model-backed request carries one canonical `Proposal Schema`. The
framework derives this schema from the exact typed proposal definition used by
the request:

- An Intent request uses the Intent Proposal type defined by the application.
- An Execution Plan request uses a closed envelope composed from the Operation
  definitions registered in the Operation Registry.

The request carries this schema, and the transport encodes it through the provider's native
schema-constrained output facility. Because the schema is the only model-facing structural
representation, prompt text must not repeat the response structure. The
[framework specification](framework.md#the-canonical-proposal-schema) makes this rule normative.

Write each Operation's meaning on its definition. Where the language offers doc comments and
field descriptions, put preconditions and operand meaning there; schema derivation carries
these details into the canonical schema as descriptions. The
[framework specification](framework.md#semantic-rules-ride-the-schema) requires that Operation
definitions include such semantic rules; doc comments are one way to meet that rule.

In practice, a well-shaped prompt contains:

- no response examples;
- no field lists or text descriptions of JSON structure;
- no repeated explanation of the Operation calling convention.

If the output structure needs more explanation, improve the typed proposal or
Operation definition instead of the prompt.

The canonical schema dialect can express refinements such as item counts,
numeric ranges, and enumerations. Keep those refinements in the schema without
copying them into the prompt. The dialect cannot express string-length limits
or character patterns. A rule such as "keep the reason under 480 characters"
therefore appears in two places for different purposes: the prompt presents it
as a semantic rule, and the typed crossing deterministically enforces it. The
prompt helps the first attempt conform. The typed crossing remains the
authority.

## Build the request in two messages

The Recipe defines the request-construction policy. The proving applications
use exactly two messages: a short system message and one user message.

Keep the system message short compared with the Observation sections that follow. It
may be one sentence or a compact block containing:

- one role sentence that names the Sergent Instance's job;
- one sentence that states the stable task the Sergent Instance performs on every Run (the
  Run-specific frame goes in the user message's Task section);
- only the remaining semantic advice, such as quality priorities, decision
  criteria, and behavioral rules.

Never describe the output structure in either message; the
[framework specification](framework.md#semantic-rules-ride-the-schema) forbids it in all
prompt text.

Build the user message from labeled sections separated by blank lines. A useful
stable order places the two observation channels, Scene and MindBuf, in the
middle:

```text
Task:         the bounded frame for this Run, including coordinate or
              addressing conventions when the domain has them.

Scene:        exactly one direct projection of the bounded Scene context
              relevant to the Target.

MindBuf:      the rendered MindBuf, only when non-empty.

Ask:          the closing request for this call.
```

Some applications put the Ask before the evidence. Both orders work. Choose
one labeled order and use it for every Run.

Choose one convention for each conditional section. Either omit the section
when it is empty or show an explicit marker such as "(none)" or
"(unanswered)". Consistently use the chosen convention for that section.

## Show the Scene once

Render the Scene as one direct, structure-bearing projection that suits the
domain. Show each decision-bearing fact exactly once. The proving
applications use several forms:

- Use a character grid for a board. The same form suits compact symbolic 2d or
  3d game levels and geographic maps.
- Use raw text for a document. The same form suits symbolic music sheets,
  charts, and diagrams.
- Use a labeled transcript for a conversation or interview. The same form
  suits storytelling and real-time translation.
- Use one deterministically ordered, serialized object tree for a design graph.
  The same form suits organization graphs and flow graphs.
- Put full text under per-item headers when a Scene contains several documents.
  The same form suits file management and libraries.
- Attach an image to the user message for visual work, and use text to identify
  what it shows. The same form suits vision-driven tasks and vision-based
  verification.

The Target's mutation bound and the Observation's Scene bound are different. A
Run over one region usually shows that region plus any surrounding context the
model needs for its decision. It does not show the whole Scene by default.

Once the direct projection is present, remove derived views that repeat it.
Metrics, element inventories, counts, capacity arithmetic, and summaries add no
evidence when the projection already shows the same facts. A capable model can
derive implications from clear source evidence. Supplying those implications
again can dilute attention and encourage overly conservative proposals.

```text
Good - the projection carries each fact once:

    a b c
  1 . . X
  2 . O .
  3 . . .

Wasteful - the same projection plus what it already shows:

  (the grid above)
  Cells: 9. Empty: 7. X holds c1. O holds the center.
```

A derived view may sit beside the projection when it adds new evidence. Apply
the three-fact test below to each such view. For example, a list of currently
legal actions beside a board grid adds the admissible action space before an
Execution Plan call. That information improves first-attempt legality and earns its
place. A second rendering of a fact already visible in the grid does not.

## Keep the MindBuf observational

The MindBuf carries recent information that the committed Scene
cannot hold. It never carries control. The [terminology document](terminology.md#mindbuf) and
the [execution model](execution-model.md#observation-assembly) make this rule normative. The
application decides how to curate and render the MindBuf's facts.

The proving applications use all these MindBuf styles:

- A capped factual list of about five compact lines: recent user actions, earlier Run
  outcomes, and the one current unresolved rejection, each stated as a fact. Old entries age
  out to keep the rendered content bounded.
- Labeled sections for facts such as the recent user request, the latest
  assessment, and the one current unresolved rejection. Keep durable rejection
  history in application state and the Run Record, not in model-visible text.
  Retention and model visibility are separate policies. Clear the feedback
  after the next attempt succeeds.
- A factual recent-history marker, such as "previously asked questions are
  shown above", followed by the current unresolved rejection. Put behavioral
  directives derived from those facts in the Task, Ask, or system message, and keep the
  MindBuf to the facts themselves.
- An empty MindBuf. Use one when the Scene already contains every fact needed
  for the decision. A recent fact may instead be a Scene field when it belongs
  to the domain state.

Feed a contained failure into the next attempt only when it can change the next
decision. A warning the model cannot act on adds dilution, not useful memory.

## Add semantic guidance where it belongs

Prompt text carries semantic rules when the schema cannot prove them and the
model needs them to produce acceptable output on the first attempt. The proving
applications use these categories:

- Intent decision criteria: "decide whether another pass is useful; return
  work or done with a short reason";
- quality orderings: "improve silhouette and dominant colors before minor
  texture";
- role charters in adversarial setups, where each role sees only its private
  brief: "treat the other role's comments as persuasion, not evidence";
- tactical advice and resource or topology preferences;
- instructions not to repeat an action;
- preconditions the schema can state but cannot prove: "the snippet to replace
  must appear exactly once". Put this rule on the Operation definition as the
  [framework specification](framework.md#semantic-rules-ride-the-schema)
  requires. Repeat it in the prompt only when repetition improves
  first-attempt quality.

Put stable policy in the system message. Put a rule derived from current state
in the Task or Ask. The MindBuf provides only the observed fact from which the
application derived that rule.

Deterministic code also enforces some semantic rules. A prompt line can still
help when it improves first-attempt quality. Do not turn the prompt into a
catalog of every prohibition that the runtime already enforces.

## Use the three-fact test

Before adding model-visible text, answer three questions:

1. What new information does it add beyond the Scene projection, MindBuf,
   schema descriptions, and established conventions? A deliberate repetition, such as a
   schema-carried precondition or a string-length rule that the typed crossing enforces,
   passes only when question 3 names the first-attempt improvement it buys.
2. Does the model need this information to decide well? Deterministic code may also enforce
   it; enforcement alone neither earns nor forbids the text.
3. What observable improvement should it cause in the model's decision?

Leave the text out if any answer is missing. Prompt length alone is not the
problem. Duplicated or non-decision-bearing context competes with the real
task.

## Give the Intent ask and the Execution Plan ask different shapes

The two model-backed stages, `intent_call` and `plan_call`, ask the model to do
different work:

- Phrase the Intent ask as a bounded decision: which choice to make and why,
  expressed as a small codified value.
- Phrase the Execution Plan ask as one concrete artifact: the Plan Proposal, from
  which the runtime derives the Execution Plan.

One repeated fact earns its place. The Execution Plan request repeats the validated
Intent, including its decision and reason, because the model needs that
decision context to write the Plan Proposal.

Size each call's settings for its codified output, following the execution
model's [per-call sizing](execution-model.md#per-call-sizing) guidance. A small codified
Intent usually needs tighter output limits and less thinking effort. An Execution Plan usually needs larger limits and
more effort. Judge the output shape, not the stage name. A judgment-heavy
**Stop** Intent that returns rich terminal data deserves Execution-Plan-sized settings.

## Test meaning, not wording

You can test prompt semantics without a live provider. Inject a fake model
client behind the model client interface and capture its requests. Then verify:

- which semantic fact appears in each call;
- that every decision-bearing fact appears exactly once;
- that a fact disappears after the application clears its source;
- that messages contain no response scaffolding, such as response examples, output-field
  lists, or repeated schema instructions. Structured Scene input, including a JSON object
  tree, is valid; braces and key names alone do not distinguish input data from response
  scaffolding.

Do not freeze exact sentences. Let the wording improve while the meaning stays
fixed. Separately test derived canonical schemas with golden snapshots. Prompt
tests verify semantics; snapshot tests verify structure.

When the value of a context change is uncertain, isolate that one change.
Compare the changed request with an unchanged baseline while keeping the model,
thinking effort, and every other condition fixed. Use tests to judge
deterministic protections and human evaluation to judge decision quality. Once
you have an answer, remove the comparison switch and the losing branch.

## Keep construction mechanical

- Write each prompt as one dedented multi-line block or an explicit
  concatenation. Do not build it from an implicit pile of adjacent text
  fragments.
- Join sections with blank lines. One small helper that joins non-empty
  sections keeps every request builder short and consistent.
- Deterministically serialize projections with sorted keys and stable ordering.
  Keep them ASCII-safe.
