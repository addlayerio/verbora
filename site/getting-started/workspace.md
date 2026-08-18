# The workspace map

Verbora is one Cargo workspace of 19 production crates. Knowing its shape makes
the rest of this site easier to navigate.

## What each crate is for

| Crate | Public surface |
|---|---|
| [`verbora-core`](../features/core.md) | 6 traits, `Token`, `StopWords`, `whitespace` helpers, `trim_edge_empties` |
| [`verbora-tokenizers`](../features/tokenizers.md) | 25 tokenizers, `Tokenize`, `Utf16Token`, `Pattern` |
| [`verbora-distance`](../features/distance.md) | 8 metric functions, 5 `StringMetric` impls, `units` |
| [`verbora-phonetics`](../features/phonetics.md) | 4 encoders, `phoneticize_tokens*`, `PhoneticError`, `units` |
| [`verbora-ngrams`](../features/ngrams.md) | window engine, stats, text entry points, `zh` |
| [`verbora-normalizers`](../features/normalizers.md) | 6 normalizers + 17 Japanese converters |
| [`verbora-inflectors`](../features/inflectors.md) | 6 inflectors, `Rule`, `CaseMode`, `pattern` |
| [`verbora-trie`](../features/trie.md) | `Trie`, `KeysWithPrefix`, `MatchesOnPath` |
| [`verbora-transliterators`](../features/transliterators.md) | `transliterate_ja`, `transliterate_into`, `Phase`, `Rewrite`, `Rewrites` |
| [`verbora-wordnet`](../features/wordnet.md) | `WordNet`, `Storage`, `Sense`, `Pointer`, relation traversal |
| [`verbora-tfidf`](../features/tfidf.md) | `TfIdf`, `DocumentInput`, `Interner`, `Encoding` |
| [`verbora-sentiment`](../features/sentiment.md) | `SentimentAnalyzer`, `VocabularyKind`, `Contributions` |
| [`verbora-classifiers`](../features/classifiers.md) | `BayesClassifier`, `LogisticRegressionClassifier`, `MaxEntClassifier` |
| [`verbora-stemmers`](../features/stemmers.md) | Porter/Snowball language stemmers, Lancaster, Japanese and Indonesian |
| [`verbora-spellcheck`](../features/spellcheck.md) | `Spellcheck`, `FuzzyIndex`, `DeletionIndex`, lazy edits |
| [`verbora-tagger`](../features/tagger.md) | `BrillPosTagger`, lexicons, rules, trainer and tester |
| [`verbora-analyzers`](../features/analyzers.md) | `SentenceAnalyzer`, `TaggedWord`, `SenType` |
| [`verbora-language`](../features/language.md) | script detection, optional language detectors, phonetic recommendations |
| [`verbora-util`](../features/util.md) | abbreviations, stop words, graphs, path trees and storage backends |

## The dependency graph

Deliberately shallow and acyclic. Two crates reach sideways on purpose:
`verbora-phonetics` tokenizes internally for `tokenize_and_phoneticize`, and
`verbora-transliterators` shares the normalizer's kana tables rather than
duplicating them.

```text
verbora-core ──┬── verbora-tokenizers ── regex
               ├── verbora-distance ──── rustc-hash
               ├── verbora-phonetics ─── regex, verbora-tokenizers
               ├── verbora-inflectors ── regex
               ├── verbora-ngrams ────── rustc-hash
               ├── verbora-tfidf ─────── verbora-tokenizers, rustc-hash, serde
               └── verbora-classifiers ─ verbora-stemmers, rustc-hash

verbora-normalizers      (no dependencies at all)
verbora-trie ─────────── smallvec
verbora-transliterators ─ verbora-normalizers
verbora-wordnet ───────── verbora-core, memchr
verbora-sentiment ─────── verbora-stemmers, rustc-hash
```

`verbora-core` depends on nothing outside `std`. A leaf crate can therefore be
used in isolation without dragging in data assets or a regex engine it does not
need — `verbora-normalizers` in particular ships an empty `[dependencies]`
section.

## The rest of the repository

```text
site/                     this site (VitePress)
docs/                     internal engineering and design notes
tools/bench-data/         generates the inputs every benchmark harness reads
benches/data/             those generated inputs
benchmarks/competitive/   head-to-head suite against pinned third-party crates
crates/verbora-examples/  compiled documentation snippets (dev-only)
```

`verbora-examples` is not a crate you depend on: it exists so that the code on
this site is real. Every non-trivial snippet here is extracted into a generated
example in that package and compiled *and run* against the actual crates — see
[Documentation is part of the code](../reference/docs-are-code.md).

## Next

- [Cargo features](cargo-features.md) — the four opt-ins and what they cost.
- [Features overview](../features/index.md) — what each subsystem does and when
  to reach for it.
