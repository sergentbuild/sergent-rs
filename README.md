# sergent-rs

## Introduction

sergent-rs is the Rust reference implementation of the
[Sergent Specification](sergent/docs/KNOWLEDGE.md). Sergent is a concept for
agentic software in which the user stays in charge: a model only proposes a
change, and deterministic code validates, rehearses, and commits it.

The specification is a language-agnostic rulebook and contains no code. This
workspace turns that rulebook into a working Rust framework, for two reasons.
It shows, rule by rule, that the specification can be built. It also gives Rust
developers a framework that is good enough and ready to use: add one
dependency, describe your data and your actions as Rust types, choose a model,
and start bounded Runs.

The word "reference" sets the priority. The specification is authoritative, and
a tracked copy of it lives in [sergent](sergent/docs/KNOWLEDGE.md), maintained by the
author. When this implementation and the specification disagree, the
implementation is wrong.

The implementation targets stable Rust:

- Language: the Rust 2024 edition.
- Toolchain: stable 1.96.0, pinned by `rust-toolchain.toml`; rustup installs it
  on first use. Every change is verified against that pinned compiler.
- Major dependencies: tokio for async execution, serde and serde_json for typed
  decoding and evidence, schemars at one exact version for schema derivation,
  and reqwest for HTTP transport to model providers.
- Platforms: developed and verified on Windows x64, macOS arm64, and Linux x64.
  The test suite is hermetic, so it behaves the same on all three.

## Components

Four crates form the framework stack. The runtime crate and the providers crate
each depend on the core crate alone, and the public API crate depends on all
three. These are implementation layers; they are distinct from the three
application layers of the Sergent Vision.

- [sergent-rs-core](sergent-rs-core/README.md): the shared vocabulary. It holds
  the typed specification values, the three interfaces an application
  implements or consumes (`SergentRecipe`, `SceneActions`, and `ModelClient`),
  the canonical Proposal Schema machinery, and the Operation registry. It
  performs no I/O and runs nothing.
- [sergent-rs-runtime](sergent-rs-runtime/README.md): the execution engine. It
  drives one bounded Run from observation to commit, and it is responsible for
  Scene authority, cancellation, observer delivery, Run Record construction,
  and the opt-in Run Record harness.
- [sergent-rs-providers](sergent-rs-providers/README.md): the model transport.
  It speaks to hosted and local model providers through small adapters, and it
  ships the deterministic test client that application test suites use. Its
  [provider reference](sergent-rs-providers/docs/providers.md) lists the
  supported providers.
- [sergent-rs](sergent-rs/README.md): the public API crate. It re-exports the
  curated surface and wires the defaults. An application depends on this crate
  alone and imports from `sergent_rs` only.

## How to Work In This Project

Start with [docs/KNOWLEDGE.md](docs/KNOWLEDGE.md). It explains how the four
crates materialize the specification and maps every further document. The
binding design decisions live in
[docs/technical-direction.md](docs/technical-direction.md). Each crate keeps a
README and a knowledge file with its design detail.

Contributors need the Rust 2024 edition, async programming with tokio,
derive-based modeling with serde and schemars, and the vocabulary of the
Sergent Specification.

The workflow runs from the workspace root:

- `just test` runs the full workspace test suite.
- `just lint` runs the static checks: formatting, the build, Clippy with
  warnings denied, and the document checks.
- `just fmt` formats the Rust code in place.

Run `just test` and `just lint` from the workspace root; both must pass before
a change is finished. Tests are hermetic: no live provider, no real credential,
no network.
