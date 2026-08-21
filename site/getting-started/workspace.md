# The workspace map

Verbora is one Cargo workspace of 19 production crates. Knowing its shape makes
the rest of this site easier to navigate.

## What each crate is for

| Crate | Public surface |
|---|---|
| [`verbora-core`](../features/core.md) | 5 traits, `StopWords`, `StopWordLanguage`, the process-global stop-word list |
| [`verbora-tokenizers`](../features/tokenizers.md) | `WordTokenizer`, `SegmentTokenizer`, `SentenceTokenizer` |
| [`verbora-distance`](../features/distance.md) | 7 metrics, weighted cost sets, substring search, `PreparedPattern` |
| [`verbora-phonetics`](../features/phonetics.md) | 12 encoders, `PhoneticIndex`, `BeiderMorse`, `phoneticize_tokens` |
| [`verbora-ngrams`](../features/ngrams.md) | `ngrams`, `Padded`, `char_ngrams`, `CharNGrams` |
| [`verbora-normalizers`](../features/normalizers.md) | `nfc`, `nfd`, `nfkc`, `nfkd`, `remove_diacritics` |
| [`verbora-inflectors`](../features/inflectors.md) | 6 inflectors, `Rule`, `CaseMode`, `Gender` |
| [`verbora-trie`](../features/trie.md) | `Trie`, `FrozenTrie`, `KeysWithPrefix`, `PrefixMatches` |
| [`verbora-transliterators`](../features/transliterators.md) | `transliterate_ja`, `transliterate_ja_into`, `transliterate_ja_normalized`, `Rewrite` |
| [`verbora-wordnet`](../features/wordnet.md) | `WordNet`, `Storage`, `Sense`, `Pointer`, `PrebuiltIndex`, relation traversal |
| [`verbora-tfidf`](../features/tfidf.md) | `TfIdf`, `Document`, `Analyzer`, `DocumentScore`, `TermScore` |
| [`verbora-sentiment`](../features/sentiment.md) | `SentimentAnalyzer`, `VocabularyKind`, `Vocabulary`, `Contributions` |
| [`verbora-classifiers`](../features/classifiers.md) | `BayesClassifier`, `LogisticRegressionClassifier`, `MaxEntClassifier` |
| [`verbora-stemmers`](../features/stemmers.md) | Porter/Snowball language stemmers, Lancaster, Japanese and Indonesian |
| [`verbora-spellcheck`](../features/spellcheck.md) | `Spellcheck`, `FuzzyIndex`, `DeletionIndex`, `Correction`, `Neighbor` |
| [`verbora-tagger`](../features/tagger.md) | `BrillTagger`, `Lexicon`, `RuleSet`, `Trainer`, `Evaluation` |
| [`verbora-analyzers`](../features/analyzers.md) | `analyze`, `SentenceAnalysis`, `SentenceType`, `TaggedWord`, `Role` |
| [`verbora-language`](../features/language.md) | script detection, optional language detectors, phonetic recommendations |
| [`verbora-util`](../features/util.md) | abbreviations, stop-word re-exports, graphs, path trees, topological ordering |

## The dependency graph

Deliberately shallow and acyclic. Three crates reach sideways on purpose:
`verbora-phonetics` tokenizes internally for `tokenize_and_phoneticize`,
`verbora-transliterators` shares the normalizer's kana tables rather than
duplicating them, and `verbora-language` composes the phonetic encoders with the
transliterators to turn a detected language into a recommendation.

```text
verbora-core ──┬── verbora-tokenizers ─── unicode-segmentation
               ├── verbora-phonetics ──── verbora-tokenizers, regex
               ├── verbora-stemmers ───── verbora-tokenizers
               ├── verbora-tfidf ──────── verbora-tokenizers, rustc-hash, serde
               └── verbora-util ───────── rustc-hash

verbora-ngrams           (no dependencies at all)
verbora-inflectors ────── regex
verbora-normalizers ───── unicode-normalization
verbora-distance ──────── rustc-hash
verbora-trie ──────────── smallvec
verbora-tagger ────────── rustc-hash
verbora-analyzers        (no dependencies at all)
verbora-wordnet ───────── memchr, rustc-hash
verbora-transliterators ─ verbora-normalizers
verbora-spellcheck ────── verbora-distance, rustc-hash
verbora-sentiment ─────── verbora-stemmers, verbora-tokenizers, rustc-hash
verbora-classifiers ───── verbora-stemmers, verbora-tokenizers, rustc-hash
verbora-language ──────── verbora-phonetics, verbora-transliterators
```

`rayon` is not drawn because it is optional in all fourteen crates that use it,
and `whatlang` is optional in `verbora-language` — none of them is pulled in by
a default dependency.

`verbora-core` reaches for one crate, `rustc-hash`, because `StopWords::contains`
runs on every token of every document a stemmer filters. A leaf crate can be
used in isolation without dragging in data assets or a regex engine it does not
need: `verbora-ngrams` and `verbora-analyzers` ship an empty `[dependencies]`
section, and `verbora-distance` reaches for nothing but `rustc-hash`, with no
Unicode character database of any kind behind it.

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
