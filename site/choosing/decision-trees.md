# Quick answers

Every choice on this site, condensed into one page, for when you know what you
need and just want the answer. Each section links to the page that explains it.

## Which API shape?

| You want | Shape | Example calls |
|---|---|---|
| a result you can hold, index or pass on | eager | `tokenize_borrowed()`, `process()`, `keys_with_prefix()` |
| to consume it once, in order — maybe not all of it | lazy | `tokens()`, `ngrams()`, `iter_keys_with_prefix()` |
| to do this millions of times with the same shape of output | into-buffer | `tokenize_borrowed_into()`, `pluralize_into()` |
| generic code over the trait | batch | `tokenize_batch()` (sequential today) |

→ [The four API shapes](api-shapes.md)

## Which tokenization call?

| Your situation | Call |
|---|---|
| I look at each token once, or stop early | `tokens()` |
| I want a `Vec` to index or iterate, and the input outlives it | `tokenize_borrowed()` |
| I am in a loop over many documents that all outlive the loop | `buf.clear(); tokenize_borrowed_into(doc, &mut buf)` |
| My tokens must outlive the text they came from | `Tokenizer::tokenize` → `Vec<String>` |
| My function takes "some tokenizer" and only needs slices | `BorrowingTokenizer` — all three implement it |
| I have a slice of documents and want one call | `Tokenizer::tokenize_batch` — a sequential map; `par_tokenize_batch` (feature `parallel`) if the CPU cost justifies threads |

→ [Choosing a tokenization API](tokenization.md)

## Which tokenizer?

| What you are splitting | Tokenizer |
|---|---|
| Words, in any language that uses spaces | `WordTokenizer` |
| Words *and* the punctuation and whitespace between them | `SegmentTokenizer` — concatenation reproduces the input |
| Sentences | `SentenceTokenizer` — `with_abbreviations` if you have a list |
| Text that needs re-assembly, highlighting or offsets | `SegmentTokenizer` |
| Thai, Khmer, Chinese or Japanese word segmentation | nothing here — UAX #29 does not segment languages without spaces |

→ [Tokenizers](../features/tokenizers.md)

## Which distance metric?

| What you are comparing | Metric | Working set |
|---|---|---|
| Same length by construction (codes, hashes, fixed fields) | `hamming()` → `Option<usize>`; `None` when the character counts differ | — |
| Typos, and a swap is honestly two edits | `levenshtein()` | bit-vector / 1 row weighted |
| Typos, adjacent swaps cost 1, never edited again | `osa()` | bit-vector / 3 rows weighted |
| Typos, swaps may be arbitrarily far apart | `damerau_levenshtein()` | Zhao–Sahni rows / full matrix weighted |
| Position of the best approximate occurrence in a longer string | `levenshtein_search()` / `damerau_levenshtein_search()` / `osa_search()` | bit-vector columns (plain, unit cost) / full matrix |
| Names or short records, raw score | `jaro()` | — |
| Names or short records, prefix-boosted (the usual choice) | `jaro_winkler()` | — |
| Shared content, not order or position | `dice_coefficient()` — bigram set overlap; case and whitespace significant | — |
| Any of those edits priced differently | the matching `*_weighted()`, plus a cost set from `LevenshteinCosts` / `OsaCosts` / `DamerauCosts` | scalar dynamic program |

→ [Choosing a distance API](distance.md)

## Which phonetic encoder?

| What you need | Encoder | Key |
|---|---|---|
| English surnames, cheapest possible blocking key | `SoundEx` | a letter and 3 digits, very coarse |
| General English words, one key, better precision | `Metaphone` | letters, unbounded |
| English text with names of many origins, two indexable keys | `DoubleMetaphone` | two keys of up to 4 characters; match on either |
| Slavic / Germanic / Ashkenazi-Jewish surnames | `DaitchMokotoff` | 6 digits; `codes()` returns every branch |
| American surnames, US-census rules | `Nysiis` | letters |
| German-language names and words | `Cologne` | digits |
| Names whose language of origin is itself uncertain | `BeiderMorse` | a candidate list, per language set |
| "Which encoder should I even use for this text?" | `verbora_language::recommend` | a `PhoneticStrategy`, whose `primary` is `None` rather than a guess when nothing fits |

Twelve encoders ship in all — the eight rows above are the common answers.
→ [Phonetics](../features/phonetics.md) · [Language](../features/language.md)

## Which n-gram call?

| If you… | Call |
|---|---|
| have a slice of elements and want its windows — **the default** | `ngrams(seq, n)` |
| stop early, or fold windows into a counter | `ngrams(seq, n)` — it is already lazy |
| want indexable windows | `ngrams(seq, n).collect::<Vec<_>>()` |
| want the ends of the sequence to appear in as many windows as the middle | `Padded::new(seq, n, Some(&start), Some(&end)).ngrams()` |
| need counts | fold `ngrams(seq, n)` into a `HashMap`, keyed on the window itself |
| need the windows to outlive the sequence | copy them out with `.map(<[_]>::to_vec)` |
| have text and want character windows | `char_ngrams(text, n)` |
| have text and want word windows | tokenize first, then `ngrams` over the token slice |

→ [Choosing an n-gram API](ngrams.md)

## Which trie query?

| Your question | Call |
|---|---|
| "Is this exact string stored?" | `contains()` |
| "How many words are stored?" | `len()` — words, not nodes; `O(1)` |
| "How big is the structure?" | `node_count()` — arena nodes, one per scalar; `O(1)` |
| "Which stored words start with my string?" — all of them, indexable | `keys_with_prefix()` → `Vec<String>` |
| The same, but only the first N, or I stop on a condition | `iter_keys_with_prefix().take(N)` |
| The same, but I only read them — no `String` per key | `for_each_key_with_prefix()` |
| "How many words start with this?" | `iter_keys_with_prefix(p).count()` — one descent, no traversal |
| "Does anything start with this?" | `iter_keys_with_prefix().next().is_some()` |
| "Give me every word in the trie" | `keys()` — lazy; same as `iter_keys_with_prefix("")` |
| "Which stored words are prefixes of my string?" — all, shortest first | `prefix_matches()` → `Vec<Cow<str>>` |
| The same, but only the shortest or the first few | `iter_prefix_matches().next()` / `.take(n)` |
| "Where does the longest stored prefix end?" — as text | `longest_prefix()` → `PrefixSplit { word, rest }` |
| The same, but as scalar counts, exact and allocation-free | `longest_prefix_lengths()` → `PrefixSplitLengths` |
| Built once, then queried forever | `freeze()` → `FrozenTrie`; `keys_slice()` borrows instead of allocating |

→ [Trie](../features/trie.md)

## Which normalizer?

| What you are normalizing | Call |
|---|---|
| Text you will store and show a human again | `nfc()` |
| A lookup key that must ignore width, ligation and circling | `nfkc()` |
| A lookup key that must ignore accents (Latin script) | `remove_diacritics()` |
| Both of those at once | `remove_diacritics(&nfkc(text))` |
| Text whose combining marks you will inspect yourself | `nfd()` / `nfkd()` |
| Japanese halfwidth katakana and fullwidth alphanumerics | `nfkc()` |
| Kana into Latin letters | nothing here — that is [Transliterators](../features/transliterators.md) |

→ [Normalizers](../features/normalizers.md)

## Should I optimise this?

Ask in this order:

1. **Has a profiler told me this line is hot?** No → use the high-level API and
   stop reading.
2. **Is the cost the container allocation, or the work inside it?** The work → a
   different API will not help; look at the algorithm, the input size, or how
   many candidates you are comparing.
3. **Do I consume the result once, in order?** Yes → `tokens()`, no container at
   all. No → `tokenize_borrowed_into()`, one container reused.

Still not fast enough, and measured in seconds of CPU? Check whether the
operation already has a `par_*_batch` (fourteen crates ship one, opt-in behind a
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
