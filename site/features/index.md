# Features overview

Every subsystem below is implemented, tested, and documented. Start from what
you want to *do*, or from the crate map further down.

## Start from the problem

| I want to… | Use |
|---|---|
| Split text into words | [Tokenizers](tokenizers.md) — start with `AggressiveTokenizer` |
| Split text into sentences | [`SentenceTokenizer`](tokenizers.md) |
| Tokenize a language other than English | [Tokenizers](tokenizers.md) — 16 language variants |
| Measure how similar two strings are | [String distance](distance.md) |
| Correct a typo | [`levenshtein`](distance.md), usually after narrowing candidates |
| Correct spelling against a corpus | [Spellcheck](spellcheck.md) |
| Match names that sound alike | [Phonetics](phonetics.md) |
| Search a whole dictionary for sound-alikes | [Phonetic neighbors](phonetic-index.md) |
| Match names across language boundaries (genealogy) | [Beider-Morse](beider-morse.md) |
| Match names with a shared prefix | [`jaro_winkler`](distance.md) |
| Work out which encoder or stemmer a text even needs | [Language](language.md) |
| Build an autocomplete index | [Trie](trie.md) |
| Find repeated phrases | [N-grams](ngrams.md) |
| Strip accents before comparing | [`remove_diacritics`](normalizers.md) |
| Expand English contractions | [`normalize`](normalizers.md) |
| Normalise Japanese width and kana | [`normalize_ja`](normalizers.md) |
| Romanise Japanese kana | [Transliterators](transliterators.md) |
| Pluralise a noun, or write "23rd" | [Inflectors](inflectors.md) |
| Reduce words to a stem | [Stemmers](stemmers.md) |
| Rank documents by relevance | [TF-IDF](tfidf.md) |
| Classify documents into categories | [Classifiers](classifiers.md) |
| Score sentiment over a token list | [Sentiment](sentiment.md) |
| Look up synonyms and word relations | [WordNet](wordnet.md) |
| Tag parts of speech | [POS tagger](tagger.md) |
| Split a tagged sentence into subject and predicate | [Sentence analyzers](analyzers.md) |
| Write generic code over any tokenizer | [Core vocabulary](core.md) |

## The crates

| Subsystem | Crate | Public surface |
|---|---|---|
| [Tokenizers](tokenizers.md) | `verbora-tokenizers` | 25 tokenizers, `Tokenize`, `Utf16Token` |
| [String distance](distance.md) | `verbora-distance` | 8 metrics, 5 `StringMetric` impls |
| [Phonetics](phonetics.md) | `verbora-phonetics` | 11 encoders: SoundEx, Metaphone, Double Metaphone, Daitch–Mokotoff ×2, Cologne, Nysiis, Caverphone 1/2, Phonex, Refined Soundex, Match Rating |
| [Phonetic neighbors](phonetic-index.md) | `verbora-phonetics` | `PhoneticIndex` — dictionary-wide candidate generation over any of the four core encoders |
| [Beider-Morse](beider-morse.md) | `verbora-phonetics` | Cross-language surname matching over up to 18 languages at once, with auto-detection |
| [Language](language.md) | `verbora-language` | Script detection, optional statistical language detection, and `recommend()` — language → phonetic encoder |
| [N-grams](ngrams.md) | `verbora-ngrams` | window engine, stats, Chinese n-grams |
| [Normalizers](normalizers.md) | `verbora-normalizers` | 6 normalizers, 17 Japanese converters |
| [Inflectors](inflectors.md) | `verbora-inflectors` | 6 inflectors, runtime rules |
| [Trie](trie.md) | `verbora-trie` | prefix tree, prefix and path queries |
| [Transliterators](transliterators.md) | `verbora-transliterators` | Japanese kana → romaji, five-phase pipeline |
| [WordNet](wordnet.md) | `verbora-wordnet` | lexical database, synsets, relation traversal, 4 storage strategies |
| [TF-IDF](tfidf.md) | `verbora-tfidf` | term interning, incremental idf cache, `listTerms`/`tfidf`/`tfidfs` |
| [Sentiment](sentiment.md) | `verbora-sentiment` | 14 lexicons across 10 languages, sticky negation |
| [Classifiers](classifiers.md) | `verbora-classifiers` | Bayes, logistic regression, MaxEnt + GIS |
| [Stemmers](stemmers.md) | `verbora-stemmers` | 16 stemmers: Porter/Snowball across 12 languages, Lancaster, Japanese, Indonesian |
| [Spellcheck](spellcheck.md) | `verbora-spellcheck` | frequency-ranked correction, BK-tree and deletion indexes |
| [POS tagger](tagger.md) | `verbora-tagger` | Brill tagging, training and evaluation; English and Dutch data |
| [Sentence analyzers](analyzers.md) | `verbora-analyzers` | phrase annotation, subject/predicate splitting, sentence type |
| [Utilities](util.md) | `verbora-util` | stop words, abbreviations, weighted graphs and storage backends |
| [Core vocabulary](core.md) | `verbora-core` | 6 traits, `Token`, `StopWords`, whitespace helpers |

## Language support

| Language | Tokenizer | Normalizer | Inflector | Phonetics |
|---|:--:|:--:|:--:|:--:|
| English | ✅ | ✅ | ✅ nouns, verbs, ordinals | ✅ all four core encoders |
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
than the Normalizer column suggests — it is a table over 820 non-ASCII
characters, not a per-language rule set. Stemmers cover 12 languages on their
own axis; see [Stemmers](stemmers.md).

## Where else to look

- [Choosing the right API](../choosing/index.md) — cross-subsystem decisions.
- [Performance](../performance/index.md) — allocation, zero-copy, parallelism.
- [Benchmarks](../benchmarks/index.md) — measured numbers and how to reproduce.
- [Recipes](../recipes/index.md) — end-to-end pipelines.
- [Status and scope](roadmap.md) — what "available" means, and the boundaries.
- Exact signatures live in [rustdoc](../reference/api.md); these pages cover
  selection, composition, costs and common mistakes.
