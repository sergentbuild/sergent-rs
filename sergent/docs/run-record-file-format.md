# Run Record File Format Specification

This document defines the portable persistence format for Sergent Run Record files.
Conforming implementations share this opt-in framework format. The Run Record
harness writes one JSON Lines file with framework Run boundaries and
application-recorded events. Every line uses the active envelope version, the
constant `sergent.run_record.v1`.

Think of the file as an ordered stream of small event records. Framework
records mark where a Run starts and ends. Application and user records can add
the surrounding context. A reader can follow the activity in order, while the
final boundary record keeps the complete evidence for its Run together.

The [observability specification](observability.md) and the
[Run Record specification](run-record-spec.md) define the structures
stored inside these records. A Run start embeds one progress snapshot. A Run
end embeds the complete `RunRecord`. The observability specification's
data conventions, including identifier grammar, timestamps, and field presence,
govern every framework-owned value in this document; application-supplied event names and
correlation values follow the looser rules in their own section below.

Portability covers the framework-owned envelope and record shapes. Captured
values that the observability specification and the Run Record
specification mark implementation-native retain their language-binding shape
and do not participate in cross-language structural or byte equality.

The framework stream is boundary-only. For each Run ID, the harness writes at most one
acknowledged `run.start`, because it drops repeated `started` deliveries, and one `run.end`
per terminal callback it receives. The runtime delivers exactly one terminal callback per Run
to each observer slot, so a harness registered in one slot writes one Run end per Run.
Only the Run end contains the complete Run Record.

## Line envelope

Every line uses the same envelope:

- `schema_version` (string): the constant `sergent.run_record.v1`, stamped on
  each line. The file has no header.
- `timestamp_utc` (timestamp): stamped when the line is encoded.
- `activity` (string): `sergent_activity`, `app_activity`, or
  `user_activity`.
- `event` (string).
- `payload` (JSON value): an object, array, string, number, boolean, or null.
  Framework boundary lines use the object shapes defined below. Application
  and user events use the conversion rules in their section.
- `run_id`, `scene_id` (strings), and `revision` (integer): correlation keys,
  each present only when known.

The envelope tells a reader what happened and when. When correlation context
is available, it also identifies the related Run or Scene. The filename
supplies the application name and log identity, so the envelope omits them.

Framework lines use only the `sergent_activity` activity and the `run.start`
and `run.end` events. The activity channel, not the event name, identifies a
framework boundary. An application event that resembles a framework name
remains application activity.

## Run start

The Run-start `payload` is `{"snapshot": <progress snapshot>}`. The snapshot
comes only from the `started` stage, and the envelope revision is the revision
before the Run.

The harness acknowledges a Run ID only after a successful write. It silently
drops later `started` deliveries for that ID. A Run that terminates at
`started` repeats the stage in its terminal progress, so this rule prevents a
second Run-start line.

## Run end

The Run-end `payload` is `{"run_record": <the complete RunRecord>}`. The
harness serializes the record whole. Version 1 applies no projection,
filtering, or reshaping. The structures in the Run Record specification are
therefore exactly the persisted structures. An implementation must not define
a richer in-memory record and reduce it through an export filter.

A Run-end framework boundary requires the record's Scene transition. The
harness raises on a record without one and writes nothing for it; on the observer delivery
path, the framework contains that failure as an observer error. The envelope
uses the committed revision when one exists. Otherwise, it uses the revision
before the Run. Ordinary terminal observer delivery is responsible for the
single Run end for a Run.

## Application and user events

The harness also provides direct recording hooks for application-defined and
user-attributed context. The event name may be any non-blank text, and the harness
stores it verbatim. The application supplies any correlation values. The harness checks
only their shape: an ID is a non-empty string, which the harness does not strip, and the
revision is a non-negative integer. It never verifies them against a Scene.

An absent payload becomes `{}`. The harness converts a supplied value to the
envelope's JSON value without truncation or a depth bound; only the fallback text below is
bounded. Temporal values become strings, binary and path values become strings, and
structured values become objects and arrays.

A cyclic or non-finite value fails before any byte is written, so the file
stays healthy. An exception value instead becomes an object with `type` and
`message` fields. Any other unconvertible value becomes an object with `type`
and `repr` fields. In both cases, the harness limits the text field to 2048 characters,
and `type` is a qualified native type name. The harness does not catch process-control
exceptions raised during conversion.

A direct event call raises on a validation or persistence failure, so the
application receives the failure. If the same harness fails on the observer
delivery path, the Sergent Result contains that failure as an observer error.

## Exact line encoding

These normative rules define the interoperable line encoding. They constrain framing,
character encoding, whitespace, and key order; they do not prescribe one unique byte
representation for every JSON value:

- Write one JSON object per line, terminated by a single line feed on every
  platform.
- Write ASCII only, escaping non-ASCII characters as `\uXXXX`.
- Write no NaN or infinity tokens anywhere.
- Use compact separators, `,` and `:`, with no added whitespace.
- Sort object keys at every nesting depth in ascending Unicode code point order of the
  unescaped key text.

Conforming serializers may use different legal string escapes or number spellings for
the same JSON value. Equivalent Runs can also differ because their implementation-native
captured values differ. Neither case requires byte equality across language bindings.

## File discipline

- Name the file `<app_name>+<log_id>+<UTC compact timestamp>.jsonl`. The
  compact timestamp is the year through the second plus six fractional digits
  and a literal `Z`, for example `20260828T094102881500Z`. Both `app_name` and
  `log_id` match `^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$`. The application may configure the
  log ID; when it does not, the harness supplies the default, a framework ID with prefix
  `log`. The `+` separator is excluded from both component grammars, so a reader can
  unambiguously recover the application name and log identity from the filename.
- Exclusively create one Run Record file for each configured log identity. Refuse an existing
  file; never truncate or append to it.
- Use owner-only file permissions where the platform supports them.
- Acknowledge a line only after writing and flushing it. Before acknowledging
  a Run end, also synchronize it to storage.
- On the first persistence failure, permanently disable the file. Later
  writes report unavailability, chained to that first failure, and never touch
  the stream again. The failed file may end in a partial line and is then
  invalid. Closing synchronizes and closes a healthy file. It is idempotent
  and reports only failures that arise during the close itself, never an
  earlier delivered failure.
- Use one writer guard so that lines never interleave, allowing concurrent Runs and
  threads to share a Run Record file. Every acknowledged line is complete. The guard
  protects thread-based hosts; it is not an async scheduling mechanism.

## Application freedoms and limits

Applications may:

- share one Run Record file across several configured runtimes and concurrent Runs;
- record events when no Run is in flight or correlate them to an
  already-finished Run;
- register additional observers after the harness, including an observer
  that records application events into the same Run Record file from its terminal
  callback;
- produce a Run Record file containing zero Runs;
- read the file while Runs are in flight.

Applications must not:

- synthesize or forward framework boundary lines;
- invoke the harness's observer callbacks outside runtime delivery;
- rewrite, truncate, or append to an existing Run Record file.

## Sensitivity

A persisted Run end carries the complete Run Record. This is sensitive forensic data:
prompts, model output, model-visible Scene projections, the canonical `Proposal Schema`
of each model call with its application descriptions, proposal payloads, and Patch facts can
all be present. Applications are responsible for its protection, including the
directory, access, retention, and redaction.
