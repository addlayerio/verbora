# Rust API reference

Rustdoc is the authoritative reference for signatures, trait implementations,
error types and exact edge-case behaviour. The guides on this site cover what
rustdoc cannot: which API to reach for, how the variants differ, and what each
one costs.

## Crates

| Crate | What is in it | Guide | Rustdoc |
|---|---|---|---|
| `verbora-core` | The 5 shared traits, `StopWords`, `StopWordLanguage`, the process-global stop-word list | [Core vocabulary](../features/core.md) | <a href="../api/verbora_core/">API</a> |
| `verbora-tokenizers` | 3 UAX #29 tokenizers behind one borrowing trait | [Tokenizers](../features/tokenizers.md) | <a href="../api/verbora_tokenizers/">API</a> |
| `verbora-distance` | 7 metrics: Levenshtein, unrestricted Damerau, OSA, Jaro, Jaro–Winkler, Dice, Hamming | [String distance](../features/distance.md) | <a href="../api/verbora_distance/">API</a> |
| `verbora-phonetics` | 12 encoders (SoundEx, Metaphone, Double Metaphone, Daitch–Mokotoff, Beider–Morse and the rest), plus `PhoneticIndex` | [Phonetics](../features/phonetics.md) | <a href="../api/verbora_phonetics/">API</a> |
| `verbora-ngrams` | Slice windows, boundary padding, character n-grams | [N-grams](../features/ngrams.md) | <a href="../api/verbora_ngrams/">API</a> |
| `verbora-normalizers` | The four Unicode normalization forms and diacritic folding, all `Cow` | [Normalizers](../features/normalizers.md) | <a href="../api/verbora_normalizers/">API</a> |
| `verbora-inflectors` | Pluralise, singularise, ordinals (en / fr / ja) | [Inflectors](../features/inflectors.md) | <a href="../api/verbora_inflectors/">API</a> |
| `verbora-trie` | Prefix tree with prefix, longest-prefix and prefix-match queries, plus `FrozenTrie` | [Trie](../features/trie.md) | <a href="../api/verbora_trie/">API</a> |
| `verbora-transliterators` | Japanese kana → romaji | [Transliterators](../features/transliterators.md) | <a href="../api/verbora_transliterators/">API</a> |
| `verbora-wordnet` | Synsets, senses, relation traversal, four storage backends | [WordNet](../features/wordnet.md) | <a href="../api/verbora_wordnet/">API</a> |
| `verbora-tfidf` | Sparse TF-IDF over an owned `Analyzer`, ranking and JSON persistence | [TF-IDF](../features/tfidf.md) | <a href="../api/verbora_tfidf/">API</a> |
| `verbora-sentiment` | 14 lexicons across 10 languages | [Sentiment](../features/sentiment.md) | <a href="../api/verbora_sentiment/">API</a> |
| `verbora-classifiers` | Naive Bayes, logistic regression, MaxEnt | [Classifiers](../features/classifiers.md) | <a href="../api/verbora_classifiers/">API</a> |
| `verbora-stemmers` | Porter/Snowball, Lancaster, Japanese, Indonesian | [Stemmers](../features/stemmers.md) | <a href="../api/verbora_stemmers/">API</a> |
| `verbora-spellcheck` | Norvig correction, a BK-tree `FuzzyIndex` and a `DeletionIndex` | [Spellcheck](../features/spellcheck.md) | <a href="../api/verbora_spellcheck/">API</a> |
| `verbora-tagger` | Brill POS tagger, trainer and evaluation | [POS tagger](../features/tagger.md) | <a href="../api/verbora_tagger/">API</a> |
| `verbora-analyzers` | Sentence structure analysis | [Sentence analyzers](../features/analyzers.md) | <a href="../api/verbora_analyzers/">API</a> |
| `verbora-language` | Script detection, optional language detectors, phonetic recommendations | [Language](../features/language.md) | <a href="../api/verbora_language/">API</a> |
| `verbora-util` | Stop-word re-exports, abbreviations, graphs, path trees, topological ordering | [Utilities](../features/util.md) | <a href="../api/verbora_util/">API</a> |

## Reading the docs locally

The `/api/` links above are populated on the deployed site. Against a local
checkout, generate rustdoc yourself:

```bash
cargo doc --workspace --no-deps --open
```

## How the two layers divide the work

- **Rustdoc** defines behaviour, invariants and error conditions per item. Its
  examples run as doctests unless explicitly marked otherwise, so what you read
  there compiles and passes.
- **This site** covers selection, composition, allocation behaviour and workload
  trade-offs — the questions a signature cannot answer.

## Where to go next

| You want | Go to |
|---|---|
| To move code from 0.1 to 0.2 | [Upgrading from 0.1 to 0.2](../getting-started/upgrading.md) |
| To pick between several similar APIs | [Choosing the right API](../choosing/index.md) |
| Allocation, laziness, batching and parallelism | [Performance](../performance/index.md) |
| Measured numbers and how they were produced | [Benchmarks](../benchmarks/index.md) |
| The snippets on this site, as compilable code | [`crates/verbora-examples/examples/`](https://github.com/addlayerio/verbora/tree/main/crates/verbora-examples/examples) |
