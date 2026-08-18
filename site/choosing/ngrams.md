# Choosing an n-gram API

`verbora-ngrams` gives you three ways to ask for the same windows, two ways to
supply the input, two ways to pick the tokenizer, and an optional frequency
table on top. The spine is one decision — **lazy or materialised** — and
everything else follows from where your input already is.

For the full surface, see [Features: n-grams](../features/ngrams.md).

<div class="callout callout-note">
<strong>Note.</strong> Blocks marked <code>rust,ignore</code> on this page do not
compile on purpose, and the prose says why. Every other Rust block is a complete
program whose assertions pass.
</div>

## The decision that matters: lazy or materialised

| API | Execution | Result | Windows copied | Random access | May outlive the sequence | Allocations |
|---|---|---|:--:|:--:|:--:|---|
| `ngrams_iter` | lazy | `NGramIter<'a, T>` | ❌ | ❌ | ❌ | none for windows; one `Vec` per pad tuple |
| `ngrams` | eager | `Vec<Cow<'a, [T]>>` | ❌ | ✅ | ❌ | one `Vec` + one per pad tuple |
| `ngrams_owned` | eager | `Vec<Vec<T>>` | ✅ | ✅ | ✅ | one `Vec` per n-gram + one `T::clone` per element |

All three produce **exactly the same n-grams in exactly the same order**. They
differ only in when the work happens and who owns the result.

| If you… | Call |
|---|---|
| stop early (`take` / `find` / `any` / `position`), or fold windows into a counter | `ngrams_iter()` |
| consume everything and want indexable windows — **the default** | `ngrams()` |
| need the tuples to outlive the token slice (returned, stored, cached, sent to another thread) | `ngrams_owned()` |
| also need frequencies or a count-of-counts | `ngrams_with_stats()` |
| want `n == 2` or `n == 3` | `bigrams()` / `trigrams()` — the same call with `n` fixed |

`multrigrams()` is an exact alias of `ngrams()`.

### `ngrams_iter()` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a> <a class="badge badge-cow" href="../performance/zero-copy">COW</a>

Nothing happens until you advance it. Windows are `Cow::Borrowed`; only padded
tuples allocate, at most `2(n-1)` of them.

`NGramIter` implements `ExactSizeIterator`, so `len()` is available without
consuming anything — you do not have to collect just to count. It also
implements `FusedIterator` and derives `Clone`, so you can cheaply restart a
scan. It is **not** a `DoubleEndedIterator`.

### `ngrams()` <a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

`ngrams_iter(…).collect()`, with the outer `Vec` reserved once from the
iterator's exact `size_hint`. This is the middle road and usually the right one:
you pay for exactly one `Vec`, and the windows themselves are still
pointer-and-length views into your tokens. Nothing is copied.

### `ngrams_owned()` <a class="badge badge-owned" href="../performance/allocation">OWNED</a>

`ngrams_iter(…).map(Cow::into_owned).collect()` — a plain owned nested vector,
detached from the sequence. The expensive one: with `T = String` and `n = 2`
over `k` tokens it allocates roughly `2k` strings plus `k` vectors. Use it when
the borrow checker tells you to, not by default.

**That is a real signal, not a nuisance.** This does not compile:

```rust  ignore
use verbora_ngrams::{ngrams, tokenize};
use std::borrow::Cow;

// error[E0515]: cannot return value referencing local variable `tokens`
fn bigrams_of(text: &str) -> Vec<Cow<'_, [String]>> {
    let tokens = tokenize(text);
    ngrams(&tokens, 2, None, None) // `tokens` is dropped when this returns
}
```

The fix is either to hoist `tokens` into the caller — keeping the zero-copy
path — or to materialise:

```rust
use verbora_ngrams::{ngrams_owned, tokenize};

fn bigrams_owned_of(text: &str) -> Vec<Vec<String>> {
    let tokens = tokenize(text);
    ngrams_owned(&tokens, 2, None, None)
}

fn main() {
    assert_eq!(bigrams_owned_of("a b c").len(), 2);
}
```

## Where laziness actually wins

Laziness does not make producing a window cheaper — a window is a borrow either
way. What it saves is the outer `Vec` and the windows past your stopping point.

```rust
use verbora_ngrams::ngrams_iter;

fn main() {
    let tokens: Vec<&str> = "the quick brown fox jumps over the lazy dog"
        .split(' ')
        .collect();

    // Only five windows are ever produced, regardless of how long `tokens` is.
    let first_five: Vec<Vec<&str>> = ngrams_iter(&tokens, 2, None, None)
        .take(5)
        .map(|gram| gram.to_vec())
        .collect();
    assert_eq!(first_five.len(), 5);

    // Stops at the first match; the tail of the sequence is never windowed.
    let hit = ngrams_iter(&tokens, 2, None, None).find(|gram| gram[0] == "brown");
    assert_eq!(hit.as_deref(), Some(&["brown", "fox"][..]));

    // A membership test needs no Vec at all.
    let has_repeat = ngrams_iter(&tokens, 2, None, None).any(|gram| gram[0] == gram[1]);
    assert!(!has_repeat);
}
```

On a nine-word sentence the saving is irrelevant. On a 20,000-token document it
is 20,000 fat pointers written to the heap and thrown away. If you consume every
n-gram anyway, `ngrams()` costs one extra allocation and gives you `len()`,
indexing and slicing in return — take it.

The same argument applies when you consume everything but never need the list —
a frequency count, a maximum, a filter into some other structure:

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

<div class="callout callout-warn">
<strong>Careful.</strong> If you supply padding symbols, some tuples are shorter
than <code>n</code> — <code>ngrams(&amp;["a","b","c"], 5, Some("&lt;s&gt;"),
Some("&lt;/s&gt;"))</code> contains a two-element tuple. Indexing
<code>gram[1]</code> in a closure will panic on those. See
<a href="../features/ngrams#padding-semantics">Padding semantics</a>.
</div>

## Pre-tokenized slice vs string input

`ngrams` takes `&[T]`. `ngrams_str` takes `&str` and **tokenizes first**, then
calls `ngrams_owned` on the result — the tokens are created and dropped inside
the call, so nothing can be borrowed from them.

| | `ngrams(&tokens, …)` | `ngrams_str(text, …)` |
|---|---|---|
| Input | `&[T]` | `&str` |
| Tokenizes | never | every call |
| Output | `Vec<Cow<'a, [T]>>` | `Vec<Vec<String>>` |
| Windows copied | ❌ | ✅ — `n` `String`s per tuple |
| Reads the process-global tokenizer | ❌ | ✅ |

Splitting the text is the expensive half; sliding a window over a slice you
already have is close to free. A caller that already has tokens should never go
through `ngrams_str`.

```rust
use verbora_ngrams::{bigrams, ngrams_str, tokenize, trigrams};
use std::borrow::Cow;

fn main() {
    let text = "the quick brown fox";

    // Two calls, two full tokenizations, fully owned output.
    let two = ngrams_str(text, 2, None, None);
    let three = ngrams_str(text, 3, None, None);
    assert_eq!((two.len(), three.len()), (3, 2));

    // One tokenization; both results borrow from `tokens`, nothing is copied.
    let tokens = tokenize(text);
    let two = bigrams(&tokens, None, None);
    let three = trigrams(&tokens, None, None);
    assert_eq!((two.len(), three.len()), (3, 2));
    assert!(matches!(two[0], Cow::Borrowed(_)));
}
```

### If your loop is over documents

Tokenize into one reused buffer and window lazily. `WordTokenizer` implements
`verbora_core::BorrowingTokenizer`, so the tokens are slices of the document and
the buffer is the only allocation:

```rust
use verbora_core::BorrowingTokenizer;
use verbora_ngrams::{WordTokenizer, ngrams_iter};

fn main() {
    let corpus = ["the quick brown fox", "jumps over the lazy dog"];
    let tokenizer = WordTokenizer;

    // `tokenize_borrowed_into` appends rather than clearing, so the caller
    // clears; the allocation survives the loop.
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

See [Buffer reuse](../performance/allocation.md) and
[Zero-copy](../performance/zero-copy.md).

## Explicit tokenizer vs global tokenizer

`ngrams_str` reads a **process-global** tokenizer binding. `ngrams_str_with`
takes one as its first argument. They produce identical output for the same
tokenizer.

| | `ngrams_str(text, …)` | `ngrams_str_with(&tok, text, …)` |
|---|---|---|
| Tokenizer | whatever `set_tokenizer` last installed, process-wide | the one at the call site |
| Reads global state | ✅ | ❌ |
| Visible in the signature | ❌ | ✅ |
| Safe to call from a multi-threaded test suite | ⚠️ | ✅ |

**Prefer `ngrams_str_with`.** The global exists for callers who genuinely want
one process-wide tokenizer that can be rebound at runtime; pass the tokenizer
explicitly unless you need that.

```rust
use verbora_ngrams::{FnTokenizer, current_tokenizer, ngrams_str_with};

fn main() {
    let by_char = FnTokenizer(|s: &str| s.chars().map(String::from).collect());
    assert_eq!(
        ngrams_str_with(&by_char, "abc", 2, None, None),
        vec![vec!["a", "b"], vec!["b", "c"]]
    );

    // The process-global binding was never touched.
    assert!(current_tokenizer().is_none());
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> Rust runs a test binary's tests on several threads in
one process. A test that calls <code>set_tokenizer</code> changes what every
concurrently running test observes, and there is no scoped or thread-local
variant. If you must use the global in tests, serialise on a mutex. See
<a href="../features/ngrams#the-process-global-tokenizer">The process-global
tokenizer</a>.
</div>

The tokenizer to pass is anything implementing `verbora_core::Tokenizer` that is
`Send + Sync` — the whole of [Tokenizers](../features/tokenizers.md) qualifies
via a blanket implementation — or a closure wrapped in `FnTokenizer`. See also
[Choosing a tokenizer](./tokenization.md).

## With stats or without

`ngrams_with_stats` does everything `ngrams` does **and** builds a frequency
table, a count-of-counts, and a total. That is not free:

| | `ngrams` | `ngrams_with_stats` |
|---|---|---|
| N-grams produced | ✅ | ✅ (same `Vec<Cow<…>>`) |
| Frequency table in first-seen order | ❌ | ✅ `frequencies: Vec<(String, u64)>` |
| Count-of-counts | ❌ | ✅ `nr: BTreeMap<u64, u64>` |
| Extra allocations | none | one key `String` **per n-gram**, one `HashMap`, one slot `Vec`, one `frequencies` `Vec`, one `BTreeMap` |
| Extra work per n-gram | none | render the key, hash it, probe the map |

The key `String` is built for every n-gram, not for every *distinct* n-gram, and
dropped again on a repeat. So if you only need the windows, do not ask for the
statistics.

| If you need… | Call |
|---|---|
| the `{ngrams, frequencies, Nr, numberOfNgrams}` shape | `ngrams_with_stats()` / `bigrams_with_stats()` / `trigrams_with_stats()` |
| counts only, with your own key format | `ngrams_iter()` folded into a `HashMap` (shown above) |
| many lookups by key | `ngrams_with_stats()`, then index `frequencies` into a `HashMap` once — `NGramStats::frequency` is a linear scan by design |

## Chinese: which door

`zh` splits per **UTF-16 code unit**, which is not the same as per character.

| API | Element type | Astral input | Use when |
|---|---|---|---|
| `zh::ngrams_zh` | `Cow<'a, str>` | correct shape; torn surrogate halves render as `U+FFFD` | BMP text — all of CJK — and string elements are fine |
| `zh::ngrams_zh_utf16` | `&'a [u16]` | exact, round-trippable | the input may contain astral characters |
| `zh::split_lossy` + `ngrams` | `Cow<'a, str>` | as `ngrams_zh` | large documents: split once, then window with borrowed tuples |

For **array** input there is no choice to make: `zh` only changes how a *string*
is split into elements before windowing.

## What this crate does not have

- **No parallel API.** `verbora-ngrams` ships no `par_*_batch`. Every entry
  point is a free function over borrowed input with no interior state, so you
  can parallelise across documents yourself. Avoid `ngrams_str` and `tokenize`
  in worker threads because they read the process-global binding; use
  `ngrams_str_with` so each worker's tokenizer is explicit.

  ```rust  ignore
  // `rayon` is not a dependency of verbora-ngrams itself; add it to YOUR
  // crate to write this.
  use rayon::prelude::*;

  let all: Vec<Vec<Vec<String>>> = corpus
      .par_iter()
      .map(|doc| verbora_ngrams::ngrams_str_with(&tokenizer, doc, 2, None, None))
      .collect();
  ```

  See [Parallelism](../performance/parallelism.md).

- **No `_into` API.** The lazy iterator is the memory-frugal path instead.
  Buffer reuse *is* available one layer down, on the tokenizer
  (`tokenize_borrowed_into`), which is where the per-document allocations
  actually are. See [Iterator vs `_into`](../performance/iterator-vs-into.md).

- **No batch API.** The only batch-shaped thing reachable from this crate is
  `verbora_core::Tokenizer::tokenize_batch`, which `WordTokenizer` inherits as a
  provided method — a sequential map that does not reuse a buffer. Loop over
  your documents yourself. See
  [Batch vs streaming](../performance/batch-vs-streaming.md).

## Related

- [Features: n-grams](../features/ngrams.md) — the full surface and the padding quirks
- [Choosing an API](./index.md) · [API shapes](./api-shapes.md) · [Choosing a tokenizer](./tokenization.md)
- [Performance](../performance/index.md) · [Iterator vs `_into`](../performance/iterator-vs-into.md)
- [Zero-copy](../performance/zero-copy.md) · [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md) · [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)
