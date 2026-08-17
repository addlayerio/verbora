# N-grams

`verbora-ngrams` turns a sequence into its sliding windows: bigrams, trigrams,
arbitrary `n`, optionally padded with start and end symbols, optionally with a
frequency table attached. It covers both a generic token-input engine and a
Chinese-specific `zh` module that splits per UTF-16 code unit. One generic
engine backs every entry point, so the token path (`&str`), the numeric path
(`i64`) and the Chinese code-unit path (`&[u16]`) cannot drift apart.

This is also the crate with the widest spread between the cheapest and the most
expensive way to ask the same question. A straightforward implementation
materialises a fresh nested array — one allocation per window — unconditionally.
Verbora's primitive is a lazy iterator that yields `Cow`, borrowing every
unpadded window and allocating only for the `2(n-1)` tuples that genuinely mix
pad symbols with sequence elements. Everything else in the crate is a wrapper
that gives some of that back in exchange for a more convenient shape.

<div class="callout callout-spec">
<strong>Specification status.</strong> Both n-gram APIs are documented and
test-pinned. Argument shapes that Rust's type system makes unreachable — a
non-integer <code>n</code>, a non-string non-sequence input — are handled by
the signature rather than at runtime, and the pages below say so explicitly.
<code>cargo test -p verbora-ngrams</code> runs <strong>43</strong> unit tests
and <strong>13</strong> doctests.
</div>

## When to use it

- You need bigrams, trigrams or arbitrary `n`-grams over tokens you already have.
- You need precise, well-defined padding at the boundaries — including edge
  cases such as `n` longer than the sequence, or padded tuples that come out
  shorter than `n` (see [Padding semantics](#padding-semantics)).
- You want a frequency table and a Good–Turing count-of-counts (`Nr`) in one
  pass.
- You are windowing Chinese (or other per-character) text and need UTF-16
  code-unit boundaries rather than Unicode scalar values — see
  [Chinese: `zh`](#chinese-zh).
- You want to stream windows over a large corpus without allocating per window.

## When not to use it

- **You want a language model.** This crate produces windows and counts. There is
  no smoothing, no probability estimation, no perplexity — `nr` is the raw
  count-of-counts a Good–Turing estimator would consume, and the estimator is
  yours to write.
- **You want linguistic tokenization.** The default tokenizer, `WordTokenizer`,
  is deliberately tiny — its character class is `[A-Za-zА-Яа-я0-9_]` and
  nothing else. `café` becomes `caf`. If you want a real tokenizer, pick one
  from [Tokenizers](./tokenizers.md) and pass it to `ngrams_str_with`.
- **You want parallel or batched generation.** There is none — see
  [Performance characteristics](#performance-characteristics).
- **You want to reuse an output buffer.** There is no `_into` variant in this
  crate. The lazy iterator is the memory-frugal path instead.

## Quick example

```rust
use verbora_ngrams::{bigrams, ngrams_str, tokenize, trigrams};

fn main() {
    // Pre-tokenized input: windows borrow from the slice, nothing is copied.
    let tokens = ["the", "quick", "brown", "fox"];
    let grams = bigrams(&tokens, None, None);
    assert_eq!(grams.len(), 3);
    assert_eq!(grams[0].to_vec(), vec!["the", "quick"]);

    // String input: tokenized with the process-global tokenizer first.
    assert_eq!(
        ngrams_str("a b c", 2, None, None),
        vec![vec!["a", "b"], vec!["b", "c"]]
    );

    // Tokenize once, window many times.
    let toks = tokenize("the quick brown fox");
    assert_eq!(trigrams(&toks, None, None).len(), 2);
}
```

## Choosing the right API

The short version is below; the full decision, with worked trade-offs, lives on
[Choosing: n-grams](../choosing/ngrams.md).

<div class="callout callout-note">
<strong>Note.</strong> Blocks marked <code>rust,ignore</code> on this page are
bare signatures — declarations without a body, which by design do not compile.
Every other Rust block is a complete program that compiles and whose assertions
pass; they are checked by
the site-wide snippet harness (<code>python3 site/check-snippets.py</code>).
</div>

### Comparison table

| API | Input | Output | Lazy | Windows borrowed | Allocations |
|---|---|---|:--:|:--:|---|
| `ngrams_iter` | `&[T]` | `NGramIter<'a, T>` | ✅ | ✅ | none for windows; one `Vec` per pad tuple |
| `ngrams` | `&[T]` | `Vec<Cow<'a, [T]>>` | ❌ | ✅ | one `Vec`; one per pad tuple |
| `ngrams_owned` | `&[T]` | `Vec<Vec<T>>` | ❌ | ❌ | one `Vec` per n-gram + one `T::clone` per element |
| `bigrams` / `trigrams` | `&[T]` | `Vec<Cow<'a, [T]>>` | ❌ | ✅ | as `ngrams` |
| `multrigrams` | `&[T]` | `Vec<Cow<'a, [T]>>` | ❌ | ✅ | exact alias of `ngrams` |
| `ngrams_with_stats` | `&[T]` | `NGramStats<'a, T>` | ❌ | ✅ | as `ngrams`, plus one key `String` per n-gram and two maps |
| `ngrams_str` | `&str` | `Vec<Vec<String>>` | ❌ | ❌ | full tokenization + one `String` per element per tuple |
| `ngrams_str_with` | `&str` + tokenizer | `Vec<Vec<String>>` | ❌ | ❌ | as `ngrams_str`, but reads no global |
| `ngrams_str_with_stats` | `&str` | `NGramStats<'static, String>` | ❌ | ❌ | as `ngrams_str` plus statistics, then a full copy |
| `ngrams_zh` | `&str` | `Vec<Vec<Cow<'a, str>>>` | ❌ | ✅ (elements) | one split `Vec` + one `Vec` per n-gram |
| `ngrams_zh_utf16` | `&[u16]` | `Vec<Vec<&'a [u16]>>` | ❌ | ✅ | one element `Vec` + one `Vec` per n-gram |

### Decision tree

```text
I need n-grams
│
├── My input is a Chinese (or otherwise per-character) string
│      ├── Input may contain astral characters and must round-trip
│      │      └── zh::code_units() then zh::ngrams_zh_utf16()
│      ├── BMP only, want ergonomic `Cow<str>` elements
│      │      └── zh::ngrams_zh() / bigrams_zh() / trigrams_zh()
│      └── Streaming a large document
│             └── zh::split_lossy() then ngrams_iter()
│
├── My input is a string of words
│      ├── I control the tokenizer
│      │      └── ngrams_str_with(&tokenizer, …)
│      ├── I want the crate's default process-global tokenizer
│      │      └── ngrams_str()
│      └── I will window the same tokens more than once
│             └── tokenize() once, then the &[T] branch below
│
└── My input is already a slice of tokens
       ├── I will stop early, or I only fold over the windows
       │      └── ngrams_iter()
       ├── I need indexable windows, tokens outlive them
       │      └── ngrams() / bigrams() / trigrams() / multrigrams()
       ├── The tuples must outlive the token slice
       │      └── ngrams_owned()
       └── I need frequencies and a count-of-counts
              └── ngrams_with_stats()
```

### `ngrams_iter` — the lazy primitive <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a> <a class="badge badge-cow" href="../performance/zero-copy">COW</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Cow&lt;'a, [T]&gt;</code> — <code>Borrowed</code> windows, <code>Owned</code> pad tuples</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None per window; one <code>Vec</code> per padded tuple (at most <code>2(n-1)</code> in total)</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A — no output buffer to reuse</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Streaming a corpus, early termination, folding into a counter</span></div>
</div>

```rust  ignore
pub fn ngrams_iter<T: Clone>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramIter<'_, T>
```

`NGramIter<'a, T>` yields `Cow<'a, [T]>` in exactly three phases:

1. `n - 1` left-padded tuples, if `start_symbol` is `Some` — always `Cow::Owned`;
2. every window of length `n` — always `Cow::Borrowed`, a pointer and a length;
3. `n - 1` right-padded tuples, if `end_symbol` is `Some` — always `Cow::Owned`.

It implements `Iterator`, `ExactSizeIterator` and `FusedIterator`, and derives
`Debug` and `Clone`. It is **not** `DoubleEndedIterator`. `size_hint` is exact,
so `len()` is free and `collect()` reserves once.

```rust
use verbora_ngrams::ngrams_iter;
use std::borrow::Cow;

fn main() {
    let tokens = ["a", "b", "c", "d"];
    let mut it = ngrams_iter(&tokens, 2, None, None);
    assert_eq!(it.len(), 3);                       // ExactSizeIterator
    let first = it.next().expect("three bigrams");
    assert!(matches!(first, Cow::Borrowed(_)));    // no copy was made
    assert_eq!(it.len(), 2);
}
```

Nothing is computed until the iterator is advanced, so `.take(k)`, `.find(…)`
and `.any(…)` genuinely stop early. See
[Choosing: n-grams](../choosing/ngrams.md) for a worked example.

### `ngrams` — borrowed windows in a `Vec` <a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Cow&lt;'a, [T]&gt;&gt;</code>, borrowing <code>sequence</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>, reserved once from the exact <code>size_hint</code>; one <code>Vec</code> per padded tuple</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Indexable windows whose token slice outlives them</span></div>
</div>

```rust  ignore
pub fn ngrams<T: Clone>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Cow<'_, [T]>>
```

This is `ngrams_iter(…).collect()`. It is the recommended default for
pre-tokenized input: the result is random-access, and the windows still cost
nothing beyond the outer `Vec` because they point back into `sequence`. The
returned value borrows `sequence`, so it cannot outlive it.

### `ngrams_owned` — fully owned windows <a class="badge badge-owned" href="../performance/allocation">OWNED</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Vec&lt;T&gt;&gt;</code>, fully detached from <code>sequence</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One outer <code>Vec</code>, one <code>Vec</code> per n-gram, one <code>T::clone</code> per element</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Tuples that must outlive the sequence, fully detached and independently owned</span></div>
</div>

```rust  ignore
pub fn ngrams_owned<T: Clone>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Vec<T>>
```

This is `ngrams_iter(…).map(Cow::into_owned).collect()`. For `T = String` it
allocates a fresh `String` for every element of every tuple — with `n = 2` over
`k` tokens that is roughly `2k` string allocations. Reach for it only when the
lifetime genuinely demands it.

### `bigrams`, `trigrams`, `multrigrams`

```rust  ignore
pub fn bigrams<T: Clone>(
    sequence: &[T],
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Cow<'_, [T]>>
pub fn trigrams<T: Clone>(
    sequence: &[T],
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Cow<'_, [T]>>
pub fn multrigrams<T: Clone>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Cow<'_, [T]>>
```

`bigrams` and `trigrams` are `ngrams` with `n` fixed at 2 and 3.
**`multrigrams` is an exact alias of `ngrams`.** It exists as a distinct name
for callers who prefer to spell out arbitrary-`n` windowing explicitly; there
is no behavioural difference to look for.

### The string family

```rust  ignore
pub fn ngrams_str(
    text: &str,
    n: usize,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>>
pub fn bigrams_str(
    text: &str,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>>
pub fn trigrams_str(
    text: &str,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>>
pub fn multrigrams_str(
    text: &str,
    n: usize,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>>
```

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Vec&lt;String&gt;&gt;</code> — fully owned, no borrowing at all</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec&lt;String&gt;</code> plus one <code>String</code> per token, then one <code>Vec</code> and <code>n</code> <code>String</code>s per n-gram</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">One-shot calls where the input really is a string</span></div>
</div>

Each of these tokenizes with the **process-global** tokenizer and then calls
`ngrams_owned`, because the tokens are created and dropped inside the call and
so cannot be borrowed from. If you already have tokens, do not route through
them — see [Choosing: n-grams](../choosing/ngrams.md).

### `ngrams_str_with` — explicit tokenizer

```rust  ignore
pub fn ngrams_str_with<T: NGramTokenizer + ?Sized>(
    tokenizer: &T,
    text: &str,
    n: usize,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>>
```

The escape hatch from the global binding. Identical cost to `ngrams_str`, minus
the atomic load and the possible `RwLock` read. Prefer it: it makes the
tokenizer visible at the call site and leaves the process-wide state alone.

```rust
use verbora_ngrams::{FnTokenizer, ngrams_str_with};

fn main() {
    let by_char = FnTokenizer(|s: &str| s.chars().map(String::from).collect());
    assert_eq!(
        ngrams_str_with(&by_char, "abc", 2, None, None),
        vec![vec!["a", "b"], vec!["b", "c"]]
    );
}
```

There is no `bigrams_str_with` / `trigrams_str_with`; pass `n = 2` or `n = 3`.

### `ngrams_of_tokens`

```rust  ignore
pub fn ngrams_of_tokens<'a>(
    tokens: &'a [&'a str],
    n: usize,
    start_symbol: Option<&'a str>,
    end_symbol: Option<&'a str>,
) -> Vec<Cow<'a, [&'a str]>>
```

A `&str`-specialised restatement of `ngrams`, living in the `text` module so
that the "I already have tokens" case is discoverable from the string-input
side. It is **not** re-exported at the crate root — reach it as
`verbora_ngrams::text::ngrams_of_tokens`.

### The statistics family

See [Frequency statistics](#frequency-statistics-ngramstats) below for the shape
of the result.

```rust  ignore
pub fn ngrams_with_stats<T: Clone + fmt::Display>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramStats<'_, T>
pub fn bigrams_with_stats<T: Clone + fmt::Display>(
    sequence: &[T],
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramStats<'_, T>
pub fn trigrams_with_stats<T: Clone + fmt::Display>(
    sequence: &[T],
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramStats<'_, T>
pub fn multrigrams_with_stats<T: Clone + fmt::Display>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramStats<'_, T>
```

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager, single pass</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>NGramStats&lt;'a, T&gt;</code>; its <code>ngrams</code> field keeps the borrowed windows</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One n-gram <code>Vec</code>; one key <code>String</code> <em>per n-gram</em> (dropped again on a repeat); one <code>HashMap</code>; one slot <code>Vec</code>; one <code>frequencies</code> <code>Vec</code>; one <code>BTreeMap</code></span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Frequency tables and Good–Turing count-of-counts in one pass</span></div>
</div>

The `T: fmt::Display` bound is what `ngram_key` needs. The string forms —
`ngrams_str_with_stats`, `bigrams_str_with_stats`, `trigrams_str_with_stats`,
`multrigrams_str_with_stats` — all return `NGramStats<'static, String>`, because
they tokenize internally and then call `into_owned()` to detach the windows from
the temporary token vector. That is a second full copy on top of the first.

### The Chinese family

Covered in [Chinese: `zh`](#chinese-zh) below.

## Padding semantics

This is the part a naive implementation gets wrong, and it deserves explicit
documentation rather than a guess.

**Padding is driven by `Option`, not by emptiness.** `None` disables padding
entirely; `Some("")` pads with empty strings — the two are different answers:

```rust
use verbora_ngrams::ngrams_owned;

fn main() {
    let seq = ["a", "b", "c"];

    // None: no padding at all.
    assert_eq!(
        ngrams_owned(&seq, 2, None, None),
        vec![vec!["a", "b"], vec!["b", "c"]]
    );

    // Some(""): padding with the empty string. Not the same thing.
    assert_eq!(
        ngrams_owned(&seq, 2, Some(""), None),
        vec![vec!["", "a"], vec!["a", "b"], vec!["b", "c"]]
    );
}
```

**Padded tuples are not always `n` elements long.** Both padding loops clamp
their sequence half independently, and the right-hand loop slices with an index
that can go *negative*, re-anchoring to `length + start` rather than clamping
to zero. Reaching for `len.saturating_sub(p)` instead silently produces longer
tuples than the algorithm intends. The canonical case:

```rust
use verbora_ngrams::ngrams;
use std::borrow::Cow;

fn main() {
    let seq = ["a", "b", "c"];
    let got: Vec<Vec<&str>> = ngrams(&seq, 5, Some("<s>"), Some("</s>"))
        .into_iter()
        .map(Cow::into_owned)
        .collect();

    assert_eq!(
        got,
        vec![
            vec!["<s>", "<s>", "<s>", "<s>", "a"],
            vec!["<s>", "<s>", "<s>", "a", "b"],
            vec!["<s>", "<s>", "a", "b", "c"],
            vec!["<s>", "a", "b", "c"],   // tail clamped: only 3 elements exist
            vec!["c", "</s>"],            // slice(-1, 3) — the LAST element only
            vec!["a", "b", "c", "</s>", "</s>"],
            vec!["b", "c", "</s>", "</s>", "</s>"],
            vec!["c", "</s>", "</s>", "</s>", "</s>"],
        ]
    );
}
```

Note the fourth tuple has four elements and the fifth has **two**. There are no
unpadded windows at all here, because `n > sequence.len()`. Unpadded windows are
always exactly `n` long; only padded tuples can be short.

**`n = 1` never pads**, even when both symbols are supplied, because the loop is
`for (p = n - 1; p > 0; p--)` and never runs. **`n = 0` yields `len + 1` empty
tuples.** **`n > len` yields no windows** — not one short window.

```rust
use verbora_ngrams::ngrams_owned;

fn main() {
    let seq = ["a", "b", "c"];

    // n == 1: symbols are supplied and ignored.
    assert_eq!(
        ngrams_owned(&seq, 1, Some("<s>"), Some("</s>")),
        vec![vec!["a"], vec!["b"], vec!["c"]]
    );

    // n == 0: four empty tuples for a three-element sequence.
    assert_eq!(ngrams_owned(&seq, 0, None, None), vec![Vec::<&str>::new(); 4]);

    // n > len: no windows.
    assert!(ngrams_owned(&seq, 5, None, None).is_empty());

    // An empty sequence still pads.
    let empty: [&str; 0] = [];
    assert_eq!(
        ngrams_owned(&empty, 2, Some("<s>"), Some("</s>")),
        vec![vec!["<s>"], vec!["</s>"]]
    );
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> Do not write code that assumes
<code>gram.len() == n</code>. Index a padded result with <code>gram.get(i)</code>,
or filter to the unpadded windows first. This is the single most common way to
panic while consuming this crate.
</div>

## Frequency statistics: `NGramStats`

```rust  ignore
pub struct NGramStats<'a, T: Clone> {
    pub ngrams: Vec<Cow<'a, [T]>>,
    pub frequencies: Vec<(String, u64)>,
    pub nr: BTreeMap<u64, u64>,
    pub number_of_ngrams: u64,
}
```

`NGramStats` bundles the windows with a frequency table and a Good–Turing
count-of-counts. Two of its fields have an **observable iteration order**, and
it is not the same order for both, which is why they are different container
types here:

- **`frequencies`** is keyed by `ngram_key`, which always begins with `(` or is
  `)`. Those keys are not integer-like, so they are kept in *insertion* order —
  first-seen order. A `HashMap` would lose that and a `BTreeMap` would replace
  it with lexicographic order, so this is a `Vec<(String, u64)>`.
- **`nr`** is keyed by a frequency *count*, so its keys are integer-like and are
  kept in ascending numeric order regardless of insertion order.
  `BTreeMap<u64, u64>` expresses that directly.

`number_of_ngrams` counts padded tuples too, so it equals `ngrams.len()`, not
the number of windows.

```rust
use verbora_ngrams::ngrams_with_stats;

fn main() {
    let seq = ["a", "b", "a", "b", "a"];
    let stats = ngrams_with_stats(&seq, 2, None, None);

    assert_eq!(stats.number_of_ngrams, 4);
    assert_eq!(stats.ngrams.len(), 4);

    // First-seen order, not lexicographic.
    assert_eq!(
        stats.frequencies,
        vec![(String::from("(a, b)"), 2), (String::from("(b, a)"), 2)]
    );

    // Two distinct n-grams occur exactly twice.
    assert_eq!(stats.nr.get(&2), Some(&2));
}
```

### `frequency()`

```rust  ignore
pub fn frequency(&self, key: &str) -> u64
```

A **linear scan** over `frequencies`, returning `0` for a key that never
occurred. That is deliberate: the type exists to preserve first-seen order,
not to serve lookups. If you need many lookups, build an index once.

```rust
use verbora_ngrams::ngrams_with_stats;
use std::collections::HashMap;

fn main() {
    let seq = ["a", "b", "a", "b", "a"];
    let stats = ngrams_with_stats(&seq, 2, None, None);

    // Fine once.
    assert_eq!(stats.frequency("(a, b)"), 2);
    assert_eq!(stats.frequency("(z, z)"), 0);

    // Wrong in a loop — build this instead.
    let index: HashMap<&str, u64> = stats
        .frequencies
        .iter()
        .map(|(key, count)| (key.as_str(), *count))
        .collect();
    assert_eq!(index["(a, b)"], 2);
}
```

### `into_owned()`

```rust  ignore
pub fn into_owned(self) -> NGramStats<'static, T>
```

Detaches the n-grams from the sequence they were built over by copying the
windows that were still `Cow::Borrowed`; the padded tuples were already owned,
and `frequencies`, `nr` and `number_of_ngrams` are moved untouched. Needed when
the token sequence is a temporary — which is exactly what
`ngrams_str_with_stats` does internally:

```rust
use verbora_ngrams::{NGramStats, ngrams_with_stats, tokenize};

fn stats_of(text: &str) -> NGramStats<'static, String> {
    let tokens = tokenize(text);
    ngrams_with_stats(&tokens, 2, None, None).into_owned()
}

fn main() {
    let s = stats_of("a b a b");
    assert_eq!(s.number_of_ngrams, 3);
    assert_eq!(s.frequency("(a, b)"), 2);
}
```

### `ngram_key` and the `")"` quirk

```rust  ignore
pub fn ngram_key<T: fmt::Display>(ngram: &[T]) -> String
```

Renders an n-gram as a parenthesized, comma-separated key: `["a","b"]` becomes
`"(a, b)"`. Two things about it are surprising:

**The empty n-gram keys as `")"`, not `"()"`.** For every non-empty n-gram the
key is built as `"("`, the elements joined by `", "`, then `")"`. The empty
n-gram is a special case that returns the bare string `")"` directly rather
than the seemingly natural `"()"`. This is reachable whenever `n == 0`.

**Keys are not injective.** Elements are concatenated raw, so a separator inside
a token is not escaped: `["a, b"]` and `["a", "b"]` produce the same key.

```rust
use verbora_ngrams::{ngram_key, ngrams_with_stats};

fn main() {
    assert_eq!(ngram_key(&["a", "b"]), "(a, b)");
    assert_eq!(ngram_key(&["a"]), "(a)");
    assert_eq!(ngram_key(&[1, 2]), "(1, 2)");

    // Not injective: the separator inside the token is not escaped.
    assert_eq!(ngram_key(&["a, b"]), "(a, b)");

    // The empty n-gram keys as a lone closing paren.
    assert_eq!(ngram_key::<&str>(&[]), ")");

    let seq = ["a", "b"];
    let stats = ngrams_with_stats(&seq, 0, None, None);
    assert_eq!(stats.frequencies, vec![(String::from(")"), 3)]);
}
```

`ngram_key` allocates one `String` with capacity `2 + ngram.len() * 16` — a
deliberate over-estimate that covers a typical word plus its separator in one
allocation rather than reallocating.

## The process-global tokenizer

<span class="badge badge-global">GLOBAL STATE</span>

Rather than threading a tokenizer through every call, `verbora-ngrams` keeps a
real **module-level mutable variable** holding the default tokenizer.
`set_tokenizer` rebinds it for the whole process, for every caller,
permanently, until `reset_tokenizer` clears it back to the default
`WordTokenizer`. A caller who would rather not touch process-wide state should
reach for `ngrams_str_with` instead, which takes the tokenizer explicitly.

```rust  ignore
pub fn set_tokenizer<T: NGramTokenizer + 'static>(tokenizer: T)
pub fn reset_tokenizer()
pub fn current_tokenizer() -> Option<Arc<dyn NGramTokenizer>>
pub fn tokenize(text: &str) -> Vec<String>
```

### How it is stored

Two statics in `tokenizer.rs`:

- `static OVERRIDDEN: AtomicBool` — whether `set_tokenizer` has ever been called;
- `static GLOBAL: RwLock<Option<Arc<dyn NGramTokenizer>>>` — the binding itself,
  `None` meaning "still the default `WordTokenizer`".

`current_tokenizer` loads the flag with `Ordering::Relaxed` and returns `None`
immediately when it is false, so **the never-overridden path takes no lock at
all**. `tokenize` clones the `Arc` out of the lock *before* calling into it, so
a tokenizer that itself touches the global cannot deadlock.

### Is it thread-safe?

Yes, in the memory-safety sense, and no, in the sense that matters to you.

- `NGramTokenizer: Send + Sync`, `GLOBAL` is an `RwLock`, and the `Arc` is
  cloned out before use. There is no data race, and installing a tokenizer from
  one thread while another reads is well-defined.
- But `OVERRIDDEN` is `Relaxed`, so there is no happens-before edge guaranteeing
  a concurrent reader observes an install promptly. During the brief window
  inside `set_tokenizer` (write `GLOBAL`, then set the flag) and inside
  `reset_tokenizer` (clear `GLOBAL`, then clear the flag), a concurrent reader
  falls back to the default `WordTokenizer`. It never panics and never sees a
  torn value — it may simply see the old behaviour.
- The three functions that touch the lock call `.expect("tokenizer lock
  poisoned")`. If a thread panics while holding the write lock — for example
  from a `Drop` impl on the tokenizer being replaced — every later call panics.

<div class="callout callout-warn">
<strong>Careful.</strong> This is shared mutable state with process-wide reach.
In a test binary, Rust runs tests on multiple threads in one process, so a test
that calls <code>set_tokenizer</code> changes what every concurrently running
test observes. The crate's own tests serialise on a private mutex for exactly
this reason. If your tests call <code>set_tokenizer</code>, do the same — or,
better, do not call it at all and use <code>ngrams_str_with</code>.
</div>

### Prefer the explicit sibling

Every function that reads the global has a sibling that takes a tokenizer
explicitly. `ngrams_str_with(&tokenizer, …)` produces identical output, reads no
global, writes no global, and makes the tokenizer visible at the call site. Use
`set_tokenizer` only when your own code genuinely depends on that rebinding
being observable elsewhere; most callers should prefer the explicit form.

```rust
use verbora_ngrams::{
    FnTokenizer, current_tokenizer, ngrams_str, reset_tokenizer, set_tokenizer,
};

fn main() {
    // `None` means "still the default WordTokenizer".
    assert!(current_tokenizer().is_none());
    assert_eq!(ngrams_str("a b", 2, None, None), vec![vec!["a", "b"]]);

    set_tokenizer(FnTokenizer(|s: &str| {
        s.split('-').map(str::to_owned).collect()
    }));
    assert!(current_tokenizer().is_some());
    // "a b" is now a single token, so there is no bigram at all.
    assert!(ngrams_str("a b", 2, None, None).is_empty());

    // `reset_tokenizer` exists so tests can isolate themselves from each
    // other's global tokenizer state.
    reset_tokenizer();
    assert!(current_tokenizer().is_none());
    assert_eq!(ngrams_str("a b", 2, None, None), vec![vec!["a", "b"]]);
}
```

### `NGramTokenizer`, `WordTokenizer`, `FnTokenizer`

```rust  ignore
pub trait NGramTokenizer: Send + Sync {
    fn tokenize_text(&self, text: &str) -> Vec<String>;
}
impl<T: Tokenizer + Send + Sync + ?Sized> NGramTokenizer for T { /* … */ }
```

`NGramTokenizer` is the `dyn`-compatible projection of
[`verbora_core::Tokenizer`](./core.md), which has a generic method
(`tokenize_batch`) and so cannot itself go behind a `dyn`. The blanket
implementation means **any** `verbora_core::Tokenizer` that is `Send + Sync` can
be installed with `set_tokenizer` or passed to `ngrams_str_with` without
implementing anything extra — including everything in
[Tokenizers](./tokenizers.md).

`WordTokenizer` is the default: a small, self-contained tokenizer whose gaps
pattern is `/[^A-Za-zА-Яа-я0-9_]+/`. It implements both `Tokenizer` and
`verbora_core::BorrowingTokenizer`, so it can produce borrowed tokens — the
zero-copy path into `ngrams`. See
[Unicode and language notes](#unicode-and-language-notes) for what that
character class costs you.

`FnTokenizer<F>(pub F)` adapts a closure `Fn(&str) -> Vec<String>` into a
`Tokenizer`, for installing an ad hoc tokenizer without writing a named type.

```rust
use verbora_core::{BorrowingTokenizer, Tokenizer};
use verbora_ngrams::{WordTokenizer, ngrams};
use std::borrow::Cow;

fn main() {
    let t = WordTokenizer;
    assert_eq!(t.tokenize("She said 'hello'."), ["She", "said", "hello"]);

    // The zero-copy path: tokens borrow from `text`, windows borrow from tokens.
    let text = String::from("hello_world 123");
    let borrowed: Vec<&str> = t.tokenize_borrowed(&text);
    assert_eq!(borrowed, ["hello_world", "123"]);
    let grams = ngrams(&borrowed, 2, None, None);
    assert!(matches!(grams[0], Cow::Borrowed(_)));
}
```

## Chinese: `zh`

<span class="badge badge-utf16">UTF-16</span>

The `zh` module is the engine behind `ngrams` with two changes: no statistics
support, and string input is split per UTF-16 code unit instead of going
through a tokenizer. It exposes only `ngrams_zh`, `bigrams_zh` and
`trigrams_zh` — no tokenizer override, no `multrigrams_zh`, no statistics
variant, no module-level state.

**Array input needs nothing from this module.** Windowing an already-split
slice is exactly what `ngrams` does, so use `ngrams` directly for that case.

### `zh` splits UTF-16 code units, not characters

This is the module's one real per-character semantic choice, and it is
observable:

```text
ngrams_zh("a👍b", 2)
  UTF-16 code units : [['a','\ud83d'], ['\ud83d','\udc4d'], ['\udc4d','b']]   (3 bigrams)
  Unicode scalar values (char) : [['a','👍'], ['👍','b']]                    (2 bigrams)
```

A single emoji such as `'👍'` is two UTF-16 code units, so it is torn into its
surrogate halves and each half becomes its own element. Combining marks
separate too: `"éx"` in NFD yields `['e','◌́']`, `['◌́','x']`.

Rust's `String` cannot hold an unpaired surrogate, so the module offers two
entry points:

| Function | Element type | Astral input |
|---|---|---|
| `ngrams_zh` | `Cow<'a, str>` | correct **shape and positions**; each torn surrogate half renders as `U+FFFD` |
| `ngrams_zh_utf16` | `&'a [u16]` | **exact**, surrogates preserved and round-trippable |

`ngrams_zh` is the ergonomic one and is exact for the entire Basic Multilingual
Plane — which includes every CJK character this module exists to serve. Reach
for `ngrams_zh_utf16` when the input may contain astral characters and the
elements must round-trip. The test suite runs the **whole** ZH fixture through
`ngrams_zh_utf16`, because that is the only representation that can hold every
UTF-16 code unit, paired or not.

### The surface

```rust  ignore
pub fn code_units(text: &str) -> Vec<u16>
pub fn split_lossy(text: &str) -> Vec<Cow<'_, str>>

pub fn ngrams_zh<'a>(
    text: &'a str,
    n: usize,
    start_symbol: Option<&'a str>,
    end_symbol: Option<&'a str>,
) -> Vec<Vec<Cow<'a, str>>>
pub fn bigrams_zh<'a>(
    text: &'a str,
    start_symbol: Option<&'a str>,
    end_symbol: Option<&'a str>,
) -> Vec<Vec<Cow<'a, str>>>
pub fn trigrams_zh<'a>(
    text: &'a str,
    start_symbol: Option<&'a str>,
    end_symbol: Option<&'a str>,
) -> Vec<Vec<Cow<'a, str>>>

pub fn ngrams_zh_utf16<'a>(
    units: &'a [u16],
    n: usize,
    start_symbol: Option<&'a [u16]>,
    end_symbol: Option<&'a [u16]>,
) -> Vec<Vec<&'a [u16]>>
pub fn bigrams_zh_utf16<'a>(
    units: &'a [u16],
    start_symbol: Option<&'a [u16]>,
    end_symbol: Option<&'a [u16]>,
) -> Vec<Vec<&'a [u16]>>
pub fn trigrams_zh_utf16<'a>(
    units: &'a [u16],
    start_symbol: Option<&'a [u16]>,
    end_symbol: Option<&'a [u16]>,
) -> Vec<Vec<&'a [u16]>>
```

Only `ngrams_zh`, `bigrams_zh` and `trigrams_zh` are re-exported at the crate
root. Everything else is `verbora_ngrams::zh::…`.

- **`code_units`** is `text.encode_utf16().collect()` — the UTF-16 code units
  that back this module's per-character splitting. Use it to prepare input
  for the `_utf16` family, and `str::encode_utf16` for the pad symbols.
- **`split_lossy`** produces one element per UTF-16 code unit, rendering each
  half of a torn surrogate pair as `U+FFFD`. Every element is `Cow::Borrowed` —
  BMP characters borrow from `text`, the replacement characters borrow a
  `'static` literal — so only the outer `Vec` allocates. Its capacity is
  `text.len()` in **bytes**, which over-reserves roughly threefold for CJK.
  `split_lossy` is what makes the cheap path possible: split once, then window
  with the borrowing engine.

```rust
use verbora_ngrams::ngrams;
use verbora_ngrams::zh::{bigrams_zh, code_units, ngrams_zh, ngrams_zh_utf16, split_lossy};
use std::borrow::Cow;

fn main() {
    assert_eq!(
        ngrams_zh("中文测试", 2, None, None),
        vec![vec!["中", "文"], vec!["文", "测"], vec!["测", "试"]]
    );
    assert_eq!(bigrams_zh("中文", None, None), vec![vec!["中", "文"]]);

    // NFD: the combining mark is its own element.
    assert_eq!(
        ngrams_zh("e\u{301}x", 2, None, None),
        vec![vec!["e", "\u{301}"], vec!["\u{301}", "x"]]
    );

    // Astral input: three bigrams over four elements, with the torn halves
    // rendered as U+FFFD.
    let got = ngrams_zh("a👍b", 2, None, None);
    assert_eq!(got.len(), 3);
    assert_eq!(got[0][1], "\u{FFFD}");

    // Exact round-trip in code-unit space.
    let units = code_units("a👍b");
    assert_eq!(units.len(), 4);
    let exact = ngrams_zh_utf16(&units, 2, None, None);
    assert_eq!(exact[1], vec![&[0xD83Du16][..], &[0xDC4Du16][..]]);

    // The cheap path: split once, then window with borrowed tuples.
    let split = split_lossy("中文测试");
    let windows = ngrams(&split, 2, None, None);
    assert_eq!(windows.len(), 3);
    assert!(matches!(windows[0], Cow::Borrowed(_)));
}
```

## Advanced usage

### Any `Clone` element type

The engine is generic, so n-grams over token IDs, part-of-speech tags or
anything else work the same way. Only the statistics family adds a bound
(`fmt::Display`, for `ngram_key`).

```rust
use verbora_ngrams::ngrams_owned;

fn main() {
    let ids = [1_i64, 2, 3];
    assert_eq!(
        ngrams_owned(&ids, 2, None, Some(0)),
        vec![vec![1, 2], vec![2, 3], vec![3, 0]]
    );
}
```

### The pad symbol has the element's type

`ngrams` takes `Option<T>`, not `Option<&str>`, so a `&[String]` needs a
`String` pad symbol. Building a `&str` view first is usually both cheaper and
more convenient:

```rust
use verbora_ngrams::ngrams;

fn main() {
    let tokens: Vec<String> = vec![String::from("a"), String::from("b")];

    // A `&[String]` needs a `String` pad symbol.
    let grams = ngrams(&tokens, 2, Some(String::from("<s>")), None);
    assert_eq!(grams.len(), 2);

    // Usually better: take a `&str` view once, then pad with `&str`.
    let view: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let grams = ngrams(&view, 2, Some("<s>"), None);
    assert_eq!(grams.len(), 2);
}
```

### Streaming a corpus with one reused buffer

There is no `_into` variant in this crate, but the *tokenizer* side has one, and
combining it with `ngrams_iter` gives a loop that allocates nothing per document
beyond what the buffer already holds:

```rust
use verbora_core::BorrowingTokenizer;
use verbora_ngrams::{WordTokenizer, ngrams_iter};

fn main() {
    let corpus = ["the quick brown fox", "jumps over the lazy dog"];
    let tokenizer = WordTokenizer;

    // One buffer for the whole corpus. `tokenize_borrowed_into` appends rather
    // than clearing, so the caller clears it; the allocation is reused.
    let mut buf: Vec<&str> = Vec::new();
    let mut total = 0usize;
    for doc in corpus {
        buf.clear();
        tokenizer.tokenize_borrowed_into(doc, &mut buf);
        total += ngrams_iter(&buf, 2, None, None).count();
    }
    assert_eq!(total, 7);
}
```

### Counting without materialising

`ngrams_with_stats` builds a `Vec` of every n-gram *and* a frequency table. If
you only want counts, fold the lazy iterator yourself and skip the `Vec`:

```rust
use verbora_ngrams::{ngram_key, ngrams_iter};
use std::collections::HashMap;

fn main() {
    let tokens = ["a", "b", "a", "b", "a"];
    let mut counts: HashMap<String, u32> = HashMap::new();
    for gram in ngrams_iter(&tokens, 2, None, None) {
        *counts.entry(ngram_key(&gram)).or_default() += 1;
    }
    assert_eq!(counts["(a, b)"], 2);
}
```

You lose the first-seen insertion order and the `Nr` count-of-counts map that
`ngrams_with_stats` provides, which is exactly the trade you wanted to make.

## Performance characteristics

Every entry point is **O(len × n)** in time: `window_count(len, n)` windows are
produced, and each window is either a borrow (constant time) or a copy of up to
`n` elements. Padding adds a fixed `2(n-1)` tuples independent of `len`.

<div class="callout callout-note">
<strong>Note.</strong> <code>verbora-ngrams</code> ships <strong>no
<code>par_*</code> API</strong> of its own. Unlike thirteen other Verbora
crates, its Rayon candidacy has <strong>not been separately evaluated</strong>
— treat this as "not yet evaluated," not "rejected." There is also <strong>no <code>_into</code> variant</strong> and
<strong>no batch entry point</strong>. The only batch-shaped thing reachable
from here is <code>verbora_core::Tokenizer::tokenize_batch</code>, which
<code>WordTokenizer</code> inherits: its default body is
<code>texts.iter().map(|t| self.tokenize(t.as_ref())).collect()</code>, a plain
sequential map that does <em>not</em> reuse a buffer despite the doc comment
claiming otherwise. See
<a href="../performance/batch-vs-streaming">Batch vs streaming</a> and
<a href="../performance/parallelism">Parallelism</a>.
</div>

All the entry points are free functions with no interior state, so a caller can
parallelise across documents with `rayon` themselves. The one thing to avoid is
`ngrams_str` / `tokenize`, which read the process-global tokenizer; use
`ngrams_str_with` so each worker's tokenizer is explicit.

```rust  ignore
// Needs `rayon` as a dependency of YOUR crate — verbora-ngrams has no
// `parallel` feature and ships no par_* entry point for this operation.
use rayon::prelude::*;

let all: Vec<Vec<Vec<String>>> = corpus
    .par_iter()
    .map(|doc| verbora_ngrams::ngrams_str_with(&tokenizer, doc, 2, None, None))
    .collect();
```

See [Parallelism](../performance/parallelism.md).

### What is measured

`crates/verbora-ngrams/benches/ngrams.rs` is a Criterion suite covering
sequence length (16 / 256 / 4,096 / 20,000), borrowed versus materialised
collection, `n` and padding, statistics, string input, and the ZH paths. A
baseline for the same shapes, measured against a widely-used JavaScript NLP
library, was recorded once.

Two figures from that baseline are worth quoting because they justify an API
recommendation directly. Over the same 4,096-word input:

| Operation (JavaScript NLP library) | ns/op |
|---|---:|
| `tokenize` alone | 152,211 |
| `ngrams_str` (tokenize + window) | 201,660 |
| windowing pre-tokenized input | 48,550 |

Tokenization is roughly three quarters of the cost of the string entry point.
That is a property of the algorithm, not of the runtime, and it is why a caller
who already has tokens should never route through `ngrams_str`.

<div class="callout callout-note">
<strong>Note.</strong> No side-by-side comparison of Verbora against that
library has been published for this crate. <code>docs/PERFORMANCE.md</code>
currently covers the 26 <code>verbora-distance</code> benchmarks only.
Everything else on this page is asymptotics and allocation behaviour read
from the source. See <a href="../benchmarks/">Benchmarks</a>.
</div>

## Allocation behaviour

Read alongside [Allocation](../performance/allocation.md) and
[Zero-copy](../performance/zero-copy.md).

| Call | Per-call allocations | Per-window allocations |
|---|---|---|
| `ngrams_iter` | none | none for windows; one `Vec` per pad tuple |
| `ngrams` | one `Vec`, reserved once from the exact `size_hint` | as above |
| `ngrams_owned` | one outer `Vec` | one `Vec` + one `T::clone` per element, for **every** tuple |
| `ngrams_with_stats` | n-gram `Vec` (exact capacity), `HashMap`, slot `Vec`, `frequencies` `Vec`, `BTreeMap` | one key `String` per n-gram |
| `ngrams_str` | one `Vec<String>` + one `String` per token | one `Vec` + `n` `String`s per tuple |
| `ngrams_str_with_stats` | as `ngrams_str`, plus the statistics containers | plus a full copy from `into_owned()` |
| `split_lossy` | one `Vec`, capacity = `text.len()` **bytes** | none — every element is `Cow::Borrowed` |
| `ngrams_zh` | one split `Vec` + one windows `Vec` | one `Vec` per n-gram; no per-character `String` |
| `ngrams_zh_utf16` | one `Vec<&[u16]>` of elements + one windows `Vec` | one `Vec` per n-gram; code units never copied |

Two details worth knowing:

- **`ngrams_with_stats` allocates a key `String` for every n-gram, not for every
  distinct n-gram.** `index.entry(ngram_key(&gram))` builds the key
  unconditionally; on a hit the freshly built `String` is dropped again. The keys
  that survive are *moved* into `frequencies` at the end, never cloned.
- **`ngrams_zh` does not allocate per character.** `Cow::into_owned` on a
  `Cow<[Cow<str>]>` clones the element `Cow`s, and cloning a `Cow::Borrowed` is
  a pointer copy. So a ZH n-gram costs one `Vec`, not `n` `String`s.

## Unicode and language notes

**The default tokenizer's character class is `[A-Za-zА-Яа-я0-9_]`, literally.**
Not "alphanumeric". Two consequences that are easy to assume away if you expect
Unicode-aware behavior, both recorded in the fixtures:

- Accented Latin letters are **separators**: `café` tokenizes as `caf`, `naïve`
  as `na` + `ve`, `Ångström` as `ngstr` + `m`.
- The Cyrillic range is exactly `U+0410..=U+042F` and `U+0430..=U+044F`, which
  **excludes** `Ё` (U+0401) and `ё` (U+0451). `Ёж ёлка` tokenizes as `ж` + `лка`.
  Greek is not in the class at all, so `Ελλάδα` yields no tokens.

If that is not what you want, do not fight it — pass a real tokenizer to
`ngrams_str_with`. See [Tokenizers](./tokenizers.md).

**`zh` splits per UTF-16 code unit, not per character.** Covered above.

**Everything else is code-point agnostic.** The engine never inspects an
element's contents, only its position, so elements can be any `Clone` type.

## Common mistakes

**Assuming every tuple has `n` elements.** Padded tuples can be shorter — see
[Padding semantics](#padding-semantics). Index with `get`, or filter the padded
tuples out.

**Using `Some("")` when you meant "no padding".** `None` disables padding;
`Some("")` pads with empty strings. This distinction bites callers coming from
contexts where an empty string is often used as a "nothing" sentinel.

**Calling `ngrams_str` in a loop over a corpus you already tokenized.** Each
call runs a full tokenization and then copies every element of every tuple.
Tokenize once, then use the slice API.

**Calling `stats.frequency(…)` inside a loop.** It is a linear scan. Build a
`HashMap` from `stats.frequencies` once.

**Calling `set_tokenizer` from a test.** Rust runs tests in one process on many
threads; the binding is process-wide. Use `ngrams_str_with`, or serialise on a
mutex the way `crates/verbora-ngrams/src/tokenizer.rs` does internally.

**Expecting `ngrams_zh` to round-trip astral characters.** It renders each half
of a torn surrogate pair as `U+FFFD` — correct shape, lossy elements. Use
`code_units` + `ngrams_zh_utf16` when exactness matters.

**Expecting `multrigrams` to differ from `ngrams`.** It does not. It is an exact
alias.

## Related

- [Choosing: n-grams](../choosing/ngrams.md) — the lazy-vs-materialised decision
  in full
- [Tokenizers](./tokenizers.md) — what to pass to `ngrams_str_with`
- [Core traits](./core.md) — `Tokenizer`, `BorrowingTokenizer`
- [Iterator vs `_into`](../performance/iterator-vs-into.md)
- [Zero-copy](../performance/zero-copy.md) · [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md) · [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)

## API reference

### `verbora_ngrams::engine` (all re-exported at the crate root)

| Item | Signature |
|---|---|
| `ngrams_iter` | `fn ngrams_iter<T: Clone>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> NGramIter<'_, T>` |
| `NGramIter` | `struct NGramIter<'a, T>`; `Iterator<Item = Cow<'a, [T]>>` + `ExactSizeIterator` + `FusedIterator`; derives `Debug`, `Clone` |
| `ngrams` | `fn ngrams<T: Clone>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Cow<'_, [T]>>` |
| `ngrams_owned` | `fn ngrams_owned<T: Clone>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Vec<T>>` |
| `bigrams` | `fn bigrams<T: Clone>(sequence: &[T], start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Cow<'_, [T]>>` |
| `trigrams` | `fn trigrams<T: Clone>(sequence: &[T], start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Cow<'_, [T]>>` |
| `multrigrams` | `fn multrigrams<T: Clone>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Cow<'_, [T]>>` — exact alias of `ngrams` |

### `verbora_ngrams::stats` (all re-exported at the crate root)

| Item | Signature |
|---|---|
| `NGramStats` | `struct NGramStats<'a, T: Clone> { ngrams: Vec<Cow<'a, [T]>>, frequencies: Vec<(String, u64)>, nr: BTreeMap<u64, u64>, number_of_ngrams: u64 }`; derives `Debug`, `Clone`, `PartialEq`, `Eq` |
| `NGramStats::frequency` | `fn frequency(&self, key: &str) -> u64` — linear scan |
| `NGramStats::into_owned` | `fn into_owned(self) -> NGramStats<'static, T>` |
| `ngram_key` | `fn ngram_key<T: fmt::Display>(ngram: &[T]) -> String` — `[]` renders as `")"` |
| `ngrams_with_stats` | `fn ngrams_with_stats<T: Clone + fmt::Display>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> NGramStats<'_, T>` |
| `bigrams_with_stats` | `fn bigrams_with_stats<T: Clone + fmt::Display>(sequence: &[T], start_symbol: Option<T>, end_symbol: Option<T>) -> NGramStats<'_, T>` |
| `trigrams_with_stats` | `fn trigrams_with_stats<T: Clone + fmt::Display>(sequence: &[T], start_symbol: Option<T>, end_symbol: Option<T>) -> NGramStats<'_, T>` |
| `multrigrams_with_stats` | `fn multrigrams_with_stats<T: Clone + fmt::Display>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> NGramStats<'_, T>` |

### `verbora_ngrams::text`

All re-exported at the crate root **except `ngrams_of_tokens`**.

| Item | Signature |
|---|---|
| `ngrams_str` | `fn ngrams_str(text: &str, n: usize, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>` |
| `ngrams_str_with` | `fn ngrams_str_with<T: NGramTokenizer + ?Sized>(tokenizer: &T, text: &str, n: usize, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>` |
| `bigrams_str` | `fn bigrams_str(text: &str, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>` |
| `trigrams_str` | `fn trigrams_str(text: &str, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>` |
| `multrigrams_str` | `fn multrigrams_str(text: &str, n: usize, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>` |
| `ngrams_str_with_stats` | `fn ngrams_str_with_stats(text: &str, n: usize, start_symbol: Option<&str>, end_symbol: Option<&str>) -> NGramStats<'static, String>` |
| `bigrams_str_with_stats` | `fn bigrams_str_with_stats(text: &str, start_symbol: Option<&str>, end_symbol: Option<&str>) -> NGramStats<'static, String>` |
| `trigrams_str_with_stats` | `fn trigrams_str_with_stats(text: &str, start_symbol: Option<&str>, end_symbol: Option<&str>) -> NGramStats<'static, String>` |
| `multrigrams_str_with_stats` | `fn multrigrams_str_with_stats(text: &str, n: usize, start_symbol: Option<&str>, end_symbol: Option<&str>) -> NGramStats<'static, String>` |
| `ngrams_of_tokens` | `fn ngrams_of_tokens<'a>(tokens: &'a [&'a str], n: usize, start_symbol: Option<&'a str>, end_symbol: Option<&'a str>) -> Vec<Cow<'a, [&'a str]>>` — **not** re-exported at the root |

### `verbora_ngrams::tokenizer` (all re-exported at the crate root)

| Item | Signature |
|---|---|
| `NGramTokenizer` | `trait NGramTokenizer: Send + Sync { fn tokenize_text(&self, text: &str) -> Vec<String>; }`, blanket-implemented for every `Tokenizer + Send + Sync + ?Sized` |
| `WordTokenizer` | `struct WordTokenizer;` — implements `Tokenizer` and `BorrowingTokenizer`; derives `Debug`, `Default`, `Clone`, `Copy`, `PartialEq`, `Eq` |
| `FnTokenizer` | `struct FnTokenizer<F>(pub F);` — implements `Tokenizer` for `F: Fn(&str) -> Vec<String>`; derives `Debug`, `Clone`, `Copy` |
| `set_tokenizer` | `fn set_tokenizer<T: NGramTokenizer + 'static>(tokenizer: T)` — process-wide |
| `reset_tokenizer` | `fn reset_tokenizer()` — clears the override, restoring the default `WordTokenizer` |
| `current_tokenizer` | `fn current_tokenizer() -> Option<Arc<dyn NGramTokenizer>>` — `None` while the default is in force |
| `tokenize` | `fn tokenize(text: &str) -> Vec<String>` — uses the global binding |

### `verbora_ngrams::zh`

Only `ngrams_zh`, `bigrams_zh` and `trigrams_zh` are re-exported at the crate
root.

| Item | Signature |
|---|---|
| `code_units` | `fn code_units(text: &str) -> Vec<u16>` |
| `split_lossy` | `fn split_lossy(text: &str) -> Vec<Cow<'_, str>>` |
| `ngrams_zh` | `fn ngrams_zh<'a>(text: &'a str, n: usize, start_symbol: Option<&'a str>, end_symbol: Option<&'a str>) -> Vec<Vec<Cow<'a, str>>>` |
| `bigrams_zh` | `fn bigrams_zh<'a>(text: &'a str, start_symbol: Option<&'a str>, end_symbol: Option<&'a str>) -> Vec<Vec<Cow<'a, str>>>` |
| `trigrams_zh` | `fn trigrams_zh<'a>(text: &'a str, start_symbol: Option<&'a str>, end_symbol: Option<&'a str>) -> Vec<Vec<Cow<'a, str>>>` |
| `ngrams_zh_utf16` | `fn ngrams_zh_utf16<'a>(units: &'a [u16], n: usize, start_symbol: Option<&'a [u16]>, end_symbol: Option<&'a [u16]>) -> Vec<Vec<&'a [u16]>>` |
| `bigrams_zh_utf16` | `fn bigrams_zh_utf16<'a>(units: &'a [u16], start_symbol: Option<&'a [u16]>, end_symbol: Option<&'a [u16]>) -> Vec<Vec<&'a [u16]>>` |
| `trigrams_zh_utf16` | `fn trigrams_zh_utf16<'a>(units: &'a [u16], start_symbol: Option<&'a [u16]>, end_symbol: Option<&'a [u16]>) -> Vec<Vec<&'a [u16]>>` |

### Not present in this crate

No `Result` anywhere — nothing in this crate is fallible, and every `n`,
sequence length and pad symbol is a defined input; `ngram_key` returns `")"`
for the empty n-gram rather than panicking (see above). The one reachable
panic is lock poisoning in `set_tokenizer` / `reset_tokenizer` /
`current_tokenizer`, described under
[The process-global tokenizer](#the-process-global-tokenizer).

No `_into` variant; no batch entry point; no parallel entry point; no
tokenizer-override function for `zh`.
