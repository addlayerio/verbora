# Quick answers

Every choice on this site, condensed into one page, for when you know what you
need and just want the answer. Each section links to the page that explains it.

## Which API shape?

| You want | Shape | Example calls |
|---|---|---|
| a result you can hold, index or pass on | eager | `tokenize()`, `process()`, `keys_with_prefix()` |
| to consume it once, in order — maybe not all of it | lazy | `tokens()`, `ngrams_iter()`, `iter_keys_with_prefix()` |
| to do this millions of times with the same shape of output | into-buffer | `tokenize_into()`, `pluralize_into()` |
| generic code over the trait | batch | `tokenize_batch()` (sequential today) |

→ [The four API shapes](api-shapes.md)

## Which tokenization call?

| Your situation | Call |
|---|---|
| I name a concrete tokenizer type, and look at each token once | `tokens()` |
| I name a concrete type and want a `Vec` to keep, index or return | `tokenize()` |
| I name a concrete type and am in a loop over many documents | `buf.clear(); tokenize_into(doc, &mut buf)` |
| My function takes "some tokenizer" and needs owned `String`s | `verbora_core::Tokenizer` |
| My function takes "some tokenizer" and only needs slices | `verbora_core::BorrowingTokenizer` (13 of 24 types) |
| My tokenizer is `RegexpTokenizer` / `WordTokenizer` / `OrthographyTokenizer` / `WordPunctTokenizer` | the inherent methods — they return `Option`, `None` meaning "no match at all" |
| I have a slice of documents and want one call | `Tokenizer::tokenize_batch` — a sequential map; `rayon` at your call site if the CPU cost justifies threads |

→ [Choosing a tokenization API](tokenization.md)

## Which tokenizer?

| What you are splitting | Tokenizer |
|---|---|
| English words, fast and simple | `AggressiveTokenizer` |
| Another language | `AggressiveTokenizer{De,Es,Fr,It,Nl,No,Pl,Pt,Ru,Sv,Uk,Vi,Id,Fa,Hi}` |
| Finnish, or another orthography-driven language | `OrthographyTokenizer::new("fi")` |
| Sentences | `SentenceTokenizer` — `with_abbreviations` if you have a list |
| Words *and* punctuation as separate tokens | `WordPunctTokenizer` |
| Penn Treebank conventions (contractions split) | `TreebankWordTokenizer` |
| Japanese | `TokenizerJa` |
| Your own pattern | `RegexpTokenizer::new(Pattern::new(re))` |

→ [Tokenizers](../features/tokenizers.md)

## Which distance metric?

| What you are comparing | Metric | Working set |
|---|---|---|
| Same length by construction (codes, hashes, fixed fields), plain number | `hamming()` — `-1` means incomparable | — |
| The same, with Rust's vocabulary for "no answer" | `hamming_checked()` → `Option<u64>` | — |
| Typos, and a swap is honestly two edits | `levenshtein()` | bit-vector / 1 row weighted |
| Typos, adjacent swaps cost 1, never edited again | `damerau_levenshtein(.., restricted: true)` | bit-vector / 3 rows weighted |
| Typos, swaps may be arbitrarily far apart | `damerau_levenshtein(.., restricted: false)` | 2 rows + per-symbol snapshots |
| Position of the best approximate occurrence in a longer string | `levenshtein_search()` / `damerau_levenshtein_search()` | full matrix |
| Names or short records, raw score | `jaro()` | — |
| Names or short records, prefix-boosted (the usual choice) | `jaro_winkler()` | — |
| Shared content, not order or position | `dice_coefficient()` — bigram set overlap; `NaN` on two empties | — |

→ [Choosing a distance API](distance.md)

## Which phonetic encoder?

| What you need | Encoder | Key |
|---|---|---|
| English surnames, cheapest possible blocking key | `SoundEx` | 4 characters, very coarse |
| General English words, one key, better precision | `Metaphone` | up to 32 characters |
| English text with names of many origins, two indexable keys | `DoubleMetaphone` | two keys; match on either |
| Slavic / Germanic / Ashkenazi-Jewish surnames | `SoundExDM` | 6 digits, multi-letter clusters |

→ [Phonetics](../features/phonetics.md)

## Which n-gram call?

| If you… | Call |
|---|---|
| stop early, or fold windows into a counter | `ngrams_iter()` |
| consume everything and want indexable windows — **the default** | `ngrams()` (or `bigrams()` / `trigrams()`) |
| need the tuples to outlive the token slice | `ngrams_owned()` |
| need the `{ngrams, frequencies, Nr, numberOfNgrams}` shape | `ngrams_with_stats()` |
| need counts only, with your own key format | `ngrams_iter()` folded into a `HashMap` |
| need many lookups by key | `ngrams_with_stats()`, then index `frequencies` into a `HashMap` once |
| have a string and control the tokenizer | `ngrams_str_with()` |
| have Chinese BMP text | `zh::ngrams_zh()` |
| have Chinese text that may contain astral characters | `zh::code_units()` + `zh::ngrams_zh_utf16()` |

→ [Choosing an n-gram API](ngrams.md)

## Which trie query?

| Your question | Call |
|---|---|
| "Is this exact string stored?" | `contains()` |
| "How big is the structure?" | `get_size()` — nodes, not words; `O(1)` |
| "Which stored words start with my string?" — all of them, indexable | `keys_with_prefix()` → `Vec<String>` |
| The same, but only the first N, or I stop on a condition | `iter_keys_with_prefix().take(N)` |
| "Does anything start with this?" | `iter_keys_with_prefix().next().is_some()` |
| "Give me every word in the trie" | `keys()` — lazy; same as `iter_keys_with_prefix("")` |
| "Which stored words are prefixes of my string?" — all, shortest first | `find_matches_on_path()` → `Vec<Cow<str>>` |
| The same, but only the shortest or the first few | `iter_matches_on_path().next()` / `.take(n)` |
| Only the longest stored prefix | `find_prefix().0` — one walk, no iterator |
| "Where does the longest stored prefix end?" — as text | `find_prefix()` → `(Option<Cow>, Cow)` |
| The same, but as offsets, exact and allocation-free | `find_prefix_lengths()` → `(Option<usize>, usize)` |

→ [Trie](../features/trie.md)

## Which normalizer?

| What you are normalizing | Call |
|---|---|
| English contractions, over a token slice (the usual case) | `normalize(&tokens)` |
| English contractions, exactly one token | `normalize_token(&token)` |
| Latin diacritics, any language, fold everything the table knows | `remove_diacritics()` |
| Norwegian — keep ä ö ü å ø æ | `normalize_no()` |
| Swedish — keep those, plus â ç ê î ñ ó ô û š | `normalize_sv()` |
| Japanese, the whole normalization | `normalize_ja()` |
| Japanese, one width/kana conversion | `ja::converters::{alphabet_fh, katakana_hf, …}` |
| Japanese, hiragana and katakana onto one syllabary | `ja::converters::{hiragana_to_katakana, katakana_to_hiragana}` |

→ [Normalizers](../features/normalizers.md)

## Should I optimise this?

Ask in this order:

1. **Has a profiler told me this line is hot?** No → use the high-level API and
   stop reading.
2. **Is the cost the container allocation, or the work inside it?** The work → a
   different API will not help; look at the algorithm, the input size, or how
   many candidates you are comparing.
3. **Do I consume the result once, in order?** Yes → `tokens()`, no container at
   all. No → `tokenize_into()`, one container reused.

Still not fast enough, and measured in seconds of CPU? Check whether the
operation already has a `par_*_batch` (thirteen crates ship one, opt-in behind a
`parallel` feature); if not, chunk the input and parallelise at your own call
site.

→ [Ergonomics vs throughput](../performance/ergonomics-vs-throughput.md) ·
[Parallelism](../performance/parallelism.md)

## Which workload am I in?

| How work arrives | Workload | What you optimise |
|---|---|---|
| One input, answer now | Interactive | latency, ergonomics |
| More input than memory, or output needed before input ends | Streaming | bounded memory, laziness, early output |
| Many documents, offline | Batch | memory reuse, shared setup |
| Many documents, and seconds of CPU to spend | Parallel corpus | chunking, per-worker state |

→ [Recipes by workload](../recipes/index.md)
