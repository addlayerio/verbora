# Every decision tree

All the trees on the site, on one page, for when you know what you need and just
want the answer. Each links to the reasoning behind it.

## Which API shape?

```text
Which shape?
│
├── I want a result I can hold, index, or pass on
│      └── eager:        tokenize(), process(), keys_with_prefix()
│
├── I want to consume it once, in order — maybe not all of it
│      └── lazy:         tokens(), ngrams_iter(), iter_keys_with_prefix()
│
├── I am doing this millions of times with the same shape of output
│      └── into-buffer:  tokenize_into(), pluralize_into()
│
└── I am writing generic code over the trait
       └── batch:        tokenize_batch()   (sequential today)
```

→ [The four API shapes](api-shapes.md)

## Tokenization

```text
I need to tokenize text
│
├── Do I name a concrete tokenizer type in my code?
│   │
│   ├── YES ── use the `Tokenize` trait
│   │      │
│   │      ├── I look at each token once and never need them together
│   │      │      └── tokens()
│   │      │
│   │      ├── I want a Vec to keep, index, or return
│   │      │      └── tokenize()
│   │      │
│   │      └── I am in a loop over many documents
│   │             └── buf.clear(); tokenize_into(doc, &mut buf)
│   │
│   └── NO — my function takes "some tokenizer"
│          │
│          ├── I need owned Strings, or must accept any tokenizer
│          │      └── verbora_core::Tokenizer
│          │
│          └── I only need slices, and can require the zero-copy ones
│                 └── verbora_core::BorrowingTokenizer   (13 of 24 types)
│
├── My tokenizer is RegexpTokenizer / WordTokenizer /
│   OrthographyTokenizer / WordPunctTokenizer
│      └── inherent methods, all returning Option — `None` means no match at all
│
└── I have a slice of documents and want one call
       └── verbora_core::Tokenizer::tokenize_batch
           (a sequential map; it saves typing, not allocation — rayon at
           your call site if the CPU cost justifies threads)
```

→ [Choosing a tokenization API](tokenization.md)

## Which tokenizer?

```text
What am I splitting?
│
├── English words, fast and simple
│      └── AggressiveTokenizer
│
├── Another language
│      └── AggressiveTokenizer{De,Es,Fr,It,Nl,No,Pl,Pt,Ru,Sv,Uk,Vi,Id,Fa,Hi}
│
├── Finnish (or another orthography-driven language)
│      └── OrthographyTokenizer::new("fi")
│
├── Sentences
│      └── SentenceTokenizer  (with_abbreviations if you have a list)
│
├── Words *and* punctuation as separate tokens
│      └── WordPunctTokenizer
│
├── Penn Treebank conventions (contractions split)
│      └── TreebankWordTokenizer
│
├── Japanese
│      └── TokenizerJa
│
└── My own pattern
       └── RegexpTokenizer::new(Pattern::new(re))
```

→ [Tokenizers](../features/tokenizers.md)

## Which distance metric?

```text
I need to compare two strings
│
├── They are the same length by construction (codes, hashes, fixed fields)
│      ├── I want a plain number, -1 meaning "incomparable"
│      │      └── hamming()
│      └── I want Rust's own vocabulary for "no answer"
│             └── hamming_checked()  ->  Option<u64>
│
├── I care about typos: insert / delete / substitute
│      ├── ...and adjacent swaps ("teh" -> "the") should cost 1, not 2
│      │      ├── swapped characters are never edited again
│      │      │      └── damerau_levenshtein(.., restricted: true)   [3 rows]
│      │      └── swaps may be arbitrarily far apart
│      │             └── damerau_levenshtein(.., restricted: false)  [matrix]
│      └── ...and a swap is honestly two edits
│             └── levenshtein()                                      [2 rows]
│
├── I need the position of the best approximate occurrence in a longer string
│      └── levenshtein_search() / damerau_levenshtein_search()       [matrix]
│
├── I am matching names or short records, and a shared prefix is meaningful
│      ├── I want the raw Jaro score
│      │      └── jaro()
│      └── I want the prefix-boosted score (the usual choice)
│             └── jaro_winkler()
│
└── I care about shared content, not order or position
       └── dice_coefficient()      (bigram set overlap; NaN on two empties)
```

→ [Choosing a distance metric](distance.md)

## Which phonetic encoder?

```text
I need a phonetic key
│
├── English surnames, and I want the cheapest possible blocking key
│      └── SoundEx           4 characters, very coarse
│
├── General English words, one key, better precision
│      └── Metaphone         up to 32 characters
│
├── English text with names of many origins, and I can index two keys
│      └── DoubleMetaphone   two keys; match on either
│
└── Slavic / Germanic / Ashkenazi-Jewish surnames
       └── SoundExDM         6 digits, multi-letter clusters
```

→ [Phonetics](../features/phonetics.md)

## N-grams

```text
I have a slice of tokens and I want its n-grams
│
├── Will I look at every n-gram?
│   │
│   ├── No — I stop early (take / find / any / position),
│   │        or I only fold them into a counter
│   │      └── ngrams_iter()          ← nothing is built that you do not read
│   │
│   └── Yes
│         │
│         ├── Do the tuples have to outlive the token slice?
│         │    (returned from a function, stored in a struct,
│         │     sent to another thread, put in a cache)
│         │   │
│         │   ├── Yes → ngrams_owned()
│         │   └── No  → ngrams()      ← the recommended default
│         │
│         └── Do I also need frequencies / a count-of-counts?
│               └── ngrams_with_stats()
│
└── (n == 2 or 3? bigrams() / trigrams() are the same call with n fixed.
    multrigrams() is an exact alias of ngrams().)
```

```text
I want frequency information
│
├── I need the {ngrams, frequencies, Nr, numberOfNgrams} shape
│      └── ngrams_with_stats() / bigrams_with_stats() / trigrams_with_stats()
│
├── I need counts only, my own key format is fine
│      └── ngrams_iter() folded into a HashMap
│
└── I need many lookups by key
       └── ngrams_with_stats(), then index `frequencies` into a HashMap once
          (NGramStats::frequency is a linear scan by design)
```

→ [Choosing an n-gram API](ngrams.md)

## Trie queries

```text
I have a Trie and a string
│
├── "Is this exact string stored?"
│      └── contains()
│
├── "How big is the structure?"
│      └── get_size()          (nodes, not words — O(1))
│
├── "Which stored words START WITH my string?"
│   │
│   ├── I need all of them, and I need to keep/index them
│   │      └── keys_with_prefix()        → Vec<String>
│   │
│   ├── I need the first N, or I stop on a condition
│   │      └── iter_keys_with_prefix().take(N)
│   │
│   ├── I only need "does anything start with this?"
│   │      └── iter_keys_with_prefix().next().is_some()
│   │
│   └── I want every word in the trie
│          └── keys()          (lazy; same as iter_keys_with_prefix(""))
│
├── "Which stored words ARE PREFIXES OF my string?"
│   │
│   ├── All of them, shortest first
│   │      └── find_matches_on_path()    → Vec<Cow<str>>
│   │
│   ├── Only the shortest / only the first few
│   │      └── iter_matches_on_path().next() / .take(n)
│   │
│   └── Only the LONGEST
│          └── find_prefix().0           (one walk, no iterator)
│
└── "Where does the longest stored prefix end?"
    │
    ├── I need the text of the two halves
    │      └── find_prefix()             → (Option<Cow>, Cow)
    │
    └── I need offsets, exactness, or zero allocation
           └── find_prefix_lengths()     → (Option<usize>, usize)
```

→ [Trie](../features/trie.md)

## Normalizers

```text
I need to normalize text
│
├── English contractions
│      ├── I have a token slice (the usual case)
│      │      └── normalize(&tokens)
│      └── I have exactly one token
│             └── normalize_token(&token)
│
├── Latin diacritics
│      ├── Any language, fold everything the table knows
│      │      └── remove_diacritics()
│      ├── Norwegian — keep ä ö ü å ø æ
│      │      └── normalize_no()
│      └── Swedish — keep those, plus â ç ê î ñ ó ô û š
│             └── normalize_sv()
│
└── Japanese
       ├── I want the whole normalization
       │      └── normalize_ja()
       ├── I want one width/kana conversion
       │      └── ja::converters::{alphabet_fh, katakana_hf, …}
       └── I want hiragana and katakana on one syllabary
              └── ja::converters::{hiragana_to_katakana, katakana_to_hiragana}
```

→ [Normalizers](../features/normalizers.md)

## Should I optimise this?

```text
1. Has a profiler told me this line is hot?
        no  → use the high-level API and stop reading
        yes → continue

2. Is the cost the container allocation, or the work inside it?
        the work → a different API will not help; look at the algorithm,
                   the input size, or how many candidates you are comparing
        the container → continue

3. Do I consume the result once, in order?
        yes → tokens()          (no container at all)
        no  → tokenize_into()   (one container, reused)
```

Still not fast enough, and measured in seconds of CPU? Check whether the
operation already has a `par_*_batch` (thirteen crates ship one, opt-in
behind a `parallel` feature); if not, chunk the input and parallelise at your
own call site.

→ [Ergonomics vs throughput](../performance/ergonomics-vs-throughput.md) ·
[Parallelism](../performance/parallelism.md)

## Which workload am I in?

```text
How does work arrive?
│
├── One input, answer now
│      └── Interactive        → latency, ergonomics
│
├── More input than memory, or output needed before input ends
│      └── Streaming          → bounded memory, laziness, early output
│
├── Many documents, offline
│      └── Batch              → memory reuse, shared setup
│
└── Many documents, and seconds of CPU to spend
       └── Parallel corpus    → chunking, per-worker state
```

→ [Recipes by workload](../recipes/index.md)
