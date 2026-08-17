# Features overview

What Verbora can do today, and where each thing lives.

## Implemented

Every subsystem below is implemented and covered by the workspace test suite.

| Subsystem | Crate | Public surface |
|---|---|---|
| [Tokenizers](tokenizers.md) | `verbora-tokenizers` | 25 tokenizers, `Tokenize`, `Utf16Token` |
| [String distance](distance.md) | `verbora-distance` | 8 metrics, 5 `StringMetric` impls |
| [Phonetics](phonetics.md) | `verbora-phonetics` | SoundEx, Metaphone, Double Metaphone, Daitch–Mokotoff |
| [N-grams](ngrams.md) | `verbora-ngrams` | window engine, stats, Chinese n-grams |
| [Normalizers](normalizers.md) | `verbora-normalizers` | 6 normalizers, 17 Japanese converters |
| [Inflectors](inflectors.md) | `verbora-inflectors` | 6 inflectors, runtime rules |
| [Trie](trie.md) | `verbora-trie` | prefix tree, prefix and path queries |
| [Transliterators](transliterators.md) | `verbora-transliterators` | Japanese kana → romaji, five-phase pipeline |
| [WordNet](wordnet.md) | `verbora-wordnet` | lexical database, synsets, relation traversal, 4 storage strategies |
| [TF-IDF](tfidf.md) | `verbora-tfidf` | term interning, incremental idf cache, `listTerms`/`tfidf`/`tfidfs` |
| [Sentiment](sentiment.md) | `verbora-sentiment` | 14 lexicons across 10 languages, sticky negation |
| [Classifiers](classifiers.md) | `verbora-classifiers` | Bayes, logistic regression, MaxEnt + GIS |
| [Core vocabulary](core.md) | `verbora-core` | 6 traits, `Token`, `StopWords`, reference string semantics |

<div class="callout callout-note">
<strong>Implemented, not yet on this site.</strong> Five crates pass their
full test suites today with no known failures, but have no feature page yet:
<code>verbora-analyzers</code> (sentence analysis — prepositional phrases,
subject/predicate splitting),
<code>verbora-util</code> (stop words, abbreviations, edge-weighted digraphs,
pluggable storage backends),
<code>verbora-spellcheck</code> (Norvig-style correction with a lazy,
frequency-ranked candidate generator),
<code>verbora-stemmers</code> (Porter × 13, Lancaster, Japanese, Indonesian),
and <code>verbora-tagger</code> (the Brill POS tagger, trainer and tester).
This is a documentation gap, not a code gap, and it is tracked on the
[roadmap](roadmap.md) rather than left silently absent.
</div>

## Coverage

The library is feature-complete: every subsystem listed above is implemented
and tested. What is left is documentation, not code — the five crates in the
callout above have no feature page yet. See the [roadmap](roadmap.md) for what
remains to be written up.

## Verbora-native extensions

Not every crate here is a reference port. Some subsystems solve a problem the
reference library never had, using the same benchmark-evidence discipline as
the crates above.

| Extension | Crate | What it adds |
|---|---|---|
| [Phonetic neighbors](phonetic-index) | `verbora-phonetics` | An index over a phonetic encoder's output. `PhoneticIndex::neighbors` answers "which stored words share a code with this query?" across a whole dictionary — phonetic candidate generation, not a search engine, and not a reimplementation of anything the reference exports. |
| [Beider-Morse](beider-morse) | `verbora-phonetics` | Cross-language surname matching across up to 18 languages at once (10 Ashkenazi, 5 Sephardic), auto-detecting which one(s) a name is plausibly spelled under. Solves what the reference's four encoders can't: the same historical family name has different "correct" spellings depending on which country transcribed it. |
| [Language](language) | `verbora-language` | Script and (optional, feature-gated) statistical language detection, plus `recommend()` — a closed lookup from `Language` to which of `verbora-phonetics`'s four encoders actually fits. Answers "which encoder should I even use?", a question no reference API ever asked. |

## By problem

If you know what you want to *do* rather than what it is called:

| I want to… | Use |
|---|---|
| Split text into words | [Tokenizers](tokenizers.md) — start with `AggressiveTokenizer` |
| Split text into sentences | [`SentenceTokenizer`](tokenizers.md) |
| Tokenize a language other than English | [Tokenizers](tokenizers.md) — 16 language variants |
| Measure how similar two strings are | [String distance](distance.md) |
| Correct a typo | [`levenshtein`](distance.md), usually after narrowing candidates |
| Match names that sound alike | [Phonetics](phonetics.md) |
| Match names across language/spelling boundaries (genealogy) | [Beider-Morse](beider-morse.md) |
| Match names with a shared prefix | [`jaro_winkler`](distance.md) |
| Build an autocomplete index | [Trie](trie.md) |
| Find repeated phrases | [N-grams](ngrams.md) |
| Strip accents before comparing | [`remove_diacritics`](normalizers.md) |
| Expand English contractions | [`normalize`](normalizers.md) |
| Normalise Japanese width and kana | [`normalize_ja`](normalizers.md) |
| Pluralise a noun, or write "23rd" | [Inflectors](inflectors.md) |
| Write generic code over any tokenizer | [Core vocabulary](core.md) |
| Rank documents by relevance | [TF-IDF](tfidf.md) |
| Score sentiment over a token list | [Sentiment](sentiment.md) |
| Classify documents into categories | [Classifiers](classifiers.md) |
| Reduce words to a stem | `verbora-stemmers` — implemented, no feature page yet (see [roadmap](roadmap.md)) |

## Language support

| Language | Tokenizer | Normalizer | Inflector | Phonetics |
|---|:--:|:--:|:--:|:--:|
| English | ✅ | ✅ | ✅ nouns, verbs, ordinals | ✅ all four encoders |
| French | ✅ | | ✅ nouns, ordinals | |
| German | ✅ | | | |
| Spanish | ✅ | | | |
| Italian | ✅ | | | |
| Portuguese | ✅ | | | |
| Dutch | ✅ | | | |
| Norwegian | ✅ | ✅ | | |
| Swedish | ✅ | ✅ | | |
| Danish/Nordic | | ✅ | | |
| Russian | ✅ | | | |
| Ukrainian | ✅ | | | |
| Polish | ✅ | | | |
| Persian | ✅ | | | |
| Hindi | ✅ | | | |
| Indonesian | ✅ | | | |
| Vietnamese | ✅ | | | |
| Finnish | ✅ `OrthographyTokenizer` | | | |
| Japanese | ✅ | ✅ + 17 converters | ✅ nouns | |
| Chinese | ✅ n-grams | | | |

Latin-script diacritic folding via `remove_diacritics` applies far more broadly
than the "Normalizer" column suggests — it is a table over 820 non-ASCII
characters, not a per-language rule set.

## How the feature pages are structured

Each one follows the same shape, so you can skim to the part you need:

```text
Overview  →  When to use  →  When not to  →  Quick example
          →  Choosing the right API   ← comparison table + decision tree
          →  Advanced usage
          →  Performance characteristics
          →  Allocation behaviour
          →  Unicode and language notes
          →  Common mistakes
          →  Related  →  API reference
```

The **Choosing the right API** section is mandatory wherever a subsystem exposes
more than one way to do the same conceptual thing. That is a project rule, not a
stylistic preference — see [Choosing the right API](../choosing/index.md).
