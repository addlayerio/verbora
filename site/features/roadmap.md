# Status and scope

Verbora is a pre-1.0 Cargo workspace. Every production crate is implemented,
tested, documented in rustdoc, and covered by a guide in this site.

## What "available" means

A capability listed in the [features overview](index.md) has:

- a public Rust API;
- tests that pin its documented behavior;
- crate-level rustdoc for exact semantics and edge cases;
- a user guide explaining when to use it and how it composes.

Optional capabilities stay explicit. Parallel operations require the `parallel`
Cargo feature *and* an explicitly named `par_*` call; language detection is
feature-gated separately. See
[Cargo features](../getting-started/cargo-features.md).

## Stability

The workspace version is `0.2.0`, and public APIs may still be refined before a
stable release. Pre-1.0, a minor bump is where that refinement lands: 0.2.0 is a
**breaking** release relative to 0.1.0 — signatures moved, several public error
and payload enums became `#[non_exhaustive]`, and a number of behaviors were
corrected — so upgrading is a code change, not a version-number change.

Documented behavior and test fixtures are treated as deliberate contracts in the
meantime: changing one means changing its tests and its documentation in the
same step, so a behavior you build on does not move *silently* — it moves in a
release that says it moved.

## Deliberate boundaries

Verbora is an NLP toolkit, not a search engine or a hosted service. Concretely:

- phonetic indexes generate candidates; ranking is yours;
- distance metrics compare strings; they do not manage a corpus;
- WordNet reads a database you supply and redistributes none of it;
- sentence analyzers consume POS-tagged input rather than tagging raw text;
- parallel APIs use your Rayon environment and never configure a global thread
  pool.

Where a capability is out of scope, its page says so plainly rather than leaving
you to discover it — most under a "When not to use it" heading, the rest in
whichever section states that crate's own contract.
