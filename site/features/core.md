# Core vocabulary (`verbora-core`)

`verbora-core` is the crate every other Verbora crate is written against. It
contains no algorithm: six traits covering five operations the library performs,
one list-trimming helper whose exact asymmetry is load-bearing for
compatibility, a mutable work-buffer that the (not yet written) Snowball stemmers will
use, the stop-word list and its process-global mirror, and two functions that
implement the reference's `\s` rather than Rust's. It depends on no other
`verbora-*` crate, which is what keeps the crate graph acyclic and lets a leaf
crate like `verbora-distance` be used without pulling in data assets it does not
need.

<div class="callout callout-spec">
<strong>Specification status.</strong> <code>verbora-core</code> is the shared
vocabulary crate: traits, <code>Token</code>, stop-word state and the string
helpers other crates build on. Behaviour is pinned by <strong>21</strong>
in-crate unit tests, plus indirect coverage from every crate that depends on
it — <code>trim_edge_empties</code> (re-exported by
<code>verbora-tokenizers</code>) and <code>is_whitespace</code> (used by
<code>verbora-inflectors</code>) are exercised through those crates' own
suites.
</div>

## When to use it

- You are writing a function generic over "any tokenizer" or "any string
  metric", and you want it to work with every implementation in the workspace.
- You are implementing your own tokenizer, stemmer or phonetic encoder and want
  the downstream crates to accept it.
- You need `trim_edge_empties`, `is_whitespace` or `collapse_whitespace`
  because you are porting a reference routine that splits or trims on `\s`.
- You need the stop-word list, or you need to observe the reference's process-wide
  mutable stop-word state.

## When not to use it

- **You just want to tokenize a string.** Call the concrete tokenizer's own
  method — see [Tokenizers](./tokenizers.md). Every tokenizer in
  `verbora-tokenizers` is built around a lazy `Tokenize::tokens` iterator that
  is strictly more capable than `verbora_core::Tokenizer`: it can yield borrowed
  slices, `Cow`, or UTF-16 tokens holding unpaired surrogates, none of which fit
  in the `Vec<String>` the core trait is fixed to.
- **You want a stemmer.** There is no stemmer in this workspace. The `Stemmer`
  trait exists ahead of its implementations; see [Roadmap](./roadmap.md).
- **You want `dyn` dispatch.** Four of the six traits are not dyn-compatible;
  see [Advanced usage](#advanced-usage).

## Quick example

```rust
use verbora_core::Tokenizer;

struct SpaceTokenizer;

// The only method you must write.
impl Tokenizer for SpaceTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(text.split(' ').map(str::to_owned));
    }
}

fn main() {
    let t = SpaceTokenizer;

    // Provided: allocates a fresh Vec, then delegates to tokenize_into.
    assert_eq!(t.tokenize("the quick fox"), ["the", "quick", "fox"]);

    // Required: appends into a buffer you own and reuse.
    let mut buf = Vec::new();
    for doc in ["one two", "three four"] {
        buf.clear();
        t.tokenize_into(doc, &mut buf);
    }
    assert_eq!(buf, ["three", "four"]);

    // Provided: one fresh Vec per document.
    assert_eq!(
        t.tokenize_batch(&["a b", "c"]),
        vec![vec!["a", "b"], vec!["c"]]
    );
}
```

## Choosing the right API

### Comparison table

| Trait | Required method(s) | Provided method(s) | Output | `dyn`-compatible | Implementors in workspace |
|---|---|---|---|:--:|:--:|
| `Tokenizer` | `tokenize_into` | `tokenize`, `tokenize_batch` | `Vec<String>` | ❌ | 22 |
| `BorrowingTokenizer` | `tokenize_borrowed_into` | `tokenize_borrowed` | `Vec<&'a str>` | ❌ | 14 |
| `Stemmer` | `stem` | `stem_into`, `stem_batch` | `Cow<'a, str>` | ❌ | **0** |
| `Phonetic` | `process` | `compare` | `String` | ✅ | 3 |
| `DoubleKeyPhonetic` | `process_double` | — | `(String, String)` | ✅ | 1 |
| `StringMetric` | `IS_SIMILARITY`, `measure` | — | `f64` | ❌ | 5 |

### Decision tree

```text
I am writing code generic over an operation
│
├── Splitting text into tokens
│      ├── Tokens are always substrings of the input
│      │      └── BorrowingTokenizer  (zero-copy)
│      └── Tokens may be rewritten (case, diacritics, placeholders)
│             └── Tokenizer
│
├── Reducing a word to a stem
│      └── Stemmer  (no implementation exists yet)
│
├── Mapping a word to a sound-alike key
│      ├── One key
│      │      └── Phonetic
│      └── Primary + alternate key
│             └── DoubleKeyPhonetic
│
└── Scoring how close two strings are
       └── StringMetric  (check IS_SIMILARITY before comparing scores)
```

And, within `Tokenizer`, which call shape:

```text
I have text to tokenize
│
├── One document, simplest possible call
│      └── tokenize()            — one fresh Vec, grown from zero
│
├── Millions of documents, one buffer
│      └── tokenize_into()       — appends; you call clear()
│
├── A slice of documents, want Vec<Vec<String>>
│      └── tokenize_batch()      — a fresh Vec per document (see the warning)
│
└── Tokens are substrings and I want no per-token String
       └── tokenize_borrowed_into()
```

### `Tokenizer`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;String&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v"><code>tokenize</code>: one <code>Vec</code> grown from zero, plus whatever <code>tokenize_into</code> pushes</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Yes, via <code>tokenize_into</code></span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v"><code>tokenize_batch</code>, sequential</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Generic code that needs owned tokens</span></div>
</div>

```rust  ignore
pub trait Tokenizer {
    fn tokenize(&self, text: &str) -> Vec<String> { /* provided */ }
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);  // required
    fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>> { /* provided */ }
}
```

**`tokenize_into` is the only required method.** Everything else is defined in
terms of it, so an implementation cannot drift from itself.

**`tokenize` (provided).** The body is exactly:

```rust  ignore
let mut out = Vec::new();
self.tokenize_into(text, &mut out);
out
```

The `Vec` starts at zero capacity and grows by doubling, so a document of *n*
tokens costs O(log n) reallocations plus one `String` per token. This is the
owning API: the returned `Vec<String>` corresponds element-for-element
with the reference `string[]`.

<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>

**`tokenize_into` (required).** `out` is **not** cleared. That is deliberate —
it lets you accumulate tokens from several inputs into one list — but it means
the reuse pattern needs an explicit `clear()`:

```rust
use verbora_core::Tokenizer;
struct SpaceTokenizer;
impl Tokenizer for SpaceTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(text.split(' ').map(str::to_owned));
    }
}

fn main() {
    let t = SpaceTokenizer;
    let mut out = vec!["stale".to_owned()];
    t.tokenize_into("a b", &mut out);
    assert_eq!(out, ["stale", "a", "b"]); // appended, not replaced
}
```

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

**`tokenize_batch` (provided).** The body is:

```rust  ignore
texts.iter().map(|t| self.tokenize(t.as_ref())).collect()
```

That is: one call to `tokenize` per document, each of which allocates a **fresh**
`Vec<String>` starting from zero capacity. The outer `Vec<Vec<String>>` is sized
once from the slice's exact size hint. Nothing is reused between documents, and
nothing is parallel. No type in the workspace overrides it.

<div class="callout callout-warn">
<strong>Careful.</strong> The doc comment on <code>tokenize_batch</code> in
<code>crates/verbora-core/src/lib.rs</code> claims that "the default implementation reuses
one output buffer's capacity across documents". <strong>It does not.</strong> The default
body maps <code>self.tokenize</code> over the slice, and <code>tokenize</code> allocates a new
<code>Vec</code> on every call. This is a known documentation defect in the crate, not a
behaviour you can rely on. If you want real buffer reuse across a corpus, write
the loop yourself with <code>tokenize_into</code> and <code>clear()</code> — see
<a href="../performance/buffer-reuse">Buffer reuse</a>.
</div>

### `BorrowingTokenizer`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Borrowed <code>&amp;'a str</code> slices of the input</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>; no per-token allocation</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Yes, via <code>tokenize_borrowed_into</code></span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Hot loops over tokenizers that only slice</span></div>
</div>

<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

```rust  ignore
pub trait BorrowingTokenizer: Tokenizer {
    fn tokenize_borrowed<'a>(&self, text: &'a str) -> Vec<&'a str> { /* provided */ }
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>);  // required
}
```

`tokenize_borrowed` allocates only the `Vec`; each element is a slice pointing
into `text`. `tokenize_borrowed_into` appends (it does not clear, matching
`tokenize_into`) and is allocation-free once `out` has capacity.

```rust
use verbora_core::BorrowingTokenizer;
use verbora_tokenizers::AggressiveTokenizer;

fn main() {
    let t = AggressiveTokenizer::new();
    let text = "the quick brown fox";

    let mut out: Vec<&str> = Vec::new();
    t.tokenize_borrowed_into(text, &mut out);
    assert_eq!(out, ["the", "quick", "brown", "fox"]);
}
```

**Why some tokenizers cannot implement it.** The return type is `&'a str`, a
slice of the *input*. A tokenizer that rewrites the text before splitting has no
slice of the input to hand back — the token it wants to yield does not exist
anywhere in `text`. That rules out:

| Tokenizer | Why it cannot borrow |
|---|---|
| `AggressiveTokenizerNo`, `AggressiveTokenizerSv` | strip diacritics before splitting; a de-accented token is a new string |
| `AggressiveTokenizerHi` | deletes punctuation characters, producing tokens that are not contiguous in the input |
| `CaseTokenizer`, `TreebankWordTokenizer`, `TokenizerJa` | cut at UTF-16 code-unit boundaries, so a token can be an unpaired surrogate — a value no `&str` can hold |
| `SentenceTokenizer` | substitutes placeholders, rewriting the text wholesale |

A case-folding tokenizer would fail for the same reason: `"The"` lower-cased is
not a substring of `"The"`. If you need that, implement `Tokenizer` only, or
return `Cow<'a, str>` from your own iterator the way `verbora-tokenizers` does
with its `Tokenize` trait.

### `Stemmer`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Cow&lt;'a, str&gt;</code> — borrowed when the token is its own stem</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None on the <code>Cow::Borrowed</code> path; one <code>String</code> otherwise</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v"><code>stem_into</code>, but the default body still round-trips through <code>stem</code></span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v"><code>stem_batch</code>, sequential</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Nothing yet — see below</span></div>
</div>

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>

```rust  ignore
pub trait Stemmer {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str>;              // required
    fn stem_into(&self, token: &str, out: &mut String) { /* provided */ }
    fn stem_batch<S: AsRef<str>>(&self, tokens: &[S]) -> Vec<String> { /* provided */ }
}
```

**The implementations live in `verbora-stemmers`.** This crate defines only the
trait, so that the crates written on top of it — and your own code — can be
written once against one vocabulary. Implement it yourself if you need a
stemmer this workspace does not ship. See [Roadmap](./roadmap.md).

**`stem` (required).** Returns `Cow::Borrowed` when the token is already its own
stem, which is the common case for short and irregular words. Call
`.into_owned()` if you need a `String`.

**`stem_into` (provided).** The body is:

```rust  ignore
out.clear();
out.push_str(&self.stem(token));
```

<div class="callout callout-warn">
<strong>Careful — the two <code>_into</code> methods disagree.</strong>
<code>Stemmer::stem_into</code> <strong>clears</strong> <code>out</code> before writing.
<code>Tokenizer::tokenize_into</code> and <code>BorrowingTokenizer::tokenize_borrowed_into</code>
<strong>append</strong> and never clear. Assume the wrong one and the failure is silent:
believing <code>tokenize_into</code> clears gives you every previous document's tokens
still in the buffer; believing <code>stem_into</code> appends gives you only the last
stem. The asymmetry is intentional — tokenizing naturally accumulates, stemming
naturally replaces — but the names do not warn you about it. Reuse one buffer per
operation, and call <code>clear()</code> yourself on the tokenizer's.
</div>

A second thing to know about the default `stem_into`: it calls `stem`, so if
your `stem` returns `Cow::Owned` the default allocates a `String` inside the
`Cow` and then copies it into `out` — strictly more work than calling `stem`
directly. If you implement `Stemmer` and care about the hot loop, **override
`stem_into`** to write into `out` directly.

**`stem_batch` (provided).** `tokens.iter().map(|t| self.stem(t.as_ref()).into_owned()).collect()`.
`into_owned` allocates a fresh `String` for every `Cow::Borrowed` result, so the
batch method costs one `String` per token even for words that are their own
stem. Sequential; nothing parallel.

```rust
use std::borrow::Cow;
use verbora_core::Stemmer;

struct StripS;

impl Stemmer for StripS {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        match token.strip_suffix('s') {
            Some(base) => Cow::Borrowed(base),
            None => Cow::Borrowed(token),
        }
    }
}

fn main() {
    let s = StripS;
    assert_eq!(s.stem("cats"), "cat");

    let mut out = String::from("stale");
    s.stem_into("cats", &mut out);
    assert_eq!(out, "cat"); // cleared first — unlike tokenize_into

    assert_eq!(s.stem_batch(&["cats", "dog"]), ["cat", "dog"]);
}
```

### `Phonetic` and `DoubleKeyPhonetic`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>String</code> (or a pair of them)</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>String</code> per <code>process</code>; the default <code>compare</code> allocates two</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A — no <code>_into</code> variant</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Generic code over any single-key encoder</span></div>
</div>

```rust  ignore
pub trait Phonetic {
    fn process(&self, token: &str) -> String;                   // required
    fn compare(&self, a: &str, b: &str) -> bool { /* provided */ }
}

pub trait DoubleKeyPhonetic {
    fn process_double(&self, token: &str) -> (String, String);  // required
}
```

The default `compare` is `self.process(a) == self.process(b)`, matching
The reference exactly. It allocates two `String`s per call.

<div class="callout callout-note">
<strong>Note.</strong> The doc comment on <code>compare</code> reads as though the default
"avoids allocating the second key when the implementation can compare
incrementally". The default body does no such thing — it allocates both keys.
Nor does any implementation in this workspace: <code>Metaphone</code>, <code>SoundEx</code> and
<code>SoundExDM</code> all override <code>compare</code> only to delegate to their inherent
<code>compare</code>, whose body is also <code>process(a) == process(b)</code>. Read the sentence
as describing what an <em>override</em> is permitted to do, not what happens today.
</div>

**Who implements them.** In `verbora-phonetics`:

| Type | Trait | Keys |
|---|---|---|
| `Metaphone` | `Phonetic` | one |
| `SoundEx` | `Phonetic` | one |
| `SoundExDM` (Daitch–Mokotoff) | `Phonetic` | one — the reference declares the genuine dual codes and never reads them, so the port is single-key too |
| `DoubleMetaphone` | `DoubleKeyPhonetic` | primary + alternate |

`DoubleMetaphone` implements `DoubleKeyPhonetic` and **not** `Phonetic`: its
inherent `process` returns `(String, String)`, which does not fit the single-key
signature. That split is exactly why the second trait exists — modelling two
keys separately avoids forcing single-key algorithms to return a tuple.

```rust
use verbora_core::{DoubleKeyPhonetic, Phonetic};
use verbora_phonetics::{DoubleMetaphone, Metaphone, SoundEx};

fn keys<P: Phonetic>(encoder: &P, words: &[&str]) -> Vec<String> {
    words.iter().map(|w| encoder.process(w)).collect()
}

fn main() {
    let m = Metaphone::new();
    assert!(Phonetic::compare(&m, "Smith", "Smyth"));
    assert_eq!(keys(&m, &["phonetics"]), ["FNTKS"]);
    assert_eq!(keys(&SoundEx::new(), &["Robert"]), ["R163"]);

    let (primary, alternate) = DoubleMetaphone::new().process_double("astromech");
    assert_eq!((primary.as_str(), alternate.as_str()), ("ATRMX", "ATRMK"));
}
```

### `StringMetric`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>f64</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">Whatever the underlying metric allocates; the trait itself adds none</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Ranking code that must work for both conventions</span></div>
</div>

```rust  ignore
pub trait StringMetric {
    const IS_SIMILARITY: bool;                        // required
    fn measure(&self, a: &str, b: &str) -> f64;       // required
}
```

The reference's metrics are plain functions with inconsistent conventions: some
return distances (lower is closer), others similarities (higher is closer). The
trait **deliberately does not normalise that**. Flipping a metric's sign or
mapping it to `0..=1` would change every caller's numbers, and this project
treats the reference's output as the specification. Instead, `IS_SIMILARITY`
*records* which convention each metric uses so generic code can adapt.

Because `IS_SIMILARITY` is an associated const, it is available at compile time,
so the branch below is resolved during monomorphisation rather than at runtime:

```rust
use verbora_core::StringMetric;
use verbora_distance::{JaroWinkler, Levenshtein};

/// The index of the closest candidate under `metric`, in whichever direction
/// that metric counts.
fn best_match<M: StringMetric>(metric: &M, query: &str, candidates: &[&str]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (i, metric.measure(query, c)))
        .filter(|(_, score)| !score.is_nan())
        .reduce(|best, next| {
            let better = if M::IS_SIMILARITY {
                next.1 > best.1
            } else {
                next.1 < best.1
            };
            if better { next } else { best }
        })
        .map(|(i, _)| i)
}

fn main() {
    let candidates = ["kitten", "sitting", "mitten"];

    // Distance: lower wins.
    assert_eq!(best_match(&Levenshtein::default(), "mitten", &candidates), Some(2));
    // Similarity: higher wins. Same answer, opposite comparison.
    assert_eq!(best_match(&JaroWinkler::default(), "mitten", &candidates), Some(2));
}
```

The five implementations, all in `verbora-distance`:

| Type | `IS_SIMILARITY` | Range | Sentinel values |
|---|:--:|---|---|
| `Levenshtein` | `false` | `0..` | — |
| `DamerauLevenshtein` | `false` | `0..` | — |
| `JaroWinkler` | `true` | `0..=1` | — |
| `Dice` | `true` | `0..=1` | `NaN` |
| `Hamming` | `false` | `-1`, `0..` | `-1.0` means incomparable lengths |

<div class="callout callout-warn">
<strong>Careful.</strong> <code>IS_SIMILARITY</code> tells you the direction but not the
sentinels. <code>Hamming::measure</code> returns <code>-1.0</code> when the two strings differ in
UTF-16 length — and <code>-1.0</code> is smaller than every real distance, so a
"lowest wins" ranking picks the <em>incomparable</em> pair as the best match.
<code>Dice</code> can return <code>NaN</code>, which loses every comparison and so is silently
dropped rather than ranked. Filter both out before ranking; see
<a href="./distance">Distance metrics</a>.
</div>

## Who implements what

| Trait | Implementing types | Crate |
|---|---|---|
| `Tokenizer` | the 16 `AggressiveTokenizer*` types, `CaseTokenizer`, `TreebankWordTokenizer`, `TokenizerJa`, `SentenceTokenizer` | `verbora-tokenizers` |
| `Tokenizer` | `WordTokenizer`, `FnTokenizer<F>` | `verbora-ngrams` |
| `BorrowingTokenizer` | the 13 character-class aggressive tokenizers — every `AggressiveTokenizer*` **except** `…No`, `…Sv` and `…Hi` | `verbora-tokenizers` |
| `BorrowingTokenizer` | `WordTokenizer` | `verbora-ngrams` |
| `Stemmer` | *none* | — |
| `Phonetic` | `Metaphone`, `SoundEx`, `SoundExDM` | `verbora-phonetics` |
| `DoubleKeyPhonetic` | `DoubleMetaphone` | `verbora-phonetics` |
| `StringMetric` | `Levenshtein`, `DamerauLevenshtein`, `JaroWinkler`, `Dice`, `Hamming` | `verbora-distance` |

Four tokenizers implement **neither** tokenizer trait: `RegexpTokenizer`,
`verbora_tokenizers::WordTokenizer`, `WordPunctTokenizer` and
`OrthographyTokenizer`. Their underlying `String#match` can return `null`, and
neither core trait has a way to express that, so they expose the same
`tokens` / `tokenize` / `tokenize_into` shape wrapped in `Option` instead.

<div class="callout callout-warn">
<strong>Careful — two different <code>WordTokenizer</code>s.</strong>
<code>verbora_ngrams::WordTokenizer</code> implements <code>Tokenizer</code> and
<code>BorrowingTokenizer</code>; <code>verbora_tokenizers::WordTokenizer</code> implements
neither. They are different types reproducing different the reference classes.
Import the one you mean by its full path.
</div>

## `trim_edge_empties`

```rust  ignore
pub fn trim_edge_empties<T: AsRef<str>>(tokens: &mut Vec<T>)
```

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

Removes empty strings from the **two ends** of a token list and leaves interior
empties exactly where they are. It pops trailing empties one at a time, then
finds the first non-empty index and `drain`s the prefix — in place, no
allocation, one `memmove` of the surviving tail.

```rust
use verbora_core::trim_edge_empties;

fn main() {
    let mut v = vec!["", "", "a", "", "b", "", ""];
    trim_edge_empties(&mut v);
    assert_eq!(v, ["a", "", "b"]);

    // An all-empty list trims to nothing.
    let mut all_empty = vec!["", ""];
    trim_edge_empties(&mut all_empty);
    assert!(all_empty.is_empty());
}
```

**Why the asymmetry is load-bearing.** The reference's `Tokenizer#trim` pops
trailing empties and shifts leading ones; it never touches the middle. Several
tokenizers depend on that. The visible case is `SentenceTokenizer`, where
`"   "` tokenizes to `[""]` rather than `[]` — a generalised "remove all
empties" would delete that token and change the output length. `verbora-tokenizers`
re-exports this function rather than defining its own, precisely so the two
cannot drift apart.

## `Token` — the stemming work-buffer

`Token` is tested against the reference token buffer: the mutable word buffer
that the Snowball-family stemmers are written against. It carries three things
at once — the word being stemmed, the alphabet's vowel set, and the named
regions (`R1`, `R2`, `RV`) that Snowball rules are scoped to.

<div class="callout callout-note">
<strong>Note.</strong> Nothing in this workspace consumes <code>Token</code> today. No crate
outside <code>verbora-core</code> references it, because the stemmers it exists to serve
are not written. It is documented here because it is public API and because it
is where the UTF-16 decision for stemming was made — not because you have a
reason to call it yet.
</div>

### Representation

```rust  ignore
pub struct Token {
    chars: Vec<char>,               // the working string
    original: String,               // the input as first supplied
    vowels: Vec<char>,              // the alphabet's vowels
    regions: Vec<(Box<str>, usize)>,// named region start offsets
}
```

<span class="badge badge-utf16">UTF-16</span>

It stores `Vec<char>` — Unicode scalar values — rather than a `String`, because
Snowball rules index constantly and indexing a Rust `&str` by character position
is O(n). `Vec<char>` makes it O(1), matching the reference cost model. That is
*identical* to the reference's UTF-16 code-unit indexing for every character in the
Basic Multilingual Plane, which covers every alphabet these stemmers target
(Latin, Cyrillic, Greek and their accented forms). The two differ only for
astral-plane characters, which reference counts as two code units and Rust as one; no
stemming rule in the reference can match such a character.

`regions` is a `Vec` of pairs rather than a `HashMap`: Snowball uses at most
three regions, and a linear scan over three short keys beats hashing on every
lookup while keeping the data contiguous.

Derives `Debug`, `Clone`, `PartialEq`, `Eq` — note that equality compares all
four fields, including `original` and the vowel set, so two tokens with the same
working string are not equal if they were built from different inputs. It also
implements `Display`, which writes the working string.

### Surface

| Method | Signature | Behaviour |
|---|---|---|
| `new` | `fn new(string: &str) -> Self` | Collects `string` into `chars`, clones it into `original`; empty vowels and regions. Two allocations. |
| `using_vowels` | `fn using_vowels(self, vowels: &str) -> Self` | Sets the vowel set and returns `self` for chaining. `#[must_use]`. |
| `as_string` | `fn as_string(&self) -> String` | **Allocates a fresh `String`** from `chars` on every call. |
| `chars` | `fn chars(&self) -> &[char]` | Free; the working string as a slice. |
| `original` | `fn original(&self) -> &str` | The input as first supplied, untouched by later rules. |
| `len` | `fn len(&self) -> usize` | Length of `chars`, in characters. |
| `is_empty` | `fn is_empty(&self) -> bool` | Whether `chars` is empty. |
| `set_string` | `fn set_string(&mut self, s: &str)` | Clears `chars` and refills from `s`. Does **not** touch `original`. |
| `mark_region` | `fn mark_region(&mut self, region: &str, index: usize) -> &mut Self` | Updates in place if the name exists, otherwise pushes. Chainable. |
| `mark_region_with` | `fn mark_region_with<F: FnOnce(&Self) -> usize>(&mut self, region: &str, f: F) -> &mut Self` | Computes the index from `&self`, then marks. |
| `region` | `fn region(&self, region: &str) -> usize` | Linear scan; returns **0** for an unmarked region, matching the reference `this.regions[region] \|\| 0`. |
| `has_vowel_at_index` | `fn has_vowel_at_index(&self, index: usize) -> bool` | Out-of-range is `false`, matching `indexOf(undefined) === -1`. |
| `next_vowel_index` | `fn next_vowel_index(&self, start: usize) -> usize` | First vowel at or after `start`, or `len()`. An out-of-range `start` clamps to `len()`. |
| `next_consonant_index` | `fn next_consonant_index(&self, start: usize) -> usize` | The mirror of the above. |
| `has_suffix` | `fn has_suffix(&self, suffix: &str) -> bool` | Character-based, case-sensitive. See the empty-suffix quirk below. |
| `has_suffix_in_region` | `fn has_suffix_in_region(&self, suffix: &str, region: &str) -> bool` | `has_suffix` **and** the suffix starts at or after the region's start. |
| `replace_suffix_in_region` | `fn replace_suffix_in_region(&mut self, suffixes: &[&str], replacement: &str, region: &str) -> &mut Self` | **First** matching suffix wins; the rest are not tried. No-op if none match. |
| `replace_all` | `fn replace_all(&mut self, find: &str, replace: &str) -> &mut Self` | `split(find).join(replace)` semantics. Empty needle is a no-op. |

### The empty-suffix quirk

The reference computes `this.string.slice(-suffix.length) === suffix`. For an empty
suffix that is `slice(-0)`, and since `-0 === 0` it yields the **entire** string
rather than an empty one — so `hasSuffix('')` is true only when the token itself
is empty. `has_suffix` reproduces that inversion. A naive `ends_with("")` would
return `true` unconditionally and silently change which stemming rules fire.

### Rule ordering matters

`replace_suffix_in_region` short-circuits on the first match, so the *order of
the suffix list is part of the rule*:

```rust
use verbora_core::Token;

fn main() {
    let mut a = Token::new("running");
    a.replace_suffix_in_region(&["ing", "ning"], "", "R1");
    assert_eq!(a.as_string(), "runn"); // "ing" matched first

    let mut b = Token::new("running");
    b.replace_suffix_in_region(&["ning", "ing"], "", "R1");
    assert_eq!(b.as_string(), "run"); // "ning" matched first
}
```

### Example

```rust
use verbora_core::Token;

fn main() {
    let mut t = Token::new("nationals").using_vowels("aeiouy");

    // R1 = the position after the first vowel-then-consonant pair.
    t.mark_region_with("R1", |t| t.next_consonant_index(t.next_vowel_index(0)) + 1);
    assert_eq!(t.region("R1"), 3);

    // "als" starts at index 6, which is inside R1, so the rule fires.
    t.replace_suffix_in_region(&["als"], "", "R1");
    assert_eq!(t.as_string(), "nation");
    assert_eq!(t.original(), "nationals");

    // An unmarked region starts at 0, matching `this.regions[r] || 0`.
    assert_eq!(t.region("R2"), 0);

    // The `slice(-0)` inversion: an empty suffix matches only an empty token.
    assert!(!Token::new("word").has_suffix(""));
    assert!(Token::new("").has_suffix(""));
}
```

### Allocation notes for `Token`

- `as_string()` allocates on **every** call. In a rule chain, prefer `chars()`
  and call `as_string()` once at the end.
- `replace_all` builds a `String` from `chars` *before* testing whether `find`
  occurs, so it allocates once per call even when it changes nothing. Only the
  empty-needle case returns without allocating. If a stemmer calls it in a hot
  loop, that is worth knowing.
- `replace_suffix_in_region` truncates and extends `chars` in place — no
  allocation unless the replacement grows the buffer past its capacity.

## Stop words

### The `StopWords` value type

```rust  ignore
pub struct StopWords { /* ordered: Vec<String>, lookup: HashSet<String> */ }
```

An **ordered** list with O(1) membership testing. Insertion order is preserved
so the list can be exposed the way the reference exposes its array, while lookups
go through a `HashSet`. Every word is therefore stored twice — once in each
structure. Derives `Debug`, `Clone`, `Default`, and implements
`FromIterator<String>`.

| Method | Behaviour | Cost |
|---|---|---|
| `new()` | Empty list. | No allocation |
| `english()` | The default English list — `DEFAULT_EN`, 170 entries. | 170 `String`s + a `HashSet` |
| `from_iter_of(words)` | From any `IntoIterator<Item: Into<String>>`, preserving order. | One `String` per input |
| `contains(word)` | Hash lookup. **Case-sensitive.** | O(1) |
| `words()` | `&[String]` in insertion order. | Free |
| `len()` / `is_empty()` | Read from the ordered view, so duplicates count. | O(1) |
| `add(word)` | Pushes **unconditionally**, like the reference `push`; a duplicate appears twice in `words()`. | O(1) amortised |
| `add_all(words)` | `add` in a loop. | — |
| `remove(word)` | Removes the **first** occurrence only, like `indexOf` + `splice(idx, 1)`. Drops from the lookup set only once no occurrence remains. | O(n) |
| `remove_all(words)` | `remove` in a loop, first occurrence of each. | — |

```rust
use verbora_core::StopWords;

fn main() {
    let mut stops = StopWords::english();
    assert_eq!(stops.len(), 170);
    assert!(stops.contains("the"));

    // Lookups are case-sensitive and the list is lowercase.
    assert!(!stops.contains("The"));

    // `add` pushes unconditionally; `remove` splices the first match only.
    stops.add("verbora");
    stops.add("verbora");
    stops.remove("verbora");
    assert!(stops.contains("verbora")); // one occurrence still there
    stops.remove("verbora");
    assert!(!stops.contains("verbora"));

    let custom = StopWords::from_iter_of(["alpha", "beta"]);
    assert_eq!(custom.words(), ["alpha", "beta"]);
    assert!(StopWords::new().is_empty());
}
```

### `DEFAULT_EN`

```rust  ignore
pub static DEFAULT_EN: &[&str];
```

170 entries, in reference source order — 132 words, then the 26 lowercase
letters `a`–`z`, then `$`, the digits `1`–`9` and `0`, and `_`. Order is
preserved because the reference exposes this array directly as its own `stopwords` array,
so callers can observe it. It is a `&'static [&'static str]` with no runtime
setup and no duplicates.

### The process-global list

<span class="badge badge-global">GLOBAL STATE</span>

In the reference, the stop-word module exports a single mutable array.
Every stemmer's `addStopWord` / `removeStopWord` mutates *that one array*, and
both the stemmers and the phonetics module read from it — so adding a stop word
through one stemmer changes the behaviour of every other stemmer and of
`tokenizeAndPhoneticize`, process-wide. That is observable behaviour, so Verbora
reproduces it rather than quietly fixing it.

```rust  ignore
pub fn is_default_stopword(word: &str) -> bool;
pub fn add_global_stopword(word: impl Into<String>);
pub fn add_global_stopwords<I, S>(words: I);
pub fn remove_global_stopword(word: &str);
pub fn remove_global_stopwords<'a, I>(words: I);
pub fn global_stopwords() -> Vec<String>;
pub fn reset_global_stopwords();
```

**How it is stored, and is it thread-safe?** Yes. The global is a
`LazyLock<RwLock<StopWords>>`, guarded by a separate `AtomicBool` that records
whether it has ever been mutated:

- **Until the first mutation**, `is_default_stopword` answers from a
  `LazyLock<Box<[&'static str]>>` — `DEFAULT_EN` sorted once — by binary search.
  No lock is taken and nothing is allocated. `global_stopwords()` on this path
  copies `DEFAULT_EN` into a fresh `Vec<String>` — one `Vec` plus 170 `String`s,
  every call.
- **After any mutation**, both functions take the `RwLock` and consult the live
  `StopWords`: a read lock plus one hash lookup for `is_default_stopword`, a read
  lock plus a full `Vec<String>` clone for `global_stopwords()`.
- The `AtomicBool` uses `Ordering::Relaxed`, which is sufficient because the flag
  only selects between two lookup strategies that are both correct; the `RwLock`
  provides the actual synchronisation.

`reset_global_stopwords()` has no counterpart in the reference — there is no way to
un-set the reference's module-level array. It exists so tests that exercise the
global can isolate themselves from one another.

```rust
use verbora_core::stopwords::{
    add_global_stopword, global_stopwords, is_default_stopword, remove_global_stopword,
    reset_global_stopwords,
};

fn main() {
    assert!(is_default_stopword("the"));
    assert_eq!(global_stopwords().len(), 170);

    add_global_stopword("verbora");
    assert!(is_default_stopword("verbora"));

    remove_global_stopword("the");
    assert!(!is_default_stopword("the"));

    reset_global_stopwords();
    assert!(is_default_stopword("the"));
    assert!(!is_default_stopword("verbora"));
}
```

<div class="callout callout-warn">
<strong>Recommendation: prefer an explicit <code>&amp;StopWords</code>.</strong> The global exists
for compatibility, not because it is a good design. It is process-wide: a
library that calls <code>add_global_stopword</code> changes what every other caller in the
binary observes, including code it has never heard of, and tests that touch it
must serialise against each other. Every consumer in this workspace offers a
sibling that takes a list explicitly — <code>verbora_phonetics::phoneticize_tokens</code>
reads the global, <code>phoneticize_tokens_with</code> takes a <code>&amp;StopWords</code>. Use the
latter unless you are deliberately reproducing the reference's shared state.
</div>

<div class="callout callout-warn">
<strong>Careful — case sensitivity.</strong> <code>DEFAULT_EN</code> is entirely lowercase and
both <code>StopWords::contains</code> and <code>is_default_stopword</code> compare raw strings.
<code>"The"</code> is <strong>not</strong> a stop word; neither is <code>"THE"</code>. This is the
reference's behaviour — <code>verbora_phonetics::phoneticize_tokens</code> filters
case-sensitively on the raw token, so a sentence-initial <code>"The"</code> survives
filtering while a mid-sentence <code>"the"</code> does not. Lower-case your tokens
yourself if you want case-insensitive filtering, and be aware that doing so is a
divergence from the reference.
</div>

<div class="callout callout-note">
<strong>Note.</strong> Every function that touches the global calls
<code>.expect("stop-word lock poisoned")</code>. If a thread panics while holding the
write lock, every later call panics too. In practice the critical sections are
<code>Vec</code>/<code>HashSet</code> operations that do not panic, but the behaviour is
worth knowing before you put the global on a request path.
</div>

## `whitespace` — reference string semantics

Two small functions that mark a place where the obvious Rust equivalent silently
disagrees with the reference implementation.

```rust  ignore
pub const fn is_whitespace(c: char) -> bool;
pub fn collapse_whitespace(s: &str) -> Cow<'_, str>;
```

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>
<a class="badge badge-cow" href="../performance/zero-copy">COW</a>

### Why the reference's `\s` is not `char::is_whitespace`

The two sets are close but not equal, and **both** differences are reachable
from ordinary text:

| Character | the reference `\s` | Rust `char::is_whitespace` |
|---|:--:|:--:|
| `U+0085` NEXT LINE | ❌ | ✅ |
| `U+FEFF` ZERO WIDTH NO-BREAK SPACE (BOM) | ✅ | ❌ |

The reference language defines `\s` as `WhiteSpace | LineTerminator`, where `WhiteSpace` is
TAB, VT, FF, SP, NBSP, ZWNBSP and the `Zs` category, and `LineTerminator` is LF,
CR, LS and PS. `U+0085` is category `Cc`, so the reference does not match it; Rust's
`White_Space` property does. `U+FEFF` is the mirror image.

`U+FEFF` is the byte-order mark. It shows up at the start of files saved by a
great deal of Windows tooling, and it is stripped by the reference's `trim` but not
by Rust's. `U+0085` appears in text transcoded from EBCDIC and from some
Latin-1-adjacent encodings. Neither is exotic.

The exact set `is_whitespace` matches:

| Range / code point | Name |
|---|---|
| `U+0009`–`U+000D` | TAB, LF, VT, FF, CR |
| `U+0020` | SPACE |
| `U+00A0` | NO-BREAK SPACE |
| `U+1680` | OGHAM SPACE MARK |
| `U+2000`–`U+200A` | EN QUAD … HAIR SPACE |
| `U+2028`, `U+2029` | LINE / PARAGRAPH SEPARATOR |
| `U+202F` | NARROW NO-BREAK SPACE |
| `U+205F` | MEDIUM MATHEMATICAL SPACE |
| `U+3000` | IDEOGRAPHIC SPACE |
| `U+FEFF` | ZERO WIDTH NO-BREAK SPACE |

Note that `U+200B` ZERO WIDTH SPACE is **not** in the set — it is not `\s` in
the reference either.

**This is the reason several classes of bug do not exist.** `verbora-tokenizers`
lists it among the standing hazards its crate documentation calls out, and
`verbora-inflectors` uses it inside its the reference number parser. Any port of a
The reference routine that splits or trims on `\s` must use this function, or it
will tokenize text containing either character differently.

`is_whitespace` is a `const fn` and `#[inline]`, implemented as a single
`matches!` over character ranges. It allocates nothing and can be evaluated in
const context.

### `collapse_whitespace`

Implements the `replace(/\s+/g, ' ').replace(/^\s+|\s+$/g, '')` idiom: every run
of reference whitespace becomes a single space, and the ends are trimmed.

It makes one scan to decide whether the string contains **any** reference whitespace.
If not, it returns `Cow::Borrowed` and allocates nothing. Otherwise it allocates
one `String` with `s.len()` bytes of capacity and makes a second scan.

<div class="callout callout-note">
<strong>Note.</strong> The borrow test is "contains no whitespace at all", not
"needs no change". An already-normalised string like <code>"a b"</code> still takes the
owned path and allocates, because it contains a space. That is fine for the
intended use — single words — but do not assume idempotent input is free.
</div>

```rust
use std::borrow::Cow;
use verbora_core::{collapse_whitespace, is_whitespace};

fn main() {
    // U+FEFF is whitespace to the reference, not to Rust.
    assert!(is_whitespace('\u{FEFF}'));
    assert!(!'\u{FEFF}'.is_whitespace());

    // U+0085 is the reverse.
    assert!(!is_whitespace('\u{0085}'));
    assert!('\u{0085}'.is_whitespace());

    assert_eq!(collapse_whitespace("  a \t\n b  "), "a b");
    assert!(matches!(collapse_whitespace("oneword"), Cow::Borrowed(_)));

    // Already normalised, but still allocates — it contains a space.
    assert!(matches!(collapse_whitespace("a b"), Cow::Owned(_)));
}
```

## Cargo features

`verbora-core` declares the **only** Cargo feature in the entire workspace. No
other crate has a `[features]` section at all.

```toml
[dependencies]
serde = { workspace = true, optional = true }

[features]
default = []
serde = ["dep:serde"]
```

| Feature | Default | What it gates |
|---|:--:|---|
| `serde` | **off** | The optional `serde` dependency, and nothing else — see below |

<div class="callout callout-warn">
<strong>Careful — the feature gates no code today.</strong> There is not a single
<code>#[cfg(feature = "serde")]</code> or <code>#[cfg_attr(feature = "serde", derive(…))]</code>
anywhere under <code>crates/verbora-core/src/</code>. Neither <code>Token</code> nor
<code>StopWords</code> derives <code>Serialize</code> or <code>Deserialize</code>, under any
configuration. Turning the feature on compiles <code>serde</code> (with its <code>derive</code>
macro, from the workspace dependency table) into your graph and changes no
Verbora API. The feature name is reserved for when those derives land; until
then, enabling it only costs you build time.
</div>

The `#![cfg_attr(docsrs, feature(doc_cfg))]` at the top of `lib.rs` is a *rustc*
cfg set by docs.rs, not a Cargo feature; you cannot enable it from a manifest.

Nothing else is feature-gated anywhere: no `no_std` mode, no `std` toggle, no
optional algorithm sets. What you get from `verbora-core` is what the default
build gives you.

## Advanced usage

### `dyn` compatibility

Four of the six traits cannot be used behind `dyn`, and the reasons are worth
knowing before you design around them:

| Trait | `dyn`-compatible | Why not |
|---|:--:|---|
| `Tokenizer` | ❌ | `tokenize_batch<S: AsRef<str>>` is a generic method |
| `BorrowingTokenizer` | ❌ | inherits the problem from its `Tokenizer` supertrait |
| `Stemmer` | ❌ | `stem_batch<S: AsRef<str>>` is a generic method |
| `Phonetic` | ✅ | — |
| `DoubleKeyPhonetic` | ✅ | — |
| `StringMetric` | ❌ | `IS_SIMILARITY` is an associated const |

If you need to store heterogeneous tokenizers at runtime, define your own
object-safe projection and blanket-implement it. `verbora-ngrams` already does
exactly this, and its `NGramTokenizer` accepts any `verbora_core::Tokenizer`
without the implementor writing anything extra:

```rust
use verbora_core::Phonetic;
use verbora_ngrams::{NGramTokenizer, WordTokenizer};
use verbora_phonetics::{Metaphone, SoundEx};

fn main() {
    // Phonetic is dyn-compatible.
    let encoders: Vec<Box<dyn Phonetic>> =
        vec![Box::new(Metaphone::new()), Box::new(SoundEx::new())];
    assert_eq!(encoders.len(), 2);

    // Tokenizer is not, so go through an object-safe projection.
    let boxed: Box<dyn NGramTokenizer> = Box::new(WordTokenizer);
    assert_eq!(boxed.tokenize_text("a b"), ["a", "b"]);
}
```

### Parallelism

`verbora-core` itself ships no `par_*` function — it is shared traits, not an
NLP algorithm, so `tokenize_batch` and its siblings stay sequential here and no
type in this crate overrides them to parallelise. That is no longer true of the
workspace as a whole: thirteen concrete crates now expose an opt-in `parallel`
Cargo feature with one or more `par_*_batch` functions, each a thin `rayon`
fan-out over their own sequential primitive. See
[Parallelism](../performance/parallelism.md) for the full table and the
reasoning behind which crates got one.

For anything not covered by those thirteen — including every trait on this
page — you can parallelise yourself. Every implementation in the workspace
takes `&self`, holds no per-call mutable state, and is `Send + Sync`, so a
`rayon` `par_iter` over your documents is sound:

```rust  ignore
// Your crate, your rayon dependency.
use rayon::prelude::*;
use verbora_core::Tokenizer;

let all: Vec<Vec<String>> = docs.par_iter().map(|d| tokenizer.tokenize(d)).collect();
```

Two caveats. First, that gives up buffer reuse entirely — each task allocates its
own `Vec`; if you want both, give each worker a thread-local buffer and call
`tokenize_into`. Second, the process-global stop-word list and
`verbora-ngrams`'s global tokenizer are shared across threads: reads are cheap
and lock-free until the first mutation, but mutating either from one thread
changes what every other thread observes.

## Performance characteristics

Nothing in `verbora-core` is an algorithm, so its own cost is small and easy to
state exactly:

| Item | Complexity | Allocations |
|---|---|---|
| `trim_edge_empties` | O(trailing empties) pops + one O(n) drain | None |
| `is_whitespace` | O(1), `const fn`, inlined | None |
| `collapse_whitespace` | Two O(n) scans (one on the borrowed path) | None when the input has no reference whitespace; otherwise one `String` of `s.len()` capacity |
| `Token::new` | O(n) | Two: `Vec<char>` and `String` |
| `Token::as_string` | O(n) | One `String`, every call |
| `Token::region` | O(regions), ≤ 3 in practice | None |
| `Token::has_suffix` | O(suffix len) after one O(suffix) collect | One `Vec<char>` for the suffix |
| `Token::replace_all` | O(n) | One `String` per call, even when nothing matches |
| `StopWords::contains` | O(1) hash | None |
| `StopWords::remove` | O(n) scan + O(n) shift + O(n) rescan | None |
| `is_default_stopword` (unmutated) | O(log 170) binary search, no lock | None |
| `is_default_stopword` (after mutation) | `RwLock` read + O(1) hash | None |
| `global_stopwords()` | O(170) | 170 `String`s + one `Vec`, every call |

No timings appear here because none have been measured for this crate. The only
measured numbers in the repository are the 26 `verbora-distance` benchmarks in
`docs/PERFORMANCE.md`.

> Not yet benchmarked — see [Benchmarks](../benchmarks/index.md).

## Allocation behaviour

Summarised across the whole crate:

| API | Allocates | Notes |
|---|---|---|
| `Tokenizer::tokenize` | One `Vec` (from zero capacity) + impl's per-token `String`s | O(log n) reallocations as the `Vec` grows |
| `Tokenizer::tokenize_into` | Only what the impl pushes | Amortises to zero across a reused buffer |
| `Tokenizer::tokenize_batch` | One outer `Vec` (exactly sized) + one inner `Vec` per document | No reuse — see the warning above |
| `BorrowingTokenizer::tokenize_borrowed` | One `Vec` of `&str` | No per-token allocation |
| `BorrowingTokenizer::tokenize_borrowed_into` | Nothing, once `out` has capacity | Appends |
| `Stemmer::stem` | Nothing on the `Cow::Borrowed` path | Impl-dependent otherwise |
| `Stemmer::stem_into` | Whatever `stem` allocates, then copies into `out` | Override it to avoid the round trip |
| `Stemmer::stem_batch` | One `String` per token, always | `into_owned()` copies even borrowed stems |
| `Phonetic::process` | One `String` | — |
| `Phonetic::compare` | Two `String`s | Default body only |
| `StringMetric::measure` | Whatever the metric does | The trait adds nothing |
| `trim_edge_empties` | Nothing | In place |
| `collapse_whitespace` | Nothing, or one `String` | Borrowed only when the input has no reference whitespace |

See [Allocation](../performance/allocation.md) and
[Buffer reuse](../performance/buffer-reuse.md).

## Unicode and language notes

- **`Token` indexes by Unicode scalar value, not UTF-16 code unit.** Identical
  to the reference for the whole BMP; differs only for astral-plane characters,
  which no reference stemming rule can match.
- **`is_whitespace` is not `char::is_whitespace`.** Two code points differ,
  in opposite directions.
- **`Token::has_suffix` is case-sensitive and character-based**, so `"époque"`
  ends with `"que"` but not with `"Que"`, and the accented character is one
  position, not two.
- **`StopWords` comparisons are byte-exact.** No case folding, no Unicode
  normalisation. `"the"` matches; `"The"` and `"the\u{0301}"` do not.
- **`DEFAULT_EN` is English only.** No other language list ships in this crate.

## Common mistakes

**Assuming `tokenize_batch` reuses a buffer.** It does not, whatever its doc
comment says. If you are batching for performance rather than for convenience,
write the `tokenize_into` + `clear()` loop.

**Assuming `tokenize_into` clears.** It appends. `stem_into` clears. Writing the
two loops symmetrically produces wrong output in one of them.

**Comparing `StringMetric` scores without checking `IS_SIMILARITY`.** A ranking
that hard-codes "higher is better" silently returns the *worst* Levenshtein
match. And even with the direction right, `Hamming`'s `-1.0` and `Dice`'s `NaN`
need filtering first.

**Reaching for `dyn Tokenizer`.** It does not compile — `tokenize_batch` is
generic. Use a generic parameter, or an object-safe projection like
`verbora_ngrams::NGramTokenizer`.

**Expecting `"The"` to be filtered as a stop word.** The list is lowercase and
lookups are exact. This is the reference's behaviour and Verbora reproduces it.

**Calling `add_global_stopword` from library code.** It changes behaviour for
the whole process, including callers that never asked. Take a `&StopWords`
parameter instead.

**Using `char::is_whitespace` in a port of a reference routine.** `U+FEFF` and
`U+0085` will disagree, and the disagreement shows up as a tokenization
difference on real-world input.

**Enabling the `serde` feature expecting serialisable types.** It gates no
derives today.

**Confusing the two `WordTokenizer`s.** `verbora_ngrams::WordTokenizer`
implements both tokenizer traits; `verbora_tokenizers::WordTokenizer` implements
neither.

**Calling `Token::as_string()` inside a rule chain.** It allocates a fresh
`String` every time. Use `chars()` and convert once at the end.

## Related

- [Tokenizers](./tokenizers.md) — the concrete implementations, and the richer
  lazy `Tokenize` trait they are actually built on.
- [Distance metrics](./distance.md) — the five `StringMetric` implementations
  and their conventions.
- [Phonetics](./phonetics.md) — the four encoders behind `Phonetic` and
  `DoubleKeyPhonetic`.
- [Roadmap](./roadmap.md) — what the `Stemmer` trait is waiting for.
- [Choosing an API](../choosing/index.md) and
  [API shapes](../choosing/api-shapes.md) — the `x` / `x_into` / `x_batch`
  convention this crate establishes.
- [Performance](../performance/index.md),
  [Iterator vs `_into`](../performance/iterator-vs-into.md),
  [Buffer reuse](../performance/buffer-reuse.md),
  [Zero-copy](../performance/zero-copy.md),
  [Allocation](../performance/allocation.md),
  [Batch vs streaming](../performance/batch-vs-streaming.md).
- [Recipes](../recipes/index.md).

## API reference

### Traits

| Item | Signature |
|---|---|
| `Tokenizer::tokenize` | `fn tokenize(&self, text: &str) -> Vec<String>` — provided |
| `Tokenizer::tokenize_into` | `fn tokenize_into(&self, text: &str, out: &mut Vec<String>)` — required |
| `Tokenizer::tokenize_batch` | `fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>>` — provided |
| `BorrowingTokenizer::tokenize_borrowed` | `fn tokenize_borrowed<'a>(&self, text: &'a str) -> Vec<&'a str>` — provided |
| `BorrowingTokenizer::tokenize_borrowed_into` | `fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>)` — required |
| `Stemmer::stem` | `fn stem<'a>(&self, token: &'a str) -> Cow<'a, str>` — required |
| `Stemmer::stem_into` | `fn stem_into(&self, token: &str, out: &mut String)` — provided, **clears `out`** |
| `Stemmer::stem_batch` | `fn stem_batch<S: AsRef<str>>(&self, tokens: &[S]) -> Vec<String>` — provided |
| `Phonetic::process` | `fn process(&self, token: &str) -> String` — required |
| `Phonetic::compare` | `fn compare(&self, a: &str, b: &str) -> bool` — provided |
| `DoubleKeyPhonetic::process_double` | `fn process_double(&self, token: &str) -> (String, String)` — required |
| `StringMetric::IS_SIMILARITY` | `const IS_SIMILARITY: bool` — required |
| `StringMetric::measure` | `fn measure(&self, a: &str, b: &str) -> f64` — required |

### Free functions

| Item | Signature |
|---|---|
| `verbora_core::trim_edge_empties` | `fn trim_edge_empties<T: AsRef<str>>(tokens: &mut Vec<T>)` |
| `verbora_core::is_whitespace` | `const fn is_whitespace(c: char) -> bool` |
| `verbora_core::collapse_whitespace` | `fn collapse_whitespace(s: &str) -> Cow<'_, str>` |
| `verbora_core::stopwords::is_default_stopword` | `fn is_default_stopword(word: &str) -> bool` |
| `verbora_core::stopwords::add_global_stopword` | `fn add_global_stopword(word: impl Into<String>)` |
| `verbora_core::stopwords::add_global_stopwords` | `fn add_global_stopwords<I, S>(words: I)` |
| `verbora_core::stopwords::remove_global_stopword` | `fn remove_global_stopword(word: &str)` |
| `verbora_core::stopwords::remove_global_stopwords` | `fn remove_global_stopwords<'a, I>(words: I)` |
| `verbora_core::stopwords::global_stopwords` | `fn global_stopwords() -> Vec<String>` |
| `verbora_core::stopwords::reset_global_stopwords` | `fn reset_global_stopwords()` |

### Types and statics

| Item | Kind |
|---|---|
| `verbora_core::Token` (`token::Token`) | struct — `Debug`, `Clone`, `PartialEq`, `Eq`, `Display` |
| `verbora_core::StopWords` (`stopwords::StopWords`) | struct — `Debug`, `Clone`, `Default`, `FromIterator<String>` |
| `verbora_core::stopwords::DEFAULT_EN` | `static &[&str]`, 170 entries |

### Re-exports

`verbora_core` re-exports `collapse_whitespace`, `is_whitespace` (from
`whitespace`), `StopWords` (from `stopwords`) and `Token` (from `token`) at the crate
root. The modules `whitespace`, `stopwords` and `token` are all public; the global
stop-word functions are reachable only through `verbora_core::stopwords::`.
