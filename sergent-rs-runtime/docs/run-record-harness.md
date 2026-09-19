# Run Record Persistence

Every Run builds a `RunRecord` in memory and hands it back inside the result.
Writing that record to disk is a separate, optional step, and
`JsonlRunRecordWriter` is the one harness this crate offers for it. The
[runtime knowledge file](KNOWLEDGE.md) covers the engine itself; this page
covers persistence alone.

The harness is deliberately narrow. It plugs into a Run as an ordinary
observer slot, so it gains no execution authority and cannot change a Run's
outcome. The
[Run Record file format](../../sergent/docs/run-record-file-format.md) defines
the lines it produces and the discipline the file follows.

## Creating the File

Creation takes three inputs: an application-selected directory, which the
harness creates when absent; a validated application name; and an optional
validated log identity. When the caller supplies no identity the harness mints
the default one. It then builds the portable filename, creates that file
exclusively, and returns both the harness and the chosen path.

Exclusive creation is the point. The harness never truncates and never appends
to an existing file, so two harnesses can never interleave their lines into
one Run Record file, and an existing record can never be silently replaced.

## What the Harness Writes

As an observer the harness writes two framework boundary lines. It writes a
Run start only for the running started snapshot, acknowledged once per Run
identity, and one Run end per terminal callback. Only a Run end forces storage
synchronization before the line counts as acknowledged, which keeps the cost
of frequent progress traffic low while still making the complete record
durable.

An application may also record application events and user events directly.
Those values convert through a graph that belongs to this crate and stays
deliberately separate from Run Record capture, because the two serve different
readers. The graph accepts shared acyclic nodes and rejects cycles and
non-finite numbers before any byte reaches the file. One recursive encoder
then produces the canonical line.

## Failure Handling and Close

One guard serializes every line. It retains the first ambiguous failure and
chains later unavailability to that original cause, so a reader learns what
actually broke rather than reading a cascade of identical follow-on errors.
Closing is idempotent, and dropping the harness closes a healthy file as a
courtesy.

The file sits behind a private writer seam. Production binds the real file to
it; a deterministic test binds a scripted one and drives partial writes, flush
failures, synchronization failures, and close failures without touching a
filesystem.

## Security and Retention

A complete Run Record is sensitive forensic data: it can carry prompts, model
output, model-visible Scene projections, and Patch facts, as the
[observability specification](../../sergent/docs/observability.md#sensitivity)
warns. The harness answers only the part it can.

On Unix the file is created with permissions admitting only the user who
created it. On Windows it inherits the directory's access rules, so the
application selects a suitably secured directory rather than expecting the
harness to tighten one. Retention, redaction, access control, and the decision
to persist evidence at all remain with the application.

Records flow one way. Neither this crate nor an application reconstructs
in-process runtime values from a Run Record file; the file is evidence for
inspection, never an input to a later Run.
