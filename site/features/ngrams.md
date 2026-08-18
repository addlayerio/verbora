# N-grams

`verbora-ngrams` turns a sequence into its sliding windows: bigrams, trigrams,
arbitrary `n`, optionally padded with start and end symbols, optionally with a
frequency table attached. One generic engine backs every entry point, so the
token path (`&str`), the numeric path (`i64`) and the Chinese code-unit path
(`&[u16]`) cannot drift apart.

The primitive is a lazy iterator that yields `Cow`: it borrows every unpadded
window and allocates only for the `2(n-1)` tuples that genuinely mix pad symbols
with sequence elements. Every other entry point is a wrapper that trades some of
that back for a more convenient shape.

<div class="callout callout-spec">
<strong>Specification status.</strong> Both n-gram APIs are documented and
test-pinned. Nothing in this crate is fallible: every <code>n</code>, sequence
length and pad symbol is a defined input.
<code>cargo test -p verbora-ngrams</code> runs <strong>43</strong> unit tests
and <strong>13</strong> doctests.
</div>

## When to use it

- You need bigrams, trigrams or arbitrary `n`-grams over tokens you already have.
- You need precise padding at the boundaries, including `n` longer than the
  sequence — see [Padding semantics](#padding-semantics).
- You want a frequency table and a Good–Turing count-of-counts (`Nr`) in one pass.
- You are windowing Chinese (or other per-character) text and need UTF-16
  code-unit boundaries rather than Unicode scalar values — see [Chinese: `zh`](#chinese-zh).
- You want to stream windows over a large corpus without allocating per window.

## When not to use it

- **You want a language model.** This crate produces windows and counts. There is
  no smoothing and no probability estimation; `nr` is the raw count-of-counts a
  Good–Turing estimator would consume, and the estimator is yours to write.
- **You want linguistic tokenization.** The default `WordTokenizer`'s character
  class is `[A-Za-zА-Яа-я0-9_]` and nothing else, so `café` becomes `caf`. Pick a
  real tokenizer from [Tokenizers](./tokenizers.md) and pass it to
  `ngrams_str_with`.
- **You want parallel, batched or buffer-reusing generation.** There is no
  `par_*` API, no `_batch` entry point and no `_into` variant. The lazy iterator
  is the memory-frugal path instead.

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

**Tokens already in hand** → `ngrams` for indexable windows, `ngrams_iter` to fold
or stop early, `ngrams_owned` only when the tuples must outlive the slice.
**A string in hand** → `ngrams_str_with(&tokenizer, …)`; if you will window the
same text twice, call `tokenize()` first and use the slice API. **Per-character
text** → `zh::ngrams_zh`. The full lazy-versus-materialised trade-off is on
[Choosing: n-grams](../choosing/ngrams.md).

<div class="callout callout-note">
<strong>Note.</strong> Blocks marked <code>rust,ignore</code> on this page are
bare signatures. Every other Rust block is a complete program that compiles and
whose assertions pass in CI.
</div>

## The slice API

```rust  ignore
pub fn ngrams_iter<T: Clone>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> NGramIter<'_, T>
pub fn ngrams<T: Clone>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Cow<'_, [T]>>
pub fn ngrams_owned<T: Clone>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Vec<T>>
pub fn bigrams<T: Clone>(sequence: &[T], start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Cow<'_, [T]>>
pub fn trigrams<T: Clone>(sequence: &[T], start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Cow<'_, [T]>>
pub fn multrigrams<T: Clone>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> Vec<Cow<'_, [T]>>
```

`NGramIter<'a, T>` yields `Cow<'a, [T]>` in three phases: `n - 1` left-padded
tuples (`Cow::Owned`) when `start_symbol` is `Some`, then every window of length
`n` (`Cow::Borrowed` — a pointer and a length), then `n - 1` right-padded tuples.
It implements `Iterator`, `ExactSizeIterator` and `FusedIterator`, derives `Debug`
and `Clone`, and is **not** `DoubleEndedIterator`. `size_hint` is exact, so
`len()` is free and `collect()` reserves once.

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

Nothing is computed until the iterator is advanced, so `.take(k)`, `.find(…)` and
`.any(…)` genuinely stop early.

- `ngrams` is `ngrams_iter(…).collect()`, and the recommended default for
  pre-tokenized input: random-access windows that still point back into
  `sequence`, so they cannot outlive it.
- `ngrams_owned` is `ngrams_iter(…).map(Cow::into_owned).collect()`. For
  `T = String` with `n = 2` over `k` tokens that is roughly `2k` string
  allocations — reach for it only when the lifetime demands it.
- `bigrams` and `trigrams` fix `n` at 2 and 3. **`multrigrams` is an exact alias
  of `ngrams`**; there is no behavioural difference to look for.

`T` is any `Clone` type — token IDs, part-of-speech tags, `i64`s — and only the
statistics family adds a bound (`fmt::Display`). The pad symbol has the element's
type, not `&str`, so a `&[String]` needs a `String` pad symbol; taking a
`Vec<&str>` view of the tokens first is usually both cheaper and more convenient.

## The string API

```rust  ignore
pub fn ngrams_str(text: &str, n: usize, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>
pub fn bigrams_str(text: &str, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>
pub fn trigrams_str(text: &str, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>
pub fn multrigrams_str(text: &str, n: usize, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>

pub fn ngrams_str_with<T: NGramTokenizer + ?Sized>(tokenizer: &T, text: &str, n: usize, start_symbol: Option<&str>, end_symbol: Option<&str>) -> Vec<Vec<String>>

// verbora_ngrams::text — NOT re-exported at the crate root
pub fn ngrams_of_tokens<'a>(tokens: &'a [&'a str], n: usize, start_symbol: Option<&'a str>, end_symbol: Option<&'a str>) -> Vec<Cow<'a, [&'a str]>>
```

The `_str` family tokenizes with the **process-global** tokenizer and then calls
`ngrams_owned`: the tokens are created and dropped inside the call, so nothing can
be borrowed from them. Output is fully owned `Vec<Vec<String>>`.

`ngrams_str_with` is the escape hatch from the global binding — identical output
and cost, minus the global read, and the tokenizer is visible at the call site.
Prefer it. There is no `bigrams_str_with`; pass `n = 2`.

`ngrams_of_tokens` is a `&str`-specialised restatement of `ngrams`, living in the
`text` module so the "I already have tokens" case is discoverable from the
string-input side. Reach it as `verbora_ngrams::text::ngrams_of_tokens`.

## Padding semantics

**Padding is driven by `Option`, not by emptiness.** `None` disables padding;
`Some("")` pads with empty strings — two different answers.

**Padded tuples are not always `n` elements long.** Each side clamps its sequence
half independently, and on the right-hand side an offset that runs past the front
of the sequence re-anchors to the *end*. Unpadded windows are always exactly `n`
long; only padded tuples can be short:

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

The degenerate values of `n` are all defined: **`n = 1` never pads** (there are
`n - 1` pad positions per side), **`n = 0` yields `len + 1` empty tuples**, and
**`n > len` yields no windows** — not one short window.

```rust
use verbora_ngrams::ngrams_owned;

fn main() {
    let seq = ["a", "b", "c"];

    // `Some("")` pads with empty strings; `None` does not pad at all.
    assert_eq!(
        ngrams_owned(&seq, 2, Some(""), None),
        vec![vec!["", "a"], vec!["a", "b"], vec!["b", "c"]]
    );

    // n == 1: symbols are supplied and ignored.
    assert_eq!(
        ngrams_owned(&seq, 1, Some("<s>"), Some("</s>")),
        vec![vec!["a"], vec!["b"], vec!["c"]]
    );
    // n == 0: four empty tuples for a three-element sequence.
    assert_eq!(ngrams_owned(&seq, 0, None, None), vec![Vec::<&str>::new(); 4]);
    // n > len: no windows.
    assert!(ngrams_owned(&seq, 5, None, None).is_empty());
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

pub fn ngrams_with_stats<T: Clone + fmt::Display>(sequence: &[T], n: usize, start_symbol: Option<T>, end_symbol: Option<T>) -> NGramStats<'_, T>
pub fn ngram_key<T: fmt::Display>(ngram: &[T]) -> String
impl<'a, T: Clone> NGramStats<'a, T> {
    pub fn frequency(&self, key: &str) -> u64;        // linear scan
    pub fn into_owned(self) -> NGramStats<'static, T>;
}
```

`bigrams_with_stats`, `trigrams_with_stats` and `multrigrams_with_stats` fix or
restate `n`; the four `*_str_with_stats` forms tokenize internally and return
`NGramStats<'static, String>`, which costs a second full copy. The
`T: fmt::Display` bound is what `ngram_key` needs.

Two fields have an **observable iteration order**, and it is not the same order:

- **`frequencies`** is a `Vec<(String, u64)>` kept in *first-seen* order.
- **`nr`** is a `BTreeMap<u64, u64>` keyed by a frequency count, so it iterates in
  ascending numeric order regardless of insertion order.

`number_of_ngrams` counts padded tuples too, so it equals `ngrams.len()`, not the
number of windows.

```rust
use verbora_ngrams::ngrams_with_stats;

fn main() {
    let seq = ["a", "b", "a", "b", "a"];
    let stats = ngrams_with_stats(&seq, 2, None, None);

    assert_eq!(stats.number_of_ngrams, 4);
    // First-seen order, not lexicographic.
    assert_eq!(
        stats.frequencies,
        vec![(String::from("(a, b)"), 2), (String::from("(b, a)"), 2)]
    );
    // Two distinct n-grams occur exactly twice.
    assert_eq!(stats.nr.get(&2), Some(&2));
}
```

`frequency()` is a **linear scan** over `frequencies`, returning `0` for a key
that never occurred — the type exists to preserve first-seen order, not to serve
lookups. For many lookups, build a `HashMap` from `stats.frequencies` once.

`into_owned()` detaches the n-grams from the sequence they were built over by
copying the windows that were still `Cow::Borrowed`; `frequencies`, `nr` and
`number_of_ngrams` move untouched. Use it when the token sequence is a temporary,
as in `ngrams_with_stats(&tokenize(text), 2, None, None).into_owned()`.

### `ngram_key`

`ngram_key` renders an n-gram as a parenthesized, comma-separated key: `["a","b"]`
becomes `"(a, b)"`. Two properties matter at the call site: **the empty n-gram
keys as `")"`**, not `"()"` (reachable whenever `n == 0`), and **keys are not
injective**, because a separator inside a token is not escaped.

```rust
use verbora_ngrams::ngram_key;

fn main() {
    assert_eq!(ngram_key(&["a", "b"]), "(a, b)");
    assert_eq!(ngram_key(&[1, 2]), "(1, 2)");
    assert_eq!(ngram_key(&["a, b"]), "(a, b)");   // not injective
    assert_eq!(ngram_key::<&str>(&[]), ")");      // the empty n-gram
}
```

## The process-global tokenizer

<span class="badge badge-global">GLOBAL STATE</span>

```rust  ignore
pub fn set_tokenizer<T: NGramTokenizer + 'static>(tokenizer: T)   // process-wide
pub fn reset_tokenizer()
pub fn current_tokenizer() -> Option<Arc<dyn NGramTokenizer>>     // None while the default is in force
pub fn tokenize(text: &str) -> Vec<String>
```

`verbora-ngrams` keeps a module-level binding holding the default tokenizer.
`set_tokenizer` rebinds it for the whole process until `reset_tokenizer` restores
the default `WordTokenizer`. The never-overridden path takes no lock at all, and
`tokenize` clones the `Arc` out of the lock before calling into it, so a tokenizer
that itself touches the global cannot deadlock.

What you can rely on across threads: installing a tokenizer while another thread
reads is well-defined — no data race, no torn value — but visibility is **not**
prompt, so during the brief window inside `set_tokenizer` / `reset_tokenizer` a
concurrent reader falls back to the default `WordTokenizer`. Lock poisoning is
the one reachable panic in the crate: if a thread panics while holding the write
lock, every later `set_tokenizer` / `reset_tokenizer` / `current_tokenizer` call
panics.

<div class="callout callout-warn">
<strong>Careful.</strong> This is shared mutable state with process-wide reach.
Rust runs tests on multiple threads in one process, so a test that calls
<code>set_tokenizer</code> changes what every concurrently running test observes.
Either serialise those tests on a mutex, or — better — do not call it and use
<code>ngrams_str_with</code>, which produces identical output, reads no global
and makes the tokenizer visible at the call site.
</div>

```rust
use verbora_ngrams::{
    FnTokenizer, current_tokenizer, ngrams_str, reset_tokenizer, set_tokenizer,
};

fn main() {
    assert!(current_tokenizer().is_none());   // still the default WordTokenizer
    assert_eq!(ngrams_str("a b", 2, None, None), vec![vec!["a", "b"]]);

    set_tokenizer(FnTokenizer(|s: &str| {
        s.split('-').map(str::to_owned).collect()
    }));
    // "a b" is now a single token, so there is no bigram at all.
    assert!(ngrams_str("a b", 2, None, None).is_empty());

    reset_tokenizer();
    assert!(current_tokenizer().is_none());
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
[`verbora_core::Tokenizer`](./core.md), which has a generic method and so cannot
itself go behind a `dyn`. The blanket impl means **any** `verbora_core::Tokenizer`
that is `Send + Sync` — including everything in [Tokenizers](./tokenizers.md) —
can be installed or passed to `ngrams_str_with` without implementing anything.

`WordTokenizer` is the default: gaps pattern `/[^A-Za-zА-Яа-я0-9_]+/`, implementing
both `Tokenizer` and `BorrowingTokenizer`. Because it borrows, it is the entry to
the fully zero-copy path — `tokenize_borrowed` slices the input, and `ngrams` then
slices those tokens. `FnTokenizer<F>(pub F)` adapts a closure
`Fn(&str) -> Vec<String>` for an ad hoc tokenizer without a named type.

## Chinese: `zh`

<span class="badge badge-utf16">UTF-16</span>

```rust  ignore
// Re-exported at the crate root: ngrams_zh, bigrams_zh, trigrams_zh.
// Everything else is verbora_ngrams::zh::…
pub fn code_units(text: &str) -> Vec<u16>
pub fn split_lossy(text: &str) -> Vec<Cow<'_, str>>
pub fn ngrams_zh<'a>(text: &'a str, n: usize, start_symbol: Option<&'a str>, end_symbol: Option<&'a str>) -> Vec<Vec<Cow<'a, str>>>
pub fn ngrams_zh_utf16<'a>(units: &'a [u16], n: usize, start_symbol: Option<&'a [u16]>, end_symbol: Option<&'a [u16]>) -> Vec<Vec<&'a [u16]>>
// bigrams_zh / trigrams_zh and bigrams_zh_utf16 / trigrams_zh_utf16 fix n at 2 and 3.
```

The `zh` module is the same engine with two changes: no statistics support, and
string input is split **per UTF-16 code unit** rather than through a tokenizer.
There is no tokenizer override, no `multrigrams_zh` and no module-level state.
Array input needs nothing from this module — window an already-split slice with
`ngrams` directly.

Splitting per code unit is observable. A single emoji is two UTF-16 code units,
so it is torn into its surrogate halves and each half becomes its own element;
combining marks separate too (`"éx"` in NFD yields `['e','◌́']`, `['◌́','x']`).

| Function | Element type | Astral input |
|---|---|---|
| `ngrams_zh` | `Cow<'a, str>` | correct **shape and positions**; each torn surrogate half renders as `U+FFFD` |
| `ngrams_zh_utf16` | `&'a [u16]` | **exact**, surrogates preserved and round-trippable |

`ngrams_zh` is exact for the entire Basic Multilingual Plane, which includes every
CJK character this module exists to serve; reach for `ngrams_zh_utf16` when astral
characters must round-trip. `code_units` is `text.encode_utf16().collect()`, and
`split_lossy` produces one `Cow::Borrowed` element per code unit — the cheap path
is to split once, then window with the borrowing engine.

```rust
use verbora_ngrams::ngrams;
use verbora_ngrams::zh::{code_units, ngrams_zh, ngrams_zh_utf16, split_lossy};
use std::borrow::Cow;

fn main() {
    assert_eq!(
        ngrams_zh("中文测试", 2, None, None),
        vec![vec!["中", "文"], vec!["文", "测"], vec!["测", "试"]]
    );

    // Astral input: three bigrams over four elements, torn halves as U+FFFD.
    let got = ngrams_zh("a👍b", 2, None, None);
    assert_eq!(got.len(), 3);
    assert_eq!(got[0][1], "\u{FFFD}");

    // Exact round-trip in code-unit space.
    let units = code_units("a👍b");
    let exact = ngrams_zh_utf16(&units, 2, None, None);
    assert_eq!(exact[1], vec![&[0xD83Du16][..], &[0xDC4Du16][..]]);

    // The cheap path: split once, then window with borrowed tuples.
    let split = split_lossy("中文测试");
    let windows = ngrams(&split, 2, None, None);
    assert!(matches!(windows[0], Cow::Borrowed(_)));
}
```

## Performance and allocation

Every entry point is **O(len × n)** in time: each window is either a borrow
(constant time) or a copy of up to `n` elements, and padding adds a fixed
`2(n-1)` tuples independent of `len`. The Allocations column of the
[comparison table](#choosing-the-right-api) is the per-call summary; three details
it cannot fit:

- **`ngrams` reserves once** from `NGramIter`'s exact `size_hint`, so the output
  `Vec` never regrows.
- **`ngrams_with_stats` allocates a key `String` for every n-gram**, not for every
  *distinct* n-gram; the keys that survive are moved into `frequencies`, never
  cloned.
- **`ngrams_zh` does not allocate per character.** Cloning a `Cow::Borrowed`
  element is a pointer copy, so a ZH n-gram costs one `Vec`, not `n` `String`s.
  `split_lossy`'s `Vec` reserves `text.len()` in **bytes**, which over-reserves
  roughly threefold for CJK.

There is no `_into` variant here, but the tokenizer side has one: pairing
`tokenize_borrowed_into` with `ngrams_iter` streams a corpus through a single
reused buffer, allocating nothing per document — see
[Recipes: streaming](../recipes/streaming.md). All entry points are free functions
with no interior state, so you can parallelise across documents with `rayon` in
your own crate; avoid `ngrams_str` and `tokenize` in a worker, since they read the
process-global tokenizer.

No n-gram benchmark results are published yet; the numbers above are allocation
counts read from the source. See [Benchmarks](../benchmarks/index.md) and
[Parallelism](../performance/parallelism.md).

## Common mistakes

**Assuming every tuple has `n` elements.** Padded tuples can be shorter — see
[Padding semantics](#padding-semantics). Index with `get`, or filter the padded
tuples out. Related: `None` disables padding, `Some("")` pads with empty strings.

**Calling `ngrams_str` in a loop over a corpus you already tokenized.** Each call
runs a full tokenization and then copies every element of every tuple. Tokenize
once, then use the slice API.

**Calling `stats.frequency(…)` inside a loop.** It is a linear scan. Build a
`HashMap` from `stats.frequencies` once.

**Calling `set_tokenizer` from a test.** The binding is process-wide and Rust
runs tests on many threads in one process. Use `ngrams_str_with`.

**Expecting `ngrams_zh` to round-trip astral characters.** It renders each half
of a torn surrogate pair as `U+FFFD` — correct shape, lossy elements. Use
`code_units` + `ngrams_zh_utf16` when exactness matters.

**Expecting the default tokenizer to be Unicode-aware.** Its class is
`[A-Za-zА-Яа-я0-9_]`, literally: accented Latin letters are separators
(`café` → `caf`), `Ё`/`ё` are outside the Cyrillic range (`Ёж ёлка` → `ж` +
`лка`), and Greek yields no tokens at all. Pass a real tokenizer to
`ngrams_str_with`. The engine itself is code-point agnostic — it never inspects
an element's contents, only its position.

## Related

- [Choosing: n-grams](../choosing/ngrams.md) — the lazy-vs-materialised decision
  in full
- [Tokenizers](./tokenizers.md) — what to pass to `ngrams_str_with`
- [Core traits](./core.md) — `Tokenizer`, `BorrowingTokenizer`
- [Zero-copy](../performance/zero-copy.md) · [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md) · [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)

## API reference

```bash
cargo doc -p verbora-ngrams --no-deps --open
```

| Module | Contents | Re-exported at the crate root |
|---|---|---|
| `engine` | `ngrams_iter`, `NGramIter`, `ngrams`, `ngrams_owned`, `bigrams`, `trigrams`, `multrigrams` | all |
| `stats` | `NGramStats`, `ngram_key`, `ngrams_with_stats`, `bigrams_with_stats`, `trigrams_with_stats`, `multrigrams_with_stats` | all |
| `text` | the `_str` family, `ngrams_str_with`, the `_str_with_stats` family, `ngrams_of_tokens` | all except `ngrams_of_tokens` |
| `tokenizer` | `NGramTokenizer`, `WordTokenizer`, `FnTokenizer`, `set_tokenizer`, `reset_tokenizer`, `current_tokenizer`, `tokenize` | all |
| `zh` | `code_units`, `split_lossy`, the `_zh` and `_zh_utf16` families | `ngrams_zh`, `bigrams_zh`, `trigrams_zh` only |

`NGramStats` derives `Debug`, `Clone`, `PartialEq`, `Eq`; `NGramIter` derives
`Debug` and `Clone`. There is no `Result` anywhere in this crate.

Source: `crates/verbora-ngrams/src/`. Benchmarks:
`crates/verbora-ngrams/benches/ngrams.rs`.
