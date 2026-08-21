# Allocation behaviour

A per-API reference for one question: **does this allocate, and how much?**

<div class="callout callout-warn">
<strong>Read from the source, not from a profiler.</strong> Every entry below
describes what the code does structurally — stable and checkable — rather than a
measured count. No per-API allocation-count table exists in the repository. The
one instrument that does is <code>verbora-spellcheck</code>'s
<code>counting_alloc</code>, a <code>#[cfg(test)]</code> global allocator its own
memory-bound tests measure peak bytes with; it is scoped to that crate's test
build, is not compiled into any published library, and no figure on this page
comes from it.
</div>

## Tokenizers

| API | Allocates | Notes |
|---|---|---|
| `tokens(text)` | **nothing** | The iterator is a small stack struct. Tokens are slices. |
| `tokenize_borrowed(text)` | one `Vec<&str>`, plus growth | No per-token allocation |
| `tokenize_borrowed_into(text, &mut buf)` | nothing once `buf` is warm | Appends; you call `clear()` |
| `Tokenizer::tokenize` | one `Vec<String>` **plus one `String` per token** | The owning API: every token is a fully owned `String`, not a borrow |
| `Tokenizer::tokenize_into` | one `String` per token | Appends; the `Vec` is yours to reuse |
| `Tokenizer::tokenize_batch` | one `Vec<String>` per input, plus a `String` per token | Sequential `map`; no shared buffer |

<div class="callout callout-note">
<strong>Two token shapes, two costs.</strong>
<code>BorrowingTokenizer::tokenize_borrowed</code> gives you
<code>Vec&lt;&amp;str&gt;</code> — borrowed, one allocation total.
<code>Tokenizer::tokenize</code> gives you <code>Vec&lt;String&gt;</code> —
owned, one allocation per token, because that trait's contract is to return fully
owned strings with no borrowed lifetime back to the input. Take the owned one
only when the tokens must outlive the text.
</div>

Construction allocates nothing: `WordTokenizer` and `SegmentTokenizer` are
zero-sized, and `SentenceTokenizer::new()` holds an empty `Vec`. Only
`SentenceTokenizer::with_abbreviations` allocates, once, for the abbreviation
list.

## String distance

| API | Allocates per call | Notes |
|---|---|---|
| `levenshtein` (unit cost) | **nothing** for ASCII operands when the shorter one is ≤ 64 scalars | Bit-vector state is registers and the Peq match table is a 2 KB stack array. A longer pattern packs its Peq rows into one `Vec<u64>`; a non-ASCII pair hashes the table instead |
| `levenshtein_weighted` | one `Vec<f64>` of length `m + 1` | Rolling-row working set. The bit-parallel kernels have no notion of a priced operation, so the weighted form is a different algorithm, not a slower spelling of the same one |
| `osa` (unit cost) | bit-vector state and Peq table, on the same terms as `levenshtein` | Hyyrö's transposition extension of the same bit-parallel family, off the same table |
| `osa_weighted` | three `Vec<f64>` | A transposition reaches row − 2, so one rolling row is not enough |
| `damerau_levenshtein` (unit cost) | **nothing** for byte operands ≤ 8 (a fixed stack matrix) or when three rows of ≤ 34 cells fit the stack buffer; otherwise one `Vec<i64>` holding all three rolling rows, plus a 256-entry (ASCII) or hashed (scalar) last-occurrence table | Zhao–Sahni's linear-space algorithm keeps two saved cells instead of the matrix the textbook recurrence would need |
| `damerau_levenshtein_weighted` | full cost **and** parent matrices | A weighted transposition reaches an arbitrary earlier row, so the linear-space reduction does not apply |
| `levenshtein_search` (unit cost) | two `Vec<u64>` holding `⌈n/64⌉` words per target scalar | **No cost matrix and no parent matrix.** The forward pass stores Myers/Hyyrö vertical deltas per column and the backtrack recomputes every cell cost and every parent choice from them — a couple of machine words per column in place of a full column of cells |
| `damerau_levenshtein_search`, `osa_search`, every `*_search_weighted` | full cost matrix **and** a parent matrix | A transposition's parent depends on state the cell costs cannot recover, and a weighted cell has no delta-bit representation at all, so these backtrack over stored parents |
| `jaro`, `jaro_winkler` | **nothing** for ASCII operands up to 64 scalars | Match flags are a stack `[bool; 128]` array on the short-input scalar loop and stack bitsets on the bit-parallel kernels. Past 64 scalars the packed match table becomes one `Vec<u64>`; a non-ASCII pair adds one `Vec<char>` per operand and hashes the table |
| `dice_coefficient` | two `FxHashSet`s of `(char, char)` keys | A bigram is a pair of scalars — 8 `Copy` bytes — so no `String` is allocated per bigram, and each set is sized up front so the fill never rehashes |
| `hamming` | **nothing**, on any input | The ASCII tiers are a scalar zip, a SWAR word kernel or a fused 16-lane pass over the borrowed bytes; everything else is one `chars()` walk that decides comparability and counts differences together, materialising no intermediate sequence |
| `PreparedPattern::levenshtein`, `PreparedPattern::osa` | **nothing** for an ASCII pattern of ≤ 64 units against an ASCII target — the Peq table was built at construction and the bit-vector state is registers | Non-ASCII targets over 64 bytes add one `Vec<char>`; shorter ones use a stack buffer. A query that falls back to the per-call function allocates exactly what that function's row above says |

Search never allocates a result string. `SearchResult::substring()` borrows from
the target and `range()` is derived from it, so the matched text and its byte
position cannot disagree, and owning the text is opt-in
(`r.substring().to_owned()`). The trade runs the other way for a
filter-and-keep loop: a retained `SearchResult` pins its whole target alive, so
copy out `(range, distance)` — or own the substring — at the filter point.

Long non-ASCII input may add **one `Vec<char>` per operand**, from the promotion
described in [Zero-copy](zero-copy.md#_3-exact-fast-paths). Plain unit-cost
Levenshtein and unrestricted Damerau keep short Unicode operands in fixed stack
buffers; ASCII input is compared as `&[u8]` borrowed from the inputs, which is
exact rather than approximate — one ASCII byte *is* one Unicode scalar.

No metric folds case, trims or normalises its operands, so none of them
allocates a rewritten copy of your input and none consults a Unicode character
database. Caseless or accent-insensitive matching is a transformation you apply
once at ingestion, where it costs one allocation per stored string rather than
one per comparison.

### Prepared state, not a scratch buffer

Two things worth keeping apart, because only one of them exists here:

- **A scratch buffer** is mutable working memory *you* lend an algorithm for
  the duration of one call, so the allocator is not asked again on the next
  one. In the Levenshtein family that would be the dynamic-programming working
  set — Myers' `Pv`/`Mv` words, the weighted paths' rolling rows. It depends on
  **both** operands, no API in this crate takes one, and every call still
  builds its own. There is no `levenshtein_with_scratch`.
- **Prepared pattern state** is immutable memory derived from **one** operand
  and valid for every comparison against it. `PreparedPattern::new(pattern)`
  builds the bit-parallel match table (`Peq`) that `levenshtein` and `osa`
  would otherwise rebuild on every call — about 2 KB inline for an ASCII
  pattern of up to 64 units, heap only past that or for a non-ASCII pattern.
  It is never written during a query, so one instance serves any number of
  threads through a shared `&`.

`PreparedPattern` therefore removes a per-call *build*, not a per-call
allocation: the table it hoists never was on the heap for short ASCII
patterns. What a candidate loop stops paying is the zeroing and refilling of
it, once per candidate.

## Phonetics

Every encoder returns an owned `String` — phonetic keys are computed, not
sliced, so there is nothing to borrow.

| API | Allocates | Notes |
|---|---|---|
| `SoundEx::process`, `Metaphone::process`, … | at least one `String` (the key) | Plus small per-encoder intermediates |
| `SoundEx::process_into`, `Metaphone::process_into` | nothing, once the buffer is warm | Offered on the two encoders whose keys are most often accumulated in bulk. **Appends**, so `clear()` yourself |
| `compare(a, b)` | two keys | It does **not** short-circuit: the body is key equality. `DoubleMetaphone::compare` is the one exception — it matches when either of the two keys agrees |
| `DoubleMetaphone::process` | a `DoubleMetaphoneCode` holding one or two keys | `primary()` and the alternate are read off it; no second pass |
| `phoneticize_tokens`, `tokenize_and_phoneticize` | one `Vec` of whatever your closure returns | The first takes `IntoIterator`, so it composes with a lazy tokenizer without an intermediate `Vec` |
| `PhoneticIndex::neighbors` | the query's key only | The index itself is built once; a query walks a precomputed bucket |

Each encoder's public surface is `process`, `compare`, and — where the shape
earns it — `process_into`. The transform stages are internal, so there is no
per-stage `String` to pay for and no way to run half an encoder by accident.

## Normalizers

| API | Allocates | Notes |
|---|---|---|
| `remove_diacritics` | **nothing** for ASCII, and nothing for text already in NFC with no combining mark | `Cow::Borrowed`; one `String` otherwise |
| `nfc`, `nfd`, `nfkc`, `nfkd` | **nothing** when the input is already in that form | `Cow::Borrowed`; one `String` otherwise |
| `par_remove_diacritics_batch` (feature `parallel`) | one outer `Vec`, plus the per-input cost | Order-preserving fan-out over the same function |

This is the crate where the `Cow` discipline pays most: it is normal for a whole
corpus to pass through `remove_diacritics` with zero allocations. The borrow is a
guarantee — `Cow::Borrowed` if and only if the result is byte-identical to the
input — not a fast path that might stop firing.

## Inflectors

| API | Allocates | Notes |
|---|---|---|
| `pluralize`, `singularize` | the result `String`, **plus** a `String` inside the matching rule | Two per call on the English path |
| `pluralize_into`, `singularize_into` | the rule's `String` only | Saves the result allocation; **appends**, so `clear()` yourself |
| `OrdinalInflector::nth` | one `String` | Takes an `i64`; there is no floating-point ordinal |
| `OrdinalInflector::nth_into` | **nothing** | **Appends**, so `clear()` yourself |
| `CaseMode::apply` / `apply_into` | one `String` / none (**appends**, like the tokenizers' `_into`) | |

`nth_into` versus `nth` is the cleanest ergonomics/allocation trade-off in the
workspace: formatting many ordinals into one buffer costs nothing per ordinal,
and the only thing you take on is remembering the `clear()`.

Every inflector method is total — no `Result`, no panic, on any input — so none
of these has a fallible sibling with a different allocation profile. The one
fallible operation in the crate is building a `Rule`, which reports a `RuleError`
at construction precisely so that applying it later cannot fail.

## N-grams

| API | Allocates | Notes |
|---|---|---|
| `ngrams(seq, n)` | **nothing** | Lazy `slice::Windows`; each window is a borrow of `seq` |
| `ngrams(seq, n).collect()` | one outer `Vec` of fat pointers | The windows themselves are still borrows |
| `char_ngrams(text, n)` | **nothing** | One pass to count scalars so `len()` is exact; iteration is free |
| `Padded::new(seq, n, s, e)` | one `Vec<T>`, once, plus `len + k_start + k_end` element clones | `ngrams()` on it then allocates nothing per window |

## Trie

| API | Allocates | Notes |
|---|---|---|
| `Trie::new()` | one `Vec` (the node arena) | Not one allocation per node |
| `insert`, `insert_all` | amortised arena growth; `SmallVec` children stay inline for the common one- and two-child cases | `reserve()` to grow once. Each insertion also maintains the subtree word counts and the hash membership set |
| `contains`, `len`, `node_count` | **nothing** | `contains` hashes the folded bytes; `len` and `node_count` are both O(1) reads |
| `iter_keys_with_prefix`, `keys`, `iter_prefix_matches` | nothing up front | Lazy |
| `keys_with_prefix` | one `Vec<String>`, one `String` per key | Keys are reconstructed by walking; the vector is sized exactly from the subtree word count |
| `for_each_key_with_prefix` | **nothing** | One shared path buffer, handed to the closure as `&str` |
| `prefix_matches` | one `Vec<Cow<str>>`; the elements borrow the search string on a case-sensitive trie | |
| `longest_prefix` | a `PrefixSplit` whose two `Cow`s borrow the search string, unless case folding rewrote it | |
| `longest_prefix_lengths` | **nothing**, ever | Returns scalar counts |
| `freeze` | the whole compressed structure, once | Build-time cost; call it after a bulk load, not per query |
| `FrozenTrie::keys_slice` | **nothing** | Borrows a contiguous range of the precomputed key table |

## Patterns that reduce allocation at your call site

**Prefer the lazy shape when you consume once.** No container at all.

**Reuse one buffer in a loop.** [Buffer reuse](buffer-reuse.md).

**Pre-size when you can estimate.** `Vec::with_capacity`, `Trie::reserve`.

**Keep inputs ASCII where the domain allows.** In `verbora-distance` it is the
difference between borrowing `&[u8]` and allocating a decoded `Vec<char>` per
operand. The phonetic encoders read one Unicode scalar at a time either way, so
there is no promotion there — only the Latin-alphabet encoders' habit of
skipping every scalar outside `A`–`Z`.

**Hoist construction out of loops.** Most tokenizers and nearly all phonetic
encoders are zero-sized or near-zero-sized types, so this matters less than you
would expect — but `SentenceTokenizer::with_abbreviations` owns a `Vec<String>`,
a stemmed `SentimentAnalyzer` rebuilds its whole vocabulary, and any prebuilt
index holds its own storage. Build those once.

**Do not chase allocations that the work dominates.** `levenshtein/ascii/1024`
takes 29.08 µs † per call. Its working state is not the story.

† Pending re-measurement, and left as recorded rather than replaced with a
guess. See [Benchmarks: string distance](../benchmarks/distance.md).

## Related

- [Zero-copy and `Cow`](zero-copy.md)
- [Buffer reuse](buffer-reuse.md)
- [Cache locality and data layout](cache-locality.md)
