# For Implementers

This document offers guidance for anyone implementing the Sergent Specification from scratch
in a new programming language. It adds no conformance requirements; the specification
documents define conformance, and the overview's
[designing a new implementation](KNOWLEDGE.md#designing-a-new-implementation) section lists
the questions to answer before coding. This document adds practical advice on three of those
questions and then recommends a code layering.

## Practical advice

### Interact with the model providers

Prefer using their official language-specific SDK.

If no SDK is available, invoke their API following their API documents.

Beware of each provider's unique restrictions and features.

### JSON deserialization

Design for type-safety and avoid directly using the model-generated JSON.

Decode it into the typed Intent Proposal or Plan Proposal first, as the
[trust boundary specification](trust-boundaries.md) requires, and let the Recipe derive the
Intent or Execution Plan from that typed value.

### Asynchronous programming

The Sergent Specification intends a Sergent Run to be non-blocking.

Prefer using the language-specific async syntax or tooling.

For a language lacking first-class support for async programming, consider a simple
abstraction over the event loop or callback and enforce type-safety and exception handling.

## Recommended implementation layering

This section is a recommendation, not a conformance requirement. It organizes an
implementation's code; it is not the three architecture layers of the Sergent Vision, which
describe an application.

This guide recommends that an implementation keep an acyclic, one-directional layering with
clear responsibilities for each layer. Dependencies point downward: each layer depends only on
the layers listed before it, and nothing depends on the application-facing layer.

- The specification values: the closed vocabularies, the identifier grammar, and the record
  shapes, plus the three interfaces that an application implements or consumes (the model
  client, the Scene Actions, and the Recipe), with no I/O and no engine.
- The execution engine: the deterministic Run flow, shared live state, progress snapshots,
  observer delivery, concurrency, and the opt-in Run Record harness.
- The concrete model transport behind the model client interface: provider adapters and
  provider-shaped test fakes.
- The application-facing layer: default wiring and the curated public vocabulary that
  applications import.

Example applications sit above these layers. They exercise the same
single-Run rulebook across different Scene shapes and orchestration patterns.
Each application defines its user experience, budgets, and persistence
policy.

With these decisions made, build the example applications in the overview's
[quick tour](KNOWLEDGE.md#example-applications-quick-tour); an implementation that can build
them is feature complete.
