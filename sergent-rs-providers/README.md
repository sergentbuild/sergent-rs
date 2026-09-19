# sergent-rs-providers

This crate makes the real model calls for the Rust reference implementation of
the Sergent Specification. It takes one typed request, speaks whichever
provider dialect the caller selected, and returns one JSON object with a
truthful record of what the call reached.

## The Big Picture

Every other part of this workspace is deterministic. This crate is the single
place that opens a network connection, the edge where the implementation meets
systems nobody in this repository controls. Keeping that edge in one crate lets
everything behind it stay predictable and testable.

The work is simple in shape. One request arrives, the prefix of the caller's
`provider/model` selection names the adapter, the call goes out, and one
parsed JSON object comes back with the evidence of that call; a failure
returns a structured error carrying every fact the call reached. What the
object means is never decided here, and selection stays opaque caller data,
so no model catalog, default model, price, or capability matrix lives here to
go stale. Four public items carry the whole offering:

- `LlmClient` implements the core model client interface, discovering provider
  values per invocation or holding one immutable snapshot.
- `ProviderConfig` is that immutable snapshot of recognized provider values.
- `CREDENTIAL_ENV_VARS` lists the variables a hermetic test suite strips from
  an inherited process configuration.
- `testing::StaticLlmClient` is the deterministic test client: it records
  requests, serves canned output, and performs no external I/O.

The `sergent-rs-core` crate supplies the request, result, error, schema, and
evidence values, and is the only workspace dependency here. The
`sergent-rs-runtime` crate never depends on this crate, not even in its tests.
Applications reach this transport through the `sergent-rs` public API crate.
These four crates are implementation layers, distinct from the three
application layers of the Sergent Vision.

## Technical Design and Philosophy

One sentence governs the crate: evidence states only what the call reached. A
clock that measured elapsed work does not license a token count nobody
received, and an interrupted call leaves no attempt row. Four habits follow.

- Translate, never interpret. An adapter receives the canonical Proposal
  Schema, the messages, the settings, and the resolved provider configuration,
  and never a proposal type, an Operation registry, a Scene, or even the
  proposal phase, so it cannot make an application decision by accident.
- Prepare once, retry identically. One invocation resolves selection and
  credentials once, builds the native request once, and allows a small fixed
  number of attempts that share the exact same bytes. A second attempt that
  differed would quietly weaken the request the application asked for.
- Admit, never repair. A reply is classified by status and headers, bounded
  before it is read, and parsed once into exactly one JSON object. Code fences,
  brace hunting, and partial recovery all hide a real failure behind a
  plausible-looking success. Typed validation happens later, in the runtime.
- Fail before inventing. A bad model name, a missing credential, or an unusable
  endpoint fails before any network access, so no fictional attempt ever enters
  the record.

## The Challenges of Different Providers

Providers agree on very little, and each difference lands somewhere concrete:

- Every provider places the schema in a different field, under a different
  name, with a different nesting. One adapter per provider absorbs that.
- One provider accepts a narrower schema facility, so exactly one adapter
  lowers the canonical schema on a private copy before sending it.
- One provider carries the model name inside the URL path and its key in a
  header, which makes exact encoding a correctness matter, not a style choice.
- A local daemon may simply not hold the model, so that adapter probes once
  before generating, and its credential is optional.
- Thinking effort translates differently everywhere, and one provider does not
  accept it at all; the recorded effort there stays caller intent.

## Navigation Map

Read [the crate knowledge file](docs/KNOWLEDGE.md) next. It explains why the
crate has this shape and walks one invocation end to end. It continues into
[the provider reference](docs/providers.md), which states every endpoint,
header, credential variable, settings mapping, error kind, and evidence rule
exactly.

The rules this crate answers to live in the specification: the fifth of the
[five trust boundaries](../sergent/docs/trust-boundaries.md#the-five-boundaries),
the retry rule in
[the execution model](../sergent/docs/execution-model.md#step-failure-and-mitigation),
and the evidence shape in
[the Run Record specification](../sergent/docs/run-record-spec.md#model-call-records).
