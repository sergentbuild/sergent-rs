# sergent-rs

## Role and Purpose

`sergent-rs` is the batteries-included public API of the Sergent Rust
reference implementation: the one crate a Sergentic application depends on,
which curates and wires the framework layers below it and adds no behavior.

## How to Use This Crate

Build your Sergentic application on `sergent-rs` alone: add this one crate to
your manifest and import from `sergent_rs`. Reaching past it into a lower
framework crate is never necessary and always a mistake.

Three reasons stand behind that rule:

- The names here form the reviewed application-facing set. A name joins it
  only when an application has a real reason to construct it, implement it,
  hand it to the framework, or inspect it in a result.
- The split into lower crates is an implementation layering, separate from
  the three application layers the Sergent Vision describes. It answers
  questions an application never asks.
- One dependency keeps the layers aligned and puts the default wiring in a
  single reviewed place.

## Navigation Map

- The [crate knowledge](docs/KNOWLEDGE.md) explains the building pattern, the
  curated groups of names, and what to study in each framework layer.
- New to Sergent? The [specification overview](../sergent/docs/KNOWLEDGE.md)
  explains the concept in a few minutes and orders the documents of the
  authoritative rulebook.
