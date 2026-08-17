# Choosing an n-gram API

`verbora-ngrams` gives you three ways to ask for the same windows, two ways to
supply the input, two ways to pick the tokenizer, and an optional frequency
table bolted on top. That is a lot of doors for one idea, so this page is
ordered by how much the choice actually costs you.

The spine of it is one decision — **lazy or materialised** — and everything else
is a consequence of where your input already is. For the full surface, see
[Features: n-grams](../features/ngrams.md).

<div class="callout callout-note">
<strong>Note.</strong> Blocks marked <code>rust,ignore</code> on this page do not
compile on purpose, and the prose says why each one fails. Every other Rust block
is a complete program that compiles and whose assertions pass; they are checked
by the site-wide snippet harness (<code>python3 site/check-snippets.py</code>).
</div>

## The decision that matters: lazy or materialised

### Comparison table

| API | Execution | Result | Windows copied | Random access | May outlive the sequence | Allocations |
|---|---|---|:--:|:--:|:--:|---|
| `ngrams_iter` | lazy | `NGramIter<'a, T>` | ❌ | ❌ | ❌ | none for windows; one `Vec` per pad tuple |
| `ngrams` | eager | `Vec<Cow<'a, [T]>>` | ❌ | ✅ | ❌ | one `Vec` + one per pad tuple |
| `ngrams_owned` | eager | `Vec<Vec<T>>` | ✅ | ✅ | ✅ | one `Vec` per n-gram + one `T::clone` per element |

All three produce **exactly the same n-grams in exactly the same order**. They
differ only in when the work happens and who owns the result.

### Decision tree

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

### `ngrams_iter()` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a> <a class="badge badge-cow" href="../performance/zero-copy">COW</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy — nothing happens until you advance it</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Cow&lt;'a, [T]&gt;</code>: <code>Borrowed</code> windows, <code>Owned</code> pad tuples</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None per window; one <code>Vec</code> per padded tuple (at most <code>2(n-1)</code>)</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A — there is no output buffer</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Early termination, streaming a corpus, folding into a counter</span></div>
</div>

`NGramIter` implements `ExactSizeIterator`, so `len()` is available without
consuming anything — you do not have to collect just to count. It also
implements `FusedIterator` and derives `Clone`, so you can cheaply restart a
scan. It is **not** a `DoubleEndedIterator`.

### `ngrams()` <a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — <code>ngrams_iter(…).collect()</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Cow&lt;'a, [T]&gt;&gt;</code>, borrowing the token slice</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>, reserved once from the iterator's exact <code>size_hint</code>; one <code>Vec</code> per padded tuple</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">The default: indexable windows whose token slice outlives them</span></div>
</div>

This is the middle road and usually the right one. You pay for exactly one
`Vec`; the windows themselves are still pointer-and-length views into your
tokens. Nothing is copied.

### `ngrams_owned()` <a class="badge badge-owned" href="../performance/allocation">OWNED</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — <code>ngrams_iter(…).map(Cow::into_owned).collect()</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Vec&lt;T&gt;&gt;</code>, detached from the sequence — matches the reference's <code>string[][]</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One outer <code>Vec</code>, one <code>Vec</code> per n-gram, one <code>T::clone</code> per element</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Tuples that must outlive the token slice</span></div>
</div>

The expensive one. With `T = String` and `n = 2` over `k` tokens it allocates
roughly `2k` strings plus `k` vectors. Use it when the borrow checker tells you
to, not by default.

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

## Worked example: where laziness actually wins

Laziness pays when the caller **never needs the rest of the sequence**. Then the
windows you skip are never built, and the outer `Vec` is never allocated at all.

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

Written with `ngrams()` instead, each of these builds a `Vec` holding a
pointer-and-length pair for **every** window in the document, and then reads
five of them. On a nine-word sentence that is irrelevant. On a 20,000-token
document it is 20,000 fat pointers written to the heap and thrown away.

**Be precise about what you save.** Laziness does not make producing a window
cheaper — a window is a borrow either way. What it saves is:

- the outer `Vec` allocation and the writes into it, and
- the windows past your stopping point.

If you consume every n-gram anyway, `ngrams()` costs one extra allocation and
gives you `len()`, indexing and slicing in return. Take it.

### Folding, not collecting

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
calls `ngrams_owned` on the result — because the tokens are created and dropped
inside the call, so nothing can be borrowed from them.

| | `ngrams(&tokens, …)` | `ngrams_str(text, …)` |
|---|---|---|
| Input | `&[T]` | `&str` |
| Tokenizes | never | every call |
| Output | `Vec<Cow<'a, [T]>>` | `Vec<Vec<String>>` |
| Windows copied | ❌ | ✅ — `n` `String`s per tuple |
| Reads the process-global tokenizer | ❌ | ✅ |

So a caller that already has tokens should never go through `ngrams_str`. It
would re-tokenize text it does not have, and copy every element of every tuple
on the way out.

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

The recorded reference baseline puts numbers on the shape of this, over a
4,096-word input:

| Reference operation | ns/op |
|---|---:|
| `tokenize` alone | 152,211 |
| `ngrams_str` (tokenize + window) | 201,660 |
| windowing pre-tokenized input | 48,550 |

Tokenizing is about three quarters of the string entry point's cost. That ratio
is a property of the algorithm rather than of the runtime, which is why the
pre-tokenized API exists at all.

<div class="callout callout-note">
<strong>Note.</strong> No Rust-versus-the reference comparison has been published
for this crate — <code>docs/PERFORMANCE.md</code> covers the 26
<code>verbora-distance</code> benchmarks only. The table above is the recorded
the reference baseline, quoted to show the <em>internal</em> ratio between
tokenizing and windowing. See <a href="../benchmarks/">Benchmarks</a>.
</div>

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
| Reproduces `NGrams.setTokenizer` semantics | ✅ | n/a |

**Prefer `ngrams_str_with`.** The global exists because the reference's
`setTokenizer` rebinds a module-level variable for the entire process, and the
reference's own spec suite depends on that being observable. Verbora reproduces
it rather than quietly making the tokenizer a parameter — but that is a compatibility
obligation, not a recommendation.

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
variant. If you must use the global in tests, serialise on a mutex — the crate's
own tests do exactly that. See
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

The key `String` is built for every n-gram, not for every *distinct* n-gram —
`index.entry(ngram_key(&gram))` constructs it unconditionally and drops it again
on a repeat. So if you only need the windows, do not ask for the statistics.

Conversely, if you need counts but not the list, `ngrams_with_stats` is still
building the full `Vec<Cow<…>>` of n-grams as a side effect. Folding
`ngrams_iter` into your own map (shown above) skips that, at the cost of
the reference's insertion order and the `Nr` map — which is usually the trade you
wanted.

```text
I want frequency information
│
├── I need the reference's exact {ngrams, frequencies, Nr, numberOfNgrams} shape
│      └── ngrams_with_stats() / bigrams_with_stats() / trigrams_with_stats()
│
├── I need counts only, my own key format is fine
│      └── ngrams_iter() folded into a HashMap
│
└── I need many lookups by key
       └── ngrams_with_stats(), then index `frequencies` into a HashMap once
          (NGramStats::frequency is a linear scan by design)
```

## Chinese: which door

`zh` splits per **UTF-16 code unit**, matching the reference's `String#split('')`,
which is not the same as per character.

| API | Element type | Astral input | Use when |
|---|---|---|---|
| `zh::ngrams_zh` | `Cow<'a, str>` | correct shape; torn surrogate halves render as `U+FFFD` | BMP text — all of CJK — and you want the reference's shape |
| `zh::ngrams_zh_utf16` | `&'a [u16]` | exact, round-trippable | the input may contain astral characters |
| `zh::split_lossy` + `ngrams` | `Cow<'a, str>` | as `ngrams_zh` | large documents: split once, then window with borrowed tuples |

For **array** input there is no choice to make: `NGramsZH.ngrams(array, …)` and
`NGrams.ngrams(array, …)` are the same function in the reference, so use
`ngrams`.

## What this crate does not have

Three shapes you will find elsewhere in Verbora — or expect from other
libraries — are simply absent here, and it is better to know that up front than
to search for them.

- **No parallel API in this crate, and not yet evaluated either way.** Unlike
  the thirteen crates that now ship an opt-in `par_*_batch` behind a
  `parallel` feature, `verbora-ngrams` was not separately assessed for one in
  that pass — treat its absence here as "not yet evaluated," not as a
  rejection. Every entry point is a free function over borrowed input with no
  interior state, so you can parallelise across documents yourself today.
  Avoid `ngrams_str` and `tokenize` in worker threads because they read the
  process-global binding; use `ngrams_str_with` so each worker's tokenizer is
  explicit.

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

- **No `_into` API.** There is no `ngrams_into(&tokens, n, …, &mut out)`. The
  lazy iterator is the memory-frugal path instead: it lets you decide where the
  output goes without the crate owning a buffer. Buffer reuse *is* available one
  layer down, on the tokenizer (`tokenize_borrowed_into`), which is where the
  per-document allocations actually are. See
  [Iterator vs `_into`](../performance/iterator-vs-into.md).

- **No batch API.** There is no `ngrams_batch`. The only batch-shaped thing
  reachable from this crate is `verbora_core::Tokenizer::tokenize_batch`, which
  `WordTokenizer` inherits as a provided method; its default body is
  `texts.iter().map(|t| self.tokenize(t.as_ref())).collect()` — a plain
  sequential map that does **not** reuse a buffer, despite the trait's doc
  comment saying it does. Loop over your documents yourself. See
  [Batch vs streaming](../performance/batch-vs-streaming.md).

## Summary

| If you… | Call |
|---|---|
| stop early, or fold windows into something else | `ngrams_iter` |
| want indexable windows and the tokens outlive them | `ngrams` (or `bigrams` / `trigrams`) |
| need the tuples after the tokens are gone | `ngrams_owned` |
| need the reference's `{ngrams, frequencies, Nr, numberOfNgrams}` | `ngrams_with_stats` |
| have a string and control the tokenizer | `ngrams_str_with` |
| have a string and are porting `NGrams.setTokenizer` behaviour | `ngrams_str` |
| have Chinese BMP text | `zh::ngrams_zh` |
| have Chinese text that may contain astral characters | `zh::code_units` + `zh::ngrams_zh_utf16` |
| are porting `multrigrams` | `multrigrams` — an exact alias of `ngrams` |

## Related

- [Features: n-grams](../features/ngrams.md) — the full surface and the padding quirks
- [Choosing an API](./index.md) · [API shapes](./api-shapes.md) · [Choosing a tokenizer](./tokenization.md)
- [Performance](../performance/index.md) · [Iterator vs `_into`](../performance/iterator-vs-into.md)
- [Zero-copy](../performance/zero-copy.md) · [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md) · [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)
