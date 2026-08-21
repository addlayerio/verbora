# Features overview

Every subsystem below is implemented, tested, and documented. Start from what
you want to *do*, or from the crate map further down.

## Start from the problem

| I want to… | Use |
|---|---|
| Split text into words | [Tokenizers](tokenizers.md) — `WordTokenizer` |
| Split text into sentences | [`SentenceTokenizer`](tokenizers.md) |
| Keep the whitespace and punctuation too, so the pieces re-assemble | [`SegmentTokenizer`](tokenizers.md) |
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
| Put text in one canonical spelling before storing or hashing it | [`nfc`](normalizers.md) |
| Fold halfwidth katakana and fullwidth Latin onto one form | [`nfkc`](normalizers.md) |
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
| [Tokenizers](tokenizers.md) | `verbora-tokenizers` | `WordTokenizer`, `SegmentTokenizer`, `SentenceTokenizer` — UAX #29 boundaries, borrowed tokens |
| [String distance](distance.md) | `verbora-distance` | 7 metrics; the three edit distances also in weighted and substring-search forms, plus `PreparedPattern` |
| [Phonetics](phonetics.md) | `verbora-phonetics` | 12 encoders: SoundEx, Metaphone, Double Metaphone, Daitch–Mokotoff, Beider–Morse, Cologne, Nysiis, Caverphone 1/2, Phonex, Refined Soundex, Match Rating |
| [Phonetic neighbors](phonetic-index.md) | `verbora-phonetics` | `PhoneticIndex` — dictionary-wide candidate generation over `SoundEx`, `Metaphone` or `DoubleMetaphone` |
| [Beider-Morse](beider-morse.md) | `verbora-phonetics` | Cross-language surname matching over up to 18 languages at once, with auto-detection |
| [Language](language.md) | `verbora-language` | Script detection, optional statistical language detection, and `recommend()` — language → phonetic encoder |
| [N-grams](ngrams.md) | `verbora-ngrams` | `ngrams`, `Padded` boundary symbols, `char_ngrams` |
| [Normalizers](normalizers.md) | `verbora-normalizers` | the four Unicode normalization forms plus `remove_diacritics` |
| [Inflectors](inflectors.md) | `verbora-inflectors` | 6 inflectors, runtime rules |
| [Trie](trie.md) | `verbora-trie` | prefix tree, prefix and path queries |
| [Transliterators](transliterators.md) | `verbora-transliterators` | Japanese kana → romaji, one left-to-right pass over a generated mora index |
| [WordNet](wordnet.md) | `verbora-wordnet` | lexical database, synsets, relation traversal, 4 storage strategies |
| [TF-IDF](tfidf.md) | `verbora-tfidf` | term interning, incremental idf cache, `list_terms`/`tfidf`/`tfidfs` |
| [Sentiment](sentiment.md) | `verbora-sentiment` | 14 lexicons across 10 languages, sticky negation |
| [Classifiers](classifiers.md) | `verbora-classifiers` | Bayes, logistic regression, MaxEnt + GIS |
| [Stemmers](stemmers.md) | `verbora-stemmers` | 16 stemmers: Porter/Snowball across 12 languages, plus Carry (French), Lancaster, Japanese, Indonesian |
| [Spellcheck](spellcheck.md) | `verbora-spellcheck` | frequency-ranked correction, BK-tree and deletion indexes |
| [POS tagger](tagger.md) | `verbora-tagger` | Brill tagging, training and evaluation; English and Dutch data |
| [Sentence analyzers](analyzers.md) | `verbora-analyzers` | phrase annotation, subject/predicate splitting, sentence type |
| [Utilities](util.md) | `verbora-util` | stop words, abbreviations, weighted graphs, topological ordering and path trees |
| [Core vocabulary](core.md) | `verbora-core` | 5 traits, `StopWordLanguage`, `StopWords`, the process-global stop-word list |

## Language support

Tokenization and normalization are not per-language axes any more: both follow
the Unicode standard and therefore cover the whole character repertoire at once.
The columns that *do* vary by language are the ones backed by per-language data.

| Language | Inflector | Phonetics |
|---|:--:|:--:|
| English | ✅ nouns, verbs, ordinals | ✅ all four core encoders |
| French | ✅ nouns, ordinals | |
| Japanese | ✅ nouns | |

**Tokenization** is [UAX #29](https://www.unicode.org/reports/tr29/) word and
sentence boundaries, so every language that separates words with spaces is
covered by the same three tokenizers — with one stated limitation: the standard's
default rules do not segment Thai, Lao, Khmer, Myanmar, Chinese or Japanese, and
Verbora ships no dictionary segmenter for them.

**Normalization** is the four Unicode normalization forms plus a
combining-mark fold defined over `Canonical_Combining_Class`, so
`remove_diacritics` handles Latin, Greek, Cyrillic, Hebrew and Arabic script
without a per-language rule set — read [Normalizers](normalizers.md) before
applying it to Thai or Devanagari.

Stemmers, sentiment lexicons, part-of-speech data and Beider-Morse each have
their own language axis; see [Stemmers](stemmers.md),
[Sentiment](sentiment.md), [POS tagger](tagger.md) and
[Beider-Morse](beider-morse.md).

## Where else to look

- [Choosing the right API](../choosing/index.md) — cross-subsystem decisions.
- [Performance](../performance/index.md) — allocation, zero-copy, parallelism.
- [Benchmarks](../benchmarks/index.md) — measured numbers and how to reproduce.
- [Recipes](../recipes/index.md) — end-to-end pipelines.
- [Status and scope](roadmap.md) — what "available" means, and the boundaries.
- Exact signatures live in [rustdoc](../reference/api.md); these pages cover
  selection, composition, costs and common mistakes.
