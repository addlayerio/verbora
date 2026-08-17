# Tokenizers

`verbora-tokenizers` splits text into tokens, twenty-five ways — sixteen
"aggressive" language splitters, four regex-driven ones, a Penn Treebank word
tokenizer, a case-based splitter, a Japanese segmenter and a sentence splitter.
Each reproduces its established output exactly, including the places where that
established behaviour is wrong. Every tokenizer is built on a lazy iterator, and the convenience methods
are defined on top of that iterator, so there is one implementation of each
behaviour and no second copy to drift.

<div class="callout callout-note">
<strong>25 exports, 24 Rust types.</strong> the reference's 25th export,
<code>SentenceTokenizerNew</code>, is the same constructor as
<code>SentenceTokenizer</code> under a second name. Verbora keeps both names —
<code>SentenceTokenizerNew</code> is a type alias — but there is only one
implementation behind them. Counts on this page that total the Rust type surface
(construction cost, trait implementors) say "24 types"; counts that total the
The public API surface counts twenty-five.
</div>

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>25</strong> tokenizer APIs
are documented and test-pinned. Comparison is defined on UTF-16 code units
rather than on <code>String</code>, because four of these tokenizers can split
inside a surrogate pair.
<code>cargo test -p verbora-tokenizers</code> runs <strong>72</strong> unit
tests and <strong>16</strong> doctests.
</div>

## When to use it

- You need the reference's exact token boundaries, because you are porting a
  system whose downstream behaviour (n-grams, classifiers, indexes) depends on
  them.
- You want a fast, allocation-light word splitter for Latin-script text and
  `AggressiveTokenizer`'s character class is the one you want.
- You need sentence segmentation with abbreviation, URI and number protection.
- You need Japanese word segmentation without a dictionary or a model file.

## When not to use it

- **You want linguistically correct tokenization for its own sake.** These are
  faithful reproductions, not designs. `AggressiveTokenizerDe` splits `Äpfel` into `pfel`;
  `AggressiveTokenizerId` deletes every capital letter; `CaseTokenizer` appends
  the literal string `undefined` to some tokens. Those are the reference's
  behaviours and this crate keeps them. If you want good German tokenization,
  do not start here.
- **You want Unicode-aware `\w` semantics.** The reference's `\w`, `\W`, `\b` and
  `\d` are ASCII-only. Unless a tokenizer's language class specifically lists an
  accented letter, that letter is a *separator*:
  `AggressiveTokenizer::tokenize("café naïve")` is `["caf", "na", "ve"]`.
- **You want subword or BPE tokenization for a neural model.** Nothing here does
  that, and nothing is planned.

## Quick example

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    assert_eq!(
        t.tokenize("the quick brown fox"),
        ["the", "quick", "brown", "fox"]
    );
}
```

`AggressiveTokenizer::new()` is a `const fn` and the type is zero-sized, so
constructing one is free. Eighteen of the twenty-four types are zero-sized
(`std::mem::size_of` says `0`); `CaseTokenizer`, `WordTokenizer` and
`WordPunctTokenizer` carry one `bool` and `OrthographyTokenizer` two. Only
`RegexpTokenizer` (which holds a compiled pattern, 48 bytes) and
`SentenceTokenizer` (which holds an abbreviation list, 32 bytes) own anything on
the heap, and only their constructors — plus `OrthographyTokenizer`'s, which
compares a `&str` — are non-`const`.

## The catalogue

Twenty-five reference exports map to **twenty-four Rust types**:
`SentenceTokenizerNew` is a `pub type` alias for `SentenceTokenizer`, because
`SentenceTokenizer === SentenceTokenizerNew` is literally true
in the reference.

Three columns need reading together. **Token type** is what one token *is*.
**`Tokenize`** is this crate's iterator trait. **`Tokenizer`** and
**`Borrowing`** are [`verbora_core::Tokenizer`](core.md) and
`verbora_core::BorrowingTokenizer`, the shared vocabulary other Verbora crates
are written against.

### Aggressive / language family (16)

All sixteen emit maximal runs of a per-language character class, except where
noted. Every class is *generated* by running the reference regex over the whole
Basic Multilingual Plane rather than transcribed by hand, which is how the
surprises below were found.

| Type | Splits on | Token | `Tokenize` | `Tokenizer` | `Borrowing` |
|---|---|---|:--:|:--:|:--:|
| `AggressiveTokenizer` (English) | runs of `A-Z a-z 0-9 ' - /` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerNl` | as English, but `_` is a word character and `/` is a separator | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerDe` | ASCII alphanumerics, `ß ä ö ü _ ' -` — **not** `Ä Ö Ü` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerFr` | ASCII alphanumerics, `-`, accented Latin-1 vowels and `œ ç` in both cases | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerEs` | letters plus `U+00C1–U+00DA`, `U+00E1–U+00FA`, `Ü ü`. **No digits** | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerIt` | `A-Z a-z 0-9 _` only — the reference's ASCII `\W` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerPt` | letters plus `U+00C0–U+00DA`, `U+00E0–U+00FA`. **No digits** | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerVi` | ASCII alphanumerics plus the Vietnamese vowel set in both cases | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerRu` | ASCII alphanumerics, `А-я`, `Ё ё`, and `U+1C80–U+1C86` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerUk` | as Russian but **without** `Ё ё`, plus `Ґ ґ Є є І і Ї ї` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerPl` | ASCII alphanumerics plus `ą ć ę ł ń ó ś ź ż` in both cases | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerId` | `a-z 0-9 -` — **lowercase only** | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerFa` | the reference language whitespace runs; punctuation stays inside tokens | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerNo` | strips 13 diacritics (first occurrence of each only), then splits on `A-Z a-z 0-9 _ æøå ÆØÅ äÄöÖüÜ`. `-` is a separator | `Cow<'_, str>` | ✅ | ✅ | ❌ |
| `AggressiveTokenizerSv` | strips `à á è é` and their uppercase forms (first occurrence only), then splits on `A-Z a-z 0-9 _ åÅäÄöÖüÜ -` | `Cow<'_, str>` | ✅ | ✅ | ❌ |
| `AggressiveTokenizerHi` | deletes `। ॥ . ? ,`, then splits on whitespace and on anything outside Devanagari and ASCII | `Cow<'_, str>` | ✅ | ✅ | ❌ |

Thirteen of these yield `&str` and implement `BorrowingTokenizer`: every token
is a contiguous slice of your input, so tokenizing allocates nothing per token.
The three that yield `Cow` rewrite the text before splitting, so they *can*
borrow — and do, whenever the rewrite turned out to be a no-op — but cannot
promise to.

### Regex-driven family (4)

These four return `Option`, implement **neither** trait, and are described in
[their own section](#the-four-optional-tokenizers) below.

| Type | Splits on | Token | `Tokenize` | `Tokenizer` | `Borrowing` |
|---|---|---|:--:|:--:|:--:|
| `RegexpTokenizer` | any `Pattern` you supply, in split (`gaps`) or match mode | `Option<&str>` inside `Option<Vec<…>>` | ❌ | ❌ | ❌ |
| `WordTokenizer` | fixed `[^A-Za-zА-Яа-я0-9_]+`. `Ё ё` are separators | `&str` inside `Option<Vec<…>>` | ❌ | ❌ | ❌ |
| `OrthographyTokenizer` | Finnish `[A-Za-zÅåÄäÖö]` (no digits); any other language falls back to `WordTokenizer` | `Utf16Token` inside `Option<Vec<…>>` | ❌ | ❌ | ❌ |
| `WordPunctTokenizer` | runs of `A-Za-zÀ-ÿŸ-`, runs of `0-9 . _`, and single characters | `Utf16Token` inside `Option<Vec<…>>` | ❌ | ❌ | ❌ |

### Structural tokenizers (4, plus one alias)

| Type | What it does | Token | `Tokenize` | `Tokenizer` | `Borrowing` |
|---|---|---|:--:|:--:|:--:|
| `TreebankWordTokenizer` | seventeen rewrite passes (contractions, punctuation padding, final period), then a whitespace split | `Utf16Token` | ✅ | ✅ | ❌ |
| `CaseTokenizer` | keeps characters that change under exactly one of `toLowerCase`/`toUpperCase`, plus ASCII digits; splits on the rest | `Utf16Token` | ✅ | ✅ | ❌ |
| `TokenizerJa` | TinySegmenter 0.1: a linear-chain classifier over ~46 weights per position | `Utf16Token` | ✅ | ✅ | ❌ |
| `SentenceTokenizer` | masks abbreviations, URIs and numbers, splits on delimiter placeholders, then unmasks | `String` | ✅ | ✅ | ❌ |
| `SentenceTokenizerNew` | `pub type SentenceTokenizerNew = SentenceTokenizer;` | — | — | — | — |

`TreebankWordTokenizer`, `CaseTokenizer` and `TokenizerJa` implement
`verbora_core::Tokenizer` by rendering unpaired surrogates as U+FFFD, because
that trait's contract is `Vec<String>` and a `String` cannot hold one. When
exactness matters, use [`Tokenize::tokens`](#tokens-—-the-primitive) and handle
[`Utf16Token`](#utf-16-tokens-and-unpaired-surrogates) yourself.

## Choosing the right API

The full treatment, with pipeline diagrams and worked examples, lives on
[Choosing an API: tokenization](../choosing/tokenization.md). This section is
the summary.

There are three method names on `Tokenize`, and one of them is the primitive:

```rust  ignore
pub trait Tokenize {
    type Token<'a>;

    // The only method an implementation writes.
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = Self::Token<'a>>;

    fn tokenize<'a>(&self, text: &'a str) -> Vec<Self::Token<'a>> {
        self.tokens(text).collect()
    }

    fn tokenize_into<'a>(&self, text: &'a str, out: &mut Vec<Self::Token<'a>>) {
        out.extend(self.tokens(text));
    }
}
```

That is the whole trait, copied from `crates/verbora-tokenizers/src/lib.rs`.
`tokenize` is `tokens().collect()` and nothing else; `tokenize_into` is
`out.extend(tokens())` and nothing else. In particular **`tokenize_into` does
not clear `out`** — it appends.

### Comparison table

| API | Best for | Lazy | Materialises | Buffer reuse | Allocations |
|---|---|:--:|:--:|:--:|---|
| `Tokenize::tokens` | streaming, folding, early exit | ✅ | ❌ | n/a | none for the 13 slicers |
| `Tokenize::tokenize` | one document, simplest call | ❌ | ✅ | ❌ | one `Vec`, grown by doubling |
| `Tokenize::tokenize_into` | a corpus through one buffer | ❌ | ✅ | ✅ | none once the buffer is warm |
| `verbora_core::Tokenizer::tokenize` | generic code over any tokenizer | ❌ | ✅ | ❌ | one `Vec` **plus one `String` per token** |
| `verbora_core::Tokenizer::tokenize_into` | generic code, warm buffer | ❌ | ✅ | ✅ (the `Vec`) | one `String` per token |
| `verbora_core::BorrowingTokenizer::tokenize_borrowed_into` | generic code, zero-copy | ❌ | ✅ | ✅ | none once warm |
| `verbora_core::Tokenizer::tokenize_batch` | a slice of documents, one call | ❌ | ✅ | ❌ | one outer `Vec`, one inner `Vec` and one `String` per token |
| `Tokenize::par_tokenize_batch` | many independent documents, feature `parallel` | ❌ | ✅ | ❌ | one outer `Vec`, plus whatever `tokenize` allocates per document |

### Decision tree

```text
I need to tokenize text
│
├── I am writing code that names a concrete tokenizer
│   │
│   ├── One document, and I want the tokens in a Vec
│   │      └── Tokenize::tokenize()
│   │
│   ├── I consume each token once and never need them all at once
│   │      └── Tokenize::tokens()
│   │
│   ├── Many documents in a loop, and I care about allocation
│   │      └── buf.clear(); Tokenize::tokenize_into(doc, &mut buf)
│   │
│   └── Many independent documents, and the `parallel` feature is on
│          └── Tokenize::par_tokenize_batch(&docs)
│
├── I am writing code generic over "any tokenizer"
│   │
│   ├── I need owned Strings
│   │      └── verbora_core::Tokenizer
│   │
│   └── I can work with slices of the input
│          └── verbora_core::BorrowingTokenizer   (13 of the 24 types)
│
└── I want RegexpTokenizer / WordTokenizer /
    OrthographyTokenizer / WordPunctTokenizer
       └── inherent tokens() / tokenize() / tokenize_into(),
           all returning Option — see below
```

### `tokens()` — the primitive

<a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>
<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>
<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy for 17 of the 20 <code>Tokenize</code> types; eager for Treebank, Japanese and Sentence, and on the non-ASCII path of <code>CaseTokenizer</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Borrowed <code>&amp;str</code>, <code>Cow</code>, <code>Utf16Token</code> or <code>String</code>, per tokenizer</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None at all for the 13 slicing tokenizers</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A — nothing is buffered</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Streaming token processing and early exit</span></div>
</div>

The three badges above describe the thirteen slicing tokenizers, which is where
this method's advantage lives. Read the card for the exceptions: `tokens()` on
`TreebankWordTokenizer`, `TokenizerJa` and `SentenceTokenizer` is neither lazy
nor allocation-free, and `SentenceTokenizer`'s tokens are owned `String`s.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();

    // Stops as soon as it finds a match; the rest of the document is never
    // scanned, and no `Vec` is ever built.
    let found = t.tokens("the quick brown fox").any(|w| w == "quick");
    assert!(found);

    let long_words = t.tokens("the quick brown fox").filter(|w| w.len() > 3).count();
    assert_eq!(long_words, 2);
}
```

Because tokens borrow the input, they can be used as `HashMap` keys without a
single `String` allocation:

```rust
use std::collections::HashMap;

use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for token in t.tokens("the cat the hat") {
        *counts.entry(token).or_default() += 1;
    }
    assert_eq!(counts["the"], 2);
}
```

### `tokenize()` — the simple one

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Self::Token&lt;'a&gt;&gt;</code> — the <em>tokens</em> may still borrow</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>, grown by doubling; no per-token allocation for the 13 slicers</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">One document; anything where the token list is the deliverable</span></div>
</div>

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    let tokens: Vec<&str> = t.tokenize("the quick brown fox");
    assert_eq!(tokens.len(), 4);
    assert_eq!(tokens[0], "the");
}
```

The `Vec` starts empty and grows by reallocation: none of these iterators
reports a useful `size_hint` lower bound (`WordRuns` reports `0` as its lower
bound and a byte-count-derived upper bound; everything else uses the default
`(0, None)`), and `Vec`'s `collect` reserves from the *lower* bound. If you know
roughly how many tokens to expect, `tokenize_into` with a pre-reserved buffer
avoids the growth entirely.

### `tokenize_into()` — the hot loop

<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>
<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Appended to the caller's <code>Vec</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None once the buffer's capacity is sufficient</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Yes — that is the entire point</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Millions of documents through one buffer</span></div>
</div>

<div class="callout callout-warn">
<strong>Careful.</strong> <code>tokenize_into</code> does <strong>not</strong>
clear <code>out</code>. Its body is <code>out.extend(self.tokens(text))</code>.
Forgetting <code>buf.clear()</code> in a loop produces a buffer that accumulates
every document — which is a real use case, but rarely the one you meant.
</div>

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    let corpus = ["the quick brown fox", "jumps over the lazy dog"];

    let mut buf: Vec<&str> = Vec::new();
    for doc in corpus {
        buf.clear(); // `tokenize_into` appends; without this the buffer grows.
        t.tokenize_into(doc, &mut buf);
        assert!(!buf.is_empty());
    }
}
```

Accumulating deliberately is just the same call without the `clear`:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    let mut all: Vec<&str> = Vec::new();
    for doc in ["a b", "c d"] {
        t.tokenize_into(doc, &mut all);
    }
    assert_eq!(all, ["a", "b", "c", "d"]);
}
```

One lifetime constraint follows from zero-copy: `Vec<Self::Token<'a>>` ties the
buffer to `'a`, so a buffer holding `&'a str` cannot be reused across documents
with *different* lifetimes unless all of them outlive the loop. In the example
above, `corpus` is an array of `&'static str`, so it works. If your documents
come from a `String` that is dropped each iteration, either move the `Vec`
inside the loop or switch to `verbora_core::Tokenizer::tokenize_into`, whose
`Vec<String>` owns its contents.

### `verbora_core::Tokenizer` — the shared vocabulary

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>
<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;String&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code> and one <code>String</code> per token</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v"><code>tokenize_into</code> reuses the <code>Vec</code>, never the <code>String</code>s</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v"><code>tokenize_batch</code>, sequential</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No — the parallel batch lives on <code>Tokenize</code>, not on this trait; see <code>par_tokenize_batch</code> below</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Code that must work with any tokenizer, or that needs owned tokens</span></div>
</div>

Twenty of the twenty-four types implement it. Use it when your code is generic
over "some tokenizer" rather than over a named one, or when you genuinely need
owned `String`s.

```rust
use verbora_core::Tokenizer;
use verbora_tokenizers::AggressiveTokenizer;

fn main() {
    let t = AggressiveTokenizer::new();
    let owned: Vec<String> = Tokenizer::tokenize(&t, "the quick");
    assert_eq!(owned, ["the", "quick"]);

    let docs = ["one two", "three four"];
    let batch: Vec<Vec<String>> = t.tokenize_batch(&docs);
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[1], ["three", "four"]);
}
```

`tokenize_batch` is a provided method whose default body is exactly:

```rust  ignore
fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>> {
    texts.iter().map(|t| self.tokenize(t.as_ref())).collect()
}
```

It is a sequential `map`. No tokenizer in this crate overrides it. Its doc
comment claims the default "reuses one output buffer's capacity across
documents"; the code does not — it calls `tokenize` per document, and each of
those calls allocates a fresh `Vec`. Calling it buys you a shorter line of code,
not fewer allocations. Because `tokenize_batch` is generic, `Tokenizer` is also
**not object-safe**: you cannot hold one behind `dyn Tokenizer`. (`verbora-ngrams`
works around this with its own `dyn`-compatible `NGramTokenizer` trait and a
blanket impl.)

### `verbora_core::BorrowingTokenizer` — generic and zero-copy

<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>
<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>

Implemented by the thirteen tokenizers whose tokens are always contiguous
substrings of the input: the twelve character-class variants and
`AggressiveTokenizerFa`. It is the generic equivalent of `Tokenize` for those
types.

```rust
use verbora_core::BorrowingTokenizer;
use verbora_tokenizers::AggressiveTokenizer;

fn main() {
    let t = AggressiveTokenizer::new();

    let mut buf: Vec<&str> = Vec::new();
    t.tokenize_borrowed_into("the quick brown fox", &mut buf);
    assert_eq!(buf.len(), 4);

    let v: Vec<&str> = t.tokenize_borrowed("a b");
    assert_eq!(v, ["a", "b"]);
}
```

`tokenize_borrowed_into` also appends rather than clearing. Every `_into` method
in this crate and in `verbora_core`'s two tokenizer traits appends;
`verbora_core::Stemmer::stem_into` is the exception in that crate — it clears its
`String` first.

### The four optional tokenizers

`RegexpTokenizer`, `WordTokenizer`, `OrthographyTokenizer` and
`WordPunctTokenizer` implement neither `Tokenize` nor `verbora_core::Tokenizer`.
They expose the same three method names as inherent methods, wrapped in
`Option`.

The reason is in the reference:

```text
tokenize (s) {
  if (this._gaps) {
    return _.without(s.split(this._pattern), '', ' ')   // always an array
  } else {
    return s.match(this._pattern)                       // array OR null
  }
}
```

In matching mode (`gaps: false`) `String#match` returns `null` when the pattern
does not match. "No match" and "no tokens" are different observable outcomes in
the reference, and the traits have no way to express the difference — `Vec::new()`
would silently merge them. So:

- `tokens()` returns `Option<…>` wrapping a named iterator type
  (`WordTokens`, `OrthographyTokens`, `WordPunctTokens`, or — for
  `RegexpTokenizer` — a plain `std::vec::IntoIter`),
- `tokenize()` returns `Option<Vec<…>>`,
- `tokenize_into()` returns `bool` — `false` where the reference returned `null`,
  and in that case nothing was appended.

```rust
use verbora_tokenizers::WordTokenizer;

fn main() {
    let t = WordTokenizer::new();
    assert_eq!(
        t.tokenize("She said 'hello'. Привет мир 123_456"),
        Some(vec!["She", "said", "hello", "Привет", "мир", "123_456"])
    );
    // Splitting mode never returns `None`.
    assert_eq!(t.tokenize(""), Some(vec![]));

    // Matching mode can: `String#match` returned `null`.
    let m = WordTokenizer::matching();
    assert_eq!(m.tokenize("abc def"), Some(vec![" "]));
    assert_eq!(m.tokenize("abcdef"), None);

    // If you do not care about the distinction, say so explicitly.
    let tokens = m.tokenize("abcdef").unwrap_or_default();
    assert!(tokens.is_empty());
}
```

`RegexpTokenizer` adds a **second** layer of `Option`, on each token. A
`String#split` with capture groups interleaves the groups into the result, and a
group that did not participate becomes the reference's `undefined` — modelled as
`None`. The full return type is therefore `Option<Vec<Option<&str>>>`: the outer
`Option` is `null`, the inner one is `undefined`.

```rust
use verbora_tokenizers::RegexpTokenizer;

fn main() {
    // `RegexpTokenizer::without_pattern()` reproduces `new RegexpTokenizer()`:
    // `String#split(undefined)` is `[s]`.
    let t = RegexpTokenizer::without_pattern();
    assert_eq!(t.tokenize("a b"), Some(vec![Some("a b")]));

    let mut out: Vec<Option<&str>> = Vec::new();
    let ok = t.tokenize_into("a b", &mut out);
    assert!(ok);
    assert_eq!(out, [Some("a b")]);
}
```

With a real pattern you must construct a `Pattern`, which pairs a compiled
`regex::Regex` with the `/g` flag the reference's match mode depends on. That
requires the `regex` crate as a direct dependency of *your* package — Verbora
does not re-export it, which is why the following block is not compiled by the
book:

```rust  ignore
use verbora_tokenizers::{Pattern, RegexpTokenizer};
use regex::Regex;

// `gaps: true` (the default) — split on the pattern.
let split = RegexpTokenizer::new(Pattern::new(Regex::new(r"[^A-Za-z0-9_]+").unwrap()));
assert_eq!(split.tokenize("hello, world"), Some(vec![Some("hello"), Some("world")]));

// Capture groups are interleaved into the result, and a group that did not
// participate is the reference's `undefined` — `None` here.
let grouped = RegexpTokenizer::new(Pattern::new(Regex::new(r"(x)|([0-9])").unwrap()));
assert_eq!(grouped.tokenize("a1b"), Some(vec![Some("a"), None, Some("1"), Some("b")]));

// `gaps: false` — match with the pattern. A global pattern that finds nothing
// is the reference's `null`.
let matching = RegexpTokenizer::matching(Pattern::global(Regex::new("[a-z]+").unwrap()));
assert_eq!(matching.tokenize("123"), None);
```

Two constructor options exist in the reference and are deliberately **absent**
here, because they are dead in the reference:

- `discardEmpty` is computed as `options.discardEmpty || true`, so it is `true`
  for every input. There is no way to switch it off, and this port offers none.
- `WordTokenizer` accepts `options.pattern` and then overwrites it
  unconditionally in its constructor, so `WordTokenizer` takes no pattern here.

`gaps` *is* honoured, by all four — with one wrinkle. `OrthographyTokenizer`
builds its fallback with `new WordTokenizer()`, passing no options at all, so an
**unknown language silently discards `gaps`**:

```rust
use verbora_tokenizers::OrthographyTokenizer;

fn main() {
    let fi = OrthographyTokenizer::new("fi");
    assert_eq!(
        fi.tokenize("Hyvää, kiitos!!  entä").unwrap(),
        ["Hyvää", "kiitos", "entä"]
    );

    // Language matching is exact and lowercase; anything else falls back to
    // `WordTokenizer`, which does not know the Finnish alphabet.
    let upper = OrthographyTokenizer::new("FI");
    assert_eq!(upper.tokenize("Hyvää kiitos").unwrap(), ["Hyv", "kiitos"]);
}
```

Only `fi` is defined in the reference's matcher table. `new OrthographyTokenizer()`
with no language *throws* in the reference; here the constructor requires a
`&str`, so the same mistake is a compile error.

## Advanced usage

### UTF-16 tokens and unpaired surrogates

<span class="badge badge-utf16">UTF-16</span>

Four tokenizers cut text at UTF-16 **code unit** boundaries:

| Tokenizer | Why |
|---|---|
| `WordPunctTokenizer` | its pattern's bare `.` matches one code unit |
| `TreebankWordTokenizer` | the punctuation-padding regex uses a bare `.` likewise |
| `TokenizerJa` | TinySegmenter starts with `text.split('')` |
| `CaseTokenizer` | it indexes `text[i]` with `i` counting code units |

`OrthographyTokenizer` joins them only in *matching* mode: the Finnish matcher
`[^A-Za-zÅåÄäÖö]` has no `+`, so it matches exactly one code unit, which for an
astral character is the high surrogate alone. That is why its token type is
`Utf16Token` even though splitting mode always slices the input.

For an astral character such as `😀`, the two halves of the surrogate pair land
in *separate* tokens. An unpaired surrogate is not a Unicode scalar value, so it
cannot be held by `char`, `String` or `&str`. A port yielding `String` would
have to pick one of three wrong answers: substitute U+FFFD (wrong content),
merge the halves (wrong token *count*), or drop them (wrong both). Verbora
returns `Utf16Token` instead:

```rust  ignore
pub enum Utf16Token<'a> {
    Text(Cow<'a, str>),   // well-formed; borrowed when the tokenizer only sliced
    Raw(Box<[u16]>),      // not well-formed — in practice, half a surrogate pair
}
```

```rust
use verbora_tokenizers::{Utf16Token, WordPunctTokenizer};

fn main() {
    let t = WordPunctTokenizer::new();
    let tokens: Vec<Utf16Token<'_>> = t.tokenize("a😀b").unwrap();

    assert_eq!(tokens.len(), 4);
    assert_eq!(tokens[0].as_str(), Some("a"));
    // The emoji became two tokens, neither representable as `&str`.
    assert_eq!(tokens[1].as_str(), None);
    assert_eq!(tokens[1].to_utf16(), vec![0xd83d]);
    assert_eq!(tokens[2].to_utf16(), vec![0xde00]);
    assert_eq!(tokens[1].to_string_lossy(), "\u{fffd}");

    // Well-formed text stays borrowed and compares against `&str` directly.
    let plain = t.tokenize("hello").unwrap();
    assert!(plain[0].is_well_formed());
    assert_eq!(plain[0], "hello");
}
```

The representation costs nothing on ordinary text. `WordPunctTokenizer`,
`OrthographyTokenizer`, `TreebankWordTokenizer` and `CaseTokenizer` (on ASCII
input) all yield `Text(Cow::Borrowed(_))` — a slice of your input — for every
token that is not a surrogate half. `TokenizerJa` is the exception: it builds
each token by concatenating code units, so its tokens are always owned, astral
input or not.

Two ways to get back to strings:

```rust
use verbora_tokenizers::{Tokenize, TreebankWordTokenizer, Utf16Token};

fn main() {
    let text = "I'll stay home.";

    // Option A — keep only the tokens that are real Rust strings. `as_str`
    // borrows from the token, so the tokens have to outlive the slices.
    let tokens: Vec<Utf16Token<'_>> = TreebankWordTokenizer::new().tokenize(text);
    let well_formed: Vec<&str> = tokens.iter().filter_map(|t| t.as_str()).collect();
    assert_eq!(well_formed, ["I", "'ll", "stay", "home", "."]);

    // Option B — accept U+FFFD for the surrogate halves.
    let strings: Vec<String> = TreebankWordTokenizer::new()
        .tokens(text)
        .map(|t| t.to_string_lossy().into_owned())
        .collect();
    assert_eq!(strings, ["I", "'ll", "stay", "home", "."]);
}
```

Option B is exactly what `verbora_core::Tokenizer::tokenize` does for
`TreebankWordTokenizer`, `CaseTokenizer` and `TokenizerJa`, and their impls say
so in their doc comments. `Utf16Token::to_utf16` is the lossless view, and it is
what the test suite compares on.

### `trim_edge_empties`

The reference's `Tokenizer#trim` pops trailing empty strings and shifts leading
ones, but leaves *interior* empties alone. That asymmetry is load-bearing —
`SentenceTokenizer::tokenize("   ")` is `[""]` rather than `[]` because of it —
so it is re-exported from this crate rather than generalised:

```rust
use verbora_tokenizers::trim_edge_empties;

fn main() {
    let mut v = vec!["", "", "a", "", "b", "", ""];
    trim_edge_empties(&mut v);
    assert_eq!(v, ["a", "", "b"]);
}
```

### Writing code generic over `Tokenize`

```rust
use verbora_tokenizers::{AggressiveTokenizer, CaseTokenizer, Tokenize};

fn count_tokens<T: Tokenize>(t: &T, text: &str) -> usize {
    t.tokens(text).count()
}

fn main() {
    assert_eq!(count_tokens(&AggressiveTokenizer::new(), "a b c"), 3);
    assert_eq!(count_tokens(&CaseTokenizer::new(), "a b c"), 3);
}
```

Note that `Tokenize::Token<'a>` is a generic associated type, so a function that
wants to *do* something with the tokens usually needs a bound such as
`for<'a> T::Token<'a>: AsRef<str>` — which `Utf16Token` does not satisfy. If you
need one signature that covers every tokenizer, `verbora_core::Tokenizer` and
its `Vec<String>` is the API that was built for it.

### Parallelism

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager, fanned out across a <code>rayon</code> thread pool</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Vec&lt;Self::Token&lt;'a&gt;&gt;&gt;</code> — one inner <code>Vec</code> per document</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One outer <code>Vec</code> sized to <code>texts.len()</code>, plus whatever <code>tokenize</code> allocates per document</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — parallel workers cannot share a <code>&amp;mut Vec</code></span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Yes — this is the batch entry point</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — feature <code>parallel</code>; per document</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Many independent documents, large enough that <code>rayon</code>'s scheduling cost is a small fraction of the total</span></div>
</div>

`verbora-tokenizers` ships one built-in parallel API: `Tokenize::par_tokenize_batch`,
a **default trait method** behind this crate's `parallel` Cargo feature
(`parallel = ["dep:rayon"]`, never on by default). Every one of the twenty
implementors of `Tokenize` gets it for free — the same twenty types that
implement `verbora_core::Tokenizer` (see the catalogue above). The four
optional tokenizers (`RegexpTokenizer`, `WordTokenizer`, `OrthographyTokenizer`,
`WordPunctTokenizer`) implement neither trait, so they have no
`par_tokenize_batch`; parallelise them by hand, as shown below.

The method's whole body is one line — a thin `rayon` fan-out over the existing
sequential `tokenize`, one task per **document**, never per token. `tokens()`
and `tokenize_into()` are untouched and remain the primitives everything else
is built on:

```rust  ignore
fn par_tokenize_batch<'a>(&self, texts: &[&'a str]) -> Vec<Vec<Self::Token<'a>>>
where
    Self: Sync,
    Self::Token<'a>: Send,
{
    use rayon::prelude::*;
    texts.par_iter().map(|text| self.tokenize(text)).collect()
}
```

```rust  ignore
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let docs = ["the quick brown fox", "jumps over the lazy dog"];
let batches: Vec<Vec<&str>> = t.par_tokenize_batch(&docs);
assert_eq!(batches[0], ["the", "quick", "brown", "fox"]);
assert_eq!(batches[1], ["jumps", "over", "the", "lazy", "dog"]);
```

<div class="callout callout-note">
<strong>Note.</strong> The block above needs the <code>parallel</code> feature
enabled on <code>verbora-tokenizers</code>, which this site's own snippet
checker builds without, so it is marked <code>ignore</code> rather than
compiled — every other block on this page compiles and runs in CI. See
<a href="../performance/parallelism">Parallelism</a> for the full cross-crate
picture, including the other twelve <code>par_*</code> APIs.
</div>

**When to reach for it.** This crate's own benchmarks put a single `tokenize`
call over an ~8192-word document at roughly 118–120 microseconds, and a
`rayon` task costs on the order of a microsecond to schedule, so per-document
parallelism only pays for itself once the batch is large enough, or the
documents are large enough, that scheduling overhead is a small fraction of
the total. For a handful of short strings, prefer a plain
`texts.iter().map(|t| self.tokenize(t))` loop — that is exactly what
`par_tokenize_batch` degrades to per item, minus the parallel scheduling.
Measure your own workload; see [Parallelism](../performance/parallelism.md)
for the cross-crate numbers that do exist.

For the four optional tokenizers, or for full control over chunking and
buffer reuse that `par_tokenize_batch` does not expose, roll your own — every
tokenizer here is zero-sized (or, for `RegexpTokenizer` and
`SentenceTokenizer`, immutable), stateless and `Send + Sync`:

```rust
use std::thread;

use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let docs: Vec<String> = vec!["a b".into(), "c d".into()];
    let counts: Vec<usize> = thread::scope(|s| {
        let handles: Vec<_> = docs
            .iter()
            .map(|d| {
                s.spawn(|| {
                    // Tokenizers are zero-sized and stateless, so each thread
                    // makes its own.
                    AggressiveTokenizer::new().tokens(d.as_str()).count()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(counts, [2, 2]);
}
```

With `rayon` in your own `Cargo.toml` and no Cargo feature required, the same
shape is `docs.par_iter().map(|d| AggressiveTokenizer::new().tokenize(d))`. See
[Parallelism](../performance/parallelism.md).

## Performance characteristics

Asymptotically, every tokenizer here is **O(n) in the input length**, with these
qualifications:

| Family | Work per input | Notes |
|---|---|---|
| 12 character-class variants | one pass, one `matches!` per character | ASCII bytes are classified without decoding |
| `AggressiveTokenizerFa` | one `str::contains` prescan, then one pass | the prescan looks for a 14-character literal that essentially never occurs |
| `AggressiveTokenizerNo` / `Sv` | one diacritic pass, then one scanning pass | the first pass returns `Cow::Borrowed` unchanged for ASCII input |
| `AggressiveTokenizerHi` | one pass | copies only a token that contained a deleted character |
| `CaseTokenizer` | one pass (ASCII) or two full-string case conversions, three UTF-16 encodings and a pass (non-ASCII) | the ASCII fast path is byte-wise |
| `TreebankWordTokenizer` | one prescreen regex, up to 13 contraction `replace_all` passes, one padding pass, three more regex passes, then a whitespace split and a lockstep re-walk | the prescreen collapses thirteen scans into one when no contraction is present |
| `TokenizerJa` | ~46 table lookups per code unit, 30 of them dense array indexing | weight tables are `static`, not rebuilt per instance |
| `SentenceTokenizer` | four masking passes, one split, then one unmasking pass per sentence | unmasking short-circuits on sentences with no `&#123;&#123;` |
| `RegexpTokenizer` | whatever your pattern costs | you supply the regex |

Two implementation choices are worth knowing because they change the constant
factor:

- The character-class scanner is generic over a zero-sized `CharClass` type
  rather than taking a function pointer, so each tokenizer's predicate inlines
  into its own loop.
- `TokenizerJa`'s weight tables are `static` data. The reference rebuilds fifty
  hash tables on every `new TokenizerJa()`; here construction is free and 30 of
  the 46 lookups per character are a shift, an add and a load.

Benchmarks exist (`crates/verbora-tokenizers/benches/tokenizers.rs`,
`cargo bench -p verbora-tokenizers`) and measure three things: scaling across
document sizes 16→8192 words, cross-language cost on one fixed document, and the
three API shapes on identical input. **No results have been recorded yet** —
`benches/results/` contains the reference baselines for distance, inflectors,
ngrams, normalizers, phonetics and trie, but not tokenizers.

> Not yet benchmarked — see [Benchmarks](../benchmarks/index.md).

## Allocation behaviour

| Call | What is allocated |
|---|---|
| construction, 18 of the 24 types | nothing; they are zero-sized |
| construction, `CaseTokenizer` / `WordTokenizer` / `WordPunctTokenizer` / `OrthographyTokenizer` | nothing; one or two `bool`s on the stack |
| `RegexpTokenizer::new(pattern)` | nothing beyond the `Regex` you already built |
| `SentenceTokenizer::with_abbreviations(…)` | one `Vec<String>` of abbreviations, once |
| `tokens()` on a slicing tokenizer | nothing |
| `tokens()` on `AggressiveTokenizerNo`/`Sv` | one `String` **only if** an accent in the table was present |
| `tokens()` on `AggressiveTokenizerHi` | one `String` per token that contained a deleted character |
| `tokens()` on `AggressiveTokenizerFa` | one `Vec<(usize, usize)>` **only if** the 14-character `clearText` literal is present |
| `tokens()` on `CaseTokenizer`, ASCII input | nothing |
| `tokens()` on `CaseTokenizer`, non-ASCII input | two `String`s (the lowercased and uppercased text) and four `Vec<u16>` (lowercase, uppercase, source, result), then one token per non-ASCII run |
| `tokens()` on `TreebankWordTokenizer` | one scratch `String` per rewrite pass that fires — up to seventeen — plus one `Vec` of tokens |
| `tokens()` on `TokenizerJa` | several `Vec<u16>` the length of the input during normalisation, then a segment table and a character-type table of the same order, then one `Vec<u16>` per token |
| `tokens()` on `SentenceTokenizer` | one `String` per masking phase that fires, plus one `String` per sentence |
| `tokenize()` | the above, plus one `Vec` |
| `tokenize_into()` | the above, plus nothing if the buffer has capacity |
| `verbora_core::Tokenizer::tokenize` | the above, plus one `String` per token |
| `tokenize_batch` | the above per document, plus one outer `Vec` |

## Unicode and language notes

### Five the reference semantics that Rust does not share

Every one of these is a place where the obvious Rust translation silently
disagrees with the reference. They live in `verbora_tokenizers::whitespace`.

| Hazard | the reference | Rust's default | Consequence |
|---|---|---|---|
| `\w \W \b \d` | ASCII only | Unicode-aware | changes Italian tokenization and every Treebank contraction boundary |
| `/i` | the reference language `Canonicalize` | full simple case folding | Rust folds `ſ`→`s` and `K`→`k`; the reference does not |
| `\s` | includes U+FEFF, excludes U+0085 | the reverse | `SPACE_CLASS` and `is_whitespace` exist for this |
| `.` | refuses four line terminators | refuses only `\n` | `\r`, U+2028 and U+2029 survive as gap text in `WordPunctTokenizer` |
| `String#replace(string, …)` | replaces the **first** match | `str::replace` replaces all | changes Norwegian and Swedish on any repeated accent |

Character classes are therefore *generated*, by running each reference regex
over the whole BMP (`crates/verbora-tokenizers/tools/gen_classes`), rather
than transcribed. That is how the Russian class turned out to admit
U+1C80–U+1C86 and the Spanish class turned out to contain `×` and `÷`.

### Bugs reproduced on purpose

Each of these is a defect in the reference that this crate keeps, because the
reference is the executable specification. 
```rust
use verbora_tokenizers::{
    AggressiveTokenizer, AggressiveTokenizerDe, AggressiveTokenizerEs, AggressiveTokenizerFa,
    AggressiveTokenizerFr, AggressiveTokenizerHi, AggressiveTokenizerId, AggressiveTokenizerIt,
    AggressiveTokenizerRu, AggressiveTokenizerUk, Tokenize,
};

fn main() {
    // German: The reference class lists only the lowercase umlauts and has no
    // `i` flag, so `Ä`, `Ö` and `Ü` are separators.
    assert_eq!(
        AggressiveTokenizerDe::new().tokenize("Äpfel Öl Über weiß"),
        ["pfel", "l", "ber", "weiß"]
    );

    // Indonesian: the class `[^a-z0-9 -]` has no `i` flag, so every uppercase
    // ASCII letter is replaced by a space.
    assert_eq!(AggressiveTokenizerId::new().tokenize("Hello World-2 !!"), ["ello", "orld-2"]);
    assert_eq!(AggressiveTokenizerId::new().tokenize("A B"), Vec::<&str>::new());

    // Spanish: no digits at all, and the raw Latin-1 ranges `á-ú` / `Á-Ú`
    // sweep in `×` (U+00D7) and `÷` (U+00F7).
    assert_eq!(AggressiveTokenizerEs::new().tokenize("123 456"), Vec::<&str>::new());
    assert_eq!(AggressiveTokenizerEs::new().tokenize("a×b÷c"), ["a×b÷c"]);

    // English: accented letters are separators.
    assert_eq!(AggressiveTokenizer::new().tokenize("café naïve"), ["caf", "na", "ve"]);

    // Ukrainian drops `ё` from Russian's class, so it deletes the letter.
    assert_eq!(AggressiveTokenizerRu::new().tokenize("мир ёж"), ["мир", "ёж"]);
    assert_eq!(AggressiveTokenizerUk::new().tokenize("мир ёж"), ["мир", "ж"]);

    // Persian: the `clearText` regex forgot its brackets, so it is a *sequence*
    // that matches essentially nothing and punctuation stays attached.
    assert_eq!(
        AggressiveTokenizerFa::new().tokenize("weiß, daß Öl ist!"),
        ["weiß,", "daß", "Öl", "ist!"]
    );

    // Hindi deletes `.` before splitting, so a token need not be a substring.
    assert_eq!(AggressiveTokenizerHi::new().tokenize("a.b"), ["ab"]);

    // French's `i` flag admits uppercase accents, which German's lacks.
    assert_eq!(AggressiveTokenizerFr::new().tokenize("ÉCOLE Œuvre"), ["ÉCOLE", "Œuvre"]);

    // Italian splits on the reference's ASCII `\W+`.
    assert_eq!(AggressiveTokenizerIt::new().tokenize("привет, мир"), Vec::<&str>::new());
}
```

**The `CaseTokenizer` `"undefined"` bug.** The reference loop is bounded by
`lower.length` but indexes into `text`:

```text
for (i = 0; i < lower.length; ++i) { ... result += text[i] ... }
```

When lowercasing *lengthens* the string — `İ` (U+0130) lowercases to two code
units — `i` runs past the end of `text`, `text[i]` evaluates to `undefined`, and
the reference's string concatenation appends the nine characters `undefined`:

```rust
use verbora_tokenizers::{CaseTokenizer, Tokenize};

fn main() {
    let t = CaseTokenizer::new();
    assert_eq!(t.tokenize("İstanbul"), ["İstanbulundefined"]);

    // The mirror case is harmless: `ß` → `SS` lengthens the *upper*case string,
    // which does not bound the loop.
    assert_eq!(t.tokenize("ß"), ["ß"]);

    // Apostrophes are dropped unless you ask for them, and only U+0027 counts.
    assert_eq!(t.tokenize("it's"), ["it", "s"]);
    assert_eq!(CaseTokenizer::preserving_apostrophes().tokenize("it's"), ["it's"]);
    assert_eq!(CaseTokenizer::preserving_apostrophes().tokenize("it’s"), ["it", "s"]);

    // Uncased scripts vanish entirely.
    assert_eq!(t.tokenize("日本語"), Vec::<&str>::new());
}
```

**First-occurrence-only diacritic removal.** `normalizer_no.removeDiacritics` is
twenty-six `text.replace('à', 'a')` calls, and passing a *string* as the first
argument to `String.prototype.replace` replaces only the first occurrence.
Rust's `str::replace` replaces all of them, which is a silent divergence on any
text with a repeated accent:

```rust
use verbora_tokenizers::{AggressiveTokenizerNo, AggressiveTokenizerSv, Tokenize};

fn main() {
    // Only the first `à` is normalised; the rest remain non-word characters.
    assert_eq!(AggressiveTokenizerNo::new().tokenize("àà ààà"), ["a"]);

    // `-` is a word character in Swedish and a separator in Norwegian.
    assert_eq!(AggressiveTokenizerSv::new().tokenize("e-post"), ["e-post"]);
    assert_eq!(AggressiveTokenizerNo::new().tokenize("e-post"), ["e", "post"]);
}
```

**Treebank's `Whadddya` rule.** The reference's regex is
`\b(Whad)(dd)(ya)\b`, which requires the literal `Whadddya` with three `d`s, so
it never fires on real text. Fixing it would change `tokenize("Whaddya")` from
one token to three, and fail its tests. Treebank's final-period rule is also
position-dependent — `\. *(\n|$)` has no `m` flag, so the same sentence yields
`"home."` mid-text and `"home", "."` at the end of the input:

```rust
use verbora_tokenizers::{Tokenize, TreebankWordTokenizer};

fn main() {
    let t = TreebankWordTokenizer::new();
    assert_eq!(
        t.tokenize("If we 'all' can't go. I'll stay home."),
        ["If", "we", "'all", "'", "ca", "n't", "go.", "I", "'ll", "stay", "home", "."]
    );
    assert_eq!(t.tokenize("a+b<c>d&e/f-g"), ["a+b<c>d&e/f-g"]);
    assert_eq!(t.tokenize("e.g. U.S.A."), ["e.g.", "U.S.A", "."]);
}
```

**`SentenceTokenizer`'s unresolved placeholders.** Masking happens in four
phases sharing one counter, and unmasking is a single ordered pass, not a
fixpoint. Abbreviations are masked before URIs, so a URI whose stored text
contains `&#123;&#123;ABBREV_n}}` is never resolved — the placeholder reaches the output.
Sentence-final periods are also swallowed by the greedy `\S+` in the URI
pattern, and whitespace-only input is one empty sentence rather than none:

```rust
use verbora_tokenizers::{SentenceTokenizer, SentenceTokenizerNew, Tokenize};

fn main() {
    let t = SentenceTokenizer::new();
    assert_eq!(t.tokenize("Hi. There!"), ["Hi.", "There!"]);

    // Array-level trim runs before per-sentence trim, so these survive.
    assert_eq!(t.tokenize("   "), [""]);
    assert_eq!(t.tokenize("Trailing space. "), ["Trailing space.", ""]);

    // Abbreviation matching is case-insensitive and ordered.
    let abbrev = SentenceTokenizer::with_abbreviations(["Dr.", "Mr."]);
    assert_eq!(
        abbrev.tokenize("Dr. Smith went home. He slept."),
        ["Dr. Smith went home.", "He slept."]
    );

    let untrimmed = SentenceTokenizer::new().trimming(false);
    assert_eq!(
        untrimmed.tokenize("This is a sentence. This is another sentence."),
        ["This is a sentence.", " This is another sentence."]
    );

    // The two the reference export names are one type.
    let _: SentenceTokenizer = SentenceTokenizerNew::new();
}
```

`SentenceTokenizer`'s second constructor argument deserves a warning of its own:
`index.d.ts` and the reference's spec both suggest a list of sentence
*demarkers*, but the implementation names the parameter `trimSentences` and only
tests it for truthiness. Passing `['.', '!', '?']` means "yes, trim". There is no
demarker feature, so Verbora exposes `.trimming(bool)` and nothing else.

### Japanese

`TokenizerJa` is TinySegmenter 0.1 with the reference's own normalisation step
(full-width to half-width, half-width katakana to full-width) applied first. It
is the only tokenizer in the reference that null-checks its argument, so empty
input returns `[]` rather than throwing; Rust models "no text" as `""`. It also
strips punctuation from *inside* tokens and drops tokens that empty as a result.

```rust
use verbora_tokenizers::{Tokenize, TokenizerJa};

fn main() {
    let t = TokenizerJa::new();
    assert_eq!(t.tokenize("日本語"), ["日本語"]);
    assert_eq!(t.tokenize("Hello, world!"), ["Hello", "world"]);
    assert_eq!(t.tokenize("。。。"), Vec::<&str>::new());
    // Normalisation runs before segmentation.
    assert_eq!(t.tokenize("ﾊﾝｶｸ"), ["ハンカク"]);
}
```

## Common mistakes

**Importing both `tokenize` traits.** `verbora_tokenizers::Tokenize` and
`verbora_core::Tokenizer` both have a `tokenize` method, so an unqualified call
with both in scope is `error[E0034]: multiple applicable items in scope`:

```rust  ignore
use verbora_core::Tokenizer;
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let v = t.tokenize("a b");   // error[E0034]
```

Import only the one you need, or disambiguate explicitly:

```rust
use verbora_core::Tokenizer as CoreTokenizer;
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    let borrowed: Vec<&str> = Tokenize::tokenize(&t, "the quick");
    let owned: Vec<String> = CoreTokenizer::tokenize(&t, "the quick");
    assert_eq!(borrowed, ["the", "quick"]);
    assert_eq!(owned, ["the", "quick"]);
}
```

**Forgetting `buf.clear()`.** `tokenize_into` appends. See the warning above.

**Treating `None` as "empty".** For the four optional tokenizers, `None` means
`String#match` returned `null`, which is not the same as returning `[]`. If your
application does not need the distinction, collapse it explicitly with
`.unwrap_or_default()` so the decision is visible in the code.

**Calling `tokenize_batch` for speed.** It is a sequential `map` over
`tokenize`, allocating a fresh `Vec` per document. It is a convenience, not an
optimisation.

**Assuming `to_string_lossy()` is lossless.** It substitutes U+FFFD for unpaired
surrogates. Two different surrogate halves render identically, so a comparison
on lossy strings can report a false match. Compare on `to_utf16()`.

**Collecting `as_str()` out of an iterator.** `Utf16Token::as_str` returns
`Option<&str>` borrowed from *the token*, not from the input, so this does not
compile (`error[E0515]`):

```rust  ignore
let words: Vec<&str> = TreebankWordTokenizer::new()
    .tokens(text)
    .filter_map(|t| t.as_str())   // error[E0515]: returns a value referencing
    .collect();                   // data owned by the current function
```

Collect the tokens first and borrow from those, as in the example above.

**Expecting a token to be a substring.** It is, for the thirteen tokenizers that
implement `BorrowingTokenizer`, for `WordTokenizer`, and for
`WordPunctTokenizer` and `OrthographyTokenizer` in splitting mode. It is not for
`AggressiveTokenizerHi` (deletes characters), `AggressiveTokenizerNo` and
`AggressiveTokenizerSv` (rewrite diacritics), `TokenizerJa` (strips punctuation
from inside tokens and rebuilds them from code units) or `SentenceTokenizer`
(substitutes placeholders). `TreebankWordTokenizer` maps every token back onto a
byte range of the input and so borrows in practice, but its `Utf16Token` type
does not promise to.

**Using the wrong tokenizer for a language.** `AggressiveTokenizerEs` and
`AggressiveTokenizerPt` have **no digits** in their classes;
`OrthographyTokenizer::new("fi")` has no digits either. If your Spanish corpus
contains numbers, they silently disappear.

## Related

- [Choosing an API: tokenization](../choosing/tokenization.md) — the long-form
  version of the section above, with pipeline diagrams.
- [API shapes](../choosing/api-shapes.md) — the workspace-wide convention that
  `_into` appends and `tokens()` is the primitive.
- [Core traits](core.md) — `verbora_core::Tokenizer`, `BorrowingTokenizer`,
  `trim_edge_empties`.
- [n-grams](ngrams.md) — consumes a tokenizer, and carries a process-global
  tokenizer binding that the reference also has.
- [Normalizers](normalizers.md) — what to run before or after tokenizing.
  deliberately does *not* match.
- [Performance](../performance/index.md), and in detail:
  [Zero-copy](../performance/zero-copy.md),
  [Buffer reuse](../performance/buffer-reuse.md),
  [Iterator vs `_into`](../performance/iterator-vs-into.md),
  [Allocation](../performance/allocation.md),
  [Batch vs streaming](../performance/batch-vs-streaming.md),
  [Parallelism](../performance/parallelism.md).
- [Benchmarks](../benchmarks/index.md).
- [Recipes](../recipes/index.md).

## API reference

Generate the rustdoc locally:

```bash
cargo doc -p verbora-tokenizers --no-deps --open
```

Once published, the same content is at
<https://docs.rs/verbora-tokenizers/latest/verbora_tokenizers/>. The items you
will use most often:

| Item | Path |
|---|---|
| `Tokenize` | `verbora_tokenizers::Tokenize` |
| `Utf16Token` | `verbora_tokenizers::Utf16Token` |
| `Pattern` | `verbora_tokenizers::Pattern` |
| `trim_edge_empties` | `verbora_tokenizers::trim_edge_empties` |
| the reference string semantics | `verbora_tokenizers::whitespace` |
| Generated character classes | `verbora_tokenizers::classes` |
| The shared scanner | `verbora_tokenizers::scan` |
| `Tokenizer`, `BorrowingTokenizer` | `verbora_core` |

Source: `crates/verbora-tokenizers/src/`. Benchmarks:
`crates/verbora-tokenizers/benches/tokenizers.rs`.
