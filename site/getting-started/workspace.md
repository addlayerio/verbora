# The workspace map

Verbora is one Cargo workspace. Knowing its shape makes the rest of this site
easier to navigate, and explains why some things live where they do.

## Crates

```text
crates/
  verbora-core/            traits, Token, StopWords, reference string semantics
  verbora-tokenizers/      25 tokenizers, the Tokenize trait
  verbora-distance/        Levenshtein, Damerau, Jaro–Winkler, Dice, Hamming
  verbora-phonetics/       SoundEx, Metaphone, Double Metaphone, Daitch–Mokotoff
  verbora-ngrams/          n-gram windows, frequency stats, Chinese n-grams
  verbora-normalizers/     diacritics, English contractions, Japanese width/kana
  verbora-inflectors/      pluralise/singularise, ordinals (en / fr / ja)
  verbora-trie/            prefix tree
  verbora-transliterators/ Japanese kana → romaji, five-phase pipeline
  verbora-wordnet/         lexical database, synsets, relation traversal
  verbora-tfidf/           term interning, incremental idf cache
  verbora-sentiment/       14 lexicons across 10 languages, sticky negation
  verbora-classifiers/     Bayes, logistic regression, MaxEnt + GIS
  verbora-examples/        the code printed on this site    (dev-only, not published)

  verbora-analyzers/       sentence analysis   ┐
  verbora-util/            stop words, graphs  │  implemented, tested,
  verbora-spellcheck/      Norvig correction   ├─ no feature page yet
  verbora-stemmers/        Porter × 13, Lancaster, ja, id
  verbora-tagger/          Brill POS tagger, trainer, tester ┘  (see roadmap)
```

Every API the reference exports is implemented and tested — the split
above is about documentation coverage, not code completeness. See the
[roadmap](../features/roadmap.md) for the five crates still missing a feature
page.

The dependency graph is deliberately shallow, and acyclic apart from the one
place a subsystem's own reference tree calls another (phonetics tokenizes
internally for `tokenize_and_phoneticize`; the transliterator's Japanese-only
sibling shares the normalizer tables rather than duplicating them):

```text
verbora-core ──┬── verbora-tokenizers ── regex
               ├── verbora-distance ──── rustc-hash
               ├── verbora-phonetics ─── regex, verbora-tokenizers
               ├── verbora-inflectors ── regex
               ├── verbora-ngrams
               ├── verbora-tfidf ─────── verbora-tokenizers, rustc-hash
               └── verbora-classifiers ─ verbora-stemmers

verbora-normalizers      (no dependencies at all)
verbora-trie ─────────── smallvec
verbora-transliterators ─ verbora-normalizers
verbora-wordnet ───────── verbora-core, memchr
verbora-sentiment ─────── verbora-stemmers, rustc-hash
```

`verbora-core` depends on nothing outside `std`. Leaf crates can therefore be
used in isolation without dragging in data assets or a regex engine they do not
need — `verbora-normalizers` in particular has an empty `[dependencies]` section
with a comment explaining, per omitted crate, why it is absent.

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

**the recorded cases in total**, across 791 suites, every one of them
captured from the reference rather than transcribed by hand. See
the roadmap.

## Supporting directories

```text
docs/
  COMPETITIVE_BENCHMARKS.md   the competitor matrix and the dossier behind it
  PERFORMANCE_GAPS.md         every measured gap, with its cause and verdict
  PERFORMANCE_MATRIX.md       per-crate optimisation review status
  design/                     design notes per subsystem
site/                         this site (VitePress)
tools/
  bench-data/                 generates the inputs every harness reads
benches/data/                 those generated inputs
benchmarks/competitive/       head-to-head suite vs. pinned third-party crates
  rust-competitors/           the benches, correctness tests and memory report
  manifests/                  pinned competitor versions and selection notes
  results/                    structured summary, raw estimates, machine metadata
  scripts/                    result collection and machine attribution
```

## A crate you will not depend on

**`verbora-examples`** exists so that the code on this site is real. Every
non-trivial snippet you see here is extracted by `site/check-snippets.py`
into a generated example in that package and compiled *and run* against the
actual crates. A snippet that stops compiling, or whose assertions stop
holding, fails the build — see
[Documentation is part of the code](../reference/docs-are-code.md).

## Where the recorded behaviour lives

The recorded behaviour was captured once and is
checked in as data — every rule table, stop-word list, character class and
model weight under each crate's own `src/data/` is machine-derived, never
transcribed by hand. The reference tree itself is **not** part of this
workspace and is not a dependency of anything:

```bash
cargo test --workspace
```

## Next

- [Cargo features](cargo-features.md) — all one of them.
- [Features overview](../features/index.md) — what is implemented and what is not.
  replayed.
