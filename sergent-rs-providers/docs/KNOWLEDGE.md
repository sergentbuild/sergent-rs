# sergent-rs-providers Knowledge

This crate is the model transport of the Rust reference implementation of the
Sergent Specification. It turns one core `ModelRequest` into one
`ParsedJsonObject` plus provider call evidence, or into one structured
`ModelError` carrying every fact the call reached. It never decides what the
returned object means.

This document explains why the crate has the shape it has. Every wire-level
fact, meaning endpoints, headers, credential variables, native schema
placement, settings mapping, error kinds, and evidence rules, is stated exactly
once, in [the provider adapter and credential reference](providers.md).

## Feynman Explanation

Think of this crate as a strict interpreter between two languages. The core
request is one stable typed language that never changes with the weather. Each
provider speaks a different HTTP and JSON language, and each of those changes
on its vendor's schedule. The crate translates outward, admits the reply
inward, and records what happened. Nothing else.

One invocation divides into three responsibilities:

- The client facade chooses live process discovery or one immutable
  configuration, binds production HTTP policy, and implements the core model
  client interface.
- One invocation lifecycle keeps selection, discovery, any provider-required
  preflight, native request construction, retry, strict extraction, and final
  evidence together in one place, because those steps share one set of facts
  and one answer about what the call reached.
- A one-request classifier handles exactly one generation try: send, inspect
  response metadata, read a relevant bounded body, and classify the native
  envelope. It never decides retry and never closes whole-call evidence, so
  retry policy stays in one place instead of being spread across four adapters.

Provider adapters are pure translators inside that lifecycle. They receive
immutable messages, settings, the canonical Proposal Schema, and the resolved
endpoint, and they return either a native request or a classified envelope.
They receive no proposal type, Operation registry, Scene, MindBuf, Intent,
Target, or Patch, and proposal phase is not among their inputs. A translator
that cannot see an application decision cannot make one.

The HTTP seam under all of this is crate-private and statically dispatched.
Production specializes it to reqwest with redirects, dependency-level retries,
and system proxies disabled, so one attempt row always means one observed wire
request. Tests specialize the same seam to an in-memory client, which is why a
scripted test still exercises the real adapters, classifiers, retry policy, and
evidence rules rather than a parallel imitation of them.

Two neighbors keep this crate small. The `sergent-rs-core` crate supplies the
request, result, error, schema, and evidence values and is the only workspace
dependency here; the `sergent-rs-runtime` crate performs typed proposal
decoding and semantic validation after this crate returns the parsed object,
and never depends on this crate. Model selection stays opaque caller data in
`provider/model` form, so no model catalog, default model, price, or
capability matrix lives here to go stale. These four crates are implementation
layers, distinct from the three application layers of the Sergent Vision.

## The End-to-End Machinery

### Configuration belongs to the external crossing

The client either reads the recognized process values when a call is invoked,
or holds one immutable `ProviderConfig` so that later process changes cannot
alter it. That configuration keeps recognized nonempty values only; combining
snapshots fills missing entries, and the receiver's values win. Neither mode
mutates process state. `CREDENTIAL_ENV_VARS` gives hermetic callers one closed
list to strip before a test inherits a process environment.

Discovery turns an external endpoint into a hierarchical HTTP base and a
credential into a sensitive typed header before any attempt can begin.
Malformed or absent configuration therefore fails before request I/O and
creates no fictional attempt evidence. Exact variable names, precedence, and
endpoint rules live in
[the provider reference](providers.md#credentials-and-endpoints).

### One request, bounded attempts

Selection and discovery produce one endpoint and credential snapshot per
invocation. Any required preflight runs once. The native request is built and
serialized once, and every retry borrows the same URL, sensitive headers,
timeout, and immutable body bytes, so a second attempt cannot differ from the
first. Only the external failures the reference marks retryable consume that
second attempt; typed decoding, semantic validation, and execution failures
never reach this crate's retry policy.
[The execution model](../../sergent/docs/execution-model.md#step-failure-and-mitigation)
states the governing rule: mitigation never weakens a model request.

The request timeout applies independently to the preflight and to each
generation attempt. It bounds one HTTP request, never the invocation and never
a Run. An application that needs a whole-evaluation deadline keeps that clock,
outcome selection, and settlement for itself.

### Admitting the reply

A reply crosses this crate once, in a fixed order: classify status and headers
before deciding whether a body matters, bound the retained bytes before
admitting the complete body, accept only the documented UTF-8 form, then decode
one small provider-native envelope. Only a documented natural completion
supplies semantic text. A recognized non-natural state keeps its exact partial
text as evidence and fails closed.

The order matters. Classifying on metadata first means a failed response never
costs a body read that cannot change the outcome, and bounding before reading
means a runaway body cannot exhaust memory on its way to a certain failure.

Semantic text then receives one strict parse to exactly one JSON object, with
no fence removal, brace search, text stripping, partial recovery, or charset
repair. The parsed object next crosses the runtime's model-output boundary,
which performs typed decoding and applies application semantics; this crate
never runs the canonical schema against a reply and never learns the proposal
type. Envelope and parsing details live in
[the provider reference](providers.md#success-envelope-classification).

## Trust, Reliability, and Error Handling

This crate implements the fifth of the five
[trust boundaries](../../sergent/docs/trust-boundaries.md#the-five-boundaries),
the one responsible for concrete model transport and process configuration. The
first boundary, model output, stays with the runtime. Splitting them this way
means a transport question never becomes a semantic question by accident.

### Evidence follows reached facts

Call timing opens before selection and closes after the last reached step, so
it covers discovery, preflight, retries, envelope classification, and strict
extraction. Each completed generation try adds one ordered attempt row.
Selection, configuration, and preflight failures happen before generation and
therefore add none.

Everything else follows from one principle: evidence states only what the call
reached.

- A response-less exit has null usage and null raw response even when the
  monotonic clock measured elapsed work.
- A completed response keeps whole-call latency plus only the token,
  request-ID, and exact admitted text facts it observed.
- A natural envelope remains a successful attempt even when strict parsing
  later rejects its text. A recognized non-natural completion is a failed,
  non-retryable attempt.
- Provider-native reason text stays in the bounded human message, separate from
  exact model text.
- Credentials, credential-bearing URLs, and raw transport error text never
  enter evidence.
- Dropping an in-flight future is not a completed attempt, so nothing is
  invented for work whose outcome was never observed.

Wall timestamps are inert evidence; elapsed durations come from a monotonic
source. The runtime nests these facts in a model call inside a Step Record, as
[the Run Record specification](../../sergent/docs/run-record-spec.md#model-call-records)
describes, and this crate neither writes nor reads Run Record files. The
fact-by-fact rules live in
[the provider reference](providers.md#attempt-and-failure-evidence).

### Error kinds are the stable surface

Every failure leaves through a small, stable set of error kinds, each with a
fixed retryability, and the retryable flag answers exactly one question:
whether an identical in-call attempt may follow. A caller branches on the kind,
never on a message, because messages are for humans and may change freely.
Run-level policy is a separate application decision, which
[the reliability guidance](../../sergent/docs/reliability-best-practices.md#model-transport-and-provider-adapters)
discusses in full. The complete mapping from condition to kind lives in
[the provider reference](providers.md#retry-and-error-mapping).

### Tests prove the real path

Provider behavior is proven through the crate-private HTTP seam, never a local
server. The production and scripted clients share one invocation lifecycle, so
scripted bytes still cross the real body bound, charset admission, envelope
classifier, strict parser, retry policy, and evidence closure. A manual clock
advances at scripted boundaries, which proves whole-call and per-attempt timing
without sleeping. Script exhaustion panics, so a wrong request count reads as a
harness defect rather than a model failure.

`testing::StaticLlmClient` serves downstream crates instead. It sits at the
model client seam, records core requests, and serves either canned text or a
closed script of output, timeout, and rate-limit outcomes. Those three variants
exist because downstream tests consume exactly them; study
`sergent-rs-examples:cog` for the deadline and containment patterns they
support.
Timeout and rate limiting reproduce a completed call after the production
attempt budget is spent, so a downstream test sees production-shaped attempts
and usage. The client reuses real model selection and the real strict parser,
and it never reads the environment, runs a preflight, opens a socket, or reads
a clock.

Scripted results prove request wiring, typed crossings, control flow, and
evidence preservation. They cannot measure model judgment or live provider
reliability, which stay with human assessment of the application's real prompts
and settings.

## Adding a New Provider

A new adapter is a translation exercise, not a design exercise. Decide these
facts first, because each one has a single place in the reference and a single
place in the code:

- the provider prefix, HTTP method, and path;
- where the canonical Proposal Schema goes in the native request, and whether
  any canonical keyword needs lowering onto a private copy;
- the credential variables, their required or optional status, and the header
  each one becomes;
- how the model remainder reaches the provider, in the body or in the URL;
- how system messages and bounded image parts are carried;
- the closed completion domain, which single value is natural, and which
  carriers must be present for the envelope to be admitted at all;
- the usage fields and the response header that carries a request ID, if any;
- the translation of thinking effort, output-token cap, and timeout;
- the status codes that deviate from the shared mapping.

Then work in this order. Write the adapter as a pure translator: it receives
the schema, messages, settings, and resolved configuration, and returns a
native request or a classified envelope, with no retry decision, no clock, and
no evidence closure. Diff it against every sentence of
[the provider reference](providers.md) and against every existing adapter; a
family governed by one document is one promise, and a new member that answers a
sentence differently is either a bug or a change that belongs in the document.
Pin each decision with a test that scripts the HTTP seam: a natural completion,
every non-natural value in the closed domain, an invalid envelope, one
retryable and one non-retryable status, the body ceiling, a rejected charset,
and the exact evidence each of those leaves behind.

Update the provider reference in the same change. That document is the only
authority for these facts, and an adapter that drifts from it silently turns
the reference into a story about the past.

Run `just test` and `just lint` from the workspace root; both must pass before
a change is finished. `just fmt` formats the Rust code in place.
