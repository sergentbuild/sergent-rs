# Provider Adapter and Credential Reference

This document enumerates the supported LLM provider adapters in the Rust
reference implementation of the Sergent Specification. No other document
restates the provider list; they link here. The live adapter set is private to
the crate, and implementation changes must keep it aligned with this reference.

This Rust binding reference is the authority for provider identifiers, HTTP
endpoint behavior, native Proposal Schema placement, exact `provider/model`
selection, credentials, settings translation, retryability, strict output
parsing, and attempt evidence. It curates no model names, no default models, no
prices, and no capability matrix. [KNOWLEDGE.md](KNOWLEDGE.md) explains how
responsibility is divided inside the crate and links here for exact behavior.

The [framework specification](../../sergent/docs/framework.md#canonical-schema-dialect)
leaves adapter strategy to implementations, so this reference defines the Rust
binding's baseline adapters. Adapters never receive proposal types, Operation
types, or the Operation registry; they receive only the canonical Proposal
Schema, messages, settings, and resolved provider configuration.

Proposal phase is not an adapter input. Every reached model-backed phase uses
the same core request seam, and a pass-through Intent proposal creates no
provider request.

The governing rules stay in the specification and are not restated here. The
[trust-boundary specification](../../sergent/docs/trust-boundaries.md#the-five-boundaries)
decides what this boundary admits and what the runtime alone admits afterwards,
and [the execution model](../../sergent/docs/execution-model.md#step-failure-and-mitigation)
states the rule that mitigation never weakens a model request. This document
states only what the Rust adapters do.

## Supported provider adapters

The prefix of the caller's `provider/model` selection names one of these four
adapters:

- `openai`
  - Request: `POST {base}/v1/responses`.
  - Credential header: `authorization: Bearer <key>`.
  - Canonical schema placement: `text.format` set to
    `{type: json_schema, name, schema, strict: true}`.
  - Request-ID evidence: the `x-request-id` response header.
- `anthropic`
  - Request: `POST {base}/v1/messages`.
  - Credential headers: `x-api-key: <key>` and `anthropic-version: 2023-06-01`.
  - Canonical schema placement: `output_config.format` set to
    `{type: json_schema, schema: <lowered>}`, after client-side lowering.
  - Request-ID evidence: the `request-id` response header.
- `gemini`
  - Request: `POST {base}/v1beta/models/{model}:generateContent`.
  - Credential header: `x-goog-api-key: <key>`.
  - Canonical schema placement: `generationConfig.responseJsonSchema` plus
    `generationConfig.responseMimeType: application/json`.
  - Request-ID evidence: none.
- `ollama`
  - Request: `POST {base}/v1/chat/completions`.
  - Credential header: `authorization: Bearer <key>`, sent only when a key is
    set.
  - Canonical schema placement: `response_format` set to
    `{type: json_schema, json_schema: {name, schema, strict: true}}`.
  - Request-ID evidence: none.

Base hosts are `https://api.openai.com`, `https://api.anthropic.com`,
`https://generativelanguage.googleapis.com`, and the Ollama daemon from the
environment. Every request sets content type `application/json`. The model
remainder is a body field for every adapter except Gemini, which encodes it as
one path segment before the literal `:generateContent` suffix. System messages
are carried as role messages by openai and ollama, in the `system` field by
anthropic, and in `systemInstruction` by gemini. Bounded PNG image parts are
sent as native content blocks: openai `input_image` data URIs, anthropic base64
`image` sources, gemini `inlineData`, and ollama OpenAI-compatible `image_url`
blocks.

The production reqwest client disables redirects, reqwest protocol retries, and
system proxies. The provider loop alone decides retry. A returned 3xx is
observed directly and becomes non-retryable `provider_error`; credentials are
never forwarded to a redirect target.

Response status and headers are available before a body is consumed.
Non-success generation status and every Ollama preflight decision ignore the
body. A generation 2xx body is collected from exact byte chunks under an
inclusive 1,048,576-byte ceiling. Overflow is non-retryable `invalid_response`;
the complete-body collector never grows beyond that ceiling.

openai, gemini, and ollama send the canonical schema unchanged. Anthropic is
the only lowering adapter.

## Selection and exact model identity

Selection splits the caller's opaque `provider/model` string at the first
slash. The prefix and remainder must each contain at least one byte, and the
prefix must name a supported adapter. No whitespace is trimmed or normalized.
The complete nonempty remainder, including leading or trailing whitespace,
colons, and later slashes, is copied byte-for-byte into `ModelIdentity.model`
and into the adapter's native model field. Gemini percent-encodes that complete
remainder as one URL path segment, so slashes, query and fragment characters,
and dot-like content remain model data rather than URL structure.

A missing slash or empty prefix or remainder is `invalid_model_name`; an
unknown prefix is `unknown_provider`. Both fail before credential discovery or
model I/O and carry zero generation attempts.

## Credentials and endpoints

`LlmClient::new` reads the recognized environment variables when a call is
invoked. `LlmClient::configured` instead reads one immutable `ProviderConfig`,
built from fixed application values or a read-only process snapshot.
Configuration merging fills only missing recognized nonempty values, so the
receiver has higher precedence. Neither path mutates process state. The public
`CREDENTIAL_ENV_VARS` deny-list enumerates every variable below; hermetic test
suites strip exactly these. A variable set to the empty string counts as unset.
A missing required credential and an Ollama base URL ending in `/api` (after
trailing slashes are trimmed) both fail before any network access and carry
zero generation attempts.

Every present credential is converted to a typed HTTP header and marked
sensitive during discovery, before native request construction or attempt
timing. A present value that cannot form an HTTP header is
`missing_credentials`, with zero attempts and zero network requests. Endpoints
are parsed once into hierarchical HTTP URLs. Query, fragment, embedded
credentials, unsupported schemes, and non-base URLs fail discovery as
`provider_unavailable` without echoing the value. Provider paths are appended as
structural segments; trailing endpoint slashes cannot change placement.

The recognized variables are:

- `openai`
  - `OPENAI_API_KEY`, required.
- `anthropic`
  - `ANTHROPIC_API_KEY`, required.
- `gemini`
  - `GEMINI_API_KEY`, required, with `GOOGLE_API_KEY` as the accepted fallback.
- `ollama`
  - `SERGENT_OLLAMA_BASE_URL`, optional, defaulting to
    `http://127.0.0.1:11434`, and it must not end in `/api`.
  - `OLLAMA_API_KEY`, an optional bearer key.

The Gemini key is sent as an HTTP header, never a query parameter, so no
credential ever appears in a URL.

## Ollama existence preflight

Once per invoke, before the retry loop and never on retry, the client probes
`POST {base}/api/show` with `{"model": <model>}` against the same daemon the
generation call will hit (a Rust binding decision). The probe uses the same
per-request timeout the caller's settings give the generation request. An HTTP
2xx status succeeds silently, 404 is `model_not_found` with no generation call,
and 3xx is `provider_error` without following the redirect. Every other status
or a transport failure is `provider_unavailable`. Every failed preflight
carries zero generation attempts. A completed failed preflight response keeps
whole-call latency with null tokens and request ID. A response-less preflight
failure has null usage. Both forms keep null raw response because preflight is
status-only and never reads its body.

## Settings translation

Core `ModelSettings` defaults are thinking effort high, output cap 4096, timeout
60 seconds. `timeout_secs` is a positive whole-second allowance applied as the
per-request reqwest timeout for every adapter. Preflight and each generation
attempt receive that full allowance independently; retries do not consume a
shared timeout budget. Selection, discovery, native translation, and strict
output parsing also belong to the invocation, so this setting is not a
whole-invocation or whole-evaluation deadline.

The three knobs translate as follows:

- Thinking effort
  - `openai`: `reasoning.effort`.
  - `anthropic`: `thinking.type` set to `adaptive`, plus
    `output_config.effort`.
  - `gemini`: not sent.
  - `ollama`: `reasoning_effort`, carrying `low`, `medium`, or `high`.
- Output-token cap
  - `openai`: `max_output_tokens`.
  - `anthropic`: `max_tokens`.
  - `gemini`: `generationConfig.maxOutputTokens`.
  - `ollama`: `max_tokens`.
- Timeout
  - Every adapter: the per-request reqwest timeout.

OpenAI and Anthropic forward each of `low`, `medium`, and `high` unchanged.
Anthropic enables adaptive thinking at every effort level. Gemini deliberately
omits `generationConfig.thinkingConfig` at every level; its recorded effort is
caller intent, while the model uses provider defaults.

Ollama preserves all three effort levels through `reasoning_effort` on the
documented
[OpenAI-compatible endpoint](https://docs.ollama.com/api/openai-compatibility).
A top-level `think` field is deliberately not sent, because the current Ollama
Chat Completions request type does not consume it. Here `low` requests low
effort; it does not disable thinking.

## Anthropic lowering

The Rust binding performs the client-side schema transform as a total function
over the validated canonical dialect. It works on a copy and never mutates the
stored canonical schema. It moves the refinements Anthropic's `json_schema`
facility does not accept into the affected node's `description`, keeping
structure intact:

- moved into description: `minimum`, `maximum`, `maxItems`;
- preserved unchanged: object structure, `additionalProperties: false`,
  `required`, `minItems`, string and scalar `enum`, `anyOf` unions, and
  `$defs`/`$ref`.

The description note format is `keyword: value`, comma-joined and appended in
parentheses to an existing description, or used as the description when none
exists. Because the dialect is a closed keyword set proven at construction,
every construct is representable and lowering is total: there is no reachable
"cannot lower" outcome in this implementation. Retries reuse the identical
lowered request.

## Success-envelope classification

Every 2xx body crosses once into a small provider-local typed view before
semantic parsing. Required completion and consumed-content carriers fail
closed; unrelated additive fields remain allowed. Usage is optional,
admission-independent evidence. When present with unsigned token counts, every
adapter maps the fields that its native interface names, including the
total-derived Gemini normalization stated with its envelope below. OpenAI and
Anthropic request IDs still come only from response headers.

Envelope decoding consumes the exact bounded bytes directly. Absent charset
metadata and case-insensitive `utf-8` are admitted; unsupported or malformed
charset declarations are `invalid_response`. Malformed UTF-8 is rejected. No
header-directed transcoding, replacement, or lossy string conversion occurs.

OpenAI requires `status` and `output`. The closed status domain is `completed`,
`failed`, `in_progress`, `cancelled`, `queued`, and `incomplete`. Every output
item requires a string `type`; non-message items are ignored. A message
requires `content`, whose closed block union is `output_text` with string
`text` or `refusal` with string `refusal`. Output text is concatenated in
provider order. Only `completed` with no refusal is natural; a refusal
overrides that status. A string `incomplete_details.reason` enriches the
bounded human failure message but never enters raw response or metadata.

Anthropic requires `stop_reason` and `content`. The closed stop-reason domain
is `end_turn`, `max_tokens`, `stop_sequence`, `tool_use`, `pause_turn`,
`refusal`, and `model_context_window_exceeded`. Every content block requires a
string `type`; `text` blocks require string `text`, are concatenated in
provider order, and other typed blocks are ignored. Only `end_turn` is natural.

Gemini requires a finish reason for every candidate. A
`promptFeedback.blockReason`, an empty candidate list, or any candidate
`finishReason` other than `STOP` is non-natural. `blockReasonMessage` and
`finishMessage` may enrich the bounded human failure message but never enter
raw response or metadata. Gemini usage maps `promptTokenCount` to normalized
input and `totalTokenCount` to normalized output; candidate, cached, and
thought-token subcounts are not exposed.

Ollama requires exactly one `choices` entry with a string `finish_reason` and
string `message.content`. The current closed finish-reason domain is `stop`,
`length`, and `tool_calls`; only `stop` is natural. The choice `index` does not
gate admission.

A missing, wrong-typed, malformed, or unknown required carrier, discriminator,
or completion value is an invalid success envelope. A recognized non-natural
state is a non-retryable `invalid_response` even when its partial text is one
valid JSON object. No adapter gates admission on usage, top-level object or
type, roles, body resource IDs, echoed model, timestamps, or Ollama choice
index.

For a recognized non-natural envelope, raw response is only exact observed
model text: concatenated `output_text` for OpenAI, concatenated `text` blocks
for Anthropic, concatenated candidate text for Gemini, or `message.content`
for Ollama. Provider status, stop reason, finish reason, refusal reason, and
other native explanation remain classification input and bounded human message
text. They never prefix, annotate, or replace exact model text.

## Retry and error mapping

Selection, credential discovery, and any Ollama preflight occur once per
invocation. The adapter then builds the native generation request once. The
retry loop allows two outer attempts and is never caller-configurable. Each
attempt sends exactly one wire request. The prepared JSON tree is serialized
once before the loop, then discarded; every retry shares the exact immutable
URL, headers, and body bytes. Redirect and protocol retry policy cannot add a
hidden wire request. Only the retryable failures below consume the second
attempt.

Retryable failures:

- request timeout, including while reading the body: `timeout`;
- connection or other transport failure: `provider_unavailable`;
- a response body that cannot be read: `provider_unavailable`;
- HTTP 429: `rate_limited`;
- HTTP 5xx: `provider_unavailable`.

Non-retryable failures:

- a generation 2xx body exceeding 1,048,576 bytes: `invalid_response`;
- an unsupported charset or malformed UTF-8: `invalid_response`;
- HTTP 3xx: `provider_error`;
- any other HTTP 4xx from a cloud adapter: `provider_error`;
- Ollama generation HTTP 400 or 422: `invalid_payload`;
- Ollama generation HTTP 404: `model_not_found`;
- an invalid success envelope, meaning unparseable, malformed, missing,
  wrong-typed, or unknown required native data: `invalid_response`;
- a recognized non-natural success envelope: `invalid_response`;
- a failed extraction from a completed envelope: `invalid_response`.

Pre-call failures, where no generation attempt exists and no retry is possible:

- a missing credential: `missing_credentials`;
- an Ollama base URL ending in `/api`: `provider_unavailable`;
- a malformed `provider/model` name: `invalid_model_name`;
- an unknown provider prefix: `unknown_provider`.

## Strict output parsing

After exact response bytes pass the provider-local envelope, the adapter
extracts semantic text. The boundary trims surrounding whitespace once, then
parses once. The result must be exactly one JSON object. Tagged and untagged
code fences, arrays, scalars, leading or trailing text, malformed or partial
JSON, a second JSON value, and other trailing content are rejected without
repair, stripping, search, or salvage. This extraction failure is
`invalid_response` and is not retried. Production transport and the
deterministic test client use the same parser.

## Attempt and failure evidence

An attempt row represents one generation wire try and uses the closed
`Attempt::Success` or `Attempt::Failure` shape with a closed time span.
Selection, credential or endpoint discovery, and Ollama preflight happen before
generation and therefore produce zero attempt rows. A request send or body-read
I/O failure is a retryable failure row. A documented refusal or incomplete
envelope is one non-retryable failure row, and the enclosing error keeps
identity and any reported usage.

Every failed attempt stores its retryability twice by rule: the attempt's
`retryable` field, and an error metadata object holding the same boolean under
the sole key `retryable`.

Attempt durations use independent monotonic measurements. Wall timestamps are
inert evidence and never determine elapsed duration.

Runtime nests this evidence in a model call inside a Step Record of the Run
Record, following the
[Run Record specification](../../sergent/docs/run-record-spec.md#model-call-records).
The record carries the canonical Proposal Schema, not a second provider-native
schema or native request body. Captured model settings retain their Rust field
names and units. The reqwest adapters use no official provider SDK, so their
identity's `sdk_package` and `sdk_version` are null.

Call latency opens at `invoke` entry and uses the same monotonic source through
selection, credential and endpoint discovery, Ollama preflight, native request
construction, every generation attempt, envelope classification, and strict
semantic extraction. A completed preflight or generation response records
that whole latency. Selection, discovery, credential, endpoint, preflight
transport, connection, send, and pre-response timeout failures have null
usage even when the clock measured elapsed time. Pre-attempt failures retain
zero generation attempts. A completed failed Ollama preflight retains latency
with null tokens and request ID. OpenAI request IDs come only from
`x-request-id`; Anthropic request IDs come only from `request-id`. A generation
failure retains the recognized header ID from the response that closes the
call, including HTTP status, body-read, byte-limit, charset, and
invalid-envelope failures. Top-level Response or Message resource IDs in
provider bodies never populate request-ID evidence.

A completed envelope whose semantic text fails strict object parsing keeps a
success attempt because generation completed. Its exact semantic text is
preserved as sensitive `raw_output`, along with identity and usage. A
recognized non-natural envelope instead keeps a failed non-retryable attempt
and preserves available usage, header request ID, and exact partial semantic
text. Provider-native reason affects classification and the bounded human
message only. An invalid success envelope preserves the exact admitted UTF-8
body. A body-read failure preserves already collected bounded bytes only when
they are valid UTF-8; otherwise raw response is null. Status-only failures,
connection failures, and send failures have null raw response. Credentials
never enter evidence, and an error message never echoes a URL, a header, or
reqwest error text.

The enclosing model error uses the open kind this crate assigns, unchanged, and
an independent retryable boolean. Runtime projects that boolean into transport
error metadata containing exactly the one `retryable` key. Neither attempt nor
transport metadata contains provider reason.

`retryable` describes the external failure, not whether another provider
attempt remains: a returned error has already ended this invocation's attempt
policy. Both `timeout` and `rate_limited` can be retryable, so the boolean alone
cannot identify a timeout. Consumers branch on the structured kind, never on
message text.
