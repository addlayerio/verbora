# Roadmap

All **105 public APIs** are implemented and tested. This page is the history:
what was hardest to build, and what is left to *document* now that nothing is
left to *implement*.

## What is left: five crates without a feature page

Every API is implemented. Five crates pass their full test and test suites
with no known failures but have no feature page on this site yet — a
documentation gap, not a code gap:

| Crate | Suites | Cases | Covers |
|---|--:|--:|---|
| `verbora-analyzers` | 49 | 29,254 | Sentence analysis — prepositional phrases, subject/predicate splitting |
| `verbora-util` | 624 | 17,798 | Stop words, abbreviations, edge-weighted digraphs, pluggable storage backends |
| `verbora-spellcheck` | 102 | 4,976 | Norvig-style correction, lazy frequency-ranked candidates |
| `verbora-stemmers` | 118 | 381,819 | Porter × 13, `CarryStemmerFr`, `LancasterStemmer`, Japanese, Indonesian |
| `verbora-tagger` | 69 | 32,306 | Brill POS tagger, trainer, tester, rule templates |

See the [features overview](index.md#implemented) for the twelve crates that
do have a page.

## Two things that were harder than a fresh implementation

### Stemmers and the tagger were fixed, not written from scratch

Both of these were, at one point, close but not there. `verbora-stemmers` had
real, substantial code — it already consumed `verbora_core::Token`'s Snowball
region-marking machinery (`mark_region`, `has_suffix_in_region`,
`replace_suffix_in_region`) and its own module docs described all seventeen
exports — but did not build. `verbora-tagger` built and passed its unit
tests, but its own flagship doctest tagged `["I", "would", "book", "a",
"flight"]`'s `"I"` as `NN` instead of `PRP`, a real bug rather than a
documentation slip. Both are fixed now: `cargo test -p verbora-stemmers` and
`cargo test -p verbora-tagger` are fully green, with 78 and 105 unit tests
respectively, plus their own test suites. Neither has a feature page yet —
see the table above.

### Classifiers, TF-IDF and sentiment went from scaffold to complete

These three were, for most of this project's history, true placeholder
crates — a `lib.rs` reading *"Implementation in progress"* and zero public
items. All three are now complete, tested, and documented:
[Classifiers](classifiers.md) (Bayes, logistic regression, and MaxEnt with
generalised iterative scaling — the largest single subsystem in the
workspace, including a from-scratch port of the reference's part-of-speech
feature-generation machinery), [TF-IDF](tfidf.md) (term interning, an
incremental idf cache, and a from-scratch port of the reference engine's `Math.log`), and
[Sentiment](sentiment.md) (fourteen lexicons across ten languages, with sticky
negation and stem-collision resolution reproduced exactly).

## How a subsystem gets migrated

The recipe is:
with `verbora-distance` as the worked reference. In outline:

```text
1. Read docs/specs/<module>.json   — the behavioural analysis of the reference module,
                                      including its quirks (333 recorded so far)
2. Record fixtures                 — captured from the reference implementation
3. Implement                       — Rust-native architecture, not transliteration
4. Replay                          — the crate's tests assert every recorded case
5. Benchmark                       — against the reference baseline, same inputs
6. Document                        — this site, in the same change
```

Step 6 is not optional. See
[Documentation is part of the code](../reference/docs-are-code.md).

## What "implemented" means here

There is no stub, no `todo!()` and no partially working type shipped as if it
were, and that was true throughout the migration, not just at the end. A
half-implemented stemmer that silently disagrees with the reference is worse
than no stemmer, because the whole premise of the project is that you can
trust the output to match. A subsystem appears on the
[features overview](index.md) when it has a public API *and* a passing
differential suite — not before. Crate size and doc-comment detail tell you
nothing about readiness on their own: a crate can read, on paper, like a
finished one and still fail to build, or build and still fail its own
doctest. Only a green `cargo test -p <crate>` does.

<div class="callout callout-note">
<strong>Checking for yourself.</strong> <code>cargo doc --workspace --no-deps</code>
lists every crate and its full public surface.
</div>
