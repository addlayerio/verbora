# Tokenizers

`verbora-tokenizers` splits text into tokens, twenty-five ways: sixteen
"aggressive" language splitters, four regex-driven ones, a Penn Treebank word
tokenizer, a case-based splitter, a Japanese segmenter and a sentence splitter.

Every tokenizer is built on a lazy iterator, and the convenience methods are
defined on top of that iterator, so there is one implementation of each behaviour
and no second copy to drift. Every tokenizer's output is byte-exact and pinned by
the crate's regression suite.

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>25</strong> tokenizer APIs are
documented and test-pinned. They are <strong>24 Rust types</strong> —
<code>SentenceTokenizerNew</code> is an alias for <code>SentenceTokenizer</code> —
and equality is pinned on UTF-16 code units rather than <code>String</code>,
because four of them can split inside a surrogate pair.
<code>cargo test -p verbora-tokenizers</code> runs <strong>72</strong> unit tests
and <strong>16</strong> doctests.
</div>

## When to use it

- You want a fast, allocation-light word splitter for Latin-script text and one
  of the language character classes below is the one you want.
- You need sentence segmentation with abbreviation, URI and number protection.
- You need Japanese word segmentation without a dictionary or a model file.

## When not to use it

- **You want Unicode-aware `\w` semantics.** This crate's `\w`, `\W`, `\b` and
  `\d` classes are ASCII-only. Unless a tokenizer's language class specifically
  lists an accented letter, that letter is a *separator*:
  `AggressiveTokenizer::tokenize("café naïve")` is `["caf", "na", "ve"]`.
- **You want linguistically ideal tokenization.** These implement specific,
  quirky character-class specifications exactly — see
  [Quirks kept on purpose](#quirks-kept-on-purpose). Those outcomes are pinned,
  not accidents.
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

Construction is free: eighteen of the twenty-four types are zero-sized, four carry
one or two `bool`s, and only `RegexpTokenizer` (a compiled pattern) and
`SentenceTokenizer` (an abbreviation list) own anything on the heap.

## The catalogue

**Token** is what one token *is*. **`Tokenize`** is this crate's iterator trait;
**`Tokenizer`** and **`Borrowing`** are [`verbora_core::Tokenizer`](core.md) and
`verbora_core::BorrowingTokenizer`, the shared vocabulary other Verbora crates are
written against.

### Aggressive / language family (16)

All sixteen emit maximal runs of a per-language character class, except where
noted. Every class is *generated* — each language's defining regular expression is
expanded over the whole Basic Multilingual Plane rather than transcribed by hand,
which is why the surprises below are exact.

| Type | Splits on | Token | `Tokenize` | `Tokenizer` | `Borrowing` |
|---|---|---|:--:|:--:|:--:|
| `AggressiveTokenizer` (English) | runs of `A-Z a-z 0-9 ' - /` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerNl` | as English, but `_` is a word character and `/` is a separator | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerDe` | ASCII alphanumerics, `ß ä ö ü _ ' -` — **not** `Ä Ö Ü` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerFr` | ASCII alphanumerics, `-`, accented Latin-1 vowels and `œ ç` in both cases | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerEs` | letters plus `U+00C1–U+00DA`, `U+00E1–U+00FA`, `Ü ü`. **No digits** | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerIt` | `A-Z a-z 0-9 _` only — ASCII `\W` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerPt` | letters plus `U+00C0–U+00DA`, `U+00E0–U+00FA`. **No digits** | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerVi` | ASCII alphanumerics plus the Vietnamese vowel set in both cases | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerRu` | ASCII alphanumerics, `А-я`, `Ё ё`, and `U+1C80–U+1C86` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerUk` | as Russian but **without** `Ё ё`, plus `Ґ ґ Є є І і Ї ї` | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerPl` | ASCII alphanumerics plus `ą ć ę ł ń ó ś ź ż` in both cases | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerId` | `a-z 0-9 -` — **lowercase only** | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerFa` | whitespace runs only; punctuation stays attached to tokens | `&str` | ✅ | ✅ | ✅ |
| `AggressiveTokenizerNo` | strips 13 diacritics (first occurrence of each only), then splits on `A-Z a-z 0-9 _ æøå ÆØÅ äÄöÖüÜ`. `-` is a separator | `Cow<'_, str>` | ✅ | ✅ | ❌ |
| `AggressiveTokenizerSv` | strips `à á è é` and their uppercase forms (first occurrence only), then splits on `A-Z a-z 0-9 _ åÅäÄöÖüÜ -` | `Cow<'_, str>` | ✅ | ✅ | ❌ |
| `AggressiveTokenizerHi` | deletes `। ॥ . ? ,`, then splits on whitespace and on anything outside Devanagari and ASCII | `Cow<'_, str>` | ✅ | ✅ | ❌ |

Thirteen of these yield `&str` and implement `BorrowingTokenizer`: every token is
a contiguous slice of your input, so tokenizing allocates nothing per token. The
three that yield `Cow` rewrite the text before splitting, so they *can* borrow —
and do, whenever the rewrite was a no-op — but cannot promise to.

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
`verbora_core::Tokenizer` by rendering unpaired surrogates as U+FFFD, because that
trait's contract is `Vec<String>` and a `String` cannot hold one. When exactness
matters, use `Tokenize::tokens` and handle
[`Utf16Token`](#utf-16-tokens-and-unpaired-surrogates) yourself.

## Choosing the right API

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

That is the whole trait. `tokenize` is `tokens().collect()` and nothing else;
`tokenize_into` is `out.extend(tokens())` and nothing else. In particular
**`tokenize_into` does not clear `out`** — it appends.

| API | Best for | Lazy | Buffer reuse | Allocations |
|---|---|:--:|:--:|---|
| `Tokenize::tokens` | streaming, folding, early exit | ✅ | n/a | none for the 13 slicers |
| `Tokenize::tokenize` | one document, simplest call | ❌ | ❌ | one `Vec`, grown by doubling |
| `Tokenize::tokenize_into` | a corpus through one buffer | ❌ | ✅ | none once the buffer is warm |
| `Tokenize::par_tokenize_batch` | many independent documents, feature `parallel` | ❌ | ❌ | one outer `Vec`, plus whatever `tokenize` allocates per document |
| `verbora_core::Tokenizer::tokenize` | generic code over any tokenizer | ❌ | ❌ | one `Vec` **plus one `String` per token** |
| `verbora_core::Tokenizer::tokenize_into` | generic code, warm buffer | ❌ | ✅ (the `Vec`) | one `String` per token |
| `verbora_core::Tokenizer::tokenize_batch` | a slice of documents, one call | ❌ | ❌ | one outer `Vec`, one inner `Vec` and one `String` per token |
| `verbora_core::BorrowingTokenizer::tokenize_borrowed_into` | generic code, zero-copy | ❌ | ✅ | none once warm |

Name a concrete tokenizer and you get `Tokenize`; write code generic over "any
tokenizer" and you need `verbora_core::Tokenizer` (owned `String`s) or
`BorrowingTokenizer` (slices of the input, 13 of the 24 types). The four optional
tokenizers implement neither and expose the same three method names inherently,
wrapped in `Option`. The long-form version of this decision, with pipeline
diagrams, is on [Choosing an API: tokenization](../choosing/tokenization.md).

### `tokens()` — the primitive

Lazy for 17 of the 20 `Tokenize` types, and allocation-free for the 13 slicing
tokenizers. The exceptions: `tokens()` on `TreebankWordTokenizer`, `TokenizerJa`
and `SentenceTokenizer` is neither lazy nor allocation-free, and
`SentenceTokenizer`'s tokens are owned `String`s.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();

    // Stops as soon as it finds a match; the rest of the document is never
    // scanned, and no `Vec` is ever built.
    assert!(t.tokens("the quick brown fox").any(|w| w == "quick"));

    // Tokens borrow the input, so they are `HashMap` keys with no `String`
    // allocation at all.
    let long_words = t.tokens("the quick brown fox").filter(|w| w.len() > 3).count();
    assert_eq!(long_words, 2);
}
```

### `tokenize()` — the simple one

`Vec<Self::Token<'a>>`, one `Vec`; the *tokens* may still borrow. The `Vec` starts
empty and grows by reallocation, because none of these iterators reports a useful
`size_hint` lower bound and `collect` reserves from the lower bound. If you know
roughly how many tokens to expect, `tokenize_into` with a pre-reserved buffer
avoids the growth.

### `tokenize_into()` — the hot loop

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

Accumulating deliberately is the same call without the `clear`.

One lifetime constraint follows from zero-copy: `Vec<Self::Token<'a>>` ties the
buffer to `'a`, so a buffer holding `&'a str` can only be reused across documents
that all outlive the loop. If your documents come from a `String` dropped each
iteration, move the `Vec` inside the loop or switch to
`verbora_core::Tokenizer::tokenize_into`, whose `Vec<String>` owns its contents.

### The `verbora_core` traits

Twenty of the twenty-four types implement `verbora_core::Tokenizer`; thirteen also
implement `BorrowingTokenizer` (the twelve character-class variants and
`AggressiveTokenizerFa`).

```rust
use verbora_core::{BorrowingTokenizer, Tokenizer};
use verbora_tokenizers::AggressiveTokenizer;

fn main() {
    let t = AggressiveTokenizer::new();
    let owned: Vec<String> = Tokenizer::tokenize(&t, "the quick");
    assert_eq!(owned, ["the", "quick"]);

    let batch: Vec<Vec<String>> = t.tokenize_batch(&["one two", "three four"]);
    assert_eq!(batch[1], ["three", "four"]);

    // Zero-copy, and generic: `tokenize_borrowed_into` appends, like every
    // `_into` method in this crate.
    let mut buf: Vec<&str> = Vec::new();
    t.tokenize_borrowed_into("the quick brown fox", &mut buf);
    assert_eq!(buf.len(), 4);
}
```

`tokenize_batch` is a provided method whose body is
`texts.iter().map(|t| self.tokenize(t.as_ref())).collect()`, and no tokenizer here
overrides it: each document gets a fresh `Vec`. It buys a shorter line of code,
not fewer allocations. Because it is generic, `Tokenizer` is also **not object
safe** — you cannot hold one behind `dyn Tokenizer`. (`verbora-ngrams` works
around that with its own `dyn`-compatible `NGramTokenizer` and a blanket impl.)

### The four optional tokenizers

`RegexpTokenizer`, `WordTokenizer`, `OrthographyTokenizer` and
`WordPunctTokenizer` expose `tokens()`, `tokenize()` and `tokenize_into()` as
inherent methods wrapped in `Option`. In matching mode (`gaps: false`), "no match"
and "no tokens" are genuinely different outcomes that a plain `Vec` would merge,
so `tokens()` returns `Option<…>`, `tokenize()` returns `Option<Vec<…>>`, and
`tokenize_into()` returns `bool` — `false` meaning "no match", in which case
nothing was appended.

```rust
use verbora_tokenizers::{OrthographyTokenizer, WordTokenizer};

fn main() {
    let t = WordTokenizer::new();
    assert_eq!(
        t.tokenize("She said 'hello'. Привет мир 123_456"),
        Some(vec!["She", "said", "hello", "Привет", "мир", "123_456"])
    );
    assert_eq!(t.tokenize(""), Some(vec![]));   // splitting mode never returns None

    // Matching mode can: no match at all is a real, distinct outcome.
    let m = WordTokenizer::matching();
    assert_eq!(m.tokenize("abc def"), Some(vec![" "]));
    assert_eq!(m.tokenize("abcdef"), None);
    // If you do not care about the distinction, say so explicitly.
    assert!(m.tokenize("abcdef").unwrap_or_default().is_empty());

    // Language matching is exact and lowercase: "FI" is not "fi", so it falls
    // back to `WordTokenizer`, which does not know the Finnish alphabet.
    let fi = OrthographyTokenizer::new("fi");
    assert_eq!(fi.tokenize("Hyvää, kiitos!!  entä").unwrap(), ["Hyvää", "kiitos", "entä"]);
    assert_eq!(OrthographyTokenizer::new("FI").tokenize("Hyvää").unwrap(), ["Hyv"]);
}
```

`RegexpTokenizer` adds a **second** layer of `Option`, on each token: splitting
with capture groups interleaves the groups into the result, and a group that did
not participate is `None`. The full return type is `Option<Vec<Option<&str>>>` —
outer "no match at all", inner "this capture group did not participate".
Constructing a `Pattern` needs the `regex` crate as a direct dependency of *your*
package, which is why this block is not compiled by the site:

```rust  ignore
use verbora_tokenizers::{Pattern, RegexpTokenizer};
use regex::Regex;

// `gaps: true` (the default) — split on the pattern.
let split = RegexpTokenizer::new(Pattern::new(Regex::new(r"[^A-Za-z0-9_]+").unwrap()));
assert_eq!(split.tokenize("hello, world"), Some(vec![Some("hello"), Some("world")]));

// Capture groups are interleaved; a group that did not participate is `None`.
let grouped = RegexpTokenizer::new(Pattern::new(Regex::new(r"(x)|([0-9])").unwrap()));
assert_eq!(grouped.tokenize("a1b"), Some(vec![Some("a"), None, Some("1"), Some("b")]));

// `gaps: false` — match with the pattern. A global pattern that finds nothing
// returns `None`.
let matching = RegexpTokenizer::matching(Pattern::global(Regex::new("[a-z]+").unwrap()));
assert_eq!(matching.tokenize("123"), None);
```

Two things are not configurable: empty tokens are always discarded, and
`WordTokenizer`'s character class is fixed, so it takes no pattern argument.
`gaps` is honoured by all four, with one wrinkle — `OrthographyTokenizer`'s
fallback path builds a default `WordTokenizer` without forwarding `gaps`, so an
**unknown language silently discards it**. Only `fi` is defined in the language
table, and `OrthographyTokenizer::new` requires a `&str`, so a missing language is
a compile error rather than a runtime surprise.

## UTF-16 tokens and unpaired surrogates

<span class="badge badge-utf16">UTF-16</span>

Four tokenizers cut text at UTF-16 **code unit** boundaries —
`WordPunctTokenizer`, `TreebankWordTokenizer`, `TokenizerJa` and `CaseTokenizer` —
and `OrthographyTokenizer` joins them in *matching* mode only. That is why their
token type is `Utf16Token` rather than `&str`.

For an astral character such as `😀`, the two halves of the surrogate pair land in
*separate* tokens. An unpaired surrogate is not a Unicode scalar value, so `char`,
`String` and `&str` cannot hold it, and an implementation yielding `String` would
have to substitute U+FFFD (wrong content), merge the halves (wrong token *count*)
or drop them (wrong both). Verbora returns `Utf16Token` instead:

```rust  ignore
pub enum Utf16Token<'a> {
    Text(Cow<'a, str>),   // well-formed; borrowed when the tokenizer only sliced
    Raw(Box<[u16]>),      // not well-formed — in practice, half a surrogate pair
}
```

```rust
use verbora_tokenizers::{Tokenize, TreebankWordTokenizer, Utf16Token, WordPunctTokenizer};

fn main() {
    let t = WordPunctTokenizer::new();
    let tokens: Vec<Utf16Token<'_>> = t.tokenize("a😀b").unwrap();

    assert_eq!(tokens.len(), 4);
    assert_eq!(tokens[0].as_str(), Some("a"));
    // The emoji became two tokens, neither representable as `&str`.
    assert_eq!(tokens[1].as_str(), None);
    assert_eq!(tokens[1].to_utf16(), vec![0xd83d]);
    assert_eq!(tokens[1].to_string_lossy(), "\u{fffd}");

    // Getting back to strings: keep only the well-formed tokens. `as_str`
    // borrows from the token, so collect the tokens first.
    let treebank: Vec<Utf16Token<'_>> = TreebankWordTokenizer::new().tokenize("I'll stay home.");
    let well_formed: Vec<&str> = treebank.iter().filter_map(|t| t.as_str()).collect();
    assert_eq!(well_formed, ["I", "'ll", "stay", "home", "."]);
}
```

The representation costs nothing on ordinary text: `WordPunctTokenizer`,
`OrthographyTokenizer`, `TreebankWordTokenizer` and `CaseTokenizer` (on ASCII
input) all yield `Text(Cow::Borrowed(_))` — a slice of your input — for every
token that is not a surrogate half. `TokenizerJa` is the exception: it builds each
token by concatenating code units, so its tokens are always owned. The other way
back to strings is `to_string_lossy()`, accepting U+FFFD for the surrogate halves,
which is exactly what `verbora_core::Tokenizer::tokenize` does for those three
types. `to_utf16()` is the lossless view, and what the test suite compares on.

### `trim_edge_empties`

Pops trailing empty strings and shifts leading ones, leaving *interior* empties
alone. That asymmetry is load-bearing — `SentenceTokenizer::tokenize("   ")` is
`[""]` rather than `[]` because of it — so it is re-exported from this crate
rather than generalised.

```rust
use verbora_tokenizers::trim_edge_empties;

fn main() {
    let mut v = vec!["", "", "a", "", "b", "", ""];
    trim_edge_empties(&mut v);
    assert_eq!(v, ["a", "", "b"]);
}
```

## Parallelism

`Tokenize::par_tokenize_batch` is the one built-in parallel API: a **default trait
method** behind this crate's `parallel` Cargo feature (`parallel = ["dep:rayon"]`,
never on by default), so all twenty `Tokenize` implementors get it for free. Its
whole body is `texts.par_iter().map(|text| self.tokenize(text)).collect()` — one
task per **document**, never per token. The four optional tokenizers implement
neither trait and so have no `par_tokenize_batch`.

```rust  ignore
// Needs the `parallel` feature, which this site's snippet checker builds
// without — so this block is marked `ignore` rather than compiled. Every
// other block on this page compiles and runs in CI.
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let docs = ["the quick brown fox", "jumps over the lazy dog"];
let batches: Vec<Vec<&str>> = t.par_tokenize_batch(&docs);
assert_eq!(batches[0], ["the", "quick", "brown", "fox"]);
```

**When it pays.** A single `tokenize` call over an ~8192-word document measures at
roughly 118–120 µs, and a `rayon` task costs on the order of a microsecond to
schedule, so per-document parallelism only wins once the batch or the documents
are large enough that scheduling is a small fraction of the total. For a handful
of short strings, a plain `texts.iter().map(…)` loop is faster. Every tokenizer
here is stateless and `Send + Sync`, so you can also roll your own fan-out — with
`rayon` in your own `Cargo.toml` and no Cargo feature required. See
[Parallelism](../performance/parallelism.md).

## Performance and allocation

Every tokenizer is **O(n) in the input length**; only the constant factor differs.
The character-class variants are a single pass with one `matches!` per character,
classifying ASCII bytes without decoding — the cheapest thing here.
`CaseTokenizer` costs one byte-wise pass on ASCII and two full-string case
conversions plus three UTF-16 encodings on anything else. `TreebankWordTokenizer`
(up to seventeen rewrite passes), `TokenizerJa` (~46 table lookups per code unit)
and `SentenceTokenizer` (four masking passes plus per-sentence unmasking) are the
expensive three. `RegexpTokenizer` costs whatever your pattern costs.

Allocation stacks up from `tokens()`:

- **Construction allocates nothing** for 22 of the 24 types. `RegexpTokenizer`
  holds the `Regex` you built; `SentenceTokenizer::with_abbreviations` holds one
  `Vec<String>`.
- **`tokens()` allocates nothing** for the 13 slicing tokenizers, and nothing for
  `CaseTokenizer` on ASCII input. `No`/`Sv`/`Hi`/`Fa` allocate only if their
  rewrite actually fires. `TreebankWordTokenizer` allocates one scratch `String`
  per rewrite pass that fires (up to seventeen), `TokenizerJa` several
  input-length `Vec<u16>` plus one per token, and `SentenceTokenizer` one `String`
  per masking phase and one per sentence.
- **`tokenize()`** adds one `Vec`; **`tokenize_into()`** adds nothing once the
  buffer has capacity; **`verbora_core::Tokenizer::tokenize`** adds one `String`
  per token; **`tokenize_batch`** adds one outer `Vec` on top of that per document.

No tokenizer benchmark results are published yet — see
[Benchmarks](../benchmarks/index.md) for what has been measured so far. The
Criterion suite (`crates/verbora-tokenizers/benches/tokenizers.rs`) covers scaling
across document sizes 16→8192 words, cross-language cost on one fixed document,
and the three API shapes on identical input.

## Unicode and language notes

### Five semantics this crate defines for itself

Each is a place where Rust's own default gives a different answer from what these
tokenizers specify. They live in `verbora_tokenizers::whitespace`.

| Hazard | This crate's semantics | Rust's default | Consequence |
|---|---|---|---|
| `\w \W \b \d` | ASCII only | Unicode-aware | changes Italian tokenization and every Treebank contraction boundary |
| Case-insensitive matching | language-specific case rules only | full simple case folding | Rust folds `ſ`→`s` and `K`→`k`; this crate does not |
| `\s` | includes U+FEFF, excludes U+0085 | the reverse | `SPACE_CLASS` and `is_whitespace` exist for this |
| `.` | refuses four line terminators | refuses only `\n` | `\r`, U+2028 and U+2029 survive as gap text in `WordPunctTokenizer` |
| First-match string replacement | replaces the **first** match | `str::replace` replaces all | changes Norwegian and Swedish on any repeated accent |

Character classes are *generated*, by expanding each language's defining regular
expression over the whole BMP, rather than transcribed. That is why the Russian
class admits U+1C80–U+1C86 and the Spanish class contains `×` and `÷`.

### Quirks kept on purpose

Each of the following looks like a defect and is specified behaviour, pinned by
the regression suite so it stays predictable across releases.

```rust
use verbora_tokenizers::{
    AggressiveTokenizer, AggressiveTokenizerDe, AggressiveTokenizerEs, AggressiveTokenizerFa,
    AggressiveTokenizerHi, AggressiveTokenizerId, AggressiveTokenizerIt, AggressiveTokenizerUk,
    Tokenize,
};

fn main() {
    // German lists only the lowercase umlauts, so `Ä`, `Ö`, `Ü` are separators.
    assert_eq!(AggressiveTokenizerDe::new().tokenize("Äpfel Öl weiß"), ["pfel", "l", "weiß"]);

    // Indonesian is lowercase-only: every uppercase ASCII letter is a separator.
    assert_eq!(AggressiveTokenizerId::new().tokenize("Hello World-2"), ["ello", "orld-2"]);

    // Spanish has no digits at all, and its Latin-1 ranges sweep in `×` and `÷`.
    assert_eq!(AggressiveTokenizerEs::new().tokenize("123 456"), Vec::<&str>::new());
    assert_eq!(AggressiveTokenizerEs::new().tokenize("a×b÷c"), ["a×b÷c"]);

    // English: accented letters are separators.
    assert_eq!(AggressiveTokenizer::new().tokenize("café naïve"), ["caf", "na", "ve"]);

    // Ukrainian drops `ё` from Russian's class, so it deletes the letter.
    assert_eq!(AggressiveTokenizerUk::new().tokenize("мир ёж"), ["мир", "ж"]);

    // Persian splits on whitespace only; punctuation stays attached.
    assert_eq!(AggressiveTokenizerFa::new().tokenize("Öl ist!"), ["Öl", "ist!"]);

    // Hindi deletes `.` before splitting, so a token need not be a substring.
    assert_eq!(AggressiveTokenizerHi::new().tokenize("a.b"), ["ab"]);

    // Italian splits on an ASCII-only `\W+` class.
    assert_eq!(AggressiveTokenizerIt::new().tokenize("привет, мир"), Vec::<&str>::new());
}
```

The structural tokenizers have four more, all pinned:

- **`CaseTokenizer` appends the literal string `undefined`** when lowercasing
  *lengthens* the text. Its filtering loop is bounded by the lowercased string's
  length in UTF-16 code units, so `İ` (U+0130), which lowercases to two units,
  runs the index past the end of the original. The mirror case (`ß` → `SS`) is
  harmless. It also drops apostrophes unless you ask for them, and only U+0027
  counts; uncased scripts vanish entirely.
- **`TreebankWordTokenizer`'s final-period rule is position-dependent**
  (`\. *(\n|$)` has no multi-line flag), so the same sentence yields `"home."`
  mid-text and `"home", "."` at the end of the input.
- **`SentenceTokenizer` trims at the array level** before the per-sentence trim,
  so whitespace-only input is one empty sentence rather than none. Delimiters are
  fixed; `.trimming(bool)` is the only option.
- **`TokenizerJa` normalises before segmenting** (full-width to half-width,
  half-width katakana to full-width), strips punctuation from *inside* tokens and
  drops tokens that empty as a result.

```rust
use verbora_tokenizers::{
    CaseTokenizer, SentenceTokenizer, Tokenize, TokenizerJa, TreebankWordTokenizer,
};

fn main() {
    let c = CaseTokenizer::new();
    assert_eq!(c.tokenize("İstanbul"), ["İstanbulundefined"]);
    assert_eq!(c.tokenize("it's"), ["it", "s"]);
    assert_eq!(CaseTokenizer::preserving_apostrophes().tokenize("it's"), ["it's"]);
    assert_eq!(c.tokenize("日本語"), Vec::<&str>::new());

    assert_eq!(TreebankWordTokenizer::new().tokenize("e.g. U.S.A."), ["e.g.", "U.S.A", "."]);

    let s = SentenceTokenizer::new();
    assert_eq!(s.tokenize("Hi. There!"), ["Hi.", "There!"]);
    assert_eq!(s.tokenize("   "), [""]);
    // Abbreviation matching is case-insensitive and ordered.
    let abbrev = SentenceTokenizer::with_abbreviations(["Dr.", "Mr."]);
    assert_eq!(
        abbrev.tokenize("Dr. Smith went home. He slept."),
        ["Dr. Smith went home.", "He slept."]
    );

    assert_eq!(TokenizerJa::new().tokenize("ﾊﾝｶｸ"), ["ハンカク"]);
}
```

The Norwegian and Swedish diacritic pass replaces only the *first* occurrence of
each of twenty-six accented characters, so `AggressiveTokenizerNo::tokenize("àà ààà")`
is `["a"]`. Swapping in Rust's `str::replace`, which replaces all matches, changes
the output on any text with a repeated accent.

## Common mistakes

**Importing both `tokenize` traits.** `verbora_tokenizers::Tokenize` and
`verbora_core::Tokenizer` both have a `tokenize` method, so an unqualified call
with both in scope is `error[E0034]`. Import only the one you need, or
disambiguate explicitly: `Tokenize::tokenize(&t, text)` for borrowed tokens,
`Tokenizer::tokenize(&t, text)` for owned `String`s.

**Forgetting `buf.clear()`.** Every `_into` method here appends. (In
`verbora_core`, `Stemmer::stem_into` is the one exception — it clears first.)

**Treating `None` as "empty".** For the four optional tokenizers, `None` means
"the pattern did not match at all". If you do not need the distinction, collapse
it explicitly with `.unwrap_or_default()`.

**Calling `tokenize_batch` for speed.** It is a sequential `map` over `tokenize`,
allocating a fresh `Vec` per document. It is a convenience, not an optimisation.

**Assuming `to_string_lossy()` is lossless.** It substitutes U+FFFD, so two
different surrogate halves render identically and a comparison on lossy strings
can report a false match. Compare on `to_utf16()`.

**Collecting `as_str()` straight out of an iterator.** `Utf16Token::as_str`
borrows from *the token*, not from the input, so
`.tokens(text).filter_map(|t| t.as_str()).collect()` is `error[E0515]`. Collect the
tokens first and borrow from those.

**Expecting a token to be a substring.** It is, for the thirteen
`BorrowingTokenizer` types, for `WordTokenizer`, and for `WordPunctTokenizer` and
`OrthographyTokenizer` in splitting mode. It is not for `AggressiveTokenizerHi`
(deletes characters), `No`/`Sv` (rewrite diacritics), `TokenizerJa` (rebuilds
tokens from code units) or `SentenceTokenizer` (substitutes placeholders).

**Using the wrong tokenizer for a language.** `AggressiveTokenizerEs`,
`AggressiveTokenizerPt` and `OrthographyTokenizer::new("fi")` have **no digits**
in their classes. If your corpus contains numbers, they silently disappear.

## Related

- [Choosing an API: tokenization](../choosing/tokenization.md) — the long-form
  decision, with pipeline diagrams
- [API shapes](../choosing/api-shapes.md) — the workspace-wide convention that
  `_into` appends and `tokens()` is the primitive
- [Core traits](core.md) — `verbora_core::Tokenizer`, `BorrowingTokenizer`,
  `trim_edge_empties`
- [n-grams](ngrams.md) — consumes a tokenizer · [Normalizers](normalizers.md) —
  what to run before or after tokenizing
- [Zero-copy](../performance/zero-copy.md) · [Buffer reuse](../performance/buffer-reuse.md) ·
  [Allocation](../performance/allocation.md) · [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)

## API reference

```bash
cargo doc -p verbora-tokenizers --no-deps --open
```

| Item | Path |
|---|---|
| `Tokenize` | `verbora_tokenizers::Tokenize` |
| `Utf16Token` | `verbora_tokenizers::Utf16Token` |
| `Pattern` | `verbora_tokenizers::Pattern` |
| `trim_edge_empties` | `verbora_tokenizers::trim_edge_empties` |
| Pinned string-matching semantics (regex flags, `\w`/`\s` classes, replace-first) | `verbora_tokenizers::whitespace` |
| Generated character classes | `verbora_tokenizers::classes` |
| The shared scanner | `verbora_tokenizers::scan` |
| `Tokenizer`, `BorrowingTokenizer` | `verbora_core` |

Source: `crates/verbora-tokenizers/src/`. Benchmarks:
`crates/verbora-tokenizers/benches/tokenizers.rs`.
