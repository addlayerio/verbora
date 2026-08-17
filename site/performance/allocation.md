# Allocation behaviour

A per-API reference for the question "does this allocate, and how much?".

<div class="callout callout-warn">
<strong>These are read from the source, not measured.</strong> Allocation counting
and peak-RSS instrumentation are planned but not yet in the repository. Every
entry below describes what the code does structurally — which is stable and
checkable — rather than a profiler reading. Where a number would need a
measurement, this page says so.
</div>

## Tokenizers

| API | Allocates | Notes |
|---|---|---|
| `tokens(text)` | **nothing** | The iterator is a small stack struct. Tokens are slices. |
| `tokenize(text)` | one `Vec`, plus growth | No per-token allocation for `&str` tokenizers |
| `tokenize_into(text, &mut buf)` | nothing once `buf` is warm | Appends; you call `clear()` |
| `verbora_core::Tokenizer::tokenize` | one `Vec<String>` **plus one `String` per token** | This is the owning API; it matches the reference `string[]` |
| `verbora_core::Tokenizer::tokenize_batch` | one `Vec<String>` per input, plus a `String` per token | Sequential `map`; no shared buffer |
| `BorrowingTokenizer::tokenize_borrowed` | one `Vec<&str>` | The zero-copy path on the core trait |

<div class="callout callout-note">
<strong>Two <code>tokenize</code> methods, two costs.</strong>
<code>verbora_tokenizers::Tokenize::tokenize</code> gives you
<code>Vec&lt;&amp;str&gt;</code> — borrowed. <code>verbora_core::Tokenizer::tokenize</code>
gives you <code>Vec&lt;String&gt;</code> — owned, one allocation per token,
because that trait's contract is to mirror the reference
<code>string[]</code> exactly. If both traits are in scope the call is ambiguous
and the compiler will tell you; import only the one you want.
</div>

The three tokenizers that pre-process (`AggressiveTokenizerNo`, `…Sv`, `…Hi`)
allocate **one** `String` for the rewritten text if the rewrite changed anything,
then slice it — their tokens are `Cow`, borrowed when the pre-pass was a no-op.

## String distance

| API | Allocates per call | Notes |
|---|---|---|
| `levenshtein` (plain) | two `Vec<f64>` of length `m + 1` | The 2-row working set |
| `levenshtein` / `damerau_levenshtein` (restricted) | three `Vec<f64>` | Transposition reaches row − 2 |
| `damerau_levenshtein` (unrestricted) | full `(n+1)×(m+1)` cost matrix **and** parent matrix | Transposition reaches an arbitrary earlier row. It shares the search path's `full_matrix`, so it allocates the parents even in distance mode, where they are never read |
| `levenshtein_search`, `damerau_levenshtein_search` | full cost matrix **and** a parent matrix, plus a `String` for the result substring | Backtracking needs the parents |
| `jaro`, `jaro_winkler` | **nothing** for inputs ≤ 128 code units | Two stack `[bool; 128]` arrays; two `Vec<bool>` above that |
| `dice_coefficient` | one `FxHashMap` of `(u16, u16)` keys | No `String` per bigram, unlike the reference |
| `hamming`, `hamming_checked` | **nothing** on ASCII with `ignore_case: false` | A single scan. With `ignore_case: true`, both operands are folded via `to_lowercase()` first — two `String`s, regardless of ASCII-ness |

Non-ASCII input adds **one `Vec<u16>` per operand** across this crate, from the
promotion described in [Zero-copy](zero-copy.md#_3-exact-fast-paths). ASCII input
is compared as `&[u8]` borrowed from the inputs.

There is no scratch-buffer API. The working rows cannot currently be hoisted out
of a loop.

## Phonetics

Every encoder returns an owned `String` — phonetic keys are computed, not
sliced, so there is nothing to borrow.

| API | Allocates | Notes |
|---|---|---|
| `SoundEx::process`, `Metaphone::process`, … | at least one `String` (the key) | Plus intermediates per pipeline stage |
| `compare(a, b)` | two keys | It does **not** short-circuit: the body is `process(a) == process(b)` |
| `phoneticize_tokens*` | one `Vec` of whatever your closure returns | Takes `IntoIterator`, so it composes with a lazy tokenizer without an intermediate `Vec` |

`Metaphone::process` runs 21 transform stages, and the individual stage methods
(`c_transform`, `drop_h`, …) each return a `String`. `SoundEx`'s nine stage
methods return `Cow`, so they are cheaper when a stage changes nothing.

## Normalizers

| API | Allocates | Notes |
|---|---|---|
| `remove_diacritics`, `normalize_no`, `normalize_sv`, `normalize_ja` | **nothing** when the text is unchanged | `Cow::Borrowed`; allocates at the first replacement |
| `ja::converters::*` (all 17) | same | `Cow`-returning |
| `normalize(&[S])`, `normalize_token(&str)` | one `Vec<String>`, plus a `String` per output token | One contraction expands to several tokens |

This is the crate where the `Cow` discipline pays most: it is normal for a whole
corpus to pass through `remove_diacritics` with zero allocations.

## Inflectors

| API | Allocates | Notes |
|---|---|---|
| `pluralize`, `singularize` | the result `String`, **plus** a `String` inside the matching rule | Two per call on the English path |
| `pluralize_into`, `singularize_into` | the rule's `String` only | Saves the result allocation; **appends**, so `clear()` yourself |
| `CountInflector::nth`, `nth_str` | one `String` | |
| `CountInflector::nth_f64` | the result, plus a float-formatting buffer | |
| `CountInflector::nth_form*` | **nothing** | Returns `&'static str` — just the suffix |
| `CaseMode::apply` / `apply_into` | one `String` / none (**appends**, like the tokenizers' `_into`) | |

`nth_form` versus `nth` is the cleanest ergonomics/allocation trade-off in the
workspace: if you are writing into an existing buffer, take the `&'static str`
suffix and `write!` it yourself.

## N-grams

| API | Allocates | Notes |
|---|---|---|
| `ngrams_iter(...)` | nothing up front | Lazy `NGramIter` |
| `ngrams(...)` | one outer `Vec`, one inner `Vec` per window | Windows hold clones of `T` |
| `ngrams_owned(...)` | as above, with owned elements | |
| `*_str(...)` | the above **plus** a full tokenization | The string entry points tokenize first |
| `*_with_stats(...)` | the above plus a frequency map and its `String` keys | Do not pay for it if you only want the windows |

## Trie

| API | Allocates | Notes |
|---|---|---|
| `Trie::new()` | one `Vec` (the node arena) | Not one allocation per node |
| `add_string` | amortised arena growth; `SmallVec` children stay inline for the common one- and two-child cases | `reserve()` to grow once |
| `contains`, `get_size` | **nothing** | `get_size` is O(1) |
| `iter_keys_with_prefix`, `keys`, `iter_matches_on_path` | nothing up front | Lazy |
| `keys_with_prefix` | one `Vec<String>`, one `String` per key | Keys are reconstructed by walking |
| `find_prefix` | a `Cow` pair — owned only when a cut lands inside a surrogate pair | |
| `find_prefix_lengths` | **nothing** | Returns code-unit indices; exact |

## Patterns that reduce allocation at your call site

**Prefer the lazy shape when you consume once.** No container at all.

**Reuse one buffer in a loop.** [Buffer reuse](buffer-reuse.md).

**Pre-size when you can estimate.** `Vec::with_capacity`, `Trie::reserve`.

**Keep inputs ASCII where the domain allows.** It is the difference between
borrowing `&[u8]` and allocating a `Vec<u16>` per operand in distance and
phonetics.

**Hoist construction out of loops.** Most tokenizers and all four phonetic
encoders are zero-sized or near-zero-sized types, so this matters less than you
would expect — but `SentenceTokenizer::with_abbreviations` owns a `Vec<String>`,
and `OrthographyTokenizer::new(lang)` and the regex-driven tokenizers hold
compiled patterns. Build those once.

**Do not chase allocations that the work dominates.** `levenshtein/ascii/1024`
takes 3.24 ms per call. Its two working rows are not the story.

## Related

- [Zero-copy and `Cow`](zero-copy.md)
- [Buffer reuse](buffer-reuse.md)
- [Cache locality and data layout](cache-locality.md)
